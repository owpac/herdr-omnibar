//! The lines of the list: the groups and the rows of a filter, for a query.
//!
//! The order of the groups follows the urgency: the agents that need the user, the other agents,
//! the tabs without an agent, the workspaces, then the projects. The creation (filter New) has 2
//! steps: "where" lists the places, "what" lists the choices for the chosen place.

use std::env;
use std::path::Path;

use crate::create::{self, Choice};
use crate::filter::Filter;
use crate::fuzzy;
use crate::model::{AgentRow, Model, Space, Status, TabRow};
use crate::places::{self, Branch, Kind, Place};

/// A row of the list.
#[derive(Clone, Debug)]
pub enum Item {
    Agent(AgentRow),
    Tab(TabRow),
    Workspace(Space),
    /// A place: a workspace, a project or a folder, for a creation.
    Place(Place),
    /// A choice of the step "what", or a worktree of the filter Worktrees.
    Choice(Choice),
}

impl Item {
    /// The name of the row.
    pub fn name(&self) -> String {
        match self {
            Item::Agent(agent) => agent.name.clone(),
            Item::Tab(tab) => tab.name.clone(),
            Item::Workspace(space) => space.label(),
            Item::Place(place) => place.label.clone(),
            Item::Choice(choice) => match choice {
                Choice::NewWorktree { branch } if branch.is_empty() => "new worktree…".into(),
                Choice::NewWorktree { branch } => format!("new worktree {branch}"),
                Choice::Existing { branch, .. } => branch.clone(),
                Choice::NewTab => "new tab".into(),
                Choice::NewWorkspace => "new workspace".into(),
                Choice::AnotherPlace => "another place…".into(),
            },
        }
    }

    /// The context of the row, in grey.
    pub fn context(&self) -> String {
        match self {
            Item::Agent(agent) => agent.place.clone(),
            Item::Tab(tab) => tab.place.clone(),
            Item::Workspace(space) => space.folder.as_deref().map(home).unwrap_or_default(),
            Item::Place(place) => home(&place.folder),
            Item::Choice(_) => String::new(),
        }
    }

    /// A key that stays the same between 2 refreshes: the selection follows it.
    pub fn key(&self) -> String {
        match self {
            Item::Agent(agent) => format!("agent:{}", agent.pane_id),
            Item::Tab(tab) => format!("tab:{}", tab.tab_id),
            Item::Workspace(space) => format!("workspace:{}", space.workspace.workspace_id),
            Item::Place(place) => format!("place:{}", place.key()),
            Item::Choice(choice) => format!("choice:{choice:?}"),
        }
    }

    /// The status of the row: the status of the agent, or of the workspace.
    pub fn status(&self) -> Status {
        match self {
            Item::Agent(agent) => agent.status,
            Item::Workspace(space) => space.workspace.agent_status,
            _ => Status::Unknown,
        }
    }
}

/// A line of the list.
#[derive(Clone, Debug)]
pub enum Line {
    /// The title of a group.
    Group(String),
    /// A row, with the positions of the letters of the query in its name.
    Row { item: Item, positions: Vec<usize> },
}

/// The filters that the popup shows, in the order of Tab.
pub const FILTERS: [Filter; 7] = [
    Filter::All,
    Filter::Agents,
    Filter::Tabs,
    Filter::Worktrees,
    Filter::Folders,
    Filter::New,
    Filter::Actions,
];

/// True if the popup can show this filter in this version.
pub fn is_ready(filter: Filter) -> bool {
    filter != Filter::Actions
}

