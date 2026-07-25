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
    pub write_tx: mpsc::UnboundedSender<Vec<u8>>,
}

/// Attaches to `pane_id`, sized to `cols`x`rows`, and starts the
/// background tasks that keep its emulator updated and forward writes.
/// `redraw_tx` gets pinged every time new output arrives so the caller
/// knows to repaint.
pub async fn attach(
    pane_id: u64,
    cols: u16,
    rows: u16,
    redraw_tx: mpsc::UnboundedSender<()>,
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
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    emulator_for_task.lock().unwrap().process(&buf[..n]);
                    let _ = redraw_tx.send(());
                }
            }
        }
    });

    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(async move {
        while let Some(bytes) = write_rx.recv().await {
            if write_half.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });

    Ok(PaneSession { emulator, write_tx })
}
