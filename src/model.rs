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
    /// The agents that herdr detects. `name` is the herdr name of a named agent, and
    /// `state_change_seq` is its last activity.
    #[serde(default)]
    pub agents: Vec<AgentInfo>,
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    /// The focused tab, for the order of use (criterion visit).
    #[serde(default)]
    pub focused_tab_id: Option<String>,
}

/// An agent of the snapshot, by pane: its herdr name and its last activity.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentInfo {
    pub pane_id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// A counter of herdr that grows at each change of state of an agent: the last activity.
    #[serde(default)]
    pub state_change_seq: Option<u64>,
}

impl Snapshot {
    /// The herdr name of the agent of a pane (`herdr agent start <name>`), if it has one.
    pub fn agent_name(&self, pane_id: &str) -> Option<&str> {
        self.agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
            .and_then(|agent| agent.name.as_deref())
            .filter(|name| !name.is_empty())
    }
}

/// The names that a terminal title can give to an agent, besides its kind.
const AGENT_NAMES: [&str; 4] = ["claude", "claude code", "codex", "opencode"];

/// The known names of the agents, then the kind of this agent.
fn agent_names(kind: &str) -> impl Iterator<Item = &str> {
    AGENT_NAMES
        .into_iter()
        .chain(Some(kind).filter(|kind| !kind.is_empty()))
}

/// True if the title is the name of the agent, in any case: `Claude Code`, `OpenCode`.
pub fn is_agent_name(title: &str, kind: &str) -> bool {
    agent_names(kind).any(|name| name.eq_ignore_ascii_case(title))
}

/// True if the title of the terminal of an agent is noise, not a task: the herdr name of the
/// agent, or the command line of the agent (an agent name, a space, then an option:
/// `claude --name x`). Claude Code writes its `--name` in the title for all the session. The
/// comparisons ignore the case, as `is_agent_name` does.
pub fn is_agent_noise(title: &str, kind: &str, name: Option<&str>) -> bool {
    let title = title.trim();
    if name.is_some_and(|name| name.eq_ignore_ascii_case(title)) {
        return true;
    }
    agent_names(kind).any(|command| {
        title
            .get(..command.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(command))
            && title[command.len()..].starts_with(" -")
    })
}

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub agent_status: Status,
    /// The git worktree data that herdr gives for a workspace in a git repository.
    #[serde(default)]
    pub worktree: Option<Box<Worktree>>,
}

/// The git worktree data of a workspace, as herdr gives it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Worktree {
    /// The key of the repository: the sidebar groups the workspaces with the same key.
    #[serde(default)]
    pub repo_key: String,
    #[serde(default)]
    pub repo_name: String,
    #[serde(default)]
    pub repo_root: Option<PathBuf>,
    #[serde(default)]
    pub checkout_path: Option<PathBuf>,
    #[serde(default)]
    pub is_linked_worktree: bool,
}

impl Workspace {
    /// The name that the user gave, if there is one. herdr does not tell it: a label is a user
    /// label if it is not the name of the repository, not the folder of the checkout and not the
    /// folder of the repository root. A workspace outside git has no such default.
    pub fn user_label(&self) -> Option<&str> {
        let worktree = self.worktree.as_ref()?;
        let label = self.label.trim();
        let folder = |path: &Option<PathBuf>| path.as_deref().map(file_name);
        let is_default = label.is_empty()
            || label == worktree.repo_name
            || folder(&worktree.checkout_path).is_some_and(|name| name == label)
            || folder(&worktree.repo_root).is_some_and(|name| name == label);
        (!is_default).then_some(label)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    /// The position of the tab in its workspace. herdr names a new tab with this number.
    #[serde(default)]
    pub number: Option<u32>,
}

impl Tab {
    /// True if the tab has the name that herdr gives: its number. A name that the user gives and
    /// that is equal to the number looks the same.
    pub fn has_default_label(&self) -> bool {
        match self.number {
            Some(number) => self.label == number.to_string(),
            None => self.label.is_empty(),
        }
    }
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
    /// The label that herdr sets after `herdr pane rename`.
    #[serde(default)]
    pub label: Option<String>,
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

