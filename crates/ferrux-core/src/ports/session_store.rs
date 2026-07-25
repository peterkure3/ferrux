use crate::domain::workspace::Workspace;

pub trait SessionStore {
    type Error;

    fn save(&self, workspace: &Workspace) -> Result<(), Self::Error>;
    fn load(&self, name: &str) -> Result<Workspace, Self::Error>;
    /// Names of all saved workspaces, for `workspace switching`.
    fn list(&self) -> Result<Vec<String>, Self::Error>;
}
