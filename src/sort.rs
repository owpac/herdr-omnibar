//! The sort of the workspaces of the herdr sidebar with the key `sidebar_order`, and the command
//! `herdr-omnibar sort`.
//! `sort --print` prints the old and the new order, and changes nothing.

use std::collections::HashMap;
use std::path::Path;

use crate::config;
use crate::herdr;
use crate::model::Workspace;
use crate::places;
use crate::sidebar::{self, is_linked, repo_key};

/// The entries of `sidebar_order` by default: the git workspaces, then the others.
pub const DEFAULT_SIDEBAR_ORDER: [&str; 2] = ["git", "other"];

/// The key of the default order of the sort:
///
/// 1. The git workspaces first, then the workspaces without git.
/// 2. The git workspaces in groups by `repo_key`. The groups go by `repo_name`, then `repo_key`.
/// 3. In a group, the main workspace first, then the linked worktrees by label. A group without
///    a main workspace stays together too.
/// 4. The workspaces without git go by label.
///
/// The comparisons of the names and of the labels ignore the case. The comparison of `repo_key`
/// keeps the case: 2 keys with a different case are 2 groups, and each group stays together. The
/// flat order breaks a tie.
type DefaultKey = (u8, String, String, bool, String, usize);

fn default_key(index: usize, workspace: &Workspace) -> DefaultKey {
    let label = workspace.label.to_lowercase();
    match &workspace.worktree {
        Some(worktree) => (
            0,
            worktree.repo_name.to_lowercase(),
            worktree.repo_key.clone(),
            is_linked(workspace),
            label,
            index,
        ),
        None => (1, String::new(), String::new(), false, label, index),
    }
}

/// True if an entry of `sidebar_order` is a folder: it has a `/`. Any other entry but "git" and
/// "other" is the label of a workspace.
fn is_folder(entry: &str) -> bool {
    entry.contains('/')
}

/// True if the entry of `sidebar_order` takes the members of a unit: a git group, or a workspace
/// without git.
fn takes(entry: &str, members: &[&Workspace], home: &Path) -> bool {
    let git = members.iter().any(|member| member.worktree.is_some());
    match entry {
        "git" => git,
        "other" => !git,
        _ if is_folder(entry) => {
            let folder = crate::places::expand_in(entry, home);
            members.iter().any(|member| {
                member
                    .worktree
                    .as_ref()
                    .and_then(|worktree| worktree.repo_root.as_deref())
                    .is_some_and(|root| root.starts_with(&folder))
            })
        }
        // One member with the label takes the full group.
        _ => members.iter().any(|member| member.label == entry),
    }
}

