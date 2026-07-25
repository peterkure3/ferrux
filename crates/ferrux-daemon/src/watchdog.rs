use std::sync::Arc;
use std::time::Duration;

use sysinfo::{Pid, ProcessesToUpdate, System};

use ferrux_core::ports::notification_sink::Notification;

use crate::supervisor::Supervisor;

const CHECK_INTERVAL: Duration = Duration::from_secs(5);
const MEMORY_LIMIT_BYTES: u64 = 500 * 1024 * 1024;

/// Periodically checks every pane's process against a memory ceiling,
/// killing (and, for `restart` panes, thereby triggering a relaunch of)
/// anything over the limit — the init-system-style resilience half of
/// the supervisor, complementing pane auto-restart.
pub async fn run(supervisor: Arc<Supervisor>) {
    loop {
        tokio::time::sleep(CHECK_INTERVAL).await;
        check_memory_pressure(&supervisor).await;
    }
}

async fn check_memory_pressure(supervisor: &Arc<Supervisor>) {
    for (id, pid, restart) in supervisor.pane_snapshot() {
        let Some(pid) = pid else { continue };

        let memory_bytes = tokio::task::spawn_blocking(move || {
            let mut system = System::new();
            system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
            system
                .process(Pid::from_u32(pid))
                .map(|process| process.memory())
                .unwrap_or(0)
        })
        .await
        .unwrap_or(0);

        if memory_bytes > MEMORY_LIMIT_BYTES {
            let _ = supervisor.kill_for_restart(id);
            let restart_note = if restart { " (restarting)" } else { "" };
            supervisor.notify(Notification {
                title: format!("pane {id}"),
                body: format!(
                    "killed: {}MB exceeds the {}MB memory limit{restart_note}",
                    memory_bytes / (1024 * 1024),
                    MEMORY_LIMIT_BYTES / (1024 * 1024),
                ),
            });
        }
    }
}
