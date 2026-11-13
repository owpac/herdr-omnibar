//! The lines of the list: the groups and the rows of a filter, for a query.
//!
//! The order of the groups follows the urgency: the agents that need the user, the other agents,
//! the tabs without an agent, the workspaces, then the projects. The creation (filter New) has 2
//! steps: "where" lists the places, "what" lists the choices for the chosen place.

use std::env;
use std::path::Path;

use crate::config::DEFAULT_FILTERS;
use crate::create::{self, Choice};
use crate::filter::Filter;
use crate::fuzzy;
use crate::herdr::PluginAction;
use crate::model::{AgentRow, Model, PaneRow, Space, Status, TabRow};
use crate::places::{self, Kind, Place, Project};

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
    /// An action of a herdr plugin.
    Action(PluginAction),
    /// The creation of a tab or a worktree with the query as its name: ↵ goes to the step "what".
    Create(String),
    /// A new workspace in ~ with the text as its label: ↵ creates it at once. Without a text, the
    /// row asks for a name.
    NewWorkspace(String),
    /// A pane of a tab (filter Panes).
    Pane(PaneRow),
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
                Choice::NewSplit { .. } => "new split".into(),
                Choice::NewWorkspace => "new workspace".into(),
                Choice::AnotherPlace => "another place…".into(),
            },
            Item::Action(action) if action.title.is_empty() => action.action_id.clone(),
            Item::Action(action) => action.title.clone(),
            Item::Create(name) => format!("create “{name}”…"),
            Item::NewWorkspace(text) if text.is_empty() => "new workspace…".to_string(),
            Item::NewWorkspace(text) => format!("new workspace “{text}”"),
            Item::Pane(pane) => pane.name.clone(),
        }
    }

    /// The context of the row, in grey.
    pub fn context(&self) -> String {
        match self {
            Item::Agent(agent) => agent.place.clone(),
            Item::Tab(tab) => tab.place.clone(),
            Item::Workspace(space) => space.folder.as_deref().map(home).unwrap_or_default(),
            Item::Place(place) => home(&place.folder),
            // The folder of a worktree: a query with the name of the project finds it.
            Item::Choice(Choice::Existing {
                worktree: Some(folder),
                ..
            }) => home(folder),
            Item::Choice(_) => String::new(),
            Item::Action(action) => action.plugin_id.clone(),
            Item::Create(_) => String::new(),
            Item::NewWorkspace(_) => format!("a new workspace in {}", home(&places::home_folder())),
            Item::Pane(pane) => format!("{} ▸ {}", pane.place, pane.tab),
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
            Item::Action(action) => format!("action:{}:{}", action.plugin_id, action.action_id),
            Item::Create(_) => "create".to_string(),
            Item::NewWorkspace(_) => "new-workspace".to_string(),
            Item::Pane(pane) => format!("pane:{}", pane.pane_id),
        }
    }

    /// The status of the row: the status of the agent, or of the workspace.
    pub fn status(&self) -> Status {
        match self {
            Item::Agent(agent) => agent.status,
            Item::Workspace(space) => space.workspace.agent_status,
            Item::Pane(pane) if pane.agent.is_some() => pane.status,
            _ => Status::Unknown,
        }
    }
}

/// A line of the list.
#[derive(Clone, Debug)]
pub enum Line {
    /// The title of a group.
    Group(String),
    /// An empty line before a group, after another group.
    Gap,
    /// A row, with the positions of the letters of the query in its name.
    Row { item: Item, positions: Vec<usize> },
}

/// The filters of the chips, in the order of Tab, from their names in config.toml. An unknown
/// name and a second copy of a name stay without effect. A list without a known name gives the
/// default filters.
pub fn filters(names: &[String]) -> Vec<Filter> {
    let mut filters = Vec::new();
    for filter in names.iter().filter_map(|name| Filter::parse(name)) {
        if !filters.contains(&filter) {
            filters.push(filter);
        }
    }
    if filters.is_empty() {
        filters = DEFAULT_FILTERS
            .iter()
            .filter_map(|name| Filter::parse(name))
            .collect();
    }
    filters
}

/// The sources of the lines of the filters that go somewhere.
pub struct Sources<'a> {
    pub model: &'a Model,
    /// The places without a workspace: projects and folders.
    pub others: &'a [Place],
    pub actions: &'a [PluginAction],
    /// The open workspaces, as places.
    pub open: &'a [Place],
    /// The current project and its local branches, for the filter All.
    pub project: Option<&'a Project>,
}

