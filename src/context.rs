//! The context of an action: the focused pane, its tab, its workspace and its folder.
//!
//! herdr gives this context to an action in HERDR_PLUGIN_CONTEXT_JSON. The action gives the same
//! JSON to the popup in OMNIBAR_CONTEXT: in the popup, the focused pane is the popup itself.

use serde::Deserialize;
use std::env;
use std::path::PathBuf;

/// The variable of herdr that gives the context of an action.
pub const CONTEXT_JSON_VARIABLE: &str = "HERDR_PLUGIN_CONTEXT_JSON";

/// The fields of the context that the omnibar uses. herdr gives more fields: serde ignores them.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct Context {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub focused_pane_cwd: Option<PathBuf>,
    #[serde(default)]
    pub workspace_cwd: Option<PathBuf>,
}

impl Context {
    /// The context of the action that runs this command.
    pub fn from_environment() -> Context {
        Context::parse(&env::var(CONTEXT_JSON_VARIABLE).unwrap_or_default())
    }

    /// The context of the action that opened the popup: the action gives it in OMNIBAR_CONTEXT.
    pub fn of_popup() -> Context {
        Context::parse(&env::var(crate::CONTEXT_VARIABLE).unwrap_or_default())
    }

    /// The context in a JSON text. A text that is not a context gives an empty context.
    pub fn parse(text: &str) -> Context {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// The folder of the focused pane, else the folder of the workspace, else the current folder.
    pub fn folder(&self) -> PathBuf {
        self.focused_pane_cwd
            .clone()
            .or_else(|| self.workspace_cwd.clone())
            .or_else(|| env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_of_herdr_gives_the_focused_pane() {
        let context = Context::parse(
            r#"{"correlation_id":"cli:plugin","focused_pane_cwd":"/work/atlas","focused_pane_id":"w2:p2",
                "tab_id":"w2:t2","workspace_cwd":"/work","workspace_id":"w2",
                "worktree":{"repo_name":"atlas"}}"#,
        );
        assert_eq!(context.workspace_id.as_deref(), Some("w2"));
        assert_eq!(context.focused_pane_id.as_deref(), Some("w2:p2"));
        assert_eq!(context.folder(), PathBuf::from("/work/atlas"));
    }

    #[test]
    fn without_a_pane_the_folder_is_the_folder_of_the_workspace() {
        let context = Context::parse(r#"{"workspace_cwd":"/work"}"#);
        assert_eq!(context.folder(), PathBuf::from("/work"));
    }

    #[test]
    fn a_text_that_is_not_json_gives_an_empty_context() {
        assert_eq!(Context::parse("not json"), Context::default());
    }
}
