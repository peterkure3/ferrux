use std::collections::HashSet;

use sysinfo::{Pid, ProcessesToUpdate, System};

use ferrux_core::domain::agent::AgentKind;
use ferrux_core::ports::agent_detector::AgentDetector;

/// Classifies a pane's shell by walking its descendant processes and
/// matching known agent CLI process names.
pub struct ProcessNameDetector;

impl Default for ProcessNameDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessNameDetector {
    pub fn new() -> Self {
        Self
    }
}

impl AgentDetector for ProcessNameDetector {
    fn detect(&self, root_pid: u32) -> Option<AgentKind> {
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        detect_in(&system, root_pid)
    }
}

/// Separated from `detect` so it can be unit-tested against a snapshot
/// without depending on real OS process state.
fn detect_in(system: &System, root_pid: u32) -> Option<AgentKind> {
    let root = Pid::from_u32(root_pid);
    let mut stack = vec![root];
    let mut visited = HashSet::new();

    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        if pid != root {
            if let Some(process) = system.processes().get(&pid) {
                if let Some(kind) = classify(&process.name().to_string_lossy()) {
                    return Some(kind);
                }
            }
        }
        for (child_pid, process) in system.processes() {
            if process.parent() == Some(pid) {
                stack.push(*child_pid);
            }
        }
    }

    None
}

fn classify(process_name: &str) -> Option<AgentKind> {
    let lower = process_name.to_lowercase();
    if lower.contains("claude") {
        Some(AgentKind::ClaudeCode)
    } else if lower.contains("codex") {
        Some(AgentKind::Codex)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_claude_process_names() {
        assert_eq!(classify("claude.exe"), Some(AgentKind::ClaudeCode));
        assert_eq!(classify("claude"), Some(AgentKind::ClaudeCode));
        assert_eq!(classify("Claude.exe"), Some(AgentKind::ClaudeCode));
    }

    #[test]
    fn classifies_codex_process_names() {
        assert_eq!(classify("codex.exe"), Some(AgentKind::Codex));
    }

    #[test]
    fn unrelated_process_names_classify_to_none() {
        assert_eq!(classify("cmd.exe"), None);
        assert_eq!(classify("pwsh.exe"), None);
        assert_eq!(classify("notepad.exe"), None);
    }
}
