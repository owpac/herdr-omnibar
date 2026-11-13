//! The order of the lists (config key `list_order`). One place compares the rows of all the lists.
//!
//! `list_order` is a list of criteria. The first criterion sorts, and the next criterion sorts the
//! rows that are equal:
//!
//! - `attention`: the agents that need you first, then the agents that are done, then the others.
//! - `activity`: the last activity first: the highest `state_change_seq` of herdr.
//! - `visit`: the last visit of the tab first (recent.json).
//! - `herdr`: the order of herdr: the workspaces as the sidebar shows them, then the number of the
//!   tab, then the order of the panes. It is always the last criterion, also if the list does not
//!   have it.
//!
//! The current tab (with its agent, its panes and its workspace) goes at the end of its section,
//! for each order. A row without an agent has no attention and no activity: the next criterion
//! sorts it.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::model::{AgentRow, Model, PaneRow, Space, Status, TabRow};
use crate::places::{Place, Project};
use crate::recent::Recent;

/// The default `list_order`.
pub const DEFAULT_LIST_ORDER: [&str; 3] = ["attention", "activity", "visit"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    Attention,
    Activity,
    Visit,
    Herdr,
}

/// The criteria of a `list_order` list: an unknown entry and a duplicate do nothing. `herdr` is
/// always the last one, also if the list does not have it. An empty list gives the order herdr.
pub fn criteria(order: &[String]) -> Vec<Criterion> {
    let mut found: Vec<Criterion> = Vec::new();
    for name in order {
        let criterion = match name.as_str() {
            "attention" => Criterion::Attention,
            "activity" => Criterion::Activity,
            "visit" => Criterion::Visit,
            "herdr" => Criterion::Herdr,
            _ => continue,
        };
        if !found.contains(&criterion) {
            found.push(criterion);
        }
    }
    found.retain(|criterion| *criterion != Criterion::Herdr);
    found.push(Criterion::Herdr);
    found
}

/// The value of a row for each criterion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rank {
    /// The current tab, or the row of the current tab: it goes last.
    current: bool,
    /// 0 needs you, 1 done, 2 another agent, 3 no agent.
    attention: u8,
    /// The highest `state_change_seq` of the agents of the row.
    activity: Option<u64>,
    /// The last visit of the tab, or of the tabs of the row. 0: never.
    visit: u64,
    /// The workspace on the screen, the number of the tab, the order of the pane.
    herdr: (usize, u32, usize),
}

/// The rank of a row without a value: no agent, no visit, after the workspaces of the screen.
const NO_RANK: Rank = Rank {
    current: false,
    attention: 3,
    activity: None,
    visit: 0,
    herdr: (usize::MAX, 0, 0),
};

impl Rank {
    /// Compare 2 rows with the criteria: the one that must come first is the smaller.
    pub fn compare(&self, other: &Rank, criteria: &[Criterion]) -> Ordering {
        let mut order = self.current.cmp(&other.current);
        for criterion in criteria {
            order = order.then_with(|| match criterion {
                Criterion::Attention => self.attention.cmp(&other.attention),
                // The highest first. No value (None) goes last.
                Criterion::Activity => other.activity.cmp(&self.activity),
                Criterion::Visit => other.visit.cmp(&self.visit),
                Criterion::Herdr => self.herdr.cmp(&other.herdr),
            });
        }
        order
    }
}

/// The ranks of the rows of a model. It keeps what it needs: it does not borrow the model, so
/// the model can sort itself with it.
#[derive(Clone, Debug, Default)]
pub struct Ranker {
    criteria: Vec<Criterion>,
    recent: Recent,
    current_tab: Option<String>,
    /// The position of each workspace on the screen.
    screen: HashMap<String, usize>,
    tab_numbers: HashMap<String, u32>,
    pane_index: HashMap<String, usize>,
    /// The rank of each workspace.
    workspaces: HashMap<String, Rank>,
}

fn attention_of(status: Status) -> u8 {
    match status {
        Status::Blocked => 0,
        Status::Done => 1,
        Status::Unknown => 3,
        _ => 2,
    }
}

