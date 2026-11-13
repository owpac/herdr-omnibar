//! herdr-omnibar: one bar to go to an agent, a tab or a workspace of herdr, and to create a tab
//! or a worktree.
//!
//! The actions of herdr-plugin.toml run `open <filter>`: it reads the context of the action, then
//! opens the popup. The popup runs `run`. `keys` shows the keys that the popup receives. `watch`
//! starts the watcher of the automatic titles of the tabs. `sort` sorts the workspaces of the
//! herdr sidebar.

mod app;
mod changes;
mod config;
mod context;
mod create;
mod demo;
mod filter;
mod fuzzy;
mod herdr;
mod icons;
mod keys;
mod model;
mod order;
mod places;
mod pulls;
mod rank;
mod recent;
mod remove;
mod rows;
mod sort;
mod theme;
mod times;
mod ui;
mod watch;

use std::env;
use std::process::ExitCode;

use filter::Filter;

const USAGE: &str = "usage: herdr-omnibar open <filter> | run | execute | invoke <plugin> <action> | fork <tab> <pane> | keys | list | watch [--run] | titles | sort [--print] | remove <worktree> | --version
filters: all, new, agents, tabs, workspaces, worktrees, folders, actions, panes, keys";

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
        Some("list") => list(),
        Some("watch") if arguments.get(1).is_some_and(|argument| argument == "--run") => {
            watch::run()
        }
        Some("watch") => watch::start(),
        Some("titles") => watch::print(),
        Some("sort") => match arguments.get(1).map(String::as_str) {
            None => sort::run(false),
            Some("--print") => sort::run(true),
            Some(_) => Err(USAGE.to_string()),
        },
        Some("remove") => match arguments.get(1) {
            Some(folder) => remove::run(std::path::Path::new(folder)),
            None => Err(USAGE.to_string()),
        },
        Some("execute") => create::execute_from_environment(),
        Some("invoke") => match (arguments.get(1), arguments.get(2)) {
            (Some(plugin), Some(action)) => create::invoke_later(plugin, action),
            _ => Err(USAGE.to_string()),
        },
        Some("fork") => match (arguments.get(1), arguments.get(2)) {
            (Some(tab), Some(pane)) => create::fork_later(tab, pane),
            _ => Err(USAGE.to_string()),
        },
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
    // Each opening starts the watcher of the titles again, if it stopped.
    if let Err(error) = watch::start() {
        eprintln!("herdr-omnibar: {error}");
    }
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

/// Print the workspaces, the agents and the tabs, as the popup sees them. A tool to check the
/// model in a running herdr.
fn list() -> Result<(), String> {
    let mut model = model::load().map_err(|error| error.to_string())?;
    // The order of the popup: the key list_order of the config. The list reads the times of use,
    // and writes none.
    let config = config::current();
    let recent = if config.orders_by_visit() {
        recent::load(&config::state_dir())
    } else {
        recent::Recent::new()
    };
    rank::Ranker::new(
        &model,
        &config.list_order,
        &recent,
        None,
        model.focused_workspace_id.as_deref(),
    )
    .sort(&mut model);
    let focused = model.focused_workspace_id.as_deref().unwrap_or("");
    for space in &model.spaces {
        let folder = space
            .folder
            .as_ref()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mark = if space.workspace.workspace_id == focused {
            "*"
        } else {
            " "
        };
        println!(
            "{mark} workspace {:<6} {} {:<40} {folder}",
            space.workspace.workspace_id,
            space.workspace.agent_status.glyph(),
            space.label()
        );
    }
    for agent in &model.agents {
        println!(
            "  agent {:<8} {:<8} {:<6} {} {:<9} {:<7} {:<40} {}",
            agent.pane_id,
            agent.tab_id,
            agent.workspace_id,
            agent.status.glyph(),
            agent.status.label(),
            agent.kind,
            agent.name,
            agent.place
        );
    }
    for tab in &model.tabs {
        println!(
            "  tab   {:<8} {:<6} {:<40} {:<30} {}",
            tab.tab_id, tab.workspace_id, tab.name, tab.command, tab.place
        );
    }
    Ok(())
}

/// The popup.
fn run() -> Result<(), String> {
    let filter = env::var(FILTER_VARIABLE)
        .ok()
        .and_then(|name| Filter::parse(&name))
        .unwrap_or(Filter::All);
    app::run(filter, context::Context::of_popup())
}
