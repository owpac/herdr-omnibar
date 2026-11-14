//! The config file of the plugin: `config.toml` in the config folder that herdr gives to the
//! plugin (HERDR_PLUGIN_CONFIG_DIR), by default ~/.config/herdr/plugins/config/herdr-omnibar.
//!
//! ```toml
//! size = "default"   # "default": 74 % × 62 % of the window. "big": 90 % × 80 %.
//! roots = ["~/Workspace"]   # the folders of the projects, 2 levels deep
//! preview = "full"   # "full": diff, log, changes of a worktree (ctrl+Left, ctrl+Right). "summary".
//! pills = "square"   # the ends of the pills: "square", "round" (a Nerd Font), "flat".
//! # The optional features. Remove a name to turn its feature off.
//! # rename_tab: ctrl+R renames the tab, the workspace or the pane of the selected row.
//! # move_workspaces (off by default): the filter Workspaces shows the order of the sidebar, and
//! # alt+Up and alt+Down move the workspace.
//! # auto_sort (off by default): the watcher sorts the workspaces with sidebar_order, at its
//! # start and when a workspace opens or closes.
//! features = ["mouse", "rename_tab", "auto_titles", "agent_logos"]
//! # The filters of the chips, in the order of Tab. Remove a name to hide its filter.
//! filters = ["all", "agents", "tabs", "workspaces", "worktrees", "folders", "new", "actions"]
//! # The order of the workspaces in the herdr sidebar: each entry takes the workspaces that no
//! # earlier entry took. "git": the git workspaces. "other": the workspaces without git. An
//! # entry with a "/": the git projects in this folder, at any depth (~ is the home folder). Any
//! # other entry: the workspace with this label, with its worktrees if it is a git workspace.
//! sidebar_order = ["git", "other"]
//! # The order of all the lists: the first criterion sorts, the next one sorts the equal rows.
//! # "attention": the agents that need you, then the agents that are done. "activity": the last
//! # activity of the agents. "visit": the last visit of the tab. "herdr": the order of herdr (it
//! # is always the last criterion).
//! list_order = ["attention", "activity", "visit"]
//! ```
//!
//! A missing file, a missing key or a wrong value gives the default.

use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::rank::Criterion;

/// The size of the popup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    /// 74 % × 62 % of the window: the size of the other popups.
    #[default]
    Default,
    /// 90 % × 80 % of the window.
    Big,
}

impl Size {
    /// The pane of herdr-plugin.toml that has this size.
    pub fn entrypoint(self) -> &'static str {
        match self {
            Size::Default => "omnibar",
            Size::Big => "omnibar-big",
        }
    }
}

/// The preview of a worktree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preview {
    /// The views of `App::views`: the diff since the default branch, the log, the changes not
    /// committed, and the pull request with the feature pull_requests.
    #[default]
    Full,
    /// One view: the branch, its commits and its diff stat.
    Summary,
}

/// The ends of the pills: the current filter, the crumb, the places of the action line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pills {
    /// Half blocks (▐ ▌): square corners, and a half cell of space on each side.
    #[default]
    Square,
    /// Half circles of a Nerd Font: the font of the terminal must be a Nerd Font.
    Round,
    /// A full cell of space on each side.
    Flat,
}

/// The features that are on by default.
pub const DEFAULT_FEATURES: [&str; 4] = ["mouse", "rename_tab", "auto_titles", "agent_logos"];

/// The filters of the chips by default, in the order of Tab. The filter panes is off by default.
pub const DEFAULT_FILTERS: [&str; 8] = [
    "all",
    "agents",
    "tabs",
    "workspaces",
    "worktrees",
    "folders",
    "new",
    "actions",
];

/// The options of the plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub size: Size,
    /// The folders where the omnibar looks for the git projects.
    pub roots: Vec<String>,
    pub preview: Preview,
    pub pills: Pills,
    /// The optional features that are on.
    pub features: Vec<String>,
    /// The names of the filters of the chips, in the order of Tab.
    pub filters: Vec<String>,
    /// The entries of the sort of the workspaces, in order (see sort::sorted).
    pub sidebar_order: Vec<String>,
    /// The criteria of the order of the lists (see rank).
    pub list_order: Vec<String>,
}

impl Config {
    /// True if the order of the lists has this criterion. With `Criterion::Visit`, the watcher and
    /// the popup record the visits of the tabs.
    pub fn orders_by(&self, criterion: Criterion) -> bool {
        crate::rank::criteria(&self.list_order).contains(&criterion)
    }

    /// True if the feature is on.
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|name| name == feature)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            size: Size::Default,
            roots: owned(&crate::places::DEFAULT_ROOTS),
            preview: Preview::Full,
            pills: Pills::Square,
            features: owned(&DEFAULT_FEATURES),
            filters: owned(&DEFAULT_FILTERS),
            list_order: owned(&crate::rank::DEFAULT_LIST_ORDER),
            sidebar_order: owned(&crate::sort::DEFAULT_SIDEBAR_ORDER),
        }
    }
}

/// A list of the defaults, as the strings of the config.
fn owned(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| item.to_string()).collect()
}

