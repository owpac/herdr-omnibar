//! The order of the workspaces in the herdr sidebar, and the sort of the workspaces.
//!
//! The number of a workspace is its index + 1 in the flat list of the snapshot. It is not its
//! position on the screen: the sidebar groups the worktrees (src/client/shell/sidebar.rs of
//! herdr, function workspace_entries). A group exists for each `repo_key` that has at least 1
//! main workspace and 1 linked worktree. It shows at the position of its first member in the flat
//! list: the main workspace first, then the linked worktrees in the flat order. Any other
//! workspace shows at its flat position.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

/// The folder of an entry: `~` at the start is the home folder.
fn expand(entry: &str, home: &Path) -> PathBuf {
    match entry.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(entry),
    }
}

/// True if the entry of `sidebar_order` takes the members of a unit: a git group, or a workspace
/// without git.
fn takes(entry: &str, members: &[&Workspace], home: &Path) -> bool {
    let git = members.iter().any(|member| member.worktree.is_some());
    match entry {
        "git" => git,
        "other" => !git,
        _ if is_folder(entry) => {
            let folder = expand(entry, home);
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
            sorted_by_default(&workspaces),
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
}
