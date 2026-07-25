mod pipe_server;
mod supervisor;
mod watchdog;

use std::sync::Arc;

use tokio::net::windows::named_pipe::ServerOptions;

use ferrux_core::domain::manifest::Manifest;
use ferrux_ipc::PIPE_NAME;
use ferrux_notify::WindowsNotifier;
use supervisor::Supervisor;

const MANIFEST_FILE: &str = "ferrux.json";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let supervisor = Arc::new(Supervisor::new(Arc::new(WindowsNotifier)));

    load_manifest(&supervisor);
    tokio::spawn(watchdog::run(supervisor.clone()));

    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(PIPE_NAME)?;

    println!("ferrux-daemon listening on {PIPE_NAME}");

    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().create(PIPE_NAME)?;

        let supervisor = supervisor.clone();
        tokio::spawn(async move {
            pipe_server::handle_connection(connected, supervisor).await;
        });
    }
}

/// Spawns every pane declared in `./ferrux.json`, if present — panes
/// marked `restart` get auto-relaunched by the supervisor when their
/// process dies.
fn load_manifest(supervisor: &Supervisor) {
    let Ok(contents) = std::fs::read_to_string(MANIFEST_FILE) else {
        return;
    };
    let manifest: Manifest = match serde_json::from_str(&contents) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!("{MANIFEST_FILE}: {e}");
            return;
        }
    };

    for pane in manifest.panes {
        match supervisor.spawn_pane_with_restart(&pane.shell, pane.cols, pane.rows, pane.restart) {
            Ok(id) => println!("{MANIFEST_FILE}: spawned pane {id} ({})", pane.shell),
            Err(e) => eprintln!("{MANIFEST_FILE}: failed to spawn {}: {e}", pane.shell),
        }
    }
}