    /// The folder of the main checkout: the parent of the common git folder. A bare repository
    /// has no main checkout: its common folder stands for it.
    pub fn main(&self) -> PathBuf {
        if self.is_main() {
            return self.root.clone();
        }
        match self.common.parent() {
            Some(parent) if self.common.file_name().is_some_and(|name| name == ".git") => {
                parent.to_path_buf()
            }
            _ => self.common.clone(),
        }
    }

    /// The name of the project: the folder of the main checkout.
    pub fn project(&self) -> String {
        let name = file_name(&self.main());
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
    /// The project of the workspace: its git repository. For a linked worktree without a checkout,
    /// the repository that herdr gives: its label is the branch. Else the name that herdr gives to
    /// the workspace, else its folder.
    pub fn project(&self) -> String {
        if let Some(checkout) = &self.checkout {
            return checkout.project();
        }
        let linked_repo = self
            .workspace
            .worktree
            .as_ref()
            .filter(|worktree| worktree.is_linked_worktree)
            .map(|worktree| &worktree.repo_name);
        if let Some(name) = linked_repo.filter(|name| !name.is_empty()) {
            return name.clone();
        }
        if !self.workspace.label.is_empty() {
            return self.workspace.label.clone();
        }
        self.folder.as_deref().map(file_name).unwrap_or_default()
    }

    /// The name of the workspace: the project, or `<project> ▸ <branch>` for a linked worktree.
    /// A name that the user gave replaces the project of a main workspace and the branch of a
    /// linked worktree.
    pub fn label(&self) -> String {
        let user = self.workspace.user_label();
        let linked = match (&self.checkout, &self.workspace.worktree) {
            (Some(checkout), _) => !checkout.is_main(),
            (None, Some(worktree)) => worktree.is_linked_worktree,
            (None, None) => false,
        };
        if !linked {
            return user.map(str::to_string).unwrap_or_else(|| self.project());
        }
        let branch = match (user, &self.checkout) {
            (Some(label), _) => label.to_string(),
            (None, Some(checkout)) => checkout
                .branch
                .clone()
                .unwrap_or_else(|| file_name(&checkout.root)),
            (None, None) => self.workspace.label.clone(),
        };
        format!("{} ▸ {}", self.project(), branch)
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
    /// The folder of the pane of the agent.
    pub cwd: Option<PathBuf>,
    /// The last change of state of the agent (`state_change_seq` of herdr): the highest is the
    /// last activity.
    pub seq: Option<u64>,
}

/// A pane of a tab (filter Panes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneRow {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    /// The title of the terminal of the pane, or "shell" for a prompt.
    pub name: String,
    /// The name of the tab of the pane.
    pub tab: String,
    pub place: String,
    /// The kind of the agent of the pane, if it has one.
    pub agent: Option<String>,
    pub status: Status,
    /// The last change of state of the agent of the pane (`state_change_seq` of herdr).
    pub seq: Option<u64>,
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
    /// The first pane of the tab: the preview reads it.
    pub pane_id: Option<String>,
}

/// A tab of a workspace, for the preview of the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSummary {
    pub name: String,
    /// The kind of the agent of the tab, if the tab has an agent.
    pub agent: Option<String>,
    pub status: Status,
}

/// All the data of the popup.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub spaces: Vec<Space>,
    pub agents: Vec<AgentRow>,
    pub tabs: Vec<TabRow>,
    /// Each pane of each tab (filter Panes).
    pub panes: Vec<PaneRow>,
    pub focused_workspace_id: Option<String>,
    /// The workspaces in the flat order of the snapshot: the sidebar order comes from it.
    pub flat_workspaces: Vec<Workspace>,
    /// The number of each tab in its workspace, by tab id: the order of herdr.
    pub tab_numbers: HashMap<String, u32>,
    /// The position of each pane in the snapshot, by pane id: the order of herdr.
    pub pane_index: HashMap<String, usize>,
    /// The tabs of each workspace, by workspace id, in their order.
    pub workspace_tabs: HashMap<String, Vec<TabSummary>>,
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
        // is a better name, because an agent writes its task there. The title of an idle shell
        // is its prompt (user@host:folder): the name is then "shell".
        let label_of = |tab_id: &str, title: Option<&str>, title_pane: Option<&str>| {
            let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id);
            let label = tab.map(|tab| tab.label.clone()).unwrap_or_default();
            let numbered = tab.is_none_or(Tab::has_default_label);
            // The pane of the title, if it holds an agent: the herdr name and the command of this
            // agent are not a task.
            let agent_pane = title_pane.and_then(|id| {
                snapshot
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == id && pane.agent.is_some())
            });
            let noise = |title: &str| {
                agent_pane.is_some_and(|pane| {
                    is_agent_noise(
                        title,
                        pane.agent.as_deref().unwrap_or_default(),
                        snapshot.agent_name(&pane.pane_id),
                    )
                })
            };
            match title.map(str::trim).filter(|title| !title.is_empty()) {
                Some(title) if numbered && is_prompt_title(title) => "shell".to_string(),
                Some(title) if numbered && !noise(title) => title.to_string(),
                _ => label,
            }
        };
        // The last activity of the agent of each pane. The first entry of a pane wins.
        let mut seqs: HashMap<&str, Option<u64>> = HashMap::new();
        for info in &snapshot.agents {
            seqs.entry(info.pane_id.as_str())
                .or_insert(info.state_change_seq);
        }
        let seq_of = |pane_id: &str| seqs.get(pane_id).copied().flatten();
        let agents: Vec<AgentRow> = snapshot
            .panes
            .iter()
            .filter_map(|pane| {
                let kind = pane.agent.clone()?;
                Some(AgentRow {
                    pane_id: pane.pane_id.clone(),
                    tab_id: pane.tab_id.clone(),
                    workspace_id: pane.workspace_id.clone(),
                    name: label_of(
                        &pane.tab_id,
                        pane.terminal_title_stripped.as_deref(),
                        Some(&pane.pane_id),
                    ),
                    kind,
                    status: pane.agent_status,
                    place: place_of(&pane.workspace_id),
                    cwd: pane.cwd.clone(),
                    seq: seq_of(&pane.pane_id),
                })
            })
            .collect();

        let panes = snapshot
            .panes
            .iter()
            .map(|pane| {
                let title = pane
                    .terminal_title_stripped
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default();
                let label = pane
                    .label
                    .as_deref()
                    .map(str::trim)
                    .filter(|l| !l.is_empty());
                let name = if let Some(label) = label {
                    label.to_string()
                } else if title.is_empty() || is_prompt_title(title) {
                    pane.agent.clone().unwrap_or_else(|| "shell".to_string())
                } else {
                    title.to_string()
                };
                PaneRow {
                    pane_id: pane.pane_id.clone(),
                    tab_id: pane.tab_id.clone(),
                    workspace_id: pane.workspace_id.clone(),
                    name,
                    tab: label_of(
                        &pane.tab_id,
                        pane.terminal_title_stripped.as_deref(),
                        Some(&pane.pane_id),
                    ),
                    place: place_of(&pane.workspace_id),
                    agent: pane.agent.clone(),
                    status: pane.agent_status,
                    seq: pane.agent.as_ref().and_then(|_| seq_of(&pane.pane_id)),
                }
            })
            .collect();
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
                let first = snapshot.panes.iter().find(|pane| pane.tab_id == tab.tab_id);
                let title = first
                    .and_then(|pane| pane.terminal_title_stripped.clone())
                    .unwrap_or_default();
                // The title of a shell that waits is its prompt: no command runs.
                let command = if is_prompt_title(&title) {
                    String::new()
                } else {
                    title.clone()
                };
                TabRow {
                    tab_id: tab.tab_id.clone(),
                    workspace_id: tab.workspace_id.clone(),
                    name: label_of(&tab.tab_id, Some(&title), None),
                    command,
                    place: place_of(&tab.workspace_id),
                    pane_id: first.map(|pane| pane.pane_id.clone()),
                }
            })
            .collect();
        let mut workspace_tabs: HashMap<String, Vec<TabSummary>> = HashMap::new();
        for tab in &snapshot.tabs {
            let agent_pane = snapshot
                .panes
                .iter()
                .find(|pane| pane.tab_id == tab.tab_id && pane.agent.is_some());
            let first = snapshot.panes.iter().find(|pane| pane.tab_id == tab.tab_id);
            let title = first.and_then(|pane| pane.terminal_title_stripped.as_deref());
            workspace_tabs
                .entry(tab.workspace_id.clone())
                .or_default()
                .push(TabSummary {
                    name: label_of(&tab.tab_id, title, first.map(|pane| pane.pane_id.as_str())),
                    agent: agent_pane.and_then(|pane| pane.agent.clone()),
                    status: agent_pane.map(|pane| pane.agent_status).unwrap_or_default(),
                });
        }
        Model {
            spaces,
            agents,
            tabs,
            panes,
            focused_workspace_id: snapshot.focused_workspace_id.clone(),
            flat_workspaces: snapshot.workspaces.clone(),
            tab_numbers: snapshot
                .tabs
                .iter()
                .filter_map(|tab| Some((tab.tab_id.clone(), tab.number?)))
                .collect(),
            pane_index: snapshot
                .panes
                .iter()
                .enumerate()
                .map(|(index, pane)| (pane.pane_id.clone(), index))
                .collect(),
            workspace_tabs,
        }
    }
}