/// The keys of config.toml. An unknown key stays without an effect.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct File {
    size: Option<String>,
    roots: Option<Vec<String>>,
    preview: Option<String>,
    pills: Option<String>,
    features: Option<Vec<String>>,
    filters: Option<Vec<String>>,
    sidebar_order: Option<Vec<String>>,
    list_order: Option<Vec<String>>,
}

/// The config folder of the plugin.
pub fn config_dir() -> PathBuf {
    if let Some(folder) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        return PathBuf::from(folder);
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/herdr/plugins/config/herdr-omnibar")
}

/// The state folder of the plugin: HERDR_PLUGIN_STATE_DIR, else the folder that herdr gives to
/// the plugin by default. A command from a shell has no HERDR_PLUGIN_STATE_DIR.
pub fn state_dir() -> PathBuf {
    if let Some(folder) = env::var_os("HERDR_PLUGIN_STATE_DIR") {
        return PathBuf::from(folder);
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/state/herdr/plugins/herdr-omnibar")
}

/// The options of config.toml in the config folder of the plugin.
pub fn current() -> Config {
    load(&config_dir())
}

/// The options of config.toml in a folder.
pub fn load(folder: &Path) -> Config {
    fs::read_to_string(folder.join("config.toml"))
        .map(|text| parse(&text))
        .unwrap_or_default()
}

fn parse(text: &str) -> Config {
    let file: File = toml::from_str(text).unwrap_or_default();
    let defaults = Config::default();
    Config {
        size: match file.size.as_deref() {
            Some("big") => Size::Big,
            _ => Size::Default,
        },
        roots: file.roots.unwrap_or(defaults.roots),
        preview: match file.preview.as_deref() {
            Some("summary") => Preview::Summary,
            _ => Preview::Full,
        },
        pills: match file.pills.as_deref() {
            Some("round") => Pills::Round,
            Some("flat") => Pills::Flat,
            _ => Pills::Square,
        },
        features: file.features.unwrap_or(defaults.features),
        filters: file.filters.unwrap_or(defaults.filters),
        sidebar_order: file.sidebar_order.unwrap_or(defaults.sidebar_order),
        list_order: file.list_order.unwrap_or(defaults.list_order),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(parse(""), Config::default());
        assert_eq!(parse("").size.entrypoint(), "omnibar");
    }

    #[test]
    fn size_big_gives_the_big_pane() {
        assert_eq!(parse("size = \"big\"").size.entrypoint(), "omnibar-big");
    }

    #[test]
    fn a_wrong_value_gives_the_default() {
        assert_eq!(parse("size = 3").size, Size::Default);
        assert_eq!(parse("size = \"huge\"").size, Size::Default);
    }

    #[test]
    fn roots_replace_the_default_roots() {
        assert_eq!(parse("roots = [\"~/code\"]").roots, ["~/code"]);
        assert!(parse("").roots.contains(&"~/Workspace".to_string()));
    }

    #[test]
    fn the_features_replace_the_default_features() {
        assert!(parse("").has("mouse"));
        let config = parse("features = [\"rename_tab\", \"running_time\"]");
        assert!(!config.has("mouse"));
        assert!(config.has("running_time"));
    }

    #[test]
    fn the_filters_replace_the_default_filters() {
        assert!(parse("").filters.contains(&"worktrees".to_string()));
        assert_eq!(
            parse("filters = [\"agents\", \"panes\"]").filters,
            ["agents", "panes"]
        );
    }

    #[test]
    fn sidebar_order_replaces_the_default_order_and_a_wrong_value_gives_the_default() {
        assert_eq!(parse("").sidebar_order, ["git", "other"]);
        assert_eq!(
            parse("sidebar_order = [\"~\", \"~/Workspace/owpac\", \"git\", \"other\"]")
                .sidebar_order,
            ["~", "~/Workspace/owpac", "git", "other"]
        );
        assert_eq!(parse("sidebar_order = 3").sidebar_order, ["git", "other"]);
    }

    #[test]
    fn list_order_has_a_default_and_a_wrong_value_gives_the_default() {
        assert_eq!(parse("").list_order, ["attention", "activity", "visit"]);
        assert_eq!(parse("list_order = [\"herdr\"]").list_order, ["herdr"]);
        assert_eq!(parse("list_order = []").list_order, Vec::<String>::new());
        assert_eq!(
            parse("list_order = 3").list_order,
            ["attention", "activity", "visit"]
        );
        assert!(parse("").orders_by(Criterion::Visit));
        assert!(!parse("list_order = [\"herdr\"]").orders_by(Criterion::Visit));
    }

    #[test]
    fn pills_takes_its_3_values() {
        assert_eq!(parse("").pills, Pills::Square);
        assert_eq!(parse("pills = \"round\"").pills, Pills::Round);
        assert_eq!(parse("pills = \"flat\"").pills, Pills::Flat);
    }

    #[test]
    fn preview_summary_gives_one_view() {
        assert_eq!(parse("preview = \"summary\"").preview, Preview::Summary);
        assert_eq!(parse("").preview, Preview::Full);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        assert_eq!(load(Path::new("/no/folder/here")), Config::default());
    }
}
