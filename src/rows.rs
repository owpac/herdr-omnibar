//! The lines of the list: the groups and the rows of a filter, for a query.
//!
//! The order of the groups follows the urgency: the agents that need the user, the other agents,
//! the tabs without an agent, then the workspaces.

use std::env;
use std::path::Path;

use crate::filter::Filter;
use crate::fuzzy;
use crate::model::{AgentRow, Model, Space, Status, TabRow};

/// A thing that the popup can go to.
#[derive(Clone, Debug)]
pub enum Item {
    Agent(AgentRow),
    Tab(TabRow),
    Workspace(Space),
}

impl Item {
    /// The name of the row: the tab, or the workspace.
    pub fn name(&self) -> String {
        match self {
            Item::Agent(agent) => agent.name.clone(),
            Item::Tab(tab) => tab.name.clone(),
            Item::Workspace(space) => space.label(),
        }
    }

    /// The context of the row, in grey: the workspace, or the folder.
    pub fn context(&self) -> String {
        match self {
            Item::Agent(agent) => agent.place.clone(),
            Item::Tab(tab) => tab.place.clone(),
            Item::Workspace(space) => space.folder.as_deref().map(home).unwrap_or_default(),
        }
    }

    /// A key that stays the same between 2 refreshes: the selection follows it.
    pub fn key(&self) -> String {
        match self {
            Item::Agent(agent) => format!("agent:{}", agent.pane_id),
            Item::Tab(tab) => format!("tab:{}", tab.tab_id),
            Item::Workspace(space) => format!("workspace:{}", space.workspace.workspace_id),
        }
    }

    /// The status of the row: the status of the agent, or of the workspace.
    pub fn status(&self) -> Status {
        match self {
            Item::Agent(agent) => agent.status,
            Item::Tab(_) => Status::Unknown,
            Item::Workspace(space) => space.workspace.agent_status,
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
    matches!(filter, Filter::All | Filter::Agents | Filter::Tabs)
}

/// The lines of a filter for a query.
pub fn build(model: &Model, filter: Filter, query: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let query = query.trim();
    if matches!(filter, Filter::All | Filter::Agents) {
        let agents = matches(
            model.agents.iter().cloned().map(Item::Agent).collect(),
            query,
        );
        let (needs, others): (Vec<_>, Vec<_>) = agents
            .into_iter()
            .partition(|(item, _)| item.status() == Status::Blocked);
        group(&mut lines, "Needs you", needs);
        group(&mut lines, "Agents", others);
    }
    if matches!(filter, Filter::All | Filter::Tabs) {
        let tabs = model.tabs.iter().cloned().map(Item::Tab).collect();
        group(&mut lines, "Tabs", matches(tabs, query));
    }
    if filter == Filter::All {
        let spaces = model.spaces.iter().cloned().map(Item::Workspace).collect();
        group(&mut lines, "Workspaces", matches(spaces, query));
    }
    lines
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
        assert_eq!(
            titles(&build(&model(), Filter::All, "")),
            [
                "# Needs you",
                "fix the login",
                "# Agents",
                "omnibar design",
                "# Tabs",
                "logs",
                "# Workspaces",
                "atlas"
            ]
        );
    }

    #[test]
    fn a_query_keeps_only_the_matches() {
        assert_eq!(
            titles(&build(&model(), Filter::All, "omni")),
            ["# Agents", "omnibar design"]
        );
    }

    #[test]
    fn the_filter_tabs_shows_only_the_tabs() {
        assert_eq!(
            titles(&build(&model(), Filter::Tabs, "")),
            ["# Tabs", "logs"]
        );
    }

    #[test]
    fn the_context_matches_too() {
        // "atlas" is the workspace of each row: all the rows match.
        assert_eq!(build(&model(), Filter::Agents, "atlas").len(), 4);
    }

    #[test]
    fn the_home_folder_becomes_a_tilde() {
        let home = env::var("HOME").unwrap();
        assert_eq!(super::home(Path::new(&format!("{home}/work"))), "~/work");
        assert_eq!(super::home(Path::new("/tmp")), "/tmp");
    }
}
