use std::collections::{HashMap, HashSet};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

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

    /// Enumerate once for all panes. Agent classification needs only names
    /// and parents, not CPU, command lines, environment or disk accounting.
    pub fn detect_many(&self, roots: &[u32]) -> Vec<Option<AgentKind>> {
        if roots.is_empty() {
            return Vec::new();
        }
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for (pid, process) in system.processes() {
            if let Some(parent) = process.parent() {
                children.entry(parent).or_default().push(*pid);
            }
        }
        roots
            .iter()
            .map(|root| detect_in(&system, &children, *root))
            .collect()
    }
}

impl AgentDetector for ProcessNameDetector {
    fn detect(&self, root_pid: u32) -> Option<AgentKind> {
        self.detect_many(&[root_pid])[0]
    }
}

/// Separated from `detect` so it can be unit-tested against a snapshot
/// without depending on real OS process state.
fn detect_in(
    system: &System,
    children: &HashMap<Pid, Vec<Pid>>,
    root_pid: u32,
) -> Option<AgentKind> {
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
        if let Some(descendants) = children.get(&pid) {
            stack.extend(descendants);
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
