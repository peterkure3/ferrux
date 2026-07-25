use serde::{Deserialize, Serialize};

fn default_cols() -> u16 {
    120
}

fn default_rows() -> u16 {
    30
}

/// One pane the daemon should spawn on startup and (optionally)
/// auto-restart if its process dies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPane {
    pub shell: String,
    #[serde(default)]
    pub restart: bool,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

/// The `ferrux.json` project manifest: panes the daemon supervises like
/// an init system, restarting the ones marked `restart` when they exit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub panes: Vec<ManifestPane>,
}
