use crate::domain::agent::AgentKind;

/// Identifies what's running under a pane's shell by inspecting the OS
/// process tree — `None` means no known agent process was found among
/// its descendants (just a plain shell, or an unrecognized program).
pub trait AgentDetector {
    fn detect(&self, root_pid: u32) -> Option<AgentKind>;
}
