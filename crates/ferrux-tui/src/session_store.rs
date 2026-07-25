use std::fs;
use std::io;
use std::path::PathBuf;

use ferrux_core::domain::workspace::Workspace;
use ferrux_core::ports::session_store::SessionStore;

/// Saves/loads `Workspace` layouts as TOML files under
/// `~/.ferrux/sessions/<name>.toml`, matching the `~/.ferrux/config.toml`
/// convention.
pub struct TomlSessionStore {
    dir: PathBuf,
}

impl TomlSessionStore {
    pub fn new() -> io::Result<Self> {
        let home = directories::BaseDirs::new()
            .ok_or_else(|| io::Error::other("could not determine home directory"))?
            .home_dir()
            .to_path_buf();
        Self::with_dir(home.join(".ferrux").join("sessions"))
    }

    fn with_dir(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.toml"))
    }
}

impl SessionStore for TomlSessionStore {
    type Error = io::Error;

    fn save(&self, workspace: &Workspace) -> Result<(), Self::Error> {
        let toml = toml::to_string_pretty(workspace).map_err(io::Error::other)?;
        fs::write(self.path_for(&workspace.name), toml)
    }

    fn load(&self, name: &str) -> Result<Workspace, Self::Error> {
        let contents = fs::read_to_string(self.path_for(name))?;
        toml::from_str(&contents).map_err(io::Error::other)
    }

    fn list(&self) -> Result<Vec<String>, Self::Error> {
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrux_core::domain::workspace::{PaneId, PaneRecord, SplitDirection, SplitNode};

    fn temp_store() -> TomlSessionStore {
        let dir = std::env::temp_dir().join(format!(
            "ferrux-session-store-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        TomlSessionStore::with_dir(dir).unwrap()
    }

    fn sample_workspace() -> Workspace {
        Workspace {
            name: "roundtrip".to_string(),
            root: Some(SplitNode::Split {
                direction: SplitDirection::Vertical,
                ratio_percent: 50,
                first: Box::new(SplitNode::Leaf(PaneId(1))),
                second: Box::new(SplitNode::Leaf(PaneId(2))),
            }),
            panes: vec![
                PaneRecord {
                    id: PaneId(1),
                    shell: "cmd.exe".to_string(),
                },
                PaneRecord {
                    id: PaneId(2),
                    shell: "pwsh.exe".to_string(),
                },
            ],
        }
    }

    #[test]
    fn save_then_load_round_trips_the_workspace() {
        let store = temp_store();
        let workspace = sample_workspace();

        store.save(&workspace).unwrap();
        let loaded = store.load(&workspace.name).unwrap();

        assert_eq!(loaded, workspace);
    }

    #[test]
    fn list_returns_saved_workspace_names() {
        let store = temp_store();
        store.save(&sample_workspace()).unwrap();

        let mut other = sample_workspace();
        other.name = "another".to_string();
        store.save(&other).unwrap();

        assert_eq!(store.list().unwrap(), vec!["another", "roundtrip"]);
    }

    #[test]
    fn load_missing_workspace_errors() {
        let store = temp_store();
        assert!(store.load("does-not-exist").is_err());
    }
}
