//! The data of the popup: the workspaces, the tabs and the agents of the herdr snapshot, with the
//! project of each workspace.
//!
//! A workspace belongs to a project: the git repository of its folder. The workspace of the main
//! checkout has the name of the project. The workspace of a linked worktree has the name
//! `<project> ▸ <branch>`: a worktree always has its own workspace, under its project.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

/// The status of an agent, of a tab or of a workspace, as herdr gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The agent waits for an answer of the user.
    Blocked,
    Working,
    /// The agent has finished its turn, and the user has not seen it yet.
    Done,
    Idle,
    #[default]
    #[serde(other)]
    Unknown,
}

impl Status {
    /// The glyph of herdr for the status.
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Blocked => "◉",
            Status::Working => "◐",
            Status::Done => "●",
            Status::Idle => "✓",
            Status::Unknown => "·",
        }
    }

    /// The status as a text.
    pub fn label(self) -> &'static str {
        match self {
            Status::Blocked => "needs you",
            Status::Working => "working",
            Status::Done => "done",
            Status::Idle => "idle",
            Status::Unknown => "",
        }
    }

    /// The order of the agents: the agents that need the user first.
    fn rank(self) -> u8 {
        match self {
            Status::Blocked => 0,
            Status::Done => 1,
            Status::Working => 2,
            Status::Idle => 3,
            Status::Unknown => 4,
        }
    }
}

/// The herdr snapshot: `herdr api snapshot`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub agent_status: Status,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Status,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
}

/// The git checkout of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkout {
    /// The top folder of the checkout.
    pub root: PathBuf,
    /// The git folder that all the checkouts of the repository share.
    pub common: PathBuf,
    /// The branch of the checkout. None for a detached HEAD.
    pub branch: Option<String>,
}

impl Checkout {
    /// True for the main checkout of the repository, false for a linked worktree.
    pub fn is_main(&self) -> bool {
        self.common == self.root.join(".git")
    }

    /// The name of the project: the folder of the main checkout.
    pub fn project(&self) -> String {
        let main = if self.common.file_name().is_some_and(|name| name == ".git") {
            self.common.parent().unwrap_or(&self.common)
        } else {
            self.common.as_path()
        };
        let name = file_name(main);
        name.strip_suffix(".git").unwrap_or(&name).to_string()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The git checkout of a folder, or None outside git.
pub fn checkout_of(folder: &Path) -> Option<Checkout> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-common-dir",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let root = PathBuf::from(lines.next()?);
    let common = PathBuf::from(lines.next()?);
    let branch = lines
        .next()
        .filter(|branch| *branch != "HEAD")
        .map(str::to_string);
    Some(Checkout {
        root,
        common,
        branch,
    })
}

/// The checkouts of some folders. git runs for each folder at the same time.
pub fn checkouts_of(folders: &[PathBuf]) -> HashMap<PathBuf, Option<Checkout>> {
    thread::scope(|scope| {
        let handles: Vec<_> = folders
            .iter()
            .map(|folder| scope.spawn(move || (folder.clone(), checkout_of(folder))))
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    })
}

/// A workspace with its folder and its checkout.
#[derive(Clone, Debug)]
pub struct Space {
    pub workspace: Workspace,
    pub folder: Option<PathBuf>,
    pub checkout: Option<Checkout>,
}

impl Space {
    /// The project of the workspace: its git repository. Outside git, the name that herdr gives to
    /// the workspace, else its folder.
    pub fn project(&self) -> String {
        if let Some(checkout) = &self.checkout {
            return checkout.project();
        }
        if !self.workspace.label.is_empty() {
            return self.workspace.label.clone();
        }
        self.folder.as_deref().map(file_name).unwrap_or_default()
    }

    /// The name of the workspace: the project, or `<project> ▸ <branch>` for a linked worktree.
    pub fn label(&self) -> String {
        match &self.checkout {
            Some(checkout) if !checkout.is_main() => {
                let branch = checkout
                    .branch
                    .clone()
                    .unwrap_or_else(|| file_name(&checkout.root));
                format!("{} ▸ {}", checkout.project(), branch)
            }
            _ => self.project(),
        }
    }
}

/// An agent: a pane where herdr detects an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRow {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    /// The label of the tab: the task title, or the name that the user gave.
    pub name: String,
    /// The kind of agent: claude, codex…
    pub kind: String,
    pub status: Status,
    /// The name of the workspace of the agent.
    pub place: String,
}

/// A tab without an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabRow {
    pub tab_id: String,
    pub workspace_id: String,
    pub name: String,
    /// The title of the terminal of the first pane: often the command that runs.
    pub command: String,
    pub place: String,
}

