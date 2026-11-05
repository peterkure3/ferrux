mod pipe_server;
mod supervisor;
mod watchdog;

use std::sync::Arc;

use tokio::net::windows::named_pipe::ServerOptions;

use ferrux_core::domain::manifest::Manifest;
use ferrux_ipc::pipe_name;
use ferrux_notify::WindowsNotifier;
use supervisor::Supervisor;

const MANIFEST_FILE: &str = "ferrux.json";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let pipe_name = pipe_name();
    let supervisor = Arc::new(Supervisor::new(Arc::new(WindowsNotifier)));

    load_manifest(&supervisor);
    tokio::spawn(watchdog::run(supervisor.clone()));
    tokio::spawn(supervisor::run_agent_detection(supervisor.clone()));

    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;

    println!("ferrux-daemon listening on {pipe_name}");

    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().create(&pipe_name)?;

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

    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());

    for pane in manifest.panes {
        match supervisor.spawn_pane_with_restart(
            &pane.shell,
            pane.cols,
            pane.rows,
            pane.restart,
            cwd.clone(),
        ) {
            Ok(id) => println!("{MANIFEST_FILE}: spawned pane {id} ({})", pane.shell),
            Err(e) => eprintln!("{MANIFEST_FILE}: failed to spawn {}: {e}", pane.shell),
        }
    }
}
