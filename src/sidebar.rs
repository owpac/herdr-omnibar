//! The order of the workspaces in the herdr sidebar, and the move of a workspace in it.
//!
//! The number of a workspace is its index + 1 in the flat list of the snapshot. It is not its
//! position on the screen: the sidebar groups the worktrees (src/client/shell/sidebar.rs of
//! herdr, function workspace_entries). A group exists for each `repo_key` that has at least 1
//! main workspace and 1 linked worktree. It shows at the position of its first member in the flat
//! list: the main workspace first, then the linked worktrees in the flat order. Any other
//! workspace shows at its flat position.

use std::collections::{HashMap, HashSet};

use crate::model::Workspace;

/// An entry of the top level of the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A workspace that is not in a group.
    Single(String),
    /// A main workspace with its linked worktrees.
    Group { main: String, children: Vec<String> },
}

impl Entry {
    /// The ids of the entry, in the order of the screen.
    pub fn ids(&self) -> Vec<String> {
        match self {
            Entry::Single(id) => vec![id.clone()],
            Entry::Group { main, children } => {
                let mut ids = vec![main.clone()];
                ids.extend(children.iter().cloned());
                ids
            }
        }
    }
}

pub(crate) fn is_linked(workspace: &Workspace) -> bool {
    workspace
        .worktree
        .as_ref()
        .is_some_and(|worktree| worktree.is_linked_worktree)
}

pub(crate) fn repo_key(workspace: &Workspace) -> Option<&str> {
    workspace
        .worktree
        .as_ref()
        .map(|worktree| worktree.repo_key.as_str())
        .filter(|key| !key.is_empty())
}

/// The entries of the sidebar, from the flat list of the workspaces.
pub fn sidebar(workspaces: &[Workspace]) -> Vec<Entry> {
    // The keys of the main workspaces, and the keys of the linked worktrees.
    let keys = |linked: bool| -> HashSet<&str> {
        workspaces
            .iter()
            .filter(|workspace| is_linked(workspace) == linked)
            .filter_map(repo_key)
            .collect()
    };
    let (mains, linked) = (keys(false), keys(true));
    let is_group = |key: &&str| mains.contains(key) && linked.contains(key);
    let mut entries: Vec<Entry> = Vec::new();
    let mut done: HashSet<&str> = HashSet::new();
    for workspace in workspaces {
        let Some(key) = repo_key(workspace).filter(is_group) else {
            entries.push(Entry::Single(workspace.workspace_id.clone()));
            continue;
        };
        if !done.insert(key) {
            continue;
        }
        let members = || {
            workspaces
                .iter()
                .filter(move |member| repo_key(member) == Some(key))
        };
        // The first main workspace leads. The other members keep the flat order.
        let Some(main) = members().find(|member| !is_linked(member)) else {
            continue;
        };
        entries.push(Entry::Group {
            main: main.workspace_id.clone(),
            children: members()
                .filter(|member| member.workspace_id != main.workspace_id)
                .map(|member| member.workspace_id.clone())
                .collect(),
        });
    }
    entries
}

/// The ids of the workspaces, in the order of the screen.
pub fn screen_order(workspaces: &[Workspace]) -> Vec<String> {
    sidebar(workspaces).iter().flat_map(Entry::ids).collect()
}

/// The position of each workspace on the screen, by workspace id.
pub fn screen_positions(workspaces: &[Workspace]) -> HashMap<String, usize> {
    screen_order(workspaces)
        .into_iter()
        .enumerate()
        .map(|(position, id)| (id, position))
        .collect()
}

/// A move of the sidebar: these workspaces, in this order, go before another workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub ids: Vec<String>,
    pub before: String,
}

