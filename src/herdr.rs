//! The calls to herdr, through its CLI. herdr gives the path of its binary in HERDR_BIN_PATH.

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::context::Context;

/// The id of the plugin in herdr-plugin.toml.
pub const PLUGIN_ID: &str = "herdr-omnibar";

/// A herdr command that failed: the code and the message of herdr, when herdr gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: Option<String>,
    pub message: String,
}

impl Error {
    fn new(message: impl Into<String>) -> Error {
        Error {
            code: None,
            message: message.into(),
        }
    }

    /// The error of the standard error of herdr: {"error":{"code":…,"message":…}}, or its text.
    fn of_stderr(stderr: &str, arguments: &[&str]) -> Error {
        let parsed: Option<serde_json::Value> = serde_json::from_str(stderr.trim()).ok();
        let field = |name: &str| {
            parsed
                .as_ref()
                .and_then(|json| json["error"][name].as_str())
                .map(str::to_string)
        };
        match (field("code"), field("message")) {
            (code, Some(message)) => Error { code, message },
            _ if stderr.trim().is_empty() => {
                Error::new(format!("herdr {} failed", arguments.join(" ")))
            }
            _ => Error::new(stderr.trim()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn herdr_executable() -> PathBuf {
    env::var_os("HERDR_BIN_PATH")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("herdr"))
}

/// Run a herdr command. Return its standard output.
fn run(arguments: &[&str]) -> Result<String, Error> {
    let output = Command::new(herdr_executable())
        .args(arguments)
        .output()
        .map_err(|error| Error::new(format!("cannot run herdr: {error}")))?;
    if !output.status.success() {
        return Err(Error::of_stderr(
            &String::from_utf8_lossy(&output.stderr),
            arguments,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run a herdr command that answers in JSON. Return the field `result`.
fn run_json(arguments: &[&str]) -> Result<serde_json::Value, Error> {
    let text = run(arguments)?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(format!("herdr {}: {error}", arguments.join(" "))))?;
    Ok(value["result"].take())
}

fn field(value: &serde_json::Value, pointer: &str, arguments: &[&str]) -> Result<String, Error> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::new(format!("herdr {}: no {pointer}", arguments.join(" "))))
}

/// The herdr snapshot: the workspaces, the tabs, the panes and the agents.
pub fn snapshot() -> Result<crate::model::Snapshot, Error> {
    let text = run(&["api", "snapshot"])?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(format!("herdr api snapshot: {error}")))?;
    serde_json::from_value(value["result"]["snapshot"].take())
        .map_err(|error| Error::new(format!("herdr api snapshot: {error}")))
}

/// Focus a tab, and its workspace.
pub fn focus_tab(tab_id: &str) -> Result<(), Error> {
    run(&["tab", "focus", tab_id]).map(|_| ())
}

/// Focus the pane of an agent.
pub fn focus_agent(pane_id: &str) -> Result<(), Error> {
    run(&["agent", "focus", pane_id]).map(|_| ())
}

/// Focus a workspace, on its active tab.
pub fn focus_workspace(workspace_id: &str) -> Result<(), Error> {
    run(&["workspace", "focus", workspace_id]).map(|_| ())
}

/// The last lines of the text of a pane.
pub fn read_pane(pane_id: &str, lines: usize) -> Result<String, Error> {
    let lines = lines.to_string();
    run(&[
        "pane", "read", pane_id, "--lines", &lines, "--source", "visible",
    ])
}

/// Create a tab in a workspace, with its folder and an optional label, and focus it. Return the id
/// of its first pane.
pub fn create_tab(workspace_id: &str, folder: &Path, label: Option<&str>) -> Result<String, Error> {
    let folder = folder.to_string_lossy();
    let mut arguments = vec![
        "tab",
        "create",
        "--workspace",
        workspace_id,
        "--cwd",
        &folder,
        "--focus",
    ];
    if let Some(label) = label {
        arguments.extend(["--label", label]);
    }
    let result = run_json(&arguments)?;
    field(&result, "/root_pane/pane_id", &arguments)
}

/// Create a workspace in a folder, and focus it. Return the ids of its first tab and of its
/// first pane.
pub fn create_workspace(folder: &Path, label: &str) -> Result<(String, String), Error> {
    let folder = folder.to_string_lossy();
    let arguments = [
        "workspace",
        "create",
        "--cwd",
        &folder,
        "--label",
        label,
        "--focus",
    ];
    let result = run_json(&arguments)?;
    Ok((
        field(&result, "/tab/tab_id", &arguments)?,
        field(&result, "/root_pane/pane_id", &arguments)?,
    ))
}

/// Open the workspace of a worktree (or find its open workspace), and focus it. Return the id of
/// its first pane. herdr refuses a --cwd in a linked worktree: give the main checkout.
pub fn open_worktree(main: &Path, worktree: &Path) -> Result<String, Error> {
    let main = main.to_string_lossy();
    let worktree = worktree.to_string_lossy();
    let arguments = [
        "worktree", "open", "--cwd", &main, "--path", &worktree, "--focus",
    ];
    let result = run_json(&arguments)?;
    field(&result, "/root_pane/pane_id", &arguments)
}

/// Show a herdr notification.
pub fn show_notification(title: &str, body: &str) -> Result<(), Error> {
    run(&["notification", "show", title, "--body", body]).map(|_| ())
}

/// An action of a herdr plugin.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct PluginAction {
    pub action_id: String,
    pub plugin_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
}

/// The actions of all the herdr plugins.
pub fn plugin_actions() -> Result<Vec<PluginAction>, Error> {
    let arguments = ["plugin", "action", "list"];
    let result = run_json(&arguments)?;
    serde_json::from_value(result["actions"].clone())
        .map_err(|error| Error::new(format!("herdr plugin action list: {error}")))
}

/// A plugin of herdr, from `herdr plugin list --json`.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Plugin {
    pub plugin_id: String,
    #[serde(default)]
    pub enabled: bool,
    /// The folder of the installed plugin.
    #[serde(default)]
    pub plugin_root: Option<PathBuf>,
}

/// The process information of a pane: its shell and its foreground process group.
pub fn process_info(pane_id: &str) -> Result<serde_json::Value, Error> {
    let result = run_json(&["pane", "process-info", "--pane", pane_id])?;
    Ok(result["process_info"].clone())
}

/// Close a workspace, with its tabs.
pub fn close_workspace(workspace_id: &str) -> Result<(), Error> {
    run(&["workspace", "close", workspace_id]).map(|_| ())
}

/// The installed plugins.
pub fn plugins() -> Result<Vec<Plugin>, Error> {
    let arguments = ["plugin", "list", "--json"];
    let result = run_json(&arguments)?;
    serde_json::from_value(result["plugins"].clone())
        .map_err(|error| Error::new(format!("herdr plugin list: {error}")))
}

/// The plugin with this id in a list of plugins, if it is enabled.
pub fn enabled<'a>(plugins: &'a [Plugin], plugin_id: &str) -> Option<&'a Plugin> {
    plugins
        .iter()
        .find(|plugin| plugin.plugin_id == plugin_id && plugin.enabled)
}

