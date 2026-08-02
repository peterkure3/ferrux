use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize as PortablePtySize};

use ferrux_core::ports::pty_backend::{PtyBackend, PtySize};

/// A live pane: the child process, the PTY master (for resize/reader
/// cloning), and a writer for sending input. `child` and `writer` are
/// wrapped in `Mutex` so `kill`/`write` can work through a shared `&Handle`
/// while a background thread streams output via `master.try_clone_reader()`.
pub struct PtyHandle {
    pub child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    // `Box<dyn MasterPty + Send>` isn't `Sync`, so it's wrapped here to
    // keep `PtyHandle` (and `Arc<PaneEntry>` in the daemon) `Send + Sync`.
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

#[derive(Default)]
pub struct ConPtyBackend;

impl ConPtyBackend {
    pub fn new() -> Self {
        Self
    }
}

impl PtyBackend for ConPtyBackend {
    type Handle = PtyHandle;

    fn spawn(&self, cmd: &str, size: PtySize, cwd: Option<&str>) -> io::Result<Self::Handle> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PortablePtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(to_io_error)?;

        let mut cmd_builder = CommandBuilder::new(cmd);
        if let Some(cwd) = cwd {
            cmd_builder.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd_builder).map_err(to_io_error)?;
        // Slave end is only needed to spawn the child; drop it so EOF
        // propagates correctly once the child exits.
        drop(pair.slave);

        let writer = pair.master.take_writer().map_err(to_io_error)?;

        Ok(PtyHandle {
            child: Arc::new(Mutex::new(child)),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
        })
    }

    fn resize(&self, handle: &Self::Handle, size: PtySize) -> io::Result<()> {
        handle
            .master
            .lock()
            .unwrap()
            .resize(PortablePtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(to_io_error)
    }

    fn kill(&self, handle: &Self::Handle) -> io::Result<()> {
        handle.child.lock().unwrap().kill()
    }
}

fn to_io_error(err: anyhow::Error) -> io::Error {
    io::Error::other(err.to_string())
}
