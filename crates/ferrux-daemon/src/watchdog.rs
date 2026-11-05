use std::sync::Arc;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

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
    let panes = supervisor.pane_snapshot();
    let pids: Vec<_> = panes
        .iter()
        .filter_map(|(_, pid, _)| pid.map(Pid::from_u32))
        .collect();
    if pids.is_empty() {
        return;
    }
    let memory = match tokio::task::spawn_blocking(move || {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        system
            .processes()
            .iter()
            .map(|(pid, process)| (pid.as_u32(), process.memory()))
            .collect::<std::collections::HashMap<_, _>>()
    })
    .await
    {
        Ok(memory) => memory,
        Err(error) => {
            eprintln!("memory watchdog failed: {error}");
            return;
        }
    };

    for (id, pid, restart) in panes {
        let memory_bytes = pid.and_then(|pid| memory.get(&pid).copied()).unwrap_or(0);

        if memory_bytes > MEMORY_LIMIT_BYTES {
            if let Err(error) = supervisor.kill_for_restart(id) {
                eprintln!("watchdog failed to kill pane {id}: {error}");
                continue;
            }
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
