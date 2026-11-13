//! The order of the workspaces in the herdr sidebar, and the sort of the workspaces.
//!
//! The number of a workspace is its index + 1 in the flat list of the snapshot. It is not its
//! position on the screen: the sidebar groups the worktrees (src/client/shell/sidebar.rs of
//! herdr, function workspace_entries). A group exists for each `repo_key` that has at least 1
//! main workspace and 1 linked worktree. It shows at the position of its first member in the flat
//! list: the main workspace first, then the linked worktrees in the flat order. Any other
//! workspace shows at its flat position.

use std::collections::HashMap;

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

fn is_linked(workspace: &Workspace) -> bool {
    workspace
        .worktree
        .as_ref()
        .is_some_and(|worktree| worktree.is_linked_worktree)
}

fn repo_key(workspace: &Workspace) -> Option<&str> {
    workspace
        .worktree
        .as_ref()
        .map(|worktree| worktree.repo_key.as_str())
        .filter(|key| !key.is_empty())
}

/// The entries of the sidebar, from the flat list of the workspaces.
pub fn sidebar(workspaces: &[Workspace]) -> Vec<Entry> {
    // The keys that have a main workspace and a linked worktree.
    let mut grouped: HashMap<&str, (bool, bool)> = HashMap::new();
    for workspace in workspaces {
        if let Some(key) = repo_key(workspace) {
            let seen = grouped.entry(key).or_default();
            if is_linked(workspace) {
                seen.1 = true;
            } else {
                seen.0 = true;
            }
        }
    }
    let is_group = |key: &str| {
        grouped
            .get(key)
            .is_some_and(|(main, linked)| *main && *linked)
    };
    let mut entries: Vec<Entry> = Vec::new();
    let mut done: Vec<&str> = Vec::new();
    for workspace in workspaces {
        let id = workspace.workspace_id.clone();
        let Some(key) = repo_key(workspace).filter(|key| is_group(key)) else {
            entries.push(Entry::Single(id));
            continue;
        };
        if done.contains(&key) {
            continue;
        }
        done.push(key);
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
    // The entry k moves before the entry k - 1: a move down is the move up of the next entry.
    let step = |position: usize| if delta < 0 { position } else { position + 1 };
    // A linked worktree of a group: it moves among the children of its group.
    for entry in &entries {
        let Entry::Group { children, .. } = entry else {
            continue;
        };
        if let Some(position) = children.iter().position(|child| child == id) {
            let k = step(position);
            if k == 0 || k >= children.len() {
                return None;
            }
            return Some(Move {
                ids: vec![children[k].clone()],
                before: children[k - 1].clone(),
            });
        }
    }
    let position = entries.iter().position(|entry| match entry {
        Entry::Single(single) => single == id,
        Entry::Group { main, .. } => main == id,
    })?;
    let k = step(position);
    if k == 0 || k >= entries.len() {
        return None;
    }
    let flat_index = |id: &String| workspaces.iter().position(|w| &w.workspace_id == id);
    let before = entries[k - 1].ids().into_iter().min_by_key(flat_index)?;
    Some(Move {
        ids: entries[k].ids(),
        before,
    })
}

/// The wanted order of the sort:
///
/// 1. The git workspaces first, then the workspaces without git.
/// 2. The git workspaces in groups by `repo_key`. The groups go by `repo_name`, then `repo_key`.
/// 3. In a group, the main workspace first, then the linked worktrees by label. A group without
///    a main workspace stays together too.
/// 4. The workspaces without git go by label.
///
/// The comparisons ignore the case. The flat order breaks a tie.
pub fn sorted(workspaces: &[Workspace]) -> Vec<String> {
    let mut keyed: Vec<(usize, &Workspace)> = workspaces.iter().enumerate().collect();
    let lower = |text: &str| text.to_lowercase();
    keyed.sort_by_cached_key(|(index, workspace)| {
        let label = lower(&workspace.label);
        match &workspace.worktree {
            Some(worktree) => (
                0,
                lower(&worktree.repo_name),
                lower(&worktree.repo_key),
                is_linked(workspace),
                label,
                *index,
            ),
            None => (1, String::new(), String::new(), false, label, *index),
        }
    });
    keyed
        .into_iter()
        .map(|(_, workspace)| workspace.workspace_id.clone())
        .collect()
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

    #[test]
    fn the_sort_puts_git_first_by_repository_then_without_git_by_label() {
        let workspaces = flat();
        // atlas, alone (a linked worktree without a main workspace), test (main first), zeta.
        assert_eq!(
            sorted(&workspaces),
            ["w7", "w4", "w5", "w6", "w2", "w3", "w8", "w1"]
        );
    }

    fn mv(ids: &[&str], before: &str) -> Option<Move> {
        Some(Move {
            ids: ids.iter().map(|id| id.to_string()).collect(),
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
        let workspaces = flat();
        for (id, delta) in [
            ("w5", 1),
            ("w5", -1),
            ("w3", -1),
            ("w6", -1),
            ("w2", 1),
            ("w4", 1),
        ] {
            let before = screen_order(&workspaces);
            let plan = plan_move(&workspaces, id, delta).unwrap();
            let after = screen_order(&apply(&workspaces, &plan));
            // The moved entry swaps with its neighbour on the screen.
            let moved: Vec<&String> = before.iter().filter(|w| plan.ids.contains(w)).collect();
            assert_eq!(moved.len(), plan.ids.len(), "{id} {delta}");
            assert_ne!(before, after, "{id} {delta}");
            let position =
                |order: &[String], target: &str| order.iter().position(|w| w == target).unwrap();
            if delta < 0 {
                assert!(position(&after, id) < position(&before, id), "{id} {delta}");
            } else {
                assert!(position(&after, id) > position(&before, id), "{id} {delta}");
            }
        }
    }

    #[test]
    fn the_sort_ignores_the_case_and_keeps_the_flat_order_on_a_tie() {
        let workspaces = vec![
            workspace("w1", "b", None),
            workspace("w2", "B", None),
            workspace("w3", "a", None),
        ];
        assert_eq!(sorted(&workspaces), ["w3", "w1", "w2"]);
    }
}