impl Ranker {
    pub fn new(
        model: &Model,
        order: &[String],
        recent: &Recent,
        current_tab: Option<&str>,
        current_workspace: Option<&str>,
    ) -> Ranker {
        let screen = crate::order::screen_positions(&model.flat_workspaces);
        // The highest activity of the agents and the last visit of the tabs of each workspace,
        // in one pass over the agents and one pass over the panes.
        let mut activities: HashMap<&str, u64> = HashMap::new();
        for agent in &model.agents {
            if let Some(seq) = agent.seq {
                let highest = activities.entry(agent.workspace_id.as_str()).or_default();
                *highest = (*highest).max(seq);
            }
        }
        let mut visits: HashMap<&str, u64> = HashMap::new();
        for pane in &model.panes {
            let visit = recent.get(&pane.tab_id).copied().unwrap_or(0);
            let last = visits.entry(pane.workspace_id.as_str()).or_default();
            *last = (*last).max(visit);
        }
        // A workspace: the attention that herdr gives, the highest activity of its agents, and
        // the last visit of its tabs.
        let workspaces = model
            .spaces
            .iter()
            .map(|space| {
                let id = &space.workspace.workspace_id;
                let rank = Rank {
                    current: current_workspace == Some(id.as_str()),
                    attention: attention_of(space.workspace.agent_status),
                    activity: activities.get(id.as_str()).copied(),
                    visit: visits.get(id.as_str()).copied().unwrap_or(0),
                    herdr: (screen.get(id).copied().unwrap_or(usize::MAX), 0, 0),
                };
                (id.clone(), rank)
            })
            .collect();
        Ranker {
            criteria: criteria(order),
            recent: recent.clone(),
            current_tab: current_tab.map(str::to_string),
            screen,
            tab_numbers: model.tab_numbers.clone(),
            pane_index: model.pane_index.clone(),
            workspaces,
        }
    }

    fn visit_of_tab(&self, tab_id: &str) -> u64 {
        self.recent.get(tab_id).copied().unwrap_or(0)
    }

    fn herdr_of(&self, workspace_id: &str, tab_id: &str, pane_id: &str) -> (usize, u32, usize) {
        (
            self.screen.get(workspace_id).copied().unwrap_or(usize::MAX),
            self.tab_numbers.get(tab_id).copied().unwrap_or(u32::MAX),
            self.pane_index.get(pane_id).copied().unwrap_or(usize::MAX),
        )
    }

    fn is_current_tab(&self, tab_id: &str) -> bool {
        self.current_tab.as_deref() == Some(tab_id)
    }

    fn agent(&self, agent: &AgentRow) -> Rank {
        Rank {
            current: self.is_current_tab(&agent.tab_id),
            attention: attention_of(agent.status),
            activity: agent.seq,
            visit: self.visit_of_tab(&agent.tab_id),
            herdr: self.herdr_of(&agent.workspace_id, &agent.tab_id, &agent.pane_id),
        }
    }

    /// A pane has the attention and the activity of its agent. A pane without an agent has none.
    fn pane(&self, pane: &PaneRow) -> Rank {
        Rank {
            current: self.is_current_tab(&pane.tab_id),
            attention: match pane.agent {
                Some(_) => attention_of(pane.status),
                None => NO_RANK.attention,
            },
            activity: pane.seq,
            visit: self.visit_of_tab(&pane.tab_id),
            herdr: self.herdr_of(&pane.workspace_id, &pane.tab_id, &pane.pane_id),
        }
    }

    /// A tab row has no agent: the visit and the order of herdr sort it.
    fn tab(&self, tab: &TabRow) -> Rank {
        Rank {
            current: self.is_current_tab(&tab.tab_id),
            visit: self.visit_of_tab(&tab.tab_id),
            herdr: self.herdr_of(
                &tab.workspace_id,
                &tab.tab_id,
                tab.pane_id.as_deref().unwrap_or_default(),
            ),
            ..NO_RANK
        }
    }

    fn space(&self, space: &Space) -> Rank {
        let id = &space.workspace.workspace_id;
        self.workspaces.get(id).copied().unwrap_or(NO_RANK)
    }

