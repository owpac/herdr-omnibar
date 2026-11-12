//! The git state of the worktrees: with or without changes that are not committed.
//!
//! `git status` takes some milliseconds for each worktree: a thread reads the states in the
//! background, and the rows show them when they are ready.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

/// The state of each worktree that the thread read: true if it has changes.
#[derive(Default)]
pub struct Changes {
    states: Arc<Mutex<HashMap<PathBuf, bool>>>,
    asked: HashSet<PathBuf>,
}

impl Changes {
    /// Read the state of the worktrees that the thread did not read, in the background.
    pub fn ask(&mut self, folders: impl IntoIterator<Item = PathBuf>) {
        let new: Vec<PathBuf> = folders
            .into_iter()
            .filter(|folder| self.asked.insert(folder.clone()))
            .collect();
        if new.is_empty() {
            return;
        }
        let states = Arc::clone(&self.states);
        thread::spawn(move || {
            for folder in new {
                if let Some(changed) = has_changes(&folder) {
                    if let Ok(mut states) = states.lock() {
                        states.insert(folder, changed);
                    }
                }
            }
        });
    }

    /// True if the worktree has changes, false if not, None if the thread did not read it yet.
    pub fn of(&self, folder: &Path) -> Option<bool> {
        self.states.lock().ok()?.get(folder).copied()
    }
}

/// True if the worktree has changes that are not committed. `--no-optional-locks`: git does not
/// write the index, so the check does not change the worktree.
fn has_changes(folder: &Path) -> Option<bool> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(folder)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(!output.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn the_thread_reads_the_state_of_this_repository() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut changes = Changes::default();
        changes.ask([here.clone()]);
        let start = Instant::now();
        while changes.of(&here).is_none() && start.elapsed() < Duration::from_secs(10) {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(changes.of(&here), has_changes(&here));
        assert_eq!(changes.of(Path::new("/missing")), None);
    }
}
