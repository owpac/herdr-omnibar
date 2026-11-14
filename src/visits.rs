//! The times of use of the tabs, for the criterion visit of the key list_order (src/rank.rs
//! sorts the rows with them).
//!
//! The watcher writes the time of each change of the focused tab in recent.json, in the state
//! folder. Each opening of the popup also writes the time of the tab that it leaves.

use crate::model::Snapshot;
use crate::times::now_seconds;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The last time of use of each tab, in seconds since 1970, by tab id.
pub type Visits = HashMap<String, u64>;

fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("recent.json")
}

/// The times of use of the tabs. Empty without the file.
pub fn load(state_dir: &Path) -> Visits {
    fs::read_to_string(path(state_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(state_dir: &Path, visits: &Visits) {
    if let Ok(text) = serde_json::to_string(visits) {
        // The order is only a help: a failed write changes nothing else.
        let _ = fs::create_dir_all(state_dir);
        let _ = fs::write(path(state_dir), text);
    }
}

/// The times of use when the popup opens: the tab of the popup gets the time now.
pub fn open(state_dir: &Path, current_tab: Option<&str>) -> Visits {
    let mut visits = load(state_dir);
    if let Some(tab_id) = current_tab {
        visits.insert(tab_id.to_string(), now_seconds());
        save(state_dir, &visits);
    }
    visits
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
    let mut visits = load(state_dir);
    let time = now_seconds();
    if let Some(left) = focused.replace(tab.clone()) {
        visits.insert(left, time);
    }
    visits.insert(tab.clone(), time);
    visits.retain(|tab_id, _| snapshot.tabs.iter().any(|tab| &tab.tab_id == tab_id));
    save(state_dir, &visits);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_change_of_focus_gives_the_time_to_the_left_tab_and_to_the_new_tab() {
        let folder = std::env::temp_dir().join(format!("omnibar-visits-{}", std::process::id()));
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
        let visits = load(&folder);
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(focused.as_deref(), Some("t2"));
        assert!(visits.contains_key("t1") && visits.contains_key("t2"));
    }
}
