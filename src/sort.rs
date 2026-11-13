//! The command `herdr-omnibar sort`: sort the workspaces of the herdr sidebar.
//! `sort --print` prints the old and the new order, and changes nothing.

use std::path::PathBuf;

use crate::config;
use crate::herdr;
use crate::model::Workspace;
use crate::order;

/// The text of an order: one line for each workspace, with its position.
fn describe(order: &[String], workspaces: &[Workspace]) -> String {
    order
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let label = workspaces
                .iter()
                .find(|workspace| &workspace.workspace_id == id)
                .map_or("?", |workspace| workspace.label.as_str());
            format!("  {:>2}. {id}  {label}\n", index + 1)
        })
        .collect()
}

/// The wanted order of the workspaces of a snapshot, with the `sort_order` of the config.
fn wanted(workspaces: &[Workspace]) -> Vec<String> {
    let config = config::current();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    order::sorted(workspaces, &config.sort_order, &home)
}

/// Sort the workspaces of a snapshot. Return the number of workspaces, or None if the order is
/// already right: then herdr gets no call.
pub fn apply(workspaces: &[Workspace]) -> Result<Option<usize>, String> {
    let wanted = wanted(workspaces);
    let flat: Vec<&String> = workspaces.iter().map(|w| &w.workspace_id).collect();
    if flat.iter().copied().eq(wanted.iter()) {
        return Ok(None);
    }
    herdr::move_workspace_block(&wanted, None).map_err(|error| error.to_string())?;
    Ok(Some(wanted.len()))
}

pub fn run(print_only: bool) -> Result<(), String> {
    let snapshot = herdr::snapshot().map_err(|error| error.to_string())?;
    let workspaces = &snapshot.workspaces;
    let flat: Vec<String> = workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.clone())
        .collect();
    let wanted = wanted(workspaces);
    if print_only {
        let shown = order::screen_order(workspaces);
        print!(
            "now, as the sidebar shows it:\n{}new:\n{}",
            describe(&shown, workspaces),
            describe(&wanted, workspaces)
        );
        return Ok(());
    }
    if flat == wanted {
        println!("the workspaces are already sorted");
        return Ok(());
    }
    apply(workspaces)?;
    // herdr answers also when the order does not change: read the order that herdr has now.
    let after = herdr::snapshot().map_err(|error| error.to_string())?;
    let now: Vec<String> = after
        .workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.clone())
        .collect();
    if now == wanted {
        println!("sorted {} workspaces", wanted.len());
        Ok(())
    } else {
        Err("herdr did not give the order that the sort wants".to_string())
    }
}
