//! The creation of a tab or a worktree: the choices of the step "what", the plan of a choice, and
//! its execution after the popup closes.
//!
//! A worktree always gets its own workspace: `wt switch --create`, then `herdr worktree open`. A
//! tab goes into the workspace of the place, or into a new workspace for a place without one.

use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::herdr;
use crate::places::{self, Branch, Place};

/// A choice of the step "what". The name is the text of the step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A new branch and its worktree, in its own workspace.
    NewWorktree { branch: String },
    /// A branch that exists: go to its workspace, or open its worktree. `main` is the main
    /// checkout of its project: the filter Worktrees shows the branches of several projects.
    Existing {
        branch: String,
        worktree: Option<PathBuf>,
        workspace_id: Option<String>,
        main: PathBuf,
    },
    /// A new tab in the workspace of the place.
    NewTab,
    /// A new pane next to the focused pane of the current workspace: a split.
    NewSplit { pane_id: String },
    /// A new workspace for a place without one, with its first tab.
    NewWorkspace,
    /// Go back to the step "where".
    AnotherPlace,
}

/// True for a valid name of a new git branch.
pub fn is_branch_name(name: &str) -> bool {
    let mut characters = name.chars();
    let first_ok = characters.next().is_some_and(|c| c.is_ascii_alphanumeric());
    first_ok
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
        && !name.contains("..")
        && !name.contains("//")
        && !name.ends_with('/')
        && !name.ends_with(".lock")
}

/// The choices of the step "what" for a place and a name. In a git project, the worktree comes
/// first. `split` is the focused pane of the current workspace, if the place is this workspace: a
/// split goes next to it.
pub fn choices(
    place: &Place,
    name: &str,
    branches: &[Branch],
    open: &[Place],
    split: Option<&str>,
) -> Vec<Choice> {
    let mut out = Vec::new();
    if let Some(main) = &place.main {
        let name = name.trim();
        match branches
            .iter()
            .find(|branch| !name.is_empty() && branch.name == name)
        {
            Some(branch) => out.push(Choice::Existing {
                branch: branch.name.clone(),
                worktree: branch.worktree.clone(),
                workspace_id: branch.worktree.as_ref().and_then(|worktree| {
                    open.iter()
                        .find(|other| &other.folder == worktree)
                        .and_then(|other| other.workspace_id.clone())
                }),
                main: main.clone(),
            }),
            None => out.push(Choice::NewWorktree {
                branch: name.to_string(),
            }),
        }
    }
    // A place with a workspace gets a tab, a split and a new workspace in its folder. A place
    // without a workspace gets the new workspace.
    if place.workspace_id.is_some() {
        out.push(Choice::NewTab);
    }
    if let (Some(_), Some(pane_id)) = (&place.workspace_id, split) {
        out.push(Choice::NewSplit {
            pane_id: pane_id.to_string(),
        });
    }
    out.push(Choice::NewWorkspace);
    out.push(Choice::AnotherPlace);
    out
}

/// What the omnibar does after the popup closes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    NewTab {
        workspace_id: String,
        folder: PathBuf,
    },
    /// A new pane next to a pane: to the right of a wide pane, below a tall pane.
    NewSplit {
        pane_id: String,
        folder: PathBuf,
    },
    NewWorkspace {
        folder: PathBuf,
        label: String,
    },
    /// wt switch, with --create for a new branch, then the workspace of the worktree.
    Worktree {
        main: PathBuf,
        branch: String,
        create: bool,
    },
    Focus {
        workspace_id: String,
    },
}

/// A creation: its action, the name of the tab, the agent, and the flag without prompts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub action: Action,
    pub name: String,
    pub agent: String,
    pub no_prompts: bool,
    /// The folder that remembers the agent.
    pub folder: PathBuf,
}

