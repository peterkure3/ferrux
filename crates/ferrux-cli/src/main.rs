use std::io;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

use ferrux_ipc::{read_message, write_message, Request, Response, PIPE_NAME};

#[derive(Parser)]
#[command(name = "ferrux", about = "Ferrux terminal/workspace multiplexer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Spawn a new pane running `shell` (e.g. `powershell.exe`, `cmd.exe`,
    /// `pwsh.exe` if installed).
    New {
        #[arg(long, default_value = "powershell.exe")]
        shell: String,
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 30)]
        rows: u16,
    },
    /// List panes owned by the daemon.
    Ls,
    /// Attach this terminal to a pane's raw input/output stream.
    Attach { id: u64 },
    /// Open a rendered (vt100 + ratatui) live view of a single pane.
    View { id: u64 },
    /// Kill a pane by id.
    KillPane { id: u64 },
    /// Open (creating if needed) a multi-pane workspace: splits, sidebar,
    /// tmux-style Ctrl+B keybindings, TOML session persistence.
    Open {
        #[arg(default_value = "default")]
        name: String,
        /// Shell for newly created panes (e.g. `powershell.exe`, `cmd.exe`,
        /// `pwsh.exe` if installed). Ignored for panes loaded from a saved
        /// workspace, which keep their own shell.
        #[arg(long, default_value = "powershell.exe")]
        shell: String,
    },
    /// List saved workspace names.
    Workspaces,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> io::Result<()> {
    match cli.command {
        Commands::New { shell, cols, rows } => {
            let mut client = connect_or_spawn_daemon().await?;
            let cwd = std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            write_message(&mut client, &Request::SpawnPane { shell, cols, rows, cwd }).await?;
            match read_message(&mut client).await? {
                Response::PaneSpawned { id } => println!("pane {id} spawned"),
                Response::Error { message } => eprintln!("error: {message}"),
                other => eprintln!("unexpected response: {other:?}"),
            }
            Ok(())
        }
        Commands::Ls => {
            let mut client = connect_or_spawn_daemon().await?;
            write_message(&mut client, &Request::ListPanes).await?;
            match read_message(&mut client).await? {
                Response::PaneList { panes } => {
                    println!("{:<6} {:<10} AGENT", "ID", "SHELL");
                    for pane in panes {
                        let agent = match pane.agent_kind {
                            Some(kind) => format!("{kind:?}"),
                            None => "-".to_string(),
                        };
                        println!("{:<6} {:<10} {}", pane.id, pane.shell, agent);
                    }
                }
                Response::Error { message } => eprintln!("error: {message}"),
                other => eprintln!("unexpected response: {other:?}"),
            }
            Ok(())
        }
        Commands::KillPane { id } => {
            let mut client = connect_or_spawn_daemon().await?;
            write_message(&mut client, &Request::KillPane { id }).await?;
            match read_message(&mut client).await? {
                Response::Ok => println!("pane {id} killed"),
                Response::Error { message } => eprintln!("error: {message}"),
                other => eprintln!("unexpected response: {other:?}"),
            }
            Ok(())
        }
        Commands::View { id } => {
            // Ensure the daemon is up, then hand off to the TUI app,
            // which makes its own attach connection.
            connect_or_spawn_daemon().await?;
            ferrux_tui::app::view_pane(id).await
        }
        Commands::Open { name, shell } => {
            connect_or_spawn_daemon().await?;
            ferrux_tui::app::run(name, shell).await
        }
        Commands::Workspaces => {
            let store = ferrux_tui::session_store::TomlSessionStore::new()?;
            match ferrux_core::ports::session_store::SessionStore::list(&store) {
                Ok(names) => {
                    for name in names {
                        println!("{name}");
                    }
                }
                Err(e) => eprintln!("error: {e}"),
            }
            Ok(())
        }
        Commands::Attach { id } => {
            let mut client = connect_or_spawn_daemon().await?;
            write_message(&mut client, &Request::AttachPane { id }).await?;
            match read_message(&mut client).await? {
                Response::Ok => attach_passthrough(client).await,
                Response::Error { message } => {
                    eprintln!("error: {message}");
                    Ok(())
                }
                other => {
                    eprintln!("unexpected response: {other:?}");
                    Ok(())
                }
            }
        }
    }
}

/// Bridges this process's stdin/stdout with the pane's raw byte stream
/// until either side closes.
async fn attach_passthrough(client: NamedPipeClient) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let result = run_passthrough_loop(client).await;
    crossterm::terminal::disable_raw_mode()?;
    result
}

async fn run_passthrough_loop(client: NamedPipeClient) -> io::Result<()> {
    let (mut pipe_read, mut pipe_write) = tokio::io::split(client);
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let to_pipe = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if pipe_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let from_pipe = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match pipe_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if stdout.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    let _ = stdout.flush().await;
                }
            }
        }
    });

    tokio::select! {
        _ = to_pipe => {}
        _ = from_pipe => {}
    }
    Ok(())
}

/// Connects to the daemon's named pipe, spawning `ferrux-daemon` (found
/// next to this executable) if the pipe doesn't exist yet.
async fn connect_or_spawn_daemon() -> io::Result<NamedPipeClient> {
    let mut spawned = false;
    for attempt in 0..10 {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(client) => return Ok(client),
            Err(_) if attempt == 0 && !spawned => {
                spawn_daemon()?;
                spawned = true;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "could not connect to ferrux-daemon",
    ))
}

fn spawn_daemon() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let daemon_path = exe.with_file_name("ferrux-daemon.exe");
    std::process::Command::new(daemon_path).spawn()?;
    Ok(())
}
