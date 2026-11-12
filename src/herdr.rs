//! The calls to herdr, through its CLI. herdr gives the path of its binary in HERDR_BIN_PATH.

use std::env;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;

use crate::context::Context;

/// The id of the plugin in herdr-plugin.toml.
pub const PLUGIN_ID: &str = "herdr-omnibar";

/// A herdr command that failed, with the message of herdr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
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
        .map_err(|error| Error(format!("cannot run herdr: {error}")))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error(if message.is_empty() {
            format!("herdr {} failed", arguments.join(" "))
        } else {
            message
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The herdr snapshot: the workspaces, the tabs, the panes and the agents.
pub fn snapshot() -> Result<crate::model::Snapshot, Error> {
    let text = run(&["api", "snapshot"])?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| Error(format!("herdr api snapshot: {error}")))?;
    serde_json::from_value(value["result"]["snapshot"].clone())
        .map_err(|error| Error(format!("herdr api snapshot: {error}")))
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