/// The lines of the filters that go somewhere (all the filters but New).
pub fn build(model: &Model, others: &[Place], filter: Filter, query: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let query = query.trim();
    if matches!(filter, Filter::All | Filter::Agents) {
        let agents = matches(
            model.agents.iter().cloned().map(Item::Agent).collect(),
            query,
        );
        let (needs, rest): (Vec<_>, Vec<_>) = agents
            .into_iter()
            .partition(|(item, _)| item.status() == Status::Blocked);
        group(&mut lines, "Needs you", needs);
        group(&mut lines, "Agents", rest);
    }
    if matches!(filter, Filter::All | Filter::Tabs) {
        let tabs = model.tabs.iter().cloned().map(Item::Tab).collect();
        group(&mut lines, "Tabs", matches(tabs, query));
    }
    if filter == Filter::All {
        let spaces = model.spaces.iter().cloned().map(Item::Workspace).collect();
        group(&mut lines, "Workspaces", matches(spaces, query));
    }
    if matches!(filter, Filter::All | Filter::Folders) {
        let (projects, folders): (Vec<_>, Vec<_>) = others
            .iter()
            .cloned()
            .partition(|place| place.kind == Kind::Project);
        let projects = projects.into_iter().map(Item::Place).collect();
        group(&mut lines, "Projects", matches(projects, query));
        if filter == Filter::Folders {
            let folders = folders.into_iter().map(Item::Place).collect();
            group(&mut lines, "Folders", matches(folders, query));
        }
    }
    lines
}

/// The lines of the filter Worktrees: the branches of the current project, and a new worktree when
/// the query is a new branch name.
pub fn worktree_lines(
    branches: &[Branch],
    open: &[Place],
    project: &str,
    query: &str,
) -> Vec<Line> {
    let query = query.trim();
    let items: Vec<Item> = branches
        .iter()
        .map(|branch| {
            Item::Choice(Choice::Existing {
                branch: branch.name.clone(),
                worktree: branch.worktree.clone(),
                workspace_id: branch.worktree.as_ref().and_then(|worktree| {
                    open.iter()
                        .find(|place| &place.folder == worktree)
                        .and_then(|place| place.workspace_id.clone())
                }),
            })
        })
        .collect();
    let mut rows = matches(items, query);
    let exists = branches.iter().any(|branch| branch.name == query);
    if create::is_branch_name(query) && !exists {
        rows.insert(
            0,
            (
                Item::Choice(Choice::NewWorktree {
                    branch: query.to_string(),
                }),
                Vec::new(),
            ),
        );
    }
    let mut lines = Vec::new();
    group(&mut lines, &format!("Worktrees of {project}"), rows);
    lines
}

/// The lines of the step "where": the current place, the open workspaces, the projects without a
/// workspace, the folders. A query that starts with ~, / or . lists the real folders.
pub fn where_lines(
    open: &[Place],
    others: &[Place],
    current: Option<&str>,
    query: &str,
) -> Vec<Line> {
    if is_path(query) {
        return path_lines(open, query);
    }
    let query = query.trim();
    let mut lines = Vec::new();
    let (here, rest): (Vec<Place>, Vec<Place>) = open
        .iter()
        .cloned()
        .partition(|place| place.workspace_id.as_deref() == current && current.is_some());
    group(
        &mut lines,
        "Current place",
        matches(here.into_iter().map(Item::Place).collect(), query),
    );
    group(
        &mut lines,
        "Open workspaces",
        matches(rest.into_iter().map(Item::Place).collect(), query),
    );
    let (projects, folders): (Vec<_>, Vec<_>) = others
        .iter()
        .cloned()
        .partition(|place| place.kind == Kind::Project);
    group(
        &mut lines,
        "Projects without a workspace",
        matches(projects.into_iter().map(Item::Place).collect(), query),
    );
    group(
        &mut lines,
        "Folders",
        matches(folders.into_iter().map(Item::Place).collect(), query),
    );
    lines
}

/// The lines of the step "what": the choices for the chosen place.
pub fn what_lines(place: &Place, choices: Vec<Choice>) -> Vec<Line> {
    let (create, other): (Vec<_>, Vec<_>) = choices
        .into_iter()
        .partition(|choice| *choice != Choice::AnotherPlace);
    let mut lines = Vec::new();
    group(
        &mut lines,
        &format!("Create in {}", place.label),
        create
            .into_iter()
            .map(|choice| (Item::Choice(choice), Vec::new()))
            .collect(),
    );
    group(
        &mut lines,
        "Place",
        other
            .into_iter()
            .map(|choice| (Item::Choice(choice), Vec::new()))
            .collect(),
    );
    lines
}

