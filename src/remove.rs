//! The removal of a worktree: ctrl+Backspace on a branch row, after the question of the action
//! line.
//!
//! The popup closes first, then the detached process `remove <folder>` runs: the removal can
//! close the workspace of the popup. herdr-worktrunk removes the worktree if it is installed: its
//! command `remove --path` records the removal for its action restore, then closes the agents and
//! the workspace. Without it, `wt remove` removes the worktree, and the omnibar closes the
//! workspace. A herdr notification gives the result.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::create;
use crate::herdr;
use crate::model;

/// The plugin that removes the worktrees with a record for its action restore.
const WORKTRUNK: &str = "herdr-worktrunk";

/// Remove the worktree of a folder, then show the result in a notification.
pub fn run(folder: &Path) -> Result<(), String> {
    let result = remove(folder);
    let (title, body) = match &result {
        Ok(message) => ("herdr-omnibar: worktree removed", message.as_str()),
        Err(error) => ("herdr-omnibar: the removal failed", error.as_str()),
    };
    let _ = herdr::show_notification(title, body);
    result.map(|_| ())
}

fn remove(folder: &Path) -> Result<String, String> {
    if let Some(binary) = worktrunk_remover() {
        let output = Command::new(&binary)
            .args(["remove", "--path"])
            .arg(folder)
            .arg("--close-agents")
            .output()
            .map_err(|error| format!("cannot run {}: {error}", binary.display()))?;
        let text = |bytes: &[u8]| {
            let text = String::from_utf8_lossy(bytes);
            text.trim()
                .trim_start_matches("herdr-worktrunk: ")
                .to_string()
        };
        match output.status.code() {
            Some(0) => return Ok(text(&output.stdout)),
            // An older herdr-worktrunk without the command gives its usage: wt removes the
            // worktree.
            Some(2) => {}
            _ => return Err(text(&output.stderr)),
        }
    }
    remove_with_wt(folder)
}

/// The binary of herdr-worktrunk, if the plugin is installed and enabled, and if it has the
/// command `remove --path` (version 2026.11.33 or later). Its usage names this command.
pub fn worktrunk_remover() -> Option<PathBuf> {
    let plugin = herdr::enabled_plugin(WORKTRUNK)?;
    let binary = plugin.plugin_root?.join("bin").join(WORKTRUNK);
    if !binary.is_file() {
        return None;
    }
    let output = Command::new(&binary).arg("--usage").output().ok()?;
    let usage = String::from_utf8_lossy(&output.stderr);
    usage.contains("remove --path").then_some(binary)
}

/// Remove the worktree with wt, then close its workspace. An agent in the workspace stops the
/// removal: only herdr-worktrunk closes the agents, with a way to resume them.
fn remove_with_wt(folder: &Path) -> Result<String, String> {
    let snapshot = herdr::snapshot().map_err(|error| error.to_string())?;
    // A pane in a subfolder of the worktree also gives its workspace.
    let workspace = snapshot
        .panes
        .iter()
        .find(|pane| {
            pane.cwd
                .as_deref()
                .is_some_and(|cwd| cwd.starts_with(folder))
        })
        .map(|pane| pane.workspace_id.clone());
    if let Some(workspace) = &workspace {
        if snapshot
            .panes
            .iter()
            .any(|pane| &pane.workspace_id == workspace && pane.agent.is_some())
        {
            return Err(
                "An agent works in the workspace of this worktree. Stop it first, or install \
                 herdr-worktrunk: it closes the agents and keeps a record for its action restore."
                    .to_string(),
            );
        }
    }
    let main = model::checkout_of(folder)
        .map(|checkout| checkout.main())
        .ok_or_else(|| format!("{} is not a git checkout", folder.display()))?;
    create::wt(
        &main,
        &[
            OsStr::new("remove"),
            OsStr::new("--foreground"),
            folder.as_os_str(),
        ],
    )?;
    if let Some(workspace) = workspace {
        herdr::close_workspace(&workspace).map_err(|error| error.to_string())?;
    }
    Ok(format!(
        "Removed the worktree {} with wt.",
        folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    ))
}
