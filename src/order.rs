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
