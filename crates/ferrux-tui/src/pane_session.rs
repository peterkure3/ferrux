use std::io;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use ferrux_ipc::{read_message, write_message, Request, Response};
use ferrux_term::Emulator;

use crate::daemon_client::connect_with_retry;

const SCROLLBACK_LINES: usize = 1000;

/// A live attach to one daemon-owned pane: an emulator kept in sync with
/// its output, and a channel to send it input.
pub struct PaneSession {
    pub emulator: Arc<Mutex<Emulator>>,
    pub write_tx: mpsc::Sender<Vec<u8>>,
    read_task: tokio::task::JoinHandle<()>,
    write_task: tokio::task::JoinHandle<io::Result<()>>,
}

impl PaneSession {
    /// Drain accepted input before a normal detach. Drop still provides
    /// prompt cancellation on errors; a stalled pipe cannot hang shutdown.
    pub async fn detach(mut self) -> io::Result<()> {
        let (closed_tx, closed_rx) = mpsc::channel(1);
        drop(closed_rx);
        drop(std::mem::replace(&mut self.write_tx, closed_tx));
        tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.write_task)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pane input did not drain before detach",
                )
            })?
            .map_err(io::Error::other)?
    }
}

impl Drop for PaneSession {
    fn drop(&mut self) {
        // Detach closes this client's pipe, never the daemon-owned PTY.
        self.read_task.abort();
        self.write_task.abort();
    }
}

/// Attaches to `pane_id`, sized to `cols`x`rows`, and starts the
/// background tasks that keep its emulator updated and forward writes.
/// `redraw_tx` gets pinged every time new output arrives so the caller
/// knows to repaint.
pub async fn attach(
    pane_id: u64,
    cols: u16,
    rows: u16,
    redraw_tx: mpsc::Sender<()>,
) -> io::Result<PaneSession> {
    let mut client = connect_with_retry().await?;
    write_message(&mut client, &Request::AttachPane { id: pane_id }).await?;
    match read_message(&mut client).await? {
        Response::Ok => {}
        Response::Error { message } => return Err(io::Error::other(message)),
        other => return Err(io::Error::other(format!("unexpected response: {other:?}"))),
    }

    let emulator = Arc::new(Mutex::new(Emulator::new(rows, cols, SCROLLBACK_LINES)));
    let (mut read_half, mut write_half) = tokio::io::split(client);

    let emulator_for_task = emulator.clone();
    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    emulator_for_task.lock().unwrap().process(&buf[..n]);
                    // One pending notification represents all output since
                    // the last draw; never queue a frame for every chunk.
                    let _ = redraw_tx.try_send(());
                }
            }
        }
    });

    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(64);
    let write_task = tokio::spawn(async move {
        while let Some(bytes) = write_rx.recv().await {
            write_half.write_all(&bytes).await?;
        }
        Ok(())
    });

    Ok(PaneSession {
        emulator,
        write_tx,
        read_task,
        write_task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropping_session_cancels_both_pipe_tasks() {
        let read_task = tokio::spawn(std::future::pending::<()>());
        let write_task = tokio::spawn(std::future::pending::<io::Result<()>>());
        let read = read_task.abort_handle();
        let write = write_task.abort_handle();
        let (write_tx, _rx) = mpsc::channel(64);
        let session = PaneSession {
            emulator: Arc::new(Mutex::new(Emulator::new(24, 80, 0))),
            write_tx,
            read_task,
            write_task,
        };
        drop(session);
        tokio::task::yield_now().await;
        assert!(read.is_finished());
        assert!(write.is_finished());
    }

    #[tokio::test]
    async fn normal_detach_drains_accepted_input() {
        let (write_tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
        write_tx.send(b"command\r".to_vec()).await.unwrap();
        let collected = Arc::new(Mutex::new(Vec::new()));
        let output = collected.clone();
        let write_task = tokio::spawn(async move {
            while let Some(bytes) = rx.recv().await {
                output.lock().unwrap().extend(bytes);
            }
            Ok(())
        });
        let session = PaneSession {
            emulator: Arc::new(Mutex::new(Emulator::new(24, 80, 0))),
            write_tx,
            read_task: tokio::spawn(std::future::pending::<()>()),
            write_task,
        };
        session.detach().await.unwrap();
        assert_eq!(&*collected.lock().unwrap(), b"command\r");
    }
}
