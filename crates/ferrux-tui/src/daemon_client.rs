use std::io;
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

use ferrux_ipc::{read_message, write_message, PaneInfo, Request, Response, PIPE_NAME};

/// The daemon briefly can't accept a new connection right after a
/// previous client disconnects, while it recycles the pipe instance
/// (`ERROR_PIPE_BUSY`), so opening the pipe gets a few retries.
pub async fn connect_with_retry() -> io::Result<NamedPipeClient> {
    let mut last_err = None;
    for _ in 0..10 {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(client) => return Ok(client),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(last_err.unwrap())
}

pub async fn spawn_pane(
    shell: &str,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
) -> io::Result<u64> {
    let mut client = connect_with_retry().await?;
    write_message(
        &mut client,
        &Request::SpawnPane {
            shell: shell.to_string(),
            cols,
            rows,
            cwd,
        },
    )
    .await?;
    match read_message(&mut client).await? {
        Response::PaneSpawned { id } => Ok(id),
        Response::Error { message } => Err(io::Error::other(message)),
        other => Err(io::Error::other(format!("unexpected response: {other:?}"))),
    }
}

pub async fn resize_pane(id: u64, cols: u16, rows: u16) -> io::Result<()> {
    let mut client = connect_with_retry().await?;
    write_message(&mut client, &Request::ResizePane { id, cols, rows }).await?;
    match read_message(&mut client).await? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(io::Error::other(message)),
        other => Err(io::Error::other(format!("unexpected response: {other:?}"))),
    }
}

pub async fn list_panes() -> io::Result<Vec<PaneInfo>> {
    let mut client = connect_with_retry().await?;
    write_message(&mut client, &Request::ListPanes).await?;
    match read_message(&mut client).await? {
        Response::PaneList { panes } => Ok(panes),
        Response::Error { message } => Err(io::Error::other(message)),
        other => Err(io::Error::other(format!("unexpected response: {other:?}"))),
    }
}

pub async fn kill_pane(id: u64) -> io::Result<()> {
    let mut client = connect_with_retry().await?;
    write_message(&mut client, &Request::KillPane { id }).await?;
    match read_message(&mut client).await? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(io::Error::other(message)),
        other => Err(io::Error::other(format!("unexpected response: {other:?}"))),
    }
}
