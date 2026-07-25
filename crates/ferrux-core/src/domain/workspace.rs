use serde::{Deserialize, Serialize};

use super::session::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: PaneId,
    pub session: SessionId,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitNode {
    Leaf(PaneId),
    Split {
        direction: SplitDirection,
        ratio_percent: u8,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

/// What's needed to respawn a pane on a fresh daemon connection. The live
/// `PaneId` a pane had when a workspace was saved is not reusable after a
/// restart, only the shape of the tree and each leaf's shell is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRecord {
    pub id: PaneId,
    pub shell: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub root: Option<SplitNode>,
    pub panes: Vec<PaneRecord>,
}

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            root: None,
            panes: Vec::new(),
        }
    }

    pub fn shell_for(&self, id: PaneId) -> Option<&str> {
        self.panes
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.shell.as_str())
    }
}
