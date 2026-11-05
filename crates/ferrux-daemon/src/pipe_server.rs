use std::io::Write;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::broadcast;

use ferrux_ipc::{read_message, write_message, Request, Response};

use crate::supervisor::{PaneEntry, Supervisor};

pub async fn handle_connection(mut stream: NamedPipeServer, supervisor: Arc<Supervisor>) {
    loop {
        let request: Request = match read_message(&mut stream).await {
            Ok(request) => request,
            Err(_) => return,
        };

        match request {
            Request::SpawnPane {
                shell,
                cols,
                rows,
                cwd,
            } => {
                let owner = supervisor.clone();
                let response =
                    match blocking(move || owner.spawn_pane(&shell, cols, rows, cwd)).await {
                        Ok(id) => Response::PaneSpawned { id },
                        Err(e) => Response::Error {
                            message: e.to_string(),
                        },
                    };
                if write_message(&mut stream, &response).await.is_err() {
                    return;
                }
            }
            Request::ListPanes => {
                let panes = supervisor.list();
                if write_message(&mut stream, &Response::PaneList { panes })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Request::ResizePane { id, cols, rows } => {
                let owner = supervisor.clone();
                let response = match blocking(move || owner.resize(id, cols, rows)).await {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                };
                if write_message(&mut stream, &response).await.is_err() {
                    return;
                }
            }
            Request::KillPane { id } => {
                let owner = supervisor.clone();
                let response = match blocking(move || owner.kill(id)).await {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                };
                if write_message(&mut stream, &response).await.is_err() {
                    return;
                }
            }
            Request::AttachPane { id } => {
                let entry = match supervisor.get(id) {
                    Some(entry) => entry,
                    None => {
                        let _ = write_message(
                            &mut stream,
                            &Response::Error {
                                message: format!("pane {id} not found"),
                            },
                        )
                        .await;
                        continue;
                    }
                };
                if write_message(&mut stream, &Response::Ok).await.is_err() {
                    return;
                }
                // Attach hands the connection over to raw passthrough for
                // its remainder, so this task's job ends here.
                run_attach(stream, entry).await;
                return;
            }
        }
    }
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(std::io::Error::other)?
}

/// Bridges the pipe connection and the pane's PTY: pipe input goes to the
/// PTY writer, PTY output (via the pane's broadcast channel) goes to the
/// pipe. Runs until either side closes.
async fn run_attach(stream: NamedPipeServer, entry: Arc<PaneEntry>) {
    let (mut read_half, mut write_half) = tokio::io::split(stream);
    let writer = entry.handle.writer.clone();
    // The reader publishes while holding this same lock, so bytes in the
    // snapshot cannot also appear in the newly subscribed stream.
    let (mut rx, snapshot) = snapshot_and_subscribe(&entry);

    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let writer = writer.clone();
                    let bytes = buf[..n].to_vec();
                    // A blocked ConPTY must not block a Tokio I/O worker.
                    let result = tokio::task::spawn_blocking(move || {
                        writer.lock().unwrap().write_all(&bytes)
                    })
                    .await;
                    if !matches!(result, Ok(Ok(()))) {
                        break;
                    }
                }
            }
        }
    });

    let write_task = tokio::spawn(async move {
        if write_half.write_all(&snapshot).await.is_err() {
            return;
        }
        loop {
            match rx.recv().await {
                Ok(data) => {
                    if write_half.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Dropping arbitrary escape-sequence bytes corrupts
                    // terminal state. Recover from the authoritative grid.
                    let (new_rx, snapshot) = snapshot_and_subscribe(&entry);
                    rx = new_rx;
                    if write_half.write_all(&snapshot).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let read_abort = read_task.abort_handle();
    let write_abort = write_task.abort_handle();
    tokio::select! {
        _ = read_task => { write_abort.abort(); }
        _ = write_task => { read_abort.abort(); }
    }
}

fn snapshot_and_subscribe(entry: &PaneEntry) -> (broadcast::Receiver<Arc<[u8]>>, Vec<u8>) {
    let emulator = entry.emulator.lock().unwrap();
    let rx = entry.output_tx.subscribe();
    (rx, emulator.screen().state_formatted())
}