/// True for a query of real folders: it starts with ~, / or .
pub fn is_path(query: &str) -> bool {
    query.starts_with(['~', '/', '.'])
}

/// The folder and the start of a name in a path query: "~/Wo" gives ("~/", "Wo").
pub fn split_path(query: &str) -> (String, String) {
    match query.rfind('/') {
        Some(cut) => (query[..=cut].to_string(), query[cut + 1..].to_string()),
        None if query.starts_with('~') => ("~/".to_string(), query[1..].to_string()),
        None if query.starts_with('.') => ("./".to_string(), query[1..].to_string()),
        None => ("/".to_string(), String::new()),
    }
}

/// The subfolders of the folder of a path query whose names start with its last part.
fn path_lines(open: &[Place], query: &str) -> Vec<Line> {
    let (folder, start) = split_path(query);
    let base = if folder.starts_with('.') {
        env::current_dir().unwrap_or_default().join(&folder)
    } else {
        places::expand(&folder)
    };
    let rows: Vec<(Item, Vec<usize>)> = places::subfolders(&base)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .is_some_and(|name| name.starts_with(&start.to_lowercase()))
        })
        .map(|path| {
            let place = open
                .iter()
                .find(|place| place.folder == path)
                .cloned()
                .unwrap_or_else(|| places::folder_place(path));
            (Item::Place(place), Vec::new())
        })
        .collect();
    let mut lines = Vec::new();
    group(&mut lines, &format!("Folders in {folder}"), rows);
    lines
}

/// The completion of a path query: the first folder of the list, with a / at the end.
pub fn complete_path(query: &str, lines: &[Line]) -> Option<String> {
    let (folder, _) = split_path(query);
    lines.iter().find_map(|line| match line {
        Line::Row {
            item: Item::Place(place),
            ..
        } => place
            .folder
            .file_name()
            .map(|name| format!("{folder}{}/", name.to_string_lossy())),
        _ => None,
    })
}

/// The items that match the query, with the positions in their names. With a query, the best
/// matches come first.
fn matches(items: Vec<Item>, query: &str) -> Vec<(Item, Vec<usize>)> {
    let mut found: Vec<(Item, Vec<usize>, i32)> = items
        .into_iter()
        .filter_map(|item| {
            let name = item.name();
            let all = fuzzy::find(query, &format!("{name} {}", item.context()))?;
            let positions = fuzzy::find(query, &name)
                .map(|found| found.positions)
                .unwrap_or_default();
            Some((item, positions, all.score))
        })
        .collect();
    if !query.is_empty() {
        found.sort_by_key(|(_, _, score)| std::cmp::Reverse(*score));
    }
    found
        .into_iter()
        .map(|(item, positions, _)| (item, positions))
        .collect()
}

fn group(lines: &mut Vec<Line>, title: &str, rows: Vec<(Item, Vec<usize>)>) {
    if rows.is_empty() {
        return;
    }
    lines.push(Line::Group(title.to_string()));
    lines.extend(
        rows.into_iter()
            .map(|(item, positions)| Line::Row { item, positions }),
    );
}

