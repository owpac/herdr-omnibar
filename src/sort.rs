//! The command `herdr-omnibar sort`: sort the workspaces of the herdr sidebar.
//! `sort --print` prints the old and the new order, and changes nothing.

use crate::config;
use crate::herdr;
use crate::model::Workspace;
use crate::places;
use crate::sidebar;

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
    sidebar::sorted(workspaces, spec, &places::home_folder())
}

/// The wanted order, or None if the sidebar already shows it. The flat list can have another
/// order: the sidebar shows a group at its first member, with the main workspace first.
fn change(workspaces: &[Workspace], spec: &[String]) -> Option<Vec<String>> {
    let wanted = wanted(workspaces, spec);
    (sidebar::screen_order(workspaces) != wanted).then_some(wanted)
}

/// Sort the workspaces of a snapshot. `spec` is the key `sidebar_order` of the config. Return the
/// order that herdr got, or None if the sidebar already shows this order: then herdr gets no call.
pub fn apply(workspaces: &[Workspace], spec: &[String]) -> Result<Option<Vec<String>>, String> {
    let Some(wanted) = change(workspaces, spec) else {
        return Ok(None);
    };
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
            describe(&sidebar::screen_order(workspaces), workspaces),
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
    if sidebar::screen_order(&after.workspaces) == wanted {
        println!("sorted {} workspaces", wanted.len());
        Ok(())
    } else {
        Err("herdr did not give the order that the sort wants".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::tests::workspace;

    fn spec() -> Vec<String> {
        vec!["git".to_string(), "other".to_string()]
    }

    #[test]
    fn the_sort_makes_no_call_if_the_sidebar_already_shows_the_wanted_order() {
        // The linked worktree comes first in the flat list, but the sidebar shows the group with
        // its main workspace first: the order is right.
        let grouped = vec![
            workspace("w1", "atlas.fix", Some(("/a", "atlas", true))),
            workspace("w2", "atlas", Some(("/a", "atlas", false))),
        ];
        assert_eq!(change(&grouped, &spec()), None);
        // A workspace without git before a git workspace: the sort moves it.
        let mixed = vec![
            workspace("w3", "notes", None),
            workspace("w2", "atlas", Some(("/a", "atlas", false))),
        ];
        assert_eq!(
            change(&mixed, &spec()),
            Some(vec!["w2".to_string(), "w3".to_string()])
        );
    }
}