/// The installed plugin with this id, if it is enabled. None if herdr does not answer.
pub fn enabled_plugin(plugin_id: &str) -> Option<Plugin> {
    enabled(&plugins().ok()?, plugin_id).cloned()
}

/// Run an action of a herdr plugin.
pub fn invoke_action(plugin_id: &str, action_id: &str) -> Result<(), Error> {
    run(&[
        "plugin", "action", "invoke", action_id, "--plugin", plugin_id,
    ])
    .map(|_| ())
}

/// Split a pane: a new pane to the right of a wide pane, below a tall pane. A cell is about 2
/// times higher than wide. The new pane gets the focus. Return its id.
pub fn split_pane(pane_id: &str, folder: &Path) -> Result<String, Error> {
    let layout = run_json(&["pane", "layout", "--pane", pane_id])?;
    let rect = layout["layout"]["panes"]
        .as_array()
        .and_then(|panes| panes.iter().find(|pane| pane["pane_id"] == pane_id))
        .map(|pane| pane["rect"].clone())
        .unwrap_or_default();
    let direction = split_direction(
        rect["width"].as_u64().unwrap_or(0),
        rect["height"].as_u64().unwrap_or(0),
    );
    let folder = folder.to_string_lossy();
    let result = run_json(&[
        "pane",
        "split",
        pane_id,
        "--direction",
        direction,
        "--cwd",
        &folder,
        "--focus",
    ])?;
    result["pane"]["pane_id"]
        .as_str()
        .or_else(|| result["pane_id"].as_str())
        .map(str::to_string)
        .ok_or_else(|| Error::new("herdr pane split gave no pane id".to_string()))
}

