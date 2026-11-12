//! The config file of the plugin: `config.toml` in the config folder that herdr gives to the
//! plugin (HERDR_PLUGIN_CONFIG_DIR), by default ~/.config/herdr/plugins/config/herdr-omnibar.
//!
//! ```toml
//! size = "default"   # "default": 74 % × 62 % of the window. "big": 90 % × 80 %.
//! roots = ["~/Workspace"]   # the folders of the projects, 2 levels deep
//! ```
//!
//! A missing file, a missing key or a wrong value gives the default.

use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

/// The options of the plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub size: Size,
    /// The folders where the omnibar looks for the git projects.
    pub roots: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            size: Size::Default,
            roots: crate::places::DEFAULT_ROOTS
                .iter()
                .map(|root| root.to_string())
                .collect(),
        }
    }
}

/// The keys of config.toml. An unknown key stays without an effect.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct File {
    size: Option<String>,
    roots: Option<Vec<String>>,
}

/// The config folder of the plugin.
pub fn config_dir() -> PathBuf {
    if let Some(folder) = env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        return PathBuf::from(folder);
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/herdr/plugins/config/herdr-omnibar")
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
    fn a_missing_file_gives_the_defaults() {
        assert_eq!(load(Path::new("/no/folder/here")), Config::default());
    }
}
