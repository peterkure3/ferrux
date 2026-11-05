use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitInfo {
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
}

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(directory)
        .args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn read_git(directory: &Path) -> GitInfo {
    let Some(root) = git(directory, &["rev-parse", "--show-toplevel"]) else {
        return GitInfo::default();
    };
    GitInfo {
        repository: Path::new(&root)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned()),
        branch: git(directory, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .or_else(|| git(directory, &["rev-parse", "--short", "HEAD"])),
        dirty: git(
            directory,
            &[
                "-c",
                "core.fsmonitor=false",
                "status",
                "--porcelain=v1",
                "--untracked-files=normal",
            ],
        )
        .map(|s| !s.is_empty()),
    }
}

pub struct GitMonitor {
    pub receiver: watch::Receiver<GitInfo>,
    task: tokio::task::JoinHandle<()>,
}
impl GitMonitor {
    pub fn start(directory: PathBuf) -> Self {
        let (sender, receiver) = watch::channel(GitInfo::default());
        let task = tokio::spawn(async move {
            loop {
                let directory = directory.clone();
                if let Ok(info) = tokio::task::spawn_blocking(move || read_git(&directory)).await {
                    sender.send_if_modified(|current| {
                        if *current == info {
                            false
                        } else {
                            *current = info;
                            true
                        }
                    });
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        Self { receiver, task }
    }
}
impl Drop for GitMonitor {
    fn drop(&mut self) {
        self.task.abort();
    }
}
