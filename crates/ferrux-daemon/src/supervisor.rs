use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;

use ferrux_agent_detect::ProcessNameDetector;
use ferrux_core::domain::agent::AgentKind;
use ferrux_core::ports::agent_detector::AgentDetector;
use ferrux_core::ports::notification_sink::{Notification, NotificationSink};
use ferrux_core::ports::pty_backend::{PtyBackend, PtySize};
use ferrux_ipc::PaneInfo;
use ferrux_pty::{ConPtyBackend, PtyHandle};
use ferrux_term::Emulator;

const OUTPUT_CHANNEL_CAPACITY: usize = 1024;
const SCROLLBACK_LINES: usize = 1000;
const AGENT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A Device Status Report / Cursor Position Report query (`ESC[6n`).
/// ConPTY's conhost sends this on startup and blocks further rendering
/// until it gets a reply — without answering it, panes on Windows appear
/// to hang forever. Real terminal emulators answer this; since we stand
/// in as "the terminal" from the PTY's point of view, we must too.
const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";

fn answer_cursor_position_queries(
    chunk: &[u8],
    emulator: &Arc<Mutex<Emulator>>,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) {
    if !chunk
        .windows(CURSOR_POSITION_QUERY.len())
        .any(|window| window == CURSOR_POSITION_QUERY)
    {
        return;
    }
    let (row, col) = emulator.lock().unwrap().screen().cursor_position();
    let response = format!("\x1b[{};{}R", row + 1, col + 1);
    let _ = writer.lock().unwrap().write_all(response.as_bytes());
}

/// Takes a freshly-constructed `Arc<Mutex<T>>` (refcount 1) and returns
/// its inner value. Used to pull the child/master/writer out of a newly
/// spawned `PtyHandle` so they can be swapped into an existing one.
fn take_uniquely_owned<T>(arc: Arc<Mutex<T>>) -> T {
    Arc::try_unwrap(arc)
        .unwrap_or_else(|_| unreachable!("freshly constructed Arc must be uniquely owned"))
        .into_inner()
        .unwrap()
}

pub struct PaneEntry {
    pub shell: String,
    pub handle: PtyHandle,
    pub output_tx: broadcast::Sender<Vec<u8>>,
    /// Kept in sync with every byte the pane has produced, so a new
    /// attach can replay the current screen instead of starting blank.
    /// Reset to a blank screen on restart.
    pub emulator: Arc<Mutex<Emulator>>,
    /// Refreshed periodically by process-name sniffing; `None` means no
    /// known agent CLI is currently running under this pane's shell.
    pub agent_kind: Arc<Mutex<Option<AgentKind>>>,
    /// The OS pid of whichever process incarnation is currently running
    /// under this pane — changes across restarts.
    current_pid: Mutex<Option<u32>>,
    current_size: Mutex<(u16, u16)>,
    /// Whether the reader thread should relaunch `shell` when the
    /// process dies rather than ending the pane.
    restart: bool,
    /// Cleared by an explicit `kill`, which distinguishes "the user
    /// asked for this pane to stop" from "the process merely died and
    /// should be restarted."
    alive: Arc<AtomicBool>,
}

pub struct Supervisor {
    backend: ConPtyBackend,
    panes: Mutex<HashMap<u64, Arc<PaneEntry>>>,
    next_id: AtomicU64,
    notifier: Arc<dyn NotificationSink + Send + Sync>,
}

impl Supervisor {
    pub fn new(notifier: Arc<dyn NotificationSink + Send + Sync>) -> Self {
        Self {
            backend: ConPtyBackend::new(),
            panes: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            notifier,
        }
    }

    pub fn spawn_pane(&self, shell: &str, cols: u16, rows: u16) -> io::Result<u64> {
        self.spawn_pane_with_restart(shell, cols, rows, false)
    }

    /// Like `spawn_pane`, but if `restart` is set the daemon relaunches
    /// `shell` in place whenever the process dies on its own (not when
    /// killed via `kill`). Used for panes declared in a `ferrux.json`
    /// manifest.
    pub fn spawn_pane_with_restart(
        &self,
        shell: &str,
        cols: u16,
        rows: u16,
        restart: bool,
    ) -> io::Result<u64> {
        let handle = self.backend.spawn(shell, PtySize { cols, rows })?;
        let child_pid = handle.child.lock().unwrap().process_id();

        let (output_tx, _rx) = broadcast::channel(OUTPUT_CHANNEL_CAPACITY);
        let emulator = Arc::new(Mutex::new(Emulator::new(rows, cols, SCROLLBACK_LINES)));
        let agent_kind = Arc::new(Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(true));

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let entry = Arc::new(PaneEntry {
            shell: shell.to_string(),
            handle,
            output_tx,
            emulator,
            agent_kind,
            current_pid: Mutex::new(child_pid),
            current_size: Mutex::new((cols, rows)),
            restart,
            alive,
        });

        spawn_reader_thread(entry.clone());
        spawn_agent_detection_task(entry.clone(), self.notifier.clone(), id);

        self.panes.lock().unwrap().insert(id, entry);
        Ok(id)
    }

    pub fn list(&self) -> Vec<PaneInfo> {
        self.panes
            .lock()
            .unwrap()
            .iter()
            .map(|(id, entry)| PaneInfo {
                id: *id,
                shell: entry.shell.clone(),
                agent_kind: *entry.agent_kind.lock().unwrap(),
            })
            .collect()
    }