/// All the data of the popup.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub spaces: Vec<Space>,
    pub agents: Vec<AgentRow>,
    pub tabs: Vec<TabRow>,
    pub focused_workspace_id: Option<String>,
}

impl Model {
    /// The model of a snapshot, with the checkouts of the folders of its workspaces.
    pub fn build(snapshot: &Snapshot, checkouts: &HashMap<PathBuf, Option<Checkout>>) -> Model {
        let spaces: Vec<Space> = snapshot
            .workspaces
            .iter()
            .map(|workspace| {
                let folder = folder_of(snapshot, &workspace.workspace_id);
                let checkout = folder
                    .as_ref()
                    .and_then(|folder| checkouts.get(folder).cloned().flatten());
                Space {
                    workspace: workspace.clone(),
                    folder,
                    checkout,
                }
            })
            .collect();
        let place_of = |workspace_id: &str| {
            spaces
                .iter()
                .find(|space| space.workspace.workspace_id == workspace_id)
                .map(Space::label)
                .unwrap_or_default()
        };
        // The name of a tab. herdr names a new tab with its number: then the title of the terminal
        // is a better name, because an agent writes its task there.
        let label_of = |tab_id: &str, title: Option<&str>| {
            let label = snapshot
                .tabs
                .iter()
                .find(|tab| tab.tab_id == tab_id)
                .map(|tab| tab.label.clone())
                .unwrap_or_default();
            let numbered = label.is_empty() || label.chars().all(|c| c.is_ascii_digit());
            match title.map(str::trim).filter(|title| !title.is_empty()) {
                Some(title) if numbered => title.to_string(),
                _ => label,
            }
        };
        let mut agents: Vec<AgentRow> = snapshot
            .panes
            .iter()
            .filter_map(|pane| {
                let kind = pane.agent.clone()?;
                Some(AgentRow {
                    pane_id: pane.pane_id.clone(),
                    tab_id: pane.tab_id.clone(),
                    workspace_id: pane.workspace_id.clone(),
                    name: label_of(&pane.tab_id, pane.terminal_title_stripped.as_deref()),
                    kind,
                    status: pane.agent_status,
                    place: place_of(&pane.workspace_id),
                })
            })
            .collect();
        agents.sort_by_key(|agent| agent.status.rank());
        let tabs = snapshot
            .tabs
            .iter()
            .filter(|tab| {
                !snapshot
                    .panes
                    .iter()
                    .any(|pane| pane.tab_id == tab.tab_id && pane.agent.is_some())
            })
            .map(|tab| {
                let command = snapshot
                    .panes
                    .iter()
                    .find(|pane| pane.tab_id == tab.tab_id)
                    .and_then(|pane| pane.terminal_title_stripped.clone())
                    .unwrap_or_default();
                TabRow {
                    tab_id: tab.tab_id.clone(),
                    workspace_id: tab.workspace_id.clone(),
                    name: label_of(&tab.tab_id, Some(&command)),
                    command,
                    place: place_of(&tab.workspace_id),
                }
            })
            .collect();
        Model {
            spaces,
            agents,
            tabs,
            focused_workspace_id: snapshot.focused_workspace_id.clone(),
        }
    }
}

/// The model of the running herdr: its snapshot, and the checkout of each folder.
pub fn load() -> Result<Model, crate::herdr::Error> {
    let snapshot = crate::herdr::snapshot()?;
    let checkouts = checkouts_of(&folders(&snapshot));
    Ok(Model::build(&snapshot, &checkouts))
}

/// The folders of the workspaces of a snapshot, without duplicates.
pub fn folders(snapshot: &Snapshot) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for workspace in &snapshot.workspaces {
        if let Some(folder) = folder_of(snapshot, &workspace.workspace_id) {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    }
    folders
}

