use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use ferrux_core::domain::agent::AgentKind;

pub const PIPE_NAME: &str = r"\\.\pipe\ferrux-daemon";

/// Optional local-only endpoint for isolated integration tests/instances.
pub fn pipe_name() -> String {
    std::env::var("FERRUX_PIPE_NAME")
        .ok()
        .filter(|name| name.starts_with(r"\\.\pipe\ferrux-") && !name[9..].contains(['\\', '/']))
        .unwrap_or_else(|| PIPE_NAME.to_owned())
}

/// Bumped whenever `Request`/`Response` change shape. The daemon and CLI
/// must agree on this to talk to each other.
pub const PROTOCOL_VERSION: u32 = 2;

/// Control messages are small; terminal output uses the raw attach stream.
/// Reject hostile/corrupt lengths before allocating a body buffer.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: u64,
    pub shell: String,
    pub agent_kind: Option<AgentKind>,
    /// Older daemons omit this additive field; the UI reports unknown then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    SpawnPane {
        shell: String,
        cols: u16,
        rows: u16,
        /// Directory the pane's shell should start in. `None` falls back
        /// to the daemon process's own working directory.
        cwd: Option<String>,
    },
    ListPanes,
    KillPane {
        id: u64,
    },
    ResizePane {
        id: u64,
        cols: u16,
        rows: u16,
    },
    /// After the daemon replies `Response::Ok`, the connection switches
    /// from framed JSON to a raw byte passthrough between the caller and
    /// the pane's PTY, until either side closes the pipe.
    AttachPane {
        id: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    PaneSpawned { id: u64 },
    PaneList { panes: Vec<PaneInfo> },
    Ok,
    Error { message: String },
}

/// Writes a length-prefixed (u32 LE) JSON message.
pub async fn write_message<W, T>(writer: &mut W, message: &T) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(message)?;
    validate_length(bytes.len())?;
    let len = bytes.len() as u32;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

/// Reads a length-prefixed (u32 LE) JSON message written by [`write_message`].
pub async fn read_message<R, T>(reader: &mut R) -> std::io::Result<T>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    validate_length(len)?;

    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;

    serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn validate_length(len: usize) -> std::io::Result<()> {
    if len > MAX_MESSAGE_BYTES {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "IPC message exceeds 4 MiB limit",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_pane_list_has_unknown_lifecycle_not_fake_running_state() {
        let pane: PaneInfo =
            serde_json::from_str(r#"{"id":1,"shell":"cmd.exe","agent_kind":null}"#).unwrap();
        assert_eq!(pane.running, None);
    }

    #[tokio::test]
    async fn preserves_version_two_json_framing() {
        let mut frame = Vec::new();
        write_message(&mut frame, &Request::AttachPane { id: 42 })
            .await
            .unwrap();
        assert_eq!(&frame[4..], br#"{"AttachPane":{"id":42}}"#);
        let request: Request = read_message(&mut frame.as_slice()).await.unwrap();
        assert!(matches!(request, Request::AttachPane { id: 42 }));
    }

    #[tokio::test]
    async fn rejects_oversized_length_without_reading_body() {
        let header = u32::MAX.to_le_bytes();
        let result = read_message::<_, Request>(&mut header.as_slice()).await;
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn rejects_truncated_and_invalid_messages() {
        let mut truncated = &b"\x04\0\0\0{}"[..];
        assert!(read_message::<_, Request>(&mut truncated).await.is_err());
        let mut invalid = &b"\x01\0\0\0?"[..];
        assert_eq!(
            read_message::<_, Request>(&mut invalid)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