/// True for the title that a shell gives to its terminal while it waits: `user@host:folder`.
pub fn is_prompt_title(title: &str) -> bool {
    title
        .split_once(':')
        .is_some_and(|(head, _)| head.contains('@') && !head.contains(char::is_whitespace))
}

/// The model of the running herdr: its snapshot, and the checkout of each folder.
pub fn load() -> Result<Model, crate::herdr::Error> {
    load_cached(&mut HashMap::new())
}

/// The model of the running herdr. The cache keeps the checkout of each folder: git runs only for
/// the new folders.
pub fn load_cached(
    cache: &mut HashMap<PathBuf, Option<Checkout>>,
) -> Result<Model, crate::herdr::Error> {
    let snapshot = crate::herdr::snapshot()?;
    let missing: Vec<PathBuf> = folders(&snapshot)
        .into_iter()
        .filter(|folder| !cache.contains_key(folder))
        .collect();
    cache.extend(checkouts_of(&missing));
    Ok(Model::build(&snapshot, cache))
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
        let mut model = Model::build(&snapshot(), &checkouts());
        let order: Vec<String> = ["attention"].iter().map(|s| s.to_string()).collect();
        crate::rank::Ranker::new(&model, &order, &HashMap::new(), None, None).sort(&mut model);
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
    fn the_seq_of_an_agent_comes_from_the_agents_of_the_snapshot() {
        let mut snapshot = snapshot();
        snapshot.agents = vec![AgentInfo {
            pane_id: "w1:p1".into(),
            name: None,
            state_change_seq: Some(785),
        }];
        let model = Model::build(&snapshot, &checkouts());
        let seqs: Vec<(String, Option<u64>)> = model
            .agents
            .iter()
            .map(|agent| (agent.pane_id.clone(), agent.seq))
            .collect();
        assert!(seqs.contains(&("w1:p1".to_string(), Some(785))));
        assert!(seqs.contains(&("w2:p1".to_string(), None)));
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

    fn git_workspace(label: &str, linked: bool, checkout: &str) -> Workspace {
        serde_json::from_value(serde_json::json!({
            "workspace_id": "w9",
            "label": label,
            "worktree": {
                "repo_key": "/work/atlas/.git",
                "repo_name": "atlas",
                "repo_root": "/work/atlas",
                "checkout_path": checkout,
                "is_linked_worktree": linked
            }
        }))
        .unwrap()
    }

    #[test]
    fn a_default_label_is_not_a_user_label() {
        assert_eq!(
            git_workspace("atlas", false, "/work/atlas").user_label(),
            None
        );
        assert_eq!(
            git_workspace("atlas.fix", true, "/work/atlas.fix").user_label(),
            None
        );
        assert_eq!(
            git_workspace("billing", false, "/work/atlas").user_label(),
            Some("billing")
        );
        // A workspace outside git has no worktree data: no user label.
        let plain: Workspace =
            serde_json::from_str(r#"{"workspace_id": "w1", "label": "notes"}"#).unwrap();
        assert_eq!(plain.user_label(), None);
    }

    #[test]
    fn a_user_label_shows_in_the_name_of_a_git_workspace() {
        let checkout = |root: &str, branch: &str| Checkout {
            root: PathBuf::from(root),
            common: PathBuf::from("/work/atlas/.git"),
            branch: Some(branch.into()),
        };
        let space = |workspace, checkout| Space {
            workspace,
            folder: None,
            checkout: Some(checkout),
        };
        let main = space(
            git_workspace("billing", false, "/work/atlas"),
            checkout("/work/atlas", "main"),
        );
        assert_eq!(main.label(), "billing");
        assert_eq!(main.project(), "atlas");
        let linked = space(
            git_workspace("my fix", true, "/work/atlas.fix"),
            checkout("/work/atlas.fix", "fix"),
        );
        assert_eq!(linked.label(), "atlas ▸ my fix");
        let default = space(
            git_workspace("atlas.fix", true, "/work/atlas.fix"),
            checkout("/work/atlas.fix", "fix"),
        );
        assert_eq!(default.label(), "atlas ▸ fix");
    }

    #[test]
    fn a_linked_worktree_without_a_checkout_takes_the_repository_of_herdr_as_its_project() {
        let space = |label| Space {
            workspace: git_workspace(label, true, "/work/atlas.fix"),
            folder: None,
            checkout: None,
        };
        assert_eq!(space("atlas.fix").project(), "atlas");
        assert_eq!(space("atlas.fix").label(), "atlas ▸ atlas.fix");
        assert_eq!(space("my fix").label(), "atlas ▸ my fix");
        // A main workspace without a checkout keeps its herdr label: 2 checkouts of one repository
        // keep 2 names.
        let main = Space {
            workspace: git_workspace("atlas-2", false, "/work/atlas-2"),
            folder: None,
            checkout: None,
        };
        assert_eq!(main.project(), "atlas-2");
    }

    #[test]
    fn the_label_of_a_pane_is_its_name() {
        let mut snapshot = snapshot();
        snapshot.panes[1].label = Some("logs".into());
        let model = Model::build(&snapshot, &checkouts());
        let pane = model
            .panes
            .iter()
            .find(|pane| pane.pane_id == "w1:p2")
            .unwrap();
        assert_eq!(pane.name, "logs");
    }

    #[test]
    fn the_prompt_of_a_shell_is_not_a_name() {
        assert!(is_prompt_title("thomas@mbp:~/Workspace/atlas"));
        assert!(!is_prompt_title("tail -f observer.log"));
        assert!(!is_prompt_title("Fix: the login"));
        assert!(!is_prompt_title("mail me@host: later"));
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
    fn a_linked_worktree_gives_the_folder_of_its_main_checkout() {
        let worktree = Checkout {
            root: PathBuf::from("/work/atlas.fix-login"),
            common: PathBuf::from("/work/atlas/.git"),
            branch: Some("fix-login".into()),
        };
        assert_eq!(worktree.main(), PathBuf::from("/work/atlas"));
        assert_eq!(worktree.project(), "atlas");
        let main = Checkout {
            root: PathBuf::from("/work/atlas"),
            common: PathBuf::from("/work/atlas/.git"),
            branch: Some("main".into()),
        };
        assert_eq!(main.main(), PathBuf::from("/work/atlas"));
    }

    #[test]
    fn the_checkout_of_this_repository_is_found() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let checkout = checkout_of(&here).expect("the repository of the plugin");
        assert_eq!(checkout.root, here);
        // The tests also run in a linked worktree: it has a `.git` file, not a folder.
        assert_eq!(checkout.is_main(), here.join(".git").is_dir());
    }
}