/// The move of a workspace one position up (`delta` < 0) or down (`delta` > 0) in the sidebar. None
/// at the first or the last position.
///
/// - A main workspace of a group moves with its group, among the entries of the top level. So does a
///   workspace without a group.
/// - A linked worktree of a group moves only among the linked worktrees of its group.
///
/// A move down is the move up of the next entry: it goes before the entry of the workspace. The
/// anchor is the member of the entry that comes first in the flat list: the group shows at its
/// first member.
pub fn plan_move(workspaces: &[Workspace], id: &str, delta: i32) -> Option<Move> {
    let entries = sidebar(workspaces);
    // A linked worktree of a group: it moves among the children of its group.
    let group = entries.iter().find_map(|entry| match entry {
        Entry::Group { children, .. } => {
            Some((children, children.iter().position(|child| child == id)?))
        }
        Entry::Single(_) => None,
    });
    if let Some((children, position)) = group {
        let (moved, before) = neighbours(children, position, delta)?;
        return Some(Move {
            ids: vec![moved.clone()],
            before: before.clone(),
        });
    }
    let position = entries.iter().position(|entry| match entry {
        Entry::Single(single) => single == id,
        Entry::Group { main, .. } => main == id,
    })?;
    let (moved, before) = neighbours(&entries, position, delta)?;
    // The anchor: the member of the entry before that comes first in the flat list.
    let members = before.ids();
    let anchor = workspaces
        .iter()
        .map(|workspace| &workspace.workspace_id)
        .find(|member| members.contains(member))?;
    Some(Move {
        ids: moved.ids(),
        before: anchor.clone(),
    })
}