/// The lines of the filters that go somewhere (all the filters but New).
pub fn build(sources: &Sources, filter: Filter, query: &str) -> Vec<Line> {
    let model = sources.model;
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
    if filter == Filter::Workspaces {
        let spaces = model.spaces.iter().cloned().map(Item::Workspace).collect();
        group(&mut lines, "Workspaces", matches(spaces, query));
    }
    if filter == Filter::All {
        // The worktrees of the current project have their own group: their workspaces are not
        // in the group Workspaces.
        let worktrees = sources
            .project
            .map(|project| existing(project, sources.open))
            .unwrap_or_default();
        let in_worktrees: Vec<String> = worktrees
            .iter()
            .filter_map(|item| match item {
                Item::Choice(Choice::Existing {
                    workspace_id: Some(id),
                    ..
                }) => Some(id.clone()),
                _ => None,
            })
            .collect();
        if let Some(project) = sources.project {
            group(
                &mut lines,
                &format!("Worktrees of {}", project.name),
                matches(worktrees, query),
            );
        }
        let spaces = model
            .spaces
            .iter()
            .filter(|space| !in_worktrees.contains(&space.workspace.workspace_id))
            .cloned()
            .map(Item::Workspace)
            .collect();
        group(&mut lines, "Workspaces", matches(spaces, query));
    }
    if matches!(filter, Filter::All | Filter::Folders) {
        let (projects, folders): (Vec<_>, Vec<_>) = sources
            .others
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
    if filter == Filter::Actions {
        let actions = sources.actions.iter().cloned().map(Item::Action).collect();
        group(&mut lines, "Actions", matches(actions, query));
    }
    if filter == Filter::Panes {
        let panes = model.panes.iter().cloned().map(Item::Pane).collect();
        group(&mut lines, "Panes", matches(panes, query));
    }
    if filter == Filter::All && !query.is_empty() {
        group(
            &mut lines,
            "Create",
            vec![(Item::Create(query.to_string()), Vec::new())],
        );
    }
    lines
}

/// The branches of a project as rows, with the open workspace of their worktrees.
fn existing(project: &Project, open: &[Place]) -> Vec<Item> {
    project
        .branches
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
                main: project.main.clone(),
            })
        })
        .collect()
}

/// The lines of the filter Worktrees: a group for each project. The current project comes first,
/// with all its branches, and a new worktree when the query is a new branch name. The other
/// projects show their worktrees.
pub fn worktree_lines(
    current: Option<&Project>,
    others: &[Project],
    open: &[Place],
    query: &str,
) -> Vec<Line> {
    let query = query.trim();
    let mut lines = Vec::new();
    if let Some(project) = current {
        let mut rows = matches(existing(project, open), query);
        let exists = project.branches.iter().any(|branch| branch.name == query);
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
        group(&mut lines, &format!("Worktrees of {}", project.name), rows);
    }
    for project in others {
        let rows = matches(existing(project, open), query);
        group(&mut lines, &format!("Worktrees of {}", project.name), rows);
    }
    lines
}

/// The lines of the step "where": the current place, the open workspaces, the projects without a
/// workspace, the folders. The places match by their name, not by their folder: a folder under
/// ~/Workspace would match almost each text. A query that starts with ~, / or . lists the real
/// folders.
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
    // The first row makes a new workspace in ~, with the typed text as its label.
    group(
        &mut lines,
        "New",
        vec![(Item::NewWorkspace(query.to_string()), Vec::new())],
    );
    let (here, rest): (Vec<Place>, Vec<Place>) = open
        .iter()
        .cloned()
        .partition(|place| place.workspace_id.as_deref() == current && current.is_some());
    group(
        &mut lines,
        "Current place",
        matches_by_name(here.into_iter().map(Item::Place).collect(), query),
    );
    group(
        &mut lines,
        "Open workspaces",
        matches_by_name(rest.into_iter().map(Item::Place).collect(), query),
    );
    let (projects, folders): (Vec<_>, Vec<_>) = others
        .iter()
        .cloned()
        .partition(|place| place.kind == Kind::Project);
    group(
        &mut lines,
        "Projects without a workspace",
        matches_by_name(projects.into_iter().map(Item::Place).collect(), query),
    );
    group(
        &mut lines,
        "Folders",
        matches_by_name(folders.into_iter().map(Item::Place).collect(), query),
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
    matches_in(items, query, true)
}

/// The items whose name matches the query: the context (the folder) does not count.
fn matches_by_name(items: Vec<Item>, query: &str) -> Vec<(Item, Vec<usize>)> {
    matches_in(items, query, false)
}