/// The wanted order of the sort. `spec` is the list `sidebar_order`. Each entry takes the
/// workspaces that no earlier entry took:
///
/// - "git": the git workspaces. "other": the workspaces without git.
/// - An entry with a `/`: the git projects with a `repo_root` in this folder, at any depth.
/// - Any other entry: the workspace with this label. For a git workspace, its full group goes
///   there.
///
/// A git workspace moves with its group (the same `repo_key`). In each entry, the order is the
/// default order (`default_key`). The workspaces that no entry takes go at the end, in the
/// default order.
pub fn sorted(workspaces: &[Workspace], spec: &[String], home: &Path) -> Vec<String> {
    // The units: the workspaces of one `repo_key` go together. Any other workspace is alone.
    let mut groups: HashMap<&str, Vec<&Workspace>> = HashMap::new();
    for workspace in workspaces {
        if let Some(key) = repo_key(workspace) {
            groups.entry(key).or_default().push(workspace);
        }
    }
    let bucket = |workspace: &Workspace| {
        let members = match repo_key(workspace) {
            Some(key) => groups[key].as_slice(),
            None => std::slice::from_ref(&workspace),
        };
        spec.iter()
            .position(|entry| takes(entry, members, home))
            .unwrap_or(spec.len())
    };
    let mut order: Vec<(usize, &Workspace)> = workspaces.iter().enumerate().collect();
    order.sort_by_cached_key(|&(index, workspace)| {
        (bucket(workspace), default_key(index, workspace))
    });
    order
        .into_iter()
        .map(|(_, workspace)| workspace.workspace_id.clone())
        .collect()
}

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
    sorted(workspaces, spec, &places::home_folder())
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
    use crate::sidebar::tests::{flat, workspace};

    fn sorted_by_default(workspaces: &[Workspace]) -> Vec<String> {
        by(&DEFAULT_SIDEBAR_ORDER, workspaces)
    }

    /// A git workspace with its repository root.
    fn project(id: &str, label: &str, name: &str, root: &str, linked: bool) -> Workspace {
        serde_json::from_value(serde_json::json!({
            "workspace_id": id, "label": label,
            "worktree": {
                "repo_key": format!("{root}/.git"), "repo_name": name,
                "repo_root": root, "is_linked_worktree": linked
            }
        }))
        .unwrap()
    }

    fn by(spec: &[&str], workspaces: &[Workspace]) -> Vec<String> {
        let spec: Vec<String> = spec.iter().map(|e| e.to_string()).collect();
        sorted(workspaces, &spec, Path::new("/home/u"))
    }

    fn tree() -> Vec<Workspace> {
        vec![
            workspace("w1", "notes", None),
            workspace("w2", "~", None),
            project("w3", "zed", "zed", "/home/u/Workspace/keplair/zed", false),
            project("w4", "app", "app", "/home/u/Workspace/owpac/app", false),
            project("w5", "tool", "tool", "/home/u/Workspace/tool", false),
            project("w6", "fix", "app", "/home/u/Workspace/owpac/app", true),
            project("w7", "misc", "misc", "/elsewhere/misc", false),
        ]
    }

    #[test]
    fn a_label_entry_takes_the_workspace_with_this_label_and_a_folder_entry_takes_projects() {
        // "~" is a label, not the home folder.
        assert_eq!(
            by(&["~", "~/Workspace/owpac", "git", "other"], &tree()),
            ["w2", "w4", "w6", "w7", "w5", "w3", "w1"]
        );
    }

    #[test]
    fn the_first_of_2_nested_folders_wins_and_the_folder_takes_any_depth() {
        assert_eq!(
            by(&["~/Workspace", "~/Workspace/owpac", "other"], &tree()),
            // The first folder takes app (with fix), tool and zed: by repo name.
            ["w4", "w6", "w5", "w3", "w1", "w2", "w7"]
        );
        assert_eq!(
            by(&["~/Workspace/owpac", "~/Workspace", "other"], &tree()),
            ["w4", "w6", "w5", "w3", "w1", "w2", "w7"]
        );
        assert_eq!(
            by(
                &["~/Workspace/keplair", "~/Workspace/owpac", "other"],
                &tree()
            ),
            ["w3", "w4", "w6", "w1", "w2", "w7", "w5"]
        );
    }

    #[test]
    fn the_label_of_a_linked_worktree_takes_the_full_group() {
        // fix is a linked worktree of app: app goes with it, main workspace first.
        assert_eq!(
            by(&["fix", "other"], &tree()),
            ["w4", "w6", "w1", "w2", "w7", "w5", "w3"]
        );
    }

    #[test]
    fn an_entry_that_takes_nothing_and_a_duplicate_do_nothing() {
        let default = by(&["git", "other"], &tree());
        assert_eq!(
            by(&["nothing", "/no/folder", "git", "other"], &tree()),
            default
        );
        assert_eq!(by(&["git", "git", "other", "other"], &tree()), default);
    }

    #[test]
    fn the_workspaces_that_no_entry_takes_go_at_the_end_in_the_default_order() {
        assert_eq!(
            by(&["~"], &tree()),
            ["w2", "w4", "w6", "w7", "w5", "w3", "w1"]
        );
        assert_eq!(by(&[], &tree()), by(&["git", "other"], &tree()));
    }

    #[test]
    fn the_sort_puts_git_first_by_repository_then_without_git_by_label() {
        let workspaces = flat();
        // atlas, alone (a linked worktree without a main workspace), test (main first), zeta.
        assert_eq!(
            sorted_by_default(&workspaces),
            ["w7", "w4", "w5", "w6", "w2", "w3", "w8", "w1"]
        );
    }

    #[test]
    fn repo_keys_with_a_different_case_stay_2_groups() {
        let workspaces = vec![
            workspace("w1", "a", Some(("/X", "x", false))),
            workspace("w2", "b", Some(("/x", "x", false))),
            workspace("w3", "c", Some(("/X", "x", true))),
            workspace("w4", "d", Some(("/x", "x", true))),
        ];
        assert_eq!(sorted_by_default(&workspaces), ["w1", "w3", "w2", "w4"]);
    }

    #[test]
    fn the_sort_ignores_the_case_and_keeps_the_flat_order_on_a_tie() {
        let workspaces = vec![
            workspace("w1", "b", None),
            workspace("w2", "B", None),
            workspace("w3", "a", None),
        ];
        assert_eq!(sorted_by_default(&workspaces), ["w3", "w1", "w2"]);
    }

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