/// A path with ~ for the home folder.
pub fn home(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    match env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => {
            format!("~{}", &text[home.len()..])
        }
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Snapshot, Workspace};

    fn agent(name: &str, status: Status) -> AgentRow {
        AgentRow {
            pane_id: format!("p-{name}"),
            tab_id: format!("t-{name}"),
            workspace_id: "w1".into(),
            name: name.into(),
            kind: "claude".into(),
            status,
            place: "atlas".into(),
        }
    }

    fn model() -> Model {
        let mut model = Model::build(&Snapshot::default(), &Default::default());
        model.agents = vec![
            agent("fix the login", Status::Blocked),
            agent("omnibar design", Status::Working),
        ];
        model.tabs = vec![TabRow {
            tab_id: "t-logs".into(),
            workspace_id: "w1".into(),
            name: "logs".into(),
            command: "tail -f observer.log".into(),
            place: "atlas".into(),
            pane_id: None,
        }];
        model.spaces = vec![Space {
            workspace: Workspace {
                workspace_id: "w1".into(),
                label: "atlas".into(),
                agent_status: Status::Working,
            },
            folder: None,
            checkout: None,
        }];
        model
    }

    fn place(kind: Kind, label: &str, workspace: Option<&str>) -> Place {
        Place {
            kind,
            label: label.into(),
            project: label.into(),
            folder: format!("/work/{label}").into(),
            workspace_id: workspace.map(str::to_string),
            main: None,
            tabs: Vec::new(),
        }
    }

    fn titles(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                Line::Group(title) => format!("# {title}"),
                Line::Row { item, .. } => item.name(),
            })
            .collect()
    }

    #[test]
    fn the_filter_all_shows_the_groups_in_the_order_of_urgency() {
        let others = [place(Kind::Project, "pac", None)];
        assert_eq!(
            titles(&build(&model(), &others, Filter::All, "")),
            [
                "# Needs you",
                "fix the login",
                "# Agents",
                "omnibar design",
                "# Tabs",
                "logs",
                "# Workspaces",
                "atlas",
                "# Projects",
                "pac"
            ]
        );
    }

    #[test]
    fn a_query_keeps_only_the_matches() {
        assert_eq!(
            titles(&build(&model(), &[], Filter::All, "omni")),
            ["# Agents", "omnibar design"]
        );
    }

    #[test]
    fn the_filter_tabs_shows_only_the_tabs() {
        assert_eq!(
            titles(&build(&model(), &[], Filter::Tabs, "")),
            ["# Tabs", "logs"]
        );
    }

    #[test]
    fn the_step_where_shows_the_current_place_first() {
        let open = [
            place(Kind::Workspace, "pac", Some("w2")),
            place(Kind::Workspace, "atlas", Some("w1")),
        ];
        let others = [
            place(Kind::Project, "prompts", None),
            place(Kind::Folder, "Downloads", None),
        ];
        assert_eq!(
            titles(&where_lines(&open, &others, Some("w1"), "")),
            [
                "# Current place",
                "atlas",
                "# Open workspaces",
                "pac",
                "# Projects without a workspace",
                "prompts",
                "# Folders",
                "Downloads"
            ]
        );
    }

    #[test]
    fn the_step_what_puts_another_place_in_its_own_group() {
        let atlas = place(Kind::Workspace, "atlas", Some("w1"));
        let lines = what_lines(
            &atlas,
            vec![
                Choice::NewWorktree {
                    branch: "notes".into(),
                },
                Choice::NewTab,
                Choice::AnotherPlace,
            ],
        );
        assert_eq!(
            titles(&lines),
            [
                "# Create in atlas",
                "new worktree notes",
                "new tab",
                "# Place",
                "another place…"
            ]
        );
    }

    #[test]
    fn the_filter_worktrees_proposes_a_new_worktree_for_a_new_branch() {
        let branches = [Branch {
            name: "main".into(),
            worktree: Some("/work/atlas".into()),
        }];
        let lines = worktree_lines(&branches, &[], "atlas", "fix-login");
        assert_eq!(
            titles(&lines),
            ["# Worktrees of atlas", "new worktree fix-login"]
        );
        let lines = worktree_lines(&branches, &[], "atlas", "");
        assert_eq!(titles(&lines), ["# Worktrees of atlas", "main"]);
    }

    #[test]
    fn a_path_query_gives_its_folder_and_its_start() {
        assert_eq!(split_path("~/Wo"), ("~/".into(), "Wo".into()));
        assert_eq!(split_path("~"), ("~/".into(), String::new()));
        assert_eq!(split_path("/Users/th"), ("/Users/".into(), "th".into()));
        assert!(is_path("~/x") && is_path("/x") && is_path("./x") && !is_path("x"));
    }

    #[test]
    fn the_home_folder_becomes_a_tilde() {
        let home = env::var("HOME").unwrap();
        assert_eq!(super::home(Path::new(&format!("{home}/work"))), "~/work");
        assert_eq!(super::home(Path::new("/tmp")), "/tmp");
    }
}