    pub fn resize(&self, id: u64, cols: u16, rows: u16) -> io::Result<()> {
        let entry = self.panes.lock().unwrap().get(&id).cloned();
        match entry {
            Some(entry) => {
                self.backend.resize(&entry.handle, PtySize { cols, rows })?;
                entry.emulator.lock().unwrap().set_size(rows, cols);
                *entry.current_size.lock().unwrap() = (cols, rows);
                Ok(())
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, "pane not found")),
        }
    }

    /// Permanently stops a pane: even if it was declared `restart`, it
    /// will not come back.
    pub fn kill(&self, id: u64) -> io::Result<()> {
        let entry = self.panes.lock().unwrap().remove(&id);
        match entry {
            Some(entry) => {
                entry.alive.store(false, Ordering::SeqCst);
                entry.handle.child.lock().unwrap().kill()
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, "pane not found")),
        }
    }

    /// Kills the pane's current process without marking it dead — if
    /// `restart` is set, the reader thread relaunches it. Used by the
    /// watchdog to bounce a misbehaving process.
    pub fn kill_for_restart(&self, id: u64) -> io::Result<()> {
        let entry = self.panes.lock().unwrap().get(&id).cloned();
        match entry {
            Some(entry) => entry.handle.child.lock().unwrap().kill(),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "pane not found")),
        }
    }

    pub fn get(&self, id: u64) -> Option<Arc<PaneEntry>> {
        self.panes.lock().unwrap().get(&id).cloned()
    }

    /// Snapshot of `(pane id, current pid, restart enabled)` for every
    /// live pane, used by the memory watchdog.
    pub fn pane_snapshot(&self) -> Vec<(u64, Option<u32>, bool)> {
        self.panes
            .lock()
            .unwrap()
            .iter()
            .map(|(id, entry)| (*id, *entry.current_pid.lock().unwrap(), entry.restart))
            .collect()
    }

    pub fn notify(&self, notification: Notification) {
        self.notifier.notify(notification);
    }
}

/// Streams a pane's PTY output for its lifetime, including across
/// restarts. Exits for good once the process dies without `restart`, or
/// once `kill` has marked the pane no longer alive.
fn spawn_reader_thread(entry: Arc<PaneEntry>) {
    std::thread::spawn(move || loop {
        let mut reader = match entry.handle.master.lock().unwrap().try_clone_reader() {
            Ok(reader) => reader,
            Err(e) => {
                eprintln!("pane reader thread exiting: {e}");
                break;
            }
        };

        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    entry.emulator.lock().unwrap().process(chunk);
                    answer_cursor_position_queries(chunk, &entry.emulator, &entry.handle.writer);
                    // No subscribers is fine; keep draining so the pane
                    // doesn't block on a full pipe.
                    let _ = entry.output_tx.send(chunk.to_vec());
                }
                Err(_) => break,
            }
        }

        if !entry.restart || !entry.alive.load(Ordering::SeqCst) {
            break;
        }

        let (cols, rows) = *entry.current_size.lock().unwrap();
        match ConPtyBackend::new().spawn(&entry.shell, PtySize { cols, rows }) {
            Ok(new_handle) => {
                let new_pid = new_handle.child.lock().unwrap().process_id();
                *entry.handle.child.lock().unwrap() = take_uniquely_owned(new_handle.child);
                *entry.handle.master.lock().unwrap() = take_uniquely_owned(new_handle.master);
                *entry.handle.writer.lock().unwrap() = take_uniquely_owned(new_handle.writer);
                *entry.current_pid.lock().unwrap() = new_pid;
                *entry.emulator.lock().unwrap() = Emulator::new(rows, cols, SCROLLBACK_LINES);
                *entry.agent_kind.lock().unwrap() = None;
            }
            Err(e) => {
                eprintln!("pane restart failed: {e}");
                break;
            }
        }
    });
}

/// Polls the pane's current process tree for a known agent CLI and
/// notifies on start/end transitions. Re-reads `current_pid` each tick
/// so it keeps working across restarts. Stops once `alive` is cleared.
fn spawn_agent_detection_task(
    entry: Arc<PaneEntry>,
    notifier: Arc<dyn NotificationSink + Send + Sync>,
    id: u64,
) {
    tokio::spawn(async move {
        let mut last: Option<AgentKind> = None;
        loop {
            if !entry.alive.load(Ordering::SeqCst) {
                break;
            }

            let pid = *entry.current_pid.lock().unwrap();
            let detected = match pid {
                Some(pid) => {
                    tokio::task::spawn_blocking(move || ProcessNameDetector::new().detect(pid))
                        .await
                        .unwrap_or(None)
                }
                None => None,
            };

            if detected != last {
                *entry.agent_kind.lock().unwrap() = detected;
                match (last, detected) {
                    (None, Some(kind)) => notifier.notify(Notification {
                        title: format!("{} — pane {id}", entry.shell),
                        body: format!("{kind:?} started"),
                    }),
                    (Some(kind), None) => notifier.notify(Notification {
                        title: format!("{} — pane {id}", entry.shell),
                        body: format!("{kind:?} ended"),
                    }),
                    _ => {}
                }
                last = detected;
            }

            tokio::time::sleep(AGENT_POLL_INTERVAL).await;
        }
    });
}