    /// A group of worktrees: the best attention, the highest activity and the last visit of its
    /// open workspaces. A project without an open workspace goes last.
    fn project(&self, project: &Project, open: &[Place]) -> Rank {
        open.iter()
            .filter(|place| place.main.as_deref() == Some(project.main.as_path()))
            .filter_map(|place| self.workspaces.get(place.workspace_id.as_deref()?))
            .fold(NO_RANK, |rank, workspace| Rank {
                current: rank.current || workspace.current,
                attention: rank.attention.min(workspace.attention),
                activity: rank.activity.max(workspace.activity),
                visit: rank.visit.max(workspace.visit),
                herdr: rank.herdr.min(workspace.herdr),
            })
    }

    /// Sort some rows by their ranks: each rank is computed one time. The sort is stable.
    fn sort_rows<T>(&self, rows: &mut Vec<T>, rank: impl Fn(&Ranker, &T) -> Rank) {
        let mut ranked: Vec<(Rank, T)> =
            rows.drain(..).map(|row| (rank(self, &row), row)).collect();
        ranked.sort_by(|a, b| a.0.compare(&b.0, &self.criteria));
        rows.extend(ranked.into_iter().map(|(_, row)| row));
    }

    /// Sort the lists of a model: the agents, the tabs, the panes and the workspaces.
    pub fn sort(&self, model: &mut Model) {
        self.sort_rows(&mut model.agents, Ranker::agent);
        self.sort_rows(&mut model.tabs, Ranker::tab);
        self.sort_rows(&mut model.panes, Ranker::pane);
        self.sort_rows(&mut model.spaces, Ranker::space);
    }