/// The direction of a split of a pane of this size, in cells: right for a wide pane, down for a
/// tall pane. A cell is about 2.2 times higher than wide.
fn split_direction(width: u64, height: u64) -> &'static str {
    if width * 10 >= height * 22 {
        "right"
    } else {
        "down"
    }
}

/// Rename a pane.
pub fn rename_pane(pane_id: &str, label: &str) -> Result<(), Error> {
    run(&["pane", "rename", pane_id, label]).map(|_| ())
}

/// Close a pane.
pub fn close_pane(pane_id: &str) -> Result<(), Error> {
    run(&["pane", "close", pane_id]).map(|_| ())
}

/// Close a tab, with its panes.
pub fn close_tab(tab_id: &str) -> Result<(), Error> {
    run(&["tab", "close", tab_id]).map(|_| ())
}

/// Rename a tab.
pub fn rename_tab(tab_id: &str, label: &str) -> Result<(), Error> {
    run(&["tab", "rename", tab_id, label]).map(|_| ())
}

/// Start an agent in a pane, with these arguments for the agent. herdr types the command in the
/// shell of the pane: while the shell starts, herdr answers agent_pane_busy, so try again.
pub fn start_agent(
    name: &str,
    kind: &str,
    pane_id: &str,
    agent_arguments: &[&str],
) -> Result<(), Error> {
    let mut arguments = vec![
        "agent",
        "start",
        name,
        "--kind",
        kind,
        "--pane",
        pane_id,
        "--timeout",
        "30000",
        "--",
    ];
    arguments.extend_from_slice(agent_arguments);
    for _ in 0..20 {
        match run(&arguments) {
            Ok(_) => return Ok(()),
            Err(error) if error.code.as_deref() == Some("agent_not_ready") => return Ok(()),
            Err(error) if error.code.as_deref() == Some("agent_pane_busy") => {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(error) => return Err(error),
        }
    }
    Err(Error::new(format!(
        "the shell of the pane {pane_id} did not get ready"
    )))
}

/// Open a pane of herdr-plugin.toml in the folder of the context, with these environment
/// variables. The manifest gives the placement of the pane: a popup.
pub fn open_plugin_pane(
    entrypoint: &str,
    context: &Context,
    environment: &[(&str, &str)],
) -> Result<(), Error> {
    let folder = context.folder();
    let folder = folder.to_string_lossy();
    let mut arguments = vec![
        "plugin",
        "pane",
        "open",
        "--plugin",
        PLUGIN_ID,
        "--entrypoint",
        entrypoint,
        "--cwd",
        &folder,
        "--focus",
    ];
    let variables: Vec<String> = environment
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    for variable in &variables {
        arguments.push("--env");
        arguments.push(variable);
    }
    run(&arguments).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_pane_splits_to_the_right_and_a_tall_pane_down() {
        assert_eq!(split_direction(197, 55), "right");
        assert_eq!(split_direction(100, 55), "down");
        assert_eq!(split_direction(0, 0), "right");
    }
}
