//! The order of use of the tabs (feature recent_first). The tab that the user used last comes
//! first, after the current tab: ⌘P then ↵ goes back to the tab before, like alt-tab.
//!
//! The watcher writes the time of each change of the focused tab in recent.json, in the state
//! folder. Each opening of the popup also writes the time of the tab that it leaves.

use crate::model::{Model, Snapshot};
use crate::times::now_seconds;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The last time of use of each tab, in seconds since 1970, by tab id.
pub type Recent = HashMap<String, u64>;

fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("recent.json")
}

/// The times of use of the tabs. Empty without the file.
pub fn load(state_dir: &Path) -> Recent {
    fs::read_to_string(path(state_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(state_dir: &Path, recent: &Recent) {
    if let Ok(text) = serde_json::to_string(recent) {
        // The order is only a help: a failed write changes nothing else.
        let _ = fs::create_dir_all(state_dir);
        let _ = fs::write(path(state_dir), text);
    }
}

/// Write the time of use of a tab: now.
pub fn touch(state_dir: &Path, tab_id: &str) {
    let mut recent = load(state_dir);
    recent.insert(tab_id.to_string(), now_seconds());
    save(state_dir, &recent);
}

/// Follow the focused tab of a snapshot, for the watcher. When the focus changes, the tab that the
/// user leaves and the new tab get the time now. The closed tabs go.
pub fn follow(state_dir: &Path, snapshot: &Snapshot, focused: &mut Option<String>) {
    let Some(tab) = snapshot.focused_tab_id.as_ref() else {
        return;
    };
    if focused.as_ref() == Some(tab) {
        return;
    }
    let mut recent = load(state_dir);
    let time = now_seconds();
    if let Some(left) = focused.replace(tab.clone()) {
        recent.insert(left, time);
    }
    recent.insert(tab.clone(), time);
    recent.retain(|tab_id, _| snapshot.tabs.iter().any(|tab| &tab.tab_id == tab_id));
    save(state_dir, &recent);
}

/// Sort the rows of a model: the tab used last first. The current tab goes after the tabs that the
/// user used: the first row is the tab before. A workspace takes the last time of its tabs. The
/// sort is stable: the rows without a time keep their order.
pub fn order(model: &mut Model, recent: &Recent, current: Option<&str>) {
    let time = |tab_id: &str| {
        if Some(tab_id) == current {
            0
        } else {
            recent.get(tab_id).copied().unwrap_or(0)
        }
    };
    let mut workspaces: HashMap<String, u64> = HashMap::new();
    let tabs = model
        .agents
        .iter()
        .map(|agent| (&agent.workspace_id, &agent.tab_id))
        .chain(
            model
                .tabs
                .iter()
                .map(|tab| (&tab.workspace_id, &tab.tab_id)),
        );
    for (workspace_id, tab_id) in tabs {
        let last = workspaces.entry(workspace_id.clone()).or_default();
        *last = (*last).max(time(tab_id));
    }
    let newest = |value: u64| std::cmp::Reverse(value);
    model
        .agents
        .sort_by_key(|agent| newest(time(&agent.tab_id)));
    model.tabs.sort_by_key(|tab| newest(time(&tab.tab_id)));
    model.panes.sort_by_key(|pane| newest(time(&pane.tab_id)));
    model.spaces.sort_by_key(|space| {
        newest(
            workspaces
                .get(&space.workspace.workspace_id)
                .copied()
                .unwrap_or(0),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentRow, Status};

    fn agent(name: &str, tab: &str) -> AgentRow {
        AgentRow {
            pane_id: format!("p-{name}"),
            tab_id: tab.into(),
            workspace_id: "w1".into(),
            name: name.into(),
            kind: "claude".into(),
            status: Status::Idle,
            place: "atlas".into(),
            cwd: None,
        }
    }

    #[test]
    fn the_tab_used_last_comes_first_and_the_current_tab_goes_after() {
        let mut model = Model {
            agents: vec![
                agent("old", "t1"),
                agent("current", "t2"),
                agent("last", "t3"),
                agent("never", "t4"),
            ],
            ..Model::default()
        };
        let recent = Recent::from([
            ("t1".to_string(), 10),
            ("t2".to_string(), 30),
            ("t3".to_string(), 20),
        ]);
        order(&mut model, &recent, Some("t2"));
        let names: Vec<&str> = model
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        assert_eq!(names, ["last", "old", "current", "never"]);
    }

    #[test]
    fn a_change_of_focus_gives_the_time_to_the_left_tab_and_to_the_new_tab() {
        let folder = std::env::temp_dir().join(format!("omnibar-recent-{}", std::process::id()));
        let snapshot = |tab: &str| -> Snapshot {
            serde_json::from_str(&format!(
                r#"{{"focused_tab_id": "{tab}", "tabs": [
                    {{"tab_id": "t1", "workspace_id": "w1"}},
                    {{"tab_id": "t2", "workspace_id": "w1"}}]}}"#
            ))
            .unwrap()
        };
        let mut focused = None;
        follow(&folder, &snapshot("t1"), &mut focused);
        follow(&folder, &snapshot("t2"), &mut focused);
        let recent = load(&folder);
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(focused.as_deref(), Some("t2"));
        assert!(recent.contains_key("t1") && recent.contains_key("t2"));
    }
}
