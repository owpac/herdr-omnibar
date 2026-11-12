//! herdr-omnibar: one bar to go to an agent, a tab or a workspace of herdr, and to create a tab
//! or a worktree.
//!
//! The actions of herdr-plugin.toml run `open <filter>`: it reads the context of the action, then
//! opens the popup. The popup runs `run`. `keys` shows the keys that the popup receives.

mod config;
mod context;
mod filter;
mod herdr;
mod keys;

use std::env;
use std::process::ExitCode;

use filter::Filter;

const USAGE: &str = "usage: herdr-omnibar open <filter> | run | keys | --version
filters: all, new, agents, tabs, worktrees, folders, actions, keys";

/// The variable that gives the filter of the popup.
pub const FILTER_VARIABLE: &str = "OMNIBAR_FILTER";
/// The variable that gives the context of the action (JSON) to the popup.
pub const CONTEXT_VARIABLE: &str = "OMNIBAR_CONTEXT";

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let result = match arguments.first().map(String::as_str) {
        Some("open") => open(arguments.get(1).map(String::as_str).unwrap_or("all")),
        Some("run") => run(),
        Some("keys") => keys::run().map_err(|error| error.to_string()),
        Some("--version") => {
            println!("herdr-omnibar {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("herdr-omnibar: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Open the popup on a filter. An action runs this command: herdr gives it the context of the
/// action. The popup gets the filter and the context in its environment.
fn open(name: &str) -> Result<(), String> {
    let context = context::Context::from_environment();
    if name == "keys" {
        return herdr::open_plugin_pane("keys", &context, &[]).map_err(|error| error.to_string());
    }
    let filter = Filter::parse(name).ok_or_else(|| format!("unknown filter: {name}\n{USAGE}"))?;
    let entrypoint = config::current().size.entrypoint();
    let raw = env::var(context::CONTEXT_JSON_VARIABLE).unwrap_or_default();
    herdr::open_plugin_pane(
        entrypoint,
        &context,
        &[(FILTER_VARIABLE, filter.name()), (CONTEXT_VARIABLE, &raw)],
    )
    .map_err(|error| error.to_string())
}

/// The popup.
fn run() -> Result<(), String> {
    let filter = env::var(FILTER_VARIABLE)
        .ok()
        .and_then(|name| Filter::parse(&name))
        .unwrap_or(Filter::All);
    Err(format!(
        "the popup ({}) comes in the next commit",
        filter.name()
    ))
}
