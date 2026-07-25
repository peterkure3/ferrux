use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use ferrux_core::domain::agent::AgentKind;

pub const PIPE_NAME: &str = r"\\.\pipe\ferrux-daemon";

/// Bumped whenever `Request`/`Response` change shape. The daemon and CLI
/// must agree on this to talk to each other.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: u64,
    pub shell: String,
    pub agent_kind: Option<AgentKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    SpawnPane {
        shell: String,
        cols: u16,
        rows: u16,
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

    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;

    serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