/// The plan of a choice, or None if the choice cannot run (no branch name, a wrong name).
pub fn plan(
    place: &Place,
    choice: &Choice,
    name: &str,
    agent: &str,
    no_prompts: bool,
) -> Option<Plan> {
    let mut name = name.trim().to_string();
    let action = match choice {
        Choice::NewWorktree { branch } if is_branch_name(branch) => Action::Worktree {
            main: place.main.clone()?,
            branch: branch.clone(),
            create: true,
        },
        Choice::NewWorktree { .. } | Choice::AnotherPlace => return None,
        Choice::Existing {
            workspace_id: Some(workspace_id),
            ..
        } => Action::Focus {
            workspace_id: workspace_id.clone(),
        },
        Choice::Existing { branch, main, .. } => Action::Worktree {
            main: main.clone(),
            branch: branch.clone(),
            create: false,
        },
        Choice::NewSplit { pane_id } => Action::NewSplit {
            pane_id: pane_id.clone(),
            folder: place.folder.clone(),
        },
        Choice::NewTab => Action::NewTab {
            workspace_id: place.workspace_id.clone()?,
            folder: place.folder.clone(),
        },
        Choice::NewWorkspace => {
            // A place that has a workspace: the typed name is the label of the new workspace, and
            // the automatic titles name its first tab.
            let label = if place.labels_new_workspace() && !name.is_empty() {
                std::mem::take(&mut name)
            } else {
                place.label.clone()
            };
            Action::NewWorkspace {
                folder: place.folder.clone(),
                label,
            }
        }
    };
    Some(Plan {
        action,
        name,
        agent: agent.to_string(),
        no_prompts,
        folder: place.folder.clone(),
    })
}

/// The message of the row "new workspace…" when no text is typed.
pub const NAME_MISSING: &str = "type the name of the new workspace";

/// The plan of the row "new workspace “text”": a workspace in ~, with the text as its label. No
/// plan without a text.
pub fn new_workspace_plan(label: &str, agent: &str, no_prompts: bool) -> Option<Plan> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let folder = places::home_folder();
    Some(Plan {
        action: Action::NewWorkspace {
            folder: folder.clone(),
            label: label.to_string(),
        },
        name: String::new(),
        agent: agent.to_string(),
        no_prompts,
        folder,
    })
}

/// The agents on the PATH, then "shell".
pub fn installed_agents() -> Vec<String> {
    // The demo shows the 3 agents, on any machine.
    if crate::demo::active() {
        return ["claude", "codex", "opencode", "shell"]
            .map(String::from)
            .to_vec();
    }
    let path = env::var_os("PATH").unwrap_or_default();
    let mut agents: Vec<String> = ["claude", "codex", "opencode"]
        .into_iter()
        .filter(|agent| env::split_paths(&path).any(|folder| folder.join(agent).is_file()))
        .map(str::to_string)
        .collect();
    agents.push("shell".to_string());
    agents
}

/// The arguments of an agent that start it without its permission prompts.
pub fn no_prompt_arguments(agent: &str) -> &'static [&'static str] {
    match agent {
        "claude" => &["--dangerously-skip-permissions"],
        "codex" => &["--dangerously-bypass-approvals-and-sandbox"],
        _ => &[],
    }
}

/// The variable that gives the plan (JSON) to the detached process `execute`.
pub const PLAN_VARIABLE: &str = "OMNIBAR_PLAN";

/// Run a plan in a detached process, then return at once: the popup closes now. A creation can
/// take some seconds (wt, the start of the agent): a popup that waits shows a black screen and
/// takes no key.
pub fn execute_detached(plan: &Plan) -> Result<(), String> {
    let json = serde_json::to_string(plan).map_err(|error| error.to_string())?;
    spawn_detached(&["execute"], &[(PLAN_VARIABLE, &json)])
}

/// The variable that gives the process id of the popup to its detached processes.
const POPUP_VARIABLE: &str = "OMNIBAR_POPUP_PID";