fn matches_in(items: Vec<Item>, query: &str, with_context: bool) -> Vec<(Item, Vec<usize>)> {
    // An empty query keeps all the items, in their order.
    if query.is_empty() {
        return items.into_iter().map(|item| (item, Vec::new())).collect();
    }
    let mut found: Vec<(Item, Vec<usize>, i32)> = items
        .into_iter()
        .filter_map(|item| {
            let name = item.name();
            let by_name = fuzzy::find(query, &name);
            // Without the context, the match of the name gives the score and the positions.
            let (score, positions) = if with_context {
                let all = fuzzy::find(query, &format!("{name} {}", item.context()))?;
                (all.score, by_name.map(|found| found.positions))
            } else {
                let found = by_name?;
                (found.score, Some(found.positions))
            };
            Some((item, positions.unwrap_or_default(), score))
        })
        .collect();
    found.sort_by_key(|(_, _, score)| std::cmp::Reverse(*score));
    found
        .into_iter()
        .map(|(item, positions, _)| (item, positions))
        .collect()
}

fn group(lines: &mut Vec<Line>, title: &str, rows: Vec<(Item, Vec<usize>)>) {
    if rows.is_empty() {
        return;
    }
    if !lines.is_empty() {
        lines.push(Line::Gap);
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
    use std::path::PathBuf;

    fn agent(name: &str, status: Status) -> AgentRow {
        AgentRow {
            pane_id: format!("p-{name}"),
            tab_id: format!("t-{name}"),
            workspace_id: "w1".into(),
            name: name.into(),
            kind: "claude".into(),
            status,
            place: "atlas".into(),
            cwd: None,
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
                worktree: None,
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

    #[test]
    fn the_row_new_workspace_has_its_own_key_and_names() {
        let folder = Place {
            folder: places::home_folder(),
            ..place(Kind::Folder, "~", None)
        };
        let empty = Item::NewWorkspace(String::new());
        assert_ne!(empty.key(), Item::Place(folder).key());
        assert_eq!(empty.name(), "new workspace…");
        assert_eq!(
            Item::NewWorkspace("ville".into()).name(),
            "new workspace “ville”"
        );
    }

    fn lines_of(
        model: &Model,
        others: &[Place],
        actions: &[PluginAction],
        filter: Filter,
        query: &str,
    ) -> Vec<Line> {
        let sources = Sources {
            model,
            others,
            actions,
            open: &[],
            project: None,
        };
        build(&sources, filter, query)
    }

    fn titles(lines: &[Line]) -> Vec<String> {
        // The gaps are only for the eye.
        lines
            .iter()
            .filter_map(|line| match line {
                Line::Group(title) => Some(format!("# {title}")),
                Line::Gap => None,
                Line::Row { item, .. } => Some(item.name()),
            })
            .collect()
    }

    #[test]
    fn the_filter_all_shows_the_groups_in_the_order_of_urgency() {
        let others = [place(Kind::Project, "pac", None)];
        assert_eq!(
            titles(&lines_of(&model(), &others, &[], Filter::All, "")),
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
            titles(&lines_of(&model(), &[], &[], Filter::All, "omni")),
            ["# Agents", "omnibar design", "# Create", "create “omni”…"]
        );
    }

    #[test]
    fn the_filter_all_shows_the_worktrees_of_the_current_project_once() {
        let model = model();
        let open = [place(Kind::Workspace, "atlas", Some("w1"))];
        let atlas = Project::sample(
            "atlas",
            &[("main", Some("/work/atlas")), ("fix-login", None)],
        );
        let sources = Sources {
            model: &model,
            others: &[],
            actions: &[],
            open: &open,
            project: Some(&atlas),
        };
        let lines = build(&sources, Filter::All, "");
        assert_eq!(
            titles(&lines)[4..],
            [
                "# Tabs",
                "logs",
                "# Worktrees of atlas",
                "main",
                "fix-login"
            ]
        );
        // The query finds a worktree by the name of its project, in its folder.
        let lines = build(&sources, Filter::All, "atlas");
        assert!(titles(&lines).contains(&"main".to_string()));
    }

    #[test]
    fn the_filter_tabs_shows_only_the_tabs() {
        assert_eq!(
            titles(&lines_of(&model(), &[], &[], Filter::Tabs, "")),
            ["# Tabs", "logs"]
        );
    }

    #[test]
    fn the_filter_workspaces_shows_all_the_workspaces() {
        let model = model();
        let open = [place(Kind::Workspace, "atlas", Some("w1"))];
        let atlas = Project::sample("atlas", &[("main", Some("/work/atlas"))]);
        let sources = Sources {
            model: &model,
            others: &[],
            actions: &[],
            open: &open,
            project: Some(&atlas),
        };
        // In All, the workspace of a worktree of the current project is in the group of the
        // worktrees. The filter Workspaces shows it.
        assert!(!titles(&build(&sources, Filter::All, "")).contains(&"# Workspaces".to_string()));
        assert_eq!(
            titles(&build(&sources, Filter::Workspaces, "")),
            ["# Workspaces", "atlas"]
        );
    }

    #[test]
    fn the_filter_actions_lists_the_actions_of_the_plugins() {
        let actions = [PluginAction {
            action_id: "clean".into(),
            plugin_id: "herdr-worktrunk".into(),
            title: "Worktree: clean the worktrees".into(),
            description: String::new(),
        }];
        assert_eq!(
            titles(&lines_of(&model(), &[], &actions, Filter::Actions, "clean")),
            ["# Actions", "Worktree: clean the worktrees"]
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
                "# New",
                "new workspace…",
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
    fn the_step_where_matches_the_places_by_their_name_and_not_by_their_folder() {
        // The folder of "atlas" is /work/Recettes: it contains the text, its name does not.
        let atlas = Place {
            folder: "/work/Recettes".into(),
            ..place(Kind::Workspace, "atlas", Some("w1"))
        };
        let others = [Place {
            folder: "/work/Recettes/pac".into(),
            ..place(Kind::Project, "prompts", None)
        }];
        let open = [atlas];
        let names = |query: &str| -> Vec<String> {
            titles(&where_lines(&open, &others, Some("w1"), query))
                .into_iter()
                .filter(|title| !title.starts_with('#'))
                .collect()
        };
        // Only the row of the new workspace is left: no place has "Recettes" in its name.
        assert_eq!(names("Recettes"), ["new workspace “Recettes”"]);
        assert_eq!(names("pac"), ["new workspace “pac”"]);
        // A name that matches lists the place.
        assert_eq!(names("atl"), ["new workspace “atl”", "atlas"]);
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
        let atlas = Project::sample("atlas", &[("main", Some("/work/atlas"))]);
        let lines = worktree_lines(Some(&atlas), &[], &[], "fix-login");
        assert_eq!(
            titles(&lines),
            ["# Worktrees of atlas", "new worktree fix-login"]
        );
        let lines = worktree_lines(Some(&atlas), &[], &[], "");
        assert_eq!(titles(&lines), ["# Worktrees of atlas", "main"]);
        // Outside a git project, no project can take the new worktree.
        assert!(worktree_lines(None, &[], &[], "fix-login").is_empty());
    }

    #[test]
    fn the_filter_worktrees_shows_the_current_project_first_then_the_others() {
        let atlas = Project::sample("atlas", &[("main", Some("/work/atlas")), ("old", None)]);
        let pac = Project::sample("pac", &[("main", Some("/work/pac"))]);
        let lines = worktree_lines(Some(&atlas), std::slice::from_ref(&pac), &[], "");
        assert_eq!(
            titles(&lines),
            [
                "# Worktrees of atlas",
                "main",
                "old",
                "# Worktrees of pac",
                "main"
            ]
        );
        // Each row knows the main checkout of its project.
        let mains: Vec<PathBuf> = lines
            .iter()
            .filter_map(|line| match line {
                Line::Row {
                    item: Item::Choice(Choice::Existing { main, .. }),
                    ..
                } => Some(main.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            mains,
            [
                PathBuf::from("/work/atlas"),
                "/work/atlas".into(),
                "/work/pac".into()
            ]
        );
        // A new branch name gives a new worktree in the current project only.
        let lines = worktree_lines(Some(&atlas), &[pac], &[], "fix-login");
        assert_eq!(
            titles(&lines),
            ["# Worktrees of atlas", "new worktree fix-login"]
        );
    }

    #[test]
    fn the_filters_follow_their_names_in_the_config() {
        let names = |list: &[&str]| list.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        assert_eq!(
            filters(&names(&["agents", "panes", "nothing", "agents"])),
            [Filter::Agents, Filter::Panes]
        );
        // A list without a known name gives the default filters: Tab needs at least one filter.
        assert_eq!(filters(&names(&[])), filters(&names(&DEFAULT_FILTERS)));
        assert_eq!(filters(&names(&[])).len(), DEFAULT_FILTERS.len());
        assert_eq!(filters(&names(&["nothing"]))[0], Filter::All);
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