/// The item that moves, and the item that it goes before: the item k moves before the item k - 1.
/// A move down is the move up of the next item. None at the first or the last position.
fn neighbours<T>(items: &[T], position: usize, delta: i32) -> Option<(&T, &T)> {
    let k = if delta < 0 { position } else { position + 1 };
    Some((items.get(k)?, items.get(k.checked_sub(1)?)?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A workspace: its id, its label, and its git data (repo key, repo name, linked).
    pub fn workspace(id: &str, label: &str, git: Option<(&str, &str, bool)>) -> Workspace {
        let worktree = git.map(|(key, name, linked)| {
            serde_json::json!({
                "repo_key": key, "repo_name": name, "is_linked_worktree": linked
            })
        });
        serde_json::from_value(serde_json::json!({
            "workspace_id": id, "label": label, "worktree": worktree
        }))
        .unwrap()
    }

    /// A git workspace with its repository root.
    pub fn project(id: &str, label: &str, name: &str, root: &str, linked: bool) -> Workspace {
        serde_json::from_value(serde_json::json!({
            "workspace_id": id, "label": label,
            "worktree": {
                "repo_key": format!("{root}/.git"), "repo_name": name,
                "repo_root": root, "is_linked_worktree": linked
            }
        }))
        .unwrap()
    }

    /// The example of herdr: test.test (number 6) and test (number 10) show together at 6.
    pub fn flat() -> Vec<Workspace> {
        vec![
            workspace("w1", "notes", None),
            workspace("w2", "test.test", Some(("/t", "test", true))),
            workspace("w3", "zeta", Some(("/z", "zeta", false))),
            workspace("w4", "Atlas", Some(("/a", "atlas", false))),
            workspace("w5", "test", Some(("/t", "test", false))),
            workspace("w6", "test.more", Some(("/t", "test", true))),
            workspace("w7", "alone.fix", Some(("/o", "alone", true))),
            workspace("w8", "Inbox", None),
        ]
    }

    #[test]
    fn the_sidebar_groups_a_main_workspace_with_its_worktrees_at_the_first_member() {
        let entries = sidebar(&flat());
        assert_eq!(
            entries,
            [
                Entry::Single("w1".into()),
                Entry::Group {
                    main: "w5".into(),
                    children: vec!["w2".into(), "w6".into()]
                },
                Entry::Single("w3".into()),
                Entry::Single("w4".into()),
                // A linked worktree without an open main workspace is not a group.
                Entry::Single("w7".into()),
                Entry::Single("w8".into()),
            ]
        );
        assert_eq!(
            screen_order(&flat()),
            ["w1", "w5", "w2", "w6", "w3", "w4", "w7", "w8"]
        );
    }

    fn mv(ids: &[&str], before: &str) -> Option<Move> {
        Some(Move {
            ids: crate::config::owned(ids),
            before: before.into(),
        })
    }

    #[test]
    fn a_main_workspace_moves_with_its_group_among_the_entries() {
        // The screen: w1, [w5 w2 w6], w3, w4, w7, w8.
        let workspaces = flat();
        // Up: the group goes before w1.
        assert_eq!(
            plan_move(&workspaces, "w5", -1),
            mv(&["w5", "w2", "w6"], "w1")
        );
        // Down: w3 goes before the group, at its first member in the flat list: w2.
        assert_eq!(plan_move(&workspaces, "w5", 1), mv(&["w3"], "w2"));
        // A workspace without a group goes before the group, not inside it.
        assert_eq!(plan_move(&workspaces, "w3", -1), mv(&["w3"], "w2"));
        assert_eq!(
            plan_move(&workspaces, "w1", 1),
            mv(&["w5", "w2", "w6"], "w1")
        );
        assert_eq!(plan_move(&workspaces, "w4", -1), mv(&["w4"], "w3"));
    }

    #[test]
    fn the_first_and_the_last_entry_do_not_move() {
        let workspaces = flat();
        assert_eq!(plan_move(&workspaces, "w1", -1), None);
        assert_eq!(plan_move(&workspaces, "w8", 1), None);
        assert_eq!(plan_move(&workspaces, "unknown", 1), None);
    }

    #[test]
    fn a_linked_worktree_moves_only_among_the_worktrees_of_its_group() {
        let workspaces = flat();
        // The children of the group are w2 and w6.
        assert_eq!(plan_move(&workspaces, "w6", -1), mv(&["w6"], "w2"));
        assert_eq!(plan_move(&workspaces, "w2", 1), mv(&["w6"], "w2"));
        assert_eq!(plan_move(&workspaces, "w2", -1), None);
        assert_eq!(plan_move(&workspaces, "w6", 1), None);
    }

    #[test]
    fn a_move_gives_the_wanted_screen_order_after_the_call() {
        // Apply the move to the flat list as herdr does: the block goes before the anchor.
        let apply = |workspaces: &[Workspace], plan: &Move| -> Vec<Workspace> {
            let mut rest: Vec<Workspace> = workspaces
                .iter()
                .filter(|w| !plan.ids.contains(&w.workspace_id))
                .cloned()
                .collect();
            let at = rest
                .iter()
                .position(|w| w.workspace_id == plan.before)
                .unwrap();
            let block = plan.ids.iter().map(|id| {
                workspaces
                    .iter()
                    .find(|w| &w.workspace_id == id)
                    .unwrap()
                    .clone()
            });
            rest.splice(at..at, block);
            rest
        };
        // The wanted screen order: the entry, or the linked worktree, swaps with its neighbour.
        let swapped = |workspaces: &[Workspace], id: &str, delta: i32| -> Vec<String> {
            let mut entries = sidebar(workspaces);
            let child = entries
                .iter()
                .enumerate()
                .find_map(|(index, entry)| match entry {
                    Entry::Group { children, .. } => {
                        Some((index, children.iter().position(|child| child == id)?))
                    }
                    Entry::Single(_) => None,
                });
            let other = |position: usize| {
                if delta < 0 {
                    position - 1
                } else {
                    position + 1
                }
            };
            match child {
                Some((index, position)) => {
                    if let Entry::Group { children, .. } = &mut entries[index] {
                        children.swap(position, other(position));
                    }
                }
                None => {
                    let position = entries
                        .iter()
                        .position(|entry| entry.ids()[0] == id)
                        .unwrap();
                    entries.swap(position, other(position));
                }
            }
            entries.iter().flat_map(Entry::ids).collect()
        };
        // flat(), and a group with a workspace between its members: the group shows at w1.
        let scattered = vec![
            workspace("w1", "app.fix", Some(("/a", "app", true))),
            workspace("w2", "notes", None),
            workspace("w3", "app", Some(("/a", "app", false))),
            workspace("w4", "app.more", Some(("/a", "app", true))),
            workspace("w5", "Inbox", None),
        ];
        let mut moves = 0;
        for workspaces in [flat(), scattered] {
            for workspace in &workspaces {
                let id = workspace.workspace_id.as_str();
                for delta in [-1, 1] {
                    let Some(plan) = plan_move(&workspaces, id, delta) else {
                        continue;
                    };
                    moves += 1;
                    assert_eq!(
                        screen_order(&apply(&workspaces, &plan)),
                        swapped(&workspaces, id, delta),
                        "{id} {delta}"
                    );
                }
            }
        }
        assert_eq!(moves, 18);
    }
}