/// Start this binary with these arguments in a detached process: it goes on after the popup
/// closes. The process gets the process id of the popup, to wait for its end.
pub fn spawn_detached(arguments: &[&str], environment: &[(&str, &str)]) -> Result<(), String> {
    let executable =
        env::current_exe().map_err(|error| format!("cannot find the binary: {error}"))?;
    Command::new(executable)
        .args(arguments)
        .envs(environment.iter().copied())
        .env(POPUP_VARIABLE, std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", arguments.join(" ")))?;
    Ok(())
}

/// The detached process `invoke`: wait until the popup closes (its process ends), then run the
/// action. herdr refuses a popup while another popup is open.
pub fn invoke_later(plugin_id: &str, action_id: &str) -> Result<(), String> {
    wait_for_popup();
    herdr::invoke_action(plugin_id, action_id).map_err(|error| notify_failure(&error.to_string()))
}

/// The plugin that forks the session of an agent, and its action that forks into a new tab.
pub const FORK_PLUGIN: &str = "herdr-fork";
const FORK_ACTION: &str = "tab";

/// The detached process `fork`: wait until the popup closes, focus the agent, then fork its
/// session into a new tab with herdr-fork. herdr-fork forks the agent of the focused pane.
pub fn fork_later(tab_id: &str, pane_id: &str) -> Result<(), String> {
    wait_for_popup();
    herdr::focus_tab(tab_id)
        .and_then(|()| herdr::focus_agent(pane_id))
        .and_then(|()| {
            // herdr gives the new focus to the context of the next action.
            std::thread::sleep(std::time::Duration::from_millis(150));
            herdr::invoke_action(FORK_PLUGIN, FORK_ACTION)
        })
        .map_err(|error| notify_failure(&error.to_string()))
}

/// Wait until the popup closes: herdr refuses a popup or a focus change while the popup is open
/// (error `ui_busy`). The popup is the parent of this process until its end. The popup often ends
/// before this process starts: then its parent is already another process, and the wait stops at
/// once.
fn wait_for_popup() {
    use std::os::unix::process::parent_id;
    use std::thread::sleep;
    use std::time::Duration;
    let popup = env::var(POPUP_VARIABLE)
        .ok()
        .and_then(|text| text.parse::<u32>().ok());
    // At most 2 s: a popup that does not end must not block the action for ever.
    for _ in 0..400 {
        if popup != Some(parent_id()) {
            break;
        }
        sleep(Duration::from_millis(5));
    }
    // herdr removes the popup about 10 ms after the end of its process: a small margin.
    sleep(Duration::from_millis(20));
}

/// Show a failure in a herdr notification, and give its message back.
fn notify_failure(message: &str) -> String {
    let _ = herdr::show_notification("herdr-omnibar: the action failed", message);
    message.to_string()
}

/// The detached process `execute`: run the plan of OMNIBAR_PLAN. The popup is closed: an error
/// goes to a herdr notification.
pub fn execute_from_environment() -> Result<(), String> {
    let json = env::var(PLAN_VARIABLE).map_err(|_| format!("{PLAN_VARIABLE} is missing"))?;
    let plan: Plan =
        serde_json::from_str(&json).map_err(|error| format!("{PLAN_VARIABLE}: {error}"))?;
    execute(&plan).inspect_err(|error| {
        let _ = herdr::show_notification("herdr-omnibar: the creation failed", error);
    })
}

/// Run a plan: create the tab, the workspace or the worktree, then start the agent in its pane.
pub fn execute(plan: &Plan) -> Result<(), String> {
    let text = |error: herdr::Error| error.to_string();
    let pane = match &plan.action {
        Action::Focus { workspace_id } => {
            return herdr::focus_workspace(workspace_id).map_err(text);
        }
        Action::NewTab {
            workspace_id,
            folder,
        } => {
            let label = (!plan.name.is_empty()).then_some(plan.name.as_str());
            herdr::create_tab(workspace_id, folder, label).map_err(text)?
        }
        Action::NewSplit { pane_id, folder } => {
            let pane = herdr::split_pane(pane_id, folder).map_err(text)?;
            if !plan.name.is_empty() {
                herdr::rename_pane(&pane, &plan.name).map_err(text)?;
            }
            pane
        }
        Action::NewWorkspace { folder, label } => {
            let (tab, pane) = herdr::create_workspace(folder, label).map_err(text)?;
            if !plan.name.is_empty() {
                herdr::rename_tab(&tab, &plan.name).map_err(text)?;
            }
            pane
        }
        Action::Worktree {
            main,
            branch,
            create,
        } => {
            let worktree = wt_switch(main, branch, *create)?;
            herdr::open_worktree(main, &worktree).map_err(text)?
        }
    };
    remember_agent(&plan.folder, &plan.agent);
    if plan.agent == "shell" {
        return Ok(());
    }
    let arguments = if plan.no_prompts {
        no_prompt_arguments(&plan.agent)
    } else {
        &[]
    };
    herdr::start_agent(&agent_name(plan), &plan.agent, &pane, arguments).map_err(text)
}

/// A name for herdr agent start: the agent, the name or the branch, and a number.
fn agent_name(plan: &Plan) -> String {
    let base = match &plan.action {
        Action::Worktree { branch, .. } => branch.clone(),
        // A new workspace with a label and without a tab name: the agent takes the label.
        Action::NewWorkspace { label, .. } if plan.name.is_empty() => label.clone(),
        _ if !plan.name.is_empty() => plan.name.clone(),
        _ => plan
            .folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let slug: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let number = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_millis() % 10_000)
        .unwrap_or(0);
    format!(
        "{}-{}-{number}",
        plan.agent,
        slug.chars().take(24).collect::<String>()
    )
}

/// Run wt in a repository, without a terminal: stdin is closed, so wt never waits for an answer.
/// Return the standard output, or the last line of the error of wt.
pub fn wt(main: &Path, arguments: &[&OsStr]) -> Result<Vec<u8>, String> {
    let name = arguments
        .iter()
        .take(2)
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let output = Command::new("wt")
        .arg("-C")
        .arg(main)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("cannot run wt: {error}"))?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        let last = details
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no message");
        return Err(format!("wt {name}: {}", last.trim()));
    }
    Ok(output.stdout)
}

