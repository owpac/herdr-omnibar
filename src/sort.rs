//! The command `herdr-omnibar sort`: sort the workspaces of the herdr sidebar.
//! `sort --print` prints the old and the new order, and changes nothing.

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

pub fn run(print_only: bool) -> Result<(), String> {
    let snapshot = herdr::snapshot().map_err(|error| error.to_string())?;
    let workspaces = &snapshot.workspaces;
    let flat: Vec<String> = workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.clone())
        .collect();
    let wanted = order::sorted(workspaces);
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
    herdr::move_workspace_block(&wanted, None).map_err(|error| error.to_string())?;
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
