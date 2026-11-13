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

/// The wanted order of the workspaces of a snapshot. `spec` is the key `sidebar_order` of the
/// config.
fn wanted(workspaces: &[Workspace], spec: &[String]) -> Vec<String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    order::sorted(workspaces, spec, &home)
}

/// True if the flat list of the workspaces has this order.
fn has_order(workspaces: &[Workspace], order: &[String]) -> bool {
    workspaces
        .iter()
        .map(|workspace| &workspace.workspace_id)
        .eq(order)
}

/// Sort the workspaces of a snapshot. `spec` is the key `sidebar_order` of the config. Return the
/// order that herdr got, or None if the order is already right: then herdr gets no call.
pub fn apply(workspaces: &[Workspace], spec: &[String]) -> Result<Option<Vec<String>>, String> {
    let wanted = wanted(workspaces, spec);
    if has_order(workspaces, &wanted) {
        return Ok(None);
    }
    herdr::move_workspace_block(&wanted, None).map_err(|error| error.to_string())?;
    Ok(Some(wanted))
}

pub fn run(print_only: bool) -> Result<(), String> {
    let snapshot = herdr::snapshot().map_err(|error| error.to_string())?;
    let workspaces = &snapshot.workspaces;
    let spec = config::current().sidebar_order;
    if print_only {
        print!(
            "now, as the sidebar shows it:\n{}new:\n{}",
            describe(&order::screen_order(workspaces), workspaces),
            describe(&wanted(workspaces, &spec), workspaces)
        );
        return Ok(());
    }
    let Some(wanted) = apply(workspaces, &spec)? else {
        println!("the workspaces are already sorted");
        return Ok(());
    };
    // herdr answers also when the order does not change: read the order that herdr has now.
    let after = herdr::snapshot().map_err(|error| error.to_string())?;
    if has_order(&after.workspaces, &wanted) {
        println!("sorted {} workspaces", wanted.len());
        Ok(())
    } else {
        Err("herdr did not give the order that the sort wants".to_string())
    }
}
