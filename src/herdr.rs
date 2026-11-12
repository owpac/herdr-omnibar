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
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(format!("herdr {}: {error}", arguments.join(" "))))?;
    Ok(value["result"].clone())
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
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error::new(format!("herdr api snapshot: {error}")))?;
    serde_json::from_value(value["result"]["snapshot"].clone())
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