/// The folder of a workspace: the folder of its first pane.
fn folder_of(snapshot: &Snapshot, workspace_id: &str) -> Option<PathBuf> {
    snapshot
        .panes
        .iter()
        .filter(|pane| pane.workspace_id == workspace_id)
        .find_map(|pane| pane.cwd.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = r#"{
      "focused_workspace_id": "w1",
      "workspaces": [
        {"workspace_id": "w1", "label": "atlas", "agent_status": "working"},
        {"workspace_id": "w2", "label": "atlas.fix-login", "agent_status": "blocked"},
        {"workspace_id": "w3", "label": "my notes", "agent_status": "unknown"}
      ],
      "tabs": [
        {"tab_id": "w1:t1", "workspace_id": "w1", "label": "omnibar design", "number": 1},
        {"tab_id": "w1:t2", "workspace_id": "w1", "label": "logs", "number": 2},
        {"tab_id": "w2:t1", "workspace_id": "w2", "label": "fix the login", "number": 1},
        {"tab_id": "w3:t1", "workspace_id": "w3", "label": "shell", "number": 1},
        {"tab_id": "w3:t2", "workspace_id": "w3", "label": "2", "number": 2}
      ],
      "panes": [
        {"pane_id": "w1:p1", "tab_id": "w1:t1", "workspace_id": "w1", "cwd": "/work/atlas", "agent": "claude", "agent_status": "working"},
        {"pane_id": "w1:p2", "tab_id": "w1:t2", "workspace_id": "w1", "cwd": "/work/atlas", "terminal_title_stripped": "tail -f observer.log"},
        {"pane_id": "w2:p1", "tab_id": "w2:t1", "workspace_id": "w2", "cwd": "/work/atlas.fix-login", "agent": "codex", "agent_status": "blocked"},
        {"pane_id": "w3:p1", "tab_id": "w3:t1", "workspace_id": "w3", "cwd": "/notes", "agent_status": "something new"},
        {"pane_id": "w3:p2", "tab_id": "w3:t2", "workspace_id": "w3", "cwd": "/notes", "agent": "claude", "agent_status": "idle", "terminal_title_stripped": "task-2vp"}
      ]
    }"#;

    fn snapshot() -> Snapshot {
        serde_json::from_str(SNAPSHOT).unwrap()
    }

    fn checkouts() -> HashMap<PathBuf, Option<Checkout>> {
        HashMap::from([
            (
                PathBuf::from("/work/atlas"),
                Some(Checkout {
                    root: PathBuf::from("/work/atlas"),
                    common: PathBuf::from("/work/atlas/.git"),
                    branch: Some("main".into()),
                }),
            ),
            (
                PathBuf::from("/work/atlas.fix-login"),
                Some(Checkout {
                    root: PathBuf::from("/work/atlas.fix-login"),
                    common: PathBuf::from("/work/atlas/.git"),
                    branch: Some("fix-login".into()),
                }),
            ),
            (PathBuf::from("/notes"), None),
        ])
    }

    #[test]
    fn a_worktree_has_the_name_of_its_project_and_its_branch() {
        let model = Model::build(&snapshot(), &checkouts());
        let labels: Vec<String> = model.spaces.iter().map(Space::label).collect();
        assert_eq!(labels, ["atlas", "atlas ▸ fix-login", "my notes"]);
        assert_eq!(model.spaces[1].project(), "atlas");
    }

    #[test]
    fn the_agents_that_need_the_user_come_first() {
        let model = Model::build(&snapshot(), &checkouts());
        let agents: Vec<(&str, &str, &str)> = model
            .agents
            .iter()
            .map(|agent| {
                (
                    agent.name.as_str(),
                    agent.kind.as_str(),
                    agent.place.as_str(),
                )
            })
            .collect();
        assert_eq!(
            agents,
            [
                ("fix the login", "codex", "atlas ▸ fix-login"),
                ("omnibar design", "claude", "atlas"),
                ("task-2vp", "claude", "my notes"),
            ]
        );
        assert_eq!(model.agents[0].status, Status::Blocked);
    }

    #[test]
    fn a_tab_without_an_agent_is_a_tab_row() {
        let model = Model::build(&snapshot(), &checkouts());
        let tabs: Vec<(&str, &str, &str)> = model
            .tabs
            .iter()
            .map(|tab| (tab.name.as_str(), tab.command.as_str(), tab.place.as_str()))
            .collect();
        assert_eq!(
            tabs,
            [
                ("logs", "tail -f observer.log", "atlas"),
                ("shell", "", "my notes")
            ]
        );
    }

    #[test]
    fn an_unknown_status_is_unknown() {
        assert_eq!(snapshot().panes[3].agent_status, Status::Unknown);
    }

    #[test]
    fn the_folders_of_the_workspaces_have_no_duplicate() {
        assert_eq!(
            folders(&snapshot()),
            [
                PathBuf::from("/work/atlas"),
                PathBuf::from("/work/atlas.fix-login"),
                PathBuf::from("/notes")
            ]
        );
    }

    #[test]
    fn a_bare_repository_gives_its_name_without_git() {
        let checkout = Checkout {
            root: PathBuf::from("/work/atlas.main"),
            common: PathBuf::from("/work/atlas.git"),
            branch: None,
        };
        assert_eq!(checkout.project(), "atlas");
        assert!(!checkout.is_main());
    }

    #[test]
    fn the_checkout_of_this_repository_is_found() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let checkout = checkout_of(&here).expect("the repository of the plugin");
        assert_eq!(checkout.root, here);
        assert!(checkout.is_main());
    }
}
