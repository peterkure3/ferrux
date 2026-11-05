//! Real Windows ConPTY interaction test, independent of the desktop capture API.
//! cargo run -p ferrux-cli --example windows_ui_smoke -- target/release target/ui-smoke
use ferrux_term::Emulator;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(
    screen: &Mutex<Emulator>,
    label: &str,
    check: impl Fn(&str) -> bool,
) -> io::Result<String> {
    let start = Instant::now();
    loop {
        let emulator = screen.lock().unwrap();
        let text = emulator
            .screen()
            .rows(0, emulator.screen().size().1)
            .collect::<Vec<_>>()
            .join("\n");
        drop(emulator);
        if check(&text) {
            return Ok(text);
        }
        if start.elapsed() > Duration::from_secs(20) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{label}:\n{text}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let binary = PathBuf::from(arguments.next().expect("binary directory")).canonicalize()?;
    let output = PathBuf::from(arguments.next().expect("output directory"));
    fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    let sessions = output.join("sessions");
    fs::create_dir_all(&sessions)?;
    fs::write(
        sessions.join("review.toml"),
        r#"name = "review"
root = { Split = { direction = "Vertical", ratio_percent = 50, first = { Leaf = 1 }, second = { Leaf = 2 } } }
[[panes]]
id = 1
shell = "powershell.exe"
[[panes]]
id = 2
shell = "cmd.exe"
"#,
    )?;
    let endpoint = format!(r"\\.\pipe\ferrux-ui-smoke-{}", std::process::id());
    // This standalone test process owns its environment and all child processes.
    std::env::set_var("FERRUX_PIPE_NAME", &endpoint);
    let mut command = Command::new(binary.join("ferrux-daemon.exe"));
    command.stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let _daemon = Daemon(command.spawn()?);
    ferrux_tui::daemon_client::list_panes().await?;
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 48,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(io::Error::other)?;
    let mut command = CommandBuilder::new(binary.join("ferrux.exe"));
    command.args(["open", "review"]);
    command.cwd(std::env::current_dir()?);
    command.env("FERRUX_SESSION_DIR", &sessions);
    command.env("FERRUX_COLOR_MODE", "rgb");
    let mut child = pty.slave.spawn_command(command).map_err(io::Error::other)?;
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().map_err(io::Error::other)?;
    let mut writer = pty.master.take_writer().map_err(io::Error::other)?;
    let screen = Arc::new(Mutex::new(Emulator::new(48, 160, 0)));
    let parser = screen.clone();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let raw = captured.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let mut raw = raw.lock().unwrap();
            if raw.len() + count <= 1024 * 1024 {
                raw.extend_from_slice(&buffer[..count]);
            }
            drop(raw);
            parser.lock().unwrap().process(&buffer[..count]);
        }
    });
    let result = (|| -> io::Result<()> {
        // A headless terminal host must answer ConPTY's initial cursor query.
        let start = Instant::now();
        while !captured
            .lock()
            .unwrap()
            .windows(4)
            .any(|bytes| bytes == b"\x1b[6n")
        {
            if start.elapsed() > Duration::from_secs(5) {
                return Err(io::Error::other("missing ConPTY cursor query"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        writer.write_all(b"\x1b[1;1R")?;
        let initial = wait_for(&screen, "initial real shells", |text| {
            text.contains("Connected")
                && text.contains("powershell.exe")
                && text.contains("cmd.exe")
                && text.contains("PS ")
        })?;
        fs::write(output.join("160x48.txt"), initial)?;
        writer.write_all(b"\x0f")?; // Existing Ctrl+O focus cycle.
        wait_for(&screen, "keyboard focus", |text| {
            text.lines().nth(2).is_some_and(|line| {
                line.find("ACTIVE")
                    .is_some_and(|i| line[..i].chars().count() > 94)
            })
        })?;
        writer.write_all(b"echo FERRUX_CMD_%OS%\r")?;
        wait_for(&screen, "CMD input routing", |text| {
            text.contains("FERRUX_CMD_Windows_NT")
        })?;
        writer.write_all(b"\x1b[<0;35;8M\x1b[<0;35;8m")?;
        wait_for(&screen, "mouse focus", |text| {
            text.lines().nth(2).is_some_and(|line| {
                line.find("ACTIVE")
                    .is_some_and(|i| line[..i].chars().count() < 94)
            })
        })?;
        writer.write_all(b"Write-Output ('FERRUX_'+'INPUT_OK')\r")?;
        wait_for(&screen, "PowerShell input routing", |text| {
            text.contains("FERRUX_INPUT_OK")
        })?;
        writer.write_all(b"\x1b[<0;5;7M\x1b[<0;5;7m")?;
        wait_for(&screen, "sidebar mouse selection", |text| {
            text.lines().nth(2).is_some_and(|line| {
                line.find("ACTIVE")
                    .is_some_and(|i| line[..i].chars().count() > 94)
            })
        })?;
        writer.write_all(b"\x02?")?;
        wait_for(&screen, "help overlay", |text| {
            text.contains("Keyboard & mouse")
        })?;
        writer.write_all(b"\x1b")?;
        writer.write_all(b"\x02s")?;
        wait_for(&screen, "save", |text| text.contains("Workspace saved"))?;
        writer.write_all(b"\x02v")?;
        wait_for(&screen, "split", |text| text.contains("SESSIONS (3)"))?;
        writer.write_all(b"\x02x")?;
        wait_for(&screen, "close", |text| text.contains("SESSIONS (2)"))?;
        for (cols, rows) in [(80, 24), (100, 30), (120, 40)] {
            screen.lock().unwrap().set_size(rows, cols);
            pty.master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(io::Error::other)?;
            std::thread::sleep(Duration::from_millis(700));
            let text = wait_for(&screen, "resize", |text| {
                text.contains("NORMAL") && text.contains("^Q")
            })?;
            fs::write(output.join(format!("{cols}x{rows}.txt")), text)?;
        }
        writer.write_all(b"\x11")?; // Detach, never terminate daemon-owned shells.
        let start = Instant::now();
        while child.try_wait().map_err(io::Error::other)?.is_none() {
            if start.elapsed() > Duration::from_secs(5) {
                return Err(io::Error::other("detach did not exit UI"));
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        Ok(())
    })();
    if result.is_err() {
        fs::write(
            output.join("failure-output.ansi"),
            &*captured.lock().unwrap(),
        )?;
        eprintln!(
            "UI child state: {:?}",
            child.try_wait().map_err(io::Error::other)?
        );
        let _ = child.kill();
    }
    // Clean up only panes on this process's unique endpoint, even on failure.
    let panes = ferrux_tui::daemon_client::list_panes().await?;
    let detached_count = panes
        .iter()
        .filter(|pane| pane.running == Some(true))
        .count();
    for pane in panes {
        ferrux_tui::daemon_client::kill_pane(pane.id).await?;
    }
    result?;
    assert_eq!(
        detached_count, 2,
        "detach must preserve the two shell processes"
    );
    println!("PASS: real ConPTY UI, mouse/keyboard/sidebar focus, PowerShell/CMD input, help, save, split/close, four sizes, detach preserves 2 shells");
    Ok(())
}