/// wt switch, without a terminal: return the folder of the worktree. The hooks of wt run.
fn wt_switch(main: &Path, branch: &str, create: bool) -> Result<PathBuf, String> {
    let mut arguments: Vec<&OsStr> = vec![OsStr::new("switch")];
    if create {
        arguments.push(OsStr::new("--create"));
    }
    arguments.extend([branch, "--no-cd", "--format", "json"].map(OsStr::new));
    let stdout = wt(main, &arguments)?;
    let result: serde_json::Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("wt switch: invalid JSON: {error}"))?;
    result["path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "wt switch gave no path".to_string())
}

/// The file that keeps the last agent of each folder, in the state folder of the plugin.
fn agents_file() -> Option<PathBuf> {
    Some(crate::config::state_dir().join("agents.json"))
}

fn read_agents(file: &Path) -> HashMap<String, String> {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The last agent of each folder that the omnibar knows, by folder. The popup reads the file one
/// time: it does not read the disk at each drawing.
pub fn remembered_agents() -> HashMap<String, String> {
    agents_file()
        .map(|file| read_agents(&file))
        .unwrap_or_default()
}

fn remember_agent(folder: &Path, agent: &str) {
    let Some(file) = agents_file() else {
        return;
    };
    let mut agents = read_agents(&file);
    agents.insert(folder.display().to_string(), agent.to_string());
    if let Ok(text) = serde_json::to_string_pretty(&agents) {
        let _ = fs::create_dir_all(file.parent().unwrap_or(Path::new(".")));
        let _ = fs::write(&file, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::places::Kind;

    fn workspace(label: &str, folder: &str, id: &str, main: Option<&str>) -> Place {
        Place {
            kind: Kind::Workspace,
            label: label.into(),
            project: label.into(),
            folder: folder.into(),
            workspace_id: Some(id.into()),
            main: main.map(PathBuf::from),
            tabs: Vec::new(),
        }
    }

    fn branches() -> Vec<Branch> {
        vec![
            Branch {
                name: "main".into(),
                worktree: Some("/work/atlas".into()),
                time: None,
            },
            Branch {
                name: "fix-login".into(),
                worktree: Some("/work/atlas.fix-login".into()),
                time: None,
            },
            Branch {
                name: "old".into(),
                worktree: None,
                time: None,
            },
        ]
    }

    #[test]
    fn a_branch_name_has_no_space_and_no_double_dot() {
        assert!(is_branch_name("fix-login"));
        assert!(is_branch_name("feat/omnibar.v2"));
        assert!(!is_branch_name("review the omnibar"));
        assert!(!is_branch_name("a..b"));
        assert!(!is_branch_name("-x"));
        assert!(!is_branch_name(""));
    }

    #[test]
    fn in_a_project_the_worktree_comes_first_then_the_tab() {
        let atlas = workspace("atlas", "/work/atlas", "w1", Some("/work/atlas"));
        assert_eq!(
            choices(&atlas, "notes", &branches(), &[], None),
            [
                Choice::NewWorktree {
                    branch: "notes".into()
                },
                Choice::NewTab,
                Choice::NewWorkspace,
                Choice::AnotherPlace
            ]
        );
    }

    #[test]
    fn a_branch_that_exists_goes_to_its_workspace() {
        let atlas = workspace("atlas", "/work/atlas", "w1", Some("/work/atlas"));
        let fix = workspace(
            "atlas ▸ fix-login",
            "/work/atlas.fix-login",
            "w2",
            Some("/work/atlas"),
        );
        let choice = &choices(
            &atlas,
            "fix-login",
            &branches(),
            &[atlas.clone(), fix],
            None,
        )[0];
        assert_eq!(
            choice,
            &Choice::Existing {
                branch: "fix-login".into(),
                worktree: Some("/work/atlas.fix-login".into()),
                workspace_id: Some("w2".into()),
                main: "/work/atlas".into(),
            }
        );
        let plan = plan(&atlas, choice, "fix-login", "claude", false).unwrap();
        assert_eq!(
            plan.action,
            Action::Focus {
                workspace_id: "w2".into()
            }
        );
    }

    #[test]
    fn a_branch_without_worktree_opens_it_with_wt() {
        let atlas = workspace("atlas", "/work/atlas", "w1", Some("/work/atlas"));
        let choice = &choices(&atlas, "old", &branches(), &[], None)[0];
        let plan = plan(&atlas, choice, "old", "codex", false).unwrap();
        assert_eq!(
            plan.action,
            Action::Worktree {
                main: "/work/atlas".into(),
                branch: "old".into(),
                create: false
            }
        );
    }

    #[test]
    fn a_folder_outside_git_gets_a_workspace_and_no_worktree() {
        let notes = Place {
            kind: Kind::Folder,
            label: "notes".into(),
            project: "notes".into(),
            folder: "/notes".into(),
            workspace_id: None,
            main: None,
            tabs: Vec::new(),
        };
        assert_eq!(
            choices(&notes, "draft", &[], &[], None),
            [Choice::NewWorkspace, Choice::AnotherPlace]
        );
    }

    #[test]
    fn a_new_workspace_in_a_place_with_a_workspace_takes_the_typed_name_as_its_label() {
        let notes = workspace("~", "/home/u", "w1", None);
        let make = |name: &str| plan(&notes, &Choice::NewWorkspace, name, "claude", false).unwrap();
        // The typed name is the label. The automatic titles name the first tab.
        let named = make("ville france");
        assert_eq!(
            named.action,
            Action::NewWorkspace {
                folder: "/home/u".into(),
                label: "ville france".into()
            }
        );
        assert_eq!(named.name, "");
        // Without a name, the label of the place.
        let unnamed = make("");
        assert_eq!(
            unnamed.action,
            Action::NewWorkspace {
                folder: "/home/u".into(),
                label: "~".into()
            }
        );
        // A place without a workspace keeps the name for the first tab.
        let folder = Place {
            workspace_id: None,
            ..notes.clone()
        };
        let plan = plan(&folder, &Choice::NewWorkspace, "draft", "claude", false).unwrap();
        assert_eq!(plan.name, "draft");
        assert_eq!(
            plan.action,
            Action::NewWorkspace {
                folder: "/home/u".into(),
                label: "~".into()
            }
        );
    }

    #[test]
    fn the_row_new_workspace_makes_a_workspace_in_home_with_its_text_as_label() {
        // Without a text: no plan.
        assert!(new_workspace_plan("  ", "claude", false).is_none());
        // With a text: the label, and no name for the first tab.
        let plan = new_workspace_plan(" ville france ", "claude", true).unwrap();
        assert_eq!(plan.name, "");
        assert!(plan.no_prompts);
        assert_eq!(
            plan.action,
            Action::NewWorkspace {
                folder: places::home_folder(),
                label: "ville france".into()
            }
        );
        assert_eq!(plan.folder, places::home_folder());
    }

    #[test]
    fn the_current_workspace_also_offers_a_split_of_its_focused_pane() {
        let notes = workspace("notes", "/notes", "w1", None);
        assert_eq!(
            choices(&notes, "", &[], &[], Some("w1:p2")),
            [
                Choice::NewTab,
                Choice::NewSplit {
                    pane_id: "w1:p2".into()
                },
                Choice::NewWorkspace,
                Choice::AnotherPlace
            ]
        );
        let plan = plan(
            &notes,
            &Choice::NewSplit {
                pane_id: "w1:p2".into(),
            },
            "logs",
            "shell",
            false,
        )
        .unwrap();
        assert_eq!(
            plan.action,
            Action::NewSplit {
                pane_id: "w1:p2".into(),
                folder: "/notes".into()
            }
        );
    }

    #[test]
    fn a_wrong_branch_name_and_another_place_have_no_plan() {
        let atlas = workspace("atlas", "/work/atlas", "w1", Some("/work/atlas"));
        let wrong = Choice::NewWorktree {
            branch: "review the omnibar".into(),
        };
        assert!(plan(&atlas, &wrong, "review the omnibar", "claude", false).is_none());
        assert!(plan(&atlas, &Choice::AnotherPlace, "", "claude", false).is_none());
    }

    #[test]
    fn the_agent_name_is_a_slug_with_a_number() {
        let atlas = workspace("atlas", "/work/atlas", "w1", Some("/work/atlas"));
        let plan = plan(
            &atlas,
            &Choice::NewTab,
            "Review: the Omnibar!",
            "claude",
            false,
        )
        .unwrap();
        let name = agent_name(&plan);
        assert!(name.starts_with("claude-review-the-omnibar-"), "{name}");
    }

    #[test]
    fn the_agent_name_of_a_new_workspace_comes_from_its_label() {
        let plan = new_workspace_plan("Ville France", "claude", false).unwrap();
        let name = agent_name(&plan);
        assert!(name.starts_with("claude-ville-france-"), "{name}");
    }

    #[test]
    fn a_plan_goes_through_json_to_the_detached_process() {
        let atlas = workspace("atlas", "/work/atlas", "w1", Some("/work/atlas"));
        let choice = Choice::NewWorktree {
            branch: "fix-login".into(),
        };
        let plan = plan(&atlas, &choice, "fix-login", "codex", true).unwrap();
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&json).unwrap(), plan);
    }

    #[test]
    fn no_prompts_has_a_flag_for_claude_and_codex_only() {
        assert_eq!(
            no_prompt_arguments("claude"),
            ["--dangerously-skip-permissions"]
        );
        assert_eq!(
            no_prompt_arguments("codex"),
            ["--dangerously-bypass-approvals-and-sandbox"]
        );
        assert!(no_prompt_arguments("shell").is_empty());
    }

    #[test]
    fn a_failed_wt_gives_its_command_and_its_message() {
        let folder = std::env::temp_dir().join(format!("omnibar-no-repo-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let error = wt(&folder, &[OsStr::new("switch"), OsStr::new("x")]).unwrap_err();
        std::fs::remove_dir_all(&folder).unwrap();
        // Without wt on the PATH, the error tells it.
        assert!(
            error.starts_with("wt switch x: ") || error.starts_with("cannot run wt"),
            "{error}"
        );
    }
}
