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
            Request::SpawnPane { shell, cols, rows } => {
                let response = match supervisor.spawn_pane(&shell, cols, rows) {
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
                let response = match supervisor.resize(id, cols, rows) {
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
                let response = match supervisor.kill(id) {
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

/// Bridges the pipe connection and the pane's PTY: pipe input goes to the
/// PTY writer, PTY output (via the pane's broadcast channel) goes to the
/// pipe. Runs until either side closes.
async fn run_attach(stream: NamedPipeServer, entry: Arc<PaneEntry>) {
    let (mut read_half, mut write_half) = tokio::io::split(stream);
    let writer = entry.handle.writer.clone();
    // Subscribe before snapshotting so no output produced in between is
    // lost; worst case a few bytes get applied twice (once via the
    // snapshot, once replayed from the channel), which is harmless.
    let mut rx = entry.output_tx.subscribe();
    let snapshot = entry.emulator.lock().unwrap().screen().state_formatted();

    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut w = writer.lock().unwrap();
                    if w.write_all(&buf[..n]).is_err() {
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
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
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