    /// The other projects of the filter Worktrees, in this order. The current project is not
    /// here: it stays first.
    pub fn projects<'a>(&self, projects: &'a [Project], open: &[Place]) -> Vec<&'a Project> {
        let mut ordered: Vec<&Project> = projects.iter().collect();
        self.sort_rows(&mut ordered, |ranker, project| {
            ranker.project(project, open)
        });
        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Snapshot, Workspace};

    fn order(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn row(name: &str, status: Status, seq: Option<u64>, tab: &str, pane: &str) -> AgentRow {
        AgentRow {
            pane_id: pane.into(),
            tab_id: tab.into(),
            workspace_id: "w1".into(),
            name: name.into(),
            kind: "claude".into(),
            status,
            place: "atlas".into(),
            cwd: None,
            seq,
        }
    }

    /// A model with 5 agents: the pane and the tab numbers follow the order of the names.
    fn model() -> Model {
        let agents = vec![
            row("a-idle-old", Status::Idle, Some(10), "t1", "p1"),
            row("b-working-new", Status::Working, Some(90), "t2", "p2"),
            row("c-done", Status::Done, Some(20), "t3", "p3"),
            row("d-blocked", Status::Blocked, Some(5), "t4", "p4"),
            row("e-no-seq", Status::Idle, None, "t5", "p5"),
        ];
        let mut model = Model {
            agents,
            ..Model::default()
        };
        for (index, tab) in ["t1", "t2", "t3", "t4", "t5"].iter().enumerate() {
            model.tab_numbers.insert(tab.to_string(), index as u32 + 1);
            model.pane_index.insert(format!("p{}", index + 1), index);
        }
        model
    }

    fn names(model: &Model) -> Vec<&str> {
        model
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect()
    }

    fn sorted(order_list: &[&str], recent: &Recent, current: Option<&str>) -> Vec<String> {
        let mut model = model();
        Ranker::new(&model, &order(order_list), recent, current, None).sort(&mut model);
        names(&model).into_iter().map(str::to_string).collect()
    }

    #[test]
    fn the_criteria_ignore_an_unknown_entry_and_a_duplicate_and_end_with_herdr() {
        use Criterion::*;
        assert_eq!(
            criteria(&order(&["visit", "nothing", "visit", "attention"])),
            [Visit, Attention, Herdr]
        );
        assert_eq!(criteria(&order(&["herdr", "activity"])), [Activity, Herdr]);
        // An empty list gives the order herdr.
        assert_eq!(criteria(&[]), [Herdr]);
        assert_eq!(criteria(&order(&["nothing"])), [Herdr]);
    }

    #[test]
    fn attention_puts_needs_you_then_done_then_the_others() {
        let result = sorted(&["attention"], &Recent::new(), None);
        // The other agents keep the order of herdr.
        assert_eq!(
            result,
            [
                "d-blocked",
                "c-done",
                "a-idle-old",
                "b-working-new",
                "e-no-seq"
            ]
        );
    }

    #[test]
    fn activity_puts_the_highest_seq_first_and_no_seq_last() {
        let result = sorted(&["activity"], &Recent::new(), None);
        assert_eq!(
            result,
            [
                "b-working-new",
                "c-done",
                "a-idle-old",
                "d-blocked",
                "e-no-seq"
            ]
        );
    }

    #[test]
    fn visit_puts_the_last_visit_first_and_the_next_criterion_sorts_the_equal_rows() {
        let recent = Recent::from([("t3".to_string(), 50), ("t1".to_string(), 20)]);
        let result = sorted(&["visit"], &recent, None);
        // The tabs never visited keep the order of herdr.
        assert_eq!(
            result,
            [
                "c-done",
                "a-idle-old",
                "b-working-new",
                "d-blocked",
                "e-no-seq"
            ]
        );
        // The first criterion sorts, the next one breaks the ties: the agents that are done or
        // blocked first, the others by visit.
        let result = sorted(&["attention", "visit"], &recent, None);
        assert_eq!(
            result,
            [
                "d-blocked",
                "c-done",
                "a-idle-old",
                "b-working-new",
                "e-no-seq"
            ]
        );
    }

    #[test]
    fn herdr_is_the_order_of_the_numbers_and_the_last_tie_breaker() {
        let by_herdr = sorted(&["herdr"], &Recent::new(), None);
        assert_eq!(
            by_herdr,
            [
                "a-idle-old",
                "b-working-new",
                "c-done",
                "d-blocked",
                "e-no-seq"
            ]
        );
        // An empty list and a list of unknown entries give the same order.
        assert_eq!(sorted(&[], &Recent::new(), None), by_herdr);
        assert_eq!(sorted(&["nothing"], &Recent::new(), None), by_herdr);
    }

    #[test]
    fn the_current_tab_goes_last_for_each_order() {
        for criteria in [vec!["attention"], vec!["activity"], vec!["herdr"], vec![]] {
            let result = sorted(&criteria, &Recent::new(), Some("t4"));
            assert_eq!(result.last().map(String::as_str), Some("d-blocked"));
        }
    }

    #[test]
    fn the_workspaces_and_the_groups_of_worktrees_follow_the_same_criteria() {
        let workspace = |id: &str, agent_status: Status| Workspace {
            agent_status,
            ..crate::order::tests::workspace(id, id, None)
        };
        let snapshot = Snapshot {
            workspaces: vec![
                workspace("w1", Status::Idle),
                workspace("w2", Status::Blocked),
                workspace("w3", Status::Done),
            ],
            ..Snapshot::default()
        };
        let model = Model::build(&snapshot, &HashMap::new());
        let ranker = Ranker::new(&model, &order(&["attention"]), &Recent::new(), None, None);
        let mut sorted = model.clone();
        ranker.sort(&mut sorted);
        let ids: Vec<&str> = sorted
            .spaces
            .iter()
            .map(|space| space.workspace.workspace_id.as_str())
            .collect();
        assert_eq!(ids, ["w2", "w3", "w1"]);
        // A project takes the best attention of its open workspaces.
        let project = |name: &str| Project::sample(name, &[("main", Some("/work"))]);
        let places: Vec<Place> = [("a", "w1"), ("b", "w2"), ("c", "w3"), ("d", "none")]
            .iter()
            .map(|(name, id)| Place {
                kind: crate::places::Kind::Workspace,
                label: name.to_string(),
                project: name.to_string(),
                folder: format!("/work/{name}").into(),
                workspace_id: Some(id.to_string()),
                main: Some(format!("/work/{name}").into()),
                tabs: Vec::new(),
            })
            .collect();
        let projects = [project("a"), project("b"), project("c"), project("d")];
        let ordered: Vec<&str> = ranker
            .projects(&projects, &places)
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(ordered, ["b", "c", "a", "d"]);
    }
}
