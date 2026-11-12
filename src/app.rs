//! The popup: its state, its keys and its loop.
//!
//! The loop draws the popup, waits for a key, and reads the herdr snapshot again every 1.5 s. Enter
//! closes the popup, then goes to the selected row, or runs the creation.
//!
//! The filter New is the creation, in 2 steps. The step "where" chooses a place: the text is a
//! filter. The step "what" chooses what to create there: the text is the name of the tab or of the
//! branch. The text is never both.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal};
use ratatui::layout::Rect;

use crate::changes::Changes;
use crate::config::{self, Config, Preview};
use crate::context::Context;
use crate::create::{self, Choice, Plan};
use crate::filter::Filter;
use crate::herdr::{self, PluginAction};
use crate::model::{self, AgentRow, Checkout, Model, Status};
use crate::places::{self, Branch, Place};
use crate::remove;
use crate::rows::{self, Item, Line};
use crate::times::{self, Times};
use crate::ui;

/// The time between 2 reads of the herdr snapshot.
const REFRESH: Duration = Duration::from_millis(1500);
/// The time between 2 drawings without a key: the threads of the running times and of the git
/// states give their results in the background.
const REDRAW: Duration = Duration::from_millis(250);

/// The step of a creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Where,
    What,
}

/// The state of a creation.
#[derive(Clone, Debug)]
pub struct Create {
    pub step: Step,
    /// The place of the step "what".
    pub place: Option<Place>,
    /// The text of the step "where": a filter.
    pub filter: String,
    /// The text of the step "what": the name of the tab or of the branch.
    pub name: String,
    /// The agent of the creation, if the user changed it with Tab.
    pub agent: Option<String>,
    pub no_prompts: bool,
    /// The local branches of the git project of the place.
    pub branches: Vec<Branch>,
    /// The default branch of the git project of the place: a new branch starts there.
    pub base: Option<String>,
}

impl Create {
    fn new() -> Create {
        Create {
            step: Step::Where,
            place: None,
            filter: String::new(),
            name: String::new(),
            agent: None,
            no_prompts: false,
            branches: Vec::new(),
            base: None,
        }
    }
}

/// A tab to rename: its id, its old name, and the new name that the user types.
#[derive(Clone, Debug)]
pub struct Rename {
    pub tab_id: String,
    pub old: String,
    pub text: String,
}

/// The question of the action line, before a close or a removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Close a tab: its id and its name.
    CloseTab { tab_id: String, name: String },
    /// Remove a worktree: its folder, its branch, and the number of agents in its workspace.
    RemoveWorktree {
        folder: PathBuf,
        branch: String,
        agents: usize,
    },
}

/// The state of the popup.
pub struct App {
    pub model: Model,
    pub filter: Filter,
    /// The filters of the chips and of Tab, from the features of config.toml.
    pub filters: Vec<Filter>,
    /// The text of the filters that go somewhere.
    pub query: String,
    pub lines: Vec<Line>,
    /// The index in `lines` of the selected row.
    pub selected: Option<usize>,
    pub preview: bool,
    pub help: bool,
    pub create: Create,
    /// The open workspaces, as places.
    pub open: Vec<Place>,
    /// The places without a workspace: projects and folders.
    pub others: Vec<Place>,
    /// The workspace of the context: the current place.
    pub current: Option<String>,
    /// The agents on the PATH, then "shell".
    pub agents: Vec<String>,
    /// The options of config.toml.
    pub config: Config,
    /// The tab to close or the worktree to remove, while the action line asks the question.
    pub confirm: Option<Confirm>,
    /// The tab to rename, while the search line holds its new name.
    pub rename: Option<Rename>,
    /// A message on the action line: the result of the last command.
    pub message: Option<String>,
    /// Read the snapshot again at the next turn of the loop.
    pub reload: bool,
    /// The area of the list and the index of its first visible line, for the mouse.
    pub list_area: Rect,
    /// The area of each chip of a filter, and of each key of the footer, for the mouse.
    pub chip_areas: Vec<(Rect, Filter)>,
    pub hint_areas: Vec<(Rect, String)>,
    pub list_offset: usize,
    /// The last click: the line and the time, for a double click.
    last_click: Option<(usize, Instant)>,
    /// The actions of the herdr plugins, read the first time that the filter Actions shows.
    actions: Option<Vec<PluginAction>>,
    /// The view of the preview of a worktree: 0 diff, 1 log, 2 changes.
    pub view: usize,
    /// The text of each view of each branch, read one time.
    views: HashMap<(String, usize), String>,
    /// The branches of the current project, for the filter Worktrees.
    current_branches: Option<Vec<Branch>>,
    /// The default branch of the current project, read with its branches.
    current_base: Option<String>,
    /// The text of each pane that the preview has read, by pane id.
    previews: HashMap<String, String>,
    /// The running times of the panes (feature running_time).
    pub times: Option<Times>,
    /// The git state of the worktrees of the branch rows.
    pub changes: Changes,
    /// Show the logos of the agents: the feature agent_logos, and the font is installed.
    pub logos: bool,
}

/// What a key asks for.
#[derive(Debug)]
pub enum Outcome {
    Continue,
    Quit,
    Go(Item),
    Create(Plan),
    /// Run an action of a herdr plugin, after the popup closes.
    Invoke(PluginAction),
    /// Remove the worktree of this folder, after the popup closes.
    Remove(PathBuf),
}

impl App {
    pub fn new(
        model: Model,
        filter: Filter,
        others: Vec<Place>,
        current: Option<String>,
        agents: Vec<String>,
        config: Config,
    ) -> App {
        let open = places::workspaces(&model);
        let filters = rows::filters(config.has("pane_rows"));
        let mut app = App {
            model,
            filter,
            filters,
            query: String::new(),
            lines: Vec::new(),
            selected: None,
            preview: true,
            help: false,
            create: Create::new(),
            open,
            others,
            current,
            agents,
            config,
            confirm: None,
            rename: None,
            message: None,
            reload: false,
            list_area: Rect::default(),
            chip_areas: Vec::new(),
            hint_areas: Vec::new(),
            list_offset: 0,
            last_click: None,
            actions: None,
            view: 0,
            views: HashMap::new(),
            current_branches: None,
            current_base: None,
            previews: HashMap::new(),
            times: None,
            changes: Changes::default(),
            logos: false,
        };
        app.rebuild(None);
        app
    }

    /// The text of the search line: the query, or the text of the step of the creation.
    pub fn text(&self) -> &str {
        if let Some(rename) = &self.rename {
            return &rename.text;
        }
        match (self.filter, self.create.step) {
            (Filter::New, Step::Where) => &self.create.filter,
            (Filter::New, Step::What) => &self.create.name,
            _ => &self.query,
        }
    }

    fn text_mut(&mut self) -> &mut String {
        if let Some(rename) = &mut self.rename {
            return &mut rename.text;
        }
        match (self.filter, self.create.step) {
            (Filter::New, Step::Where) => &mut self.create.filter,
            (Filter::New, Step::What) => &mut self.create.name,
            _ => &mut self.query,
        }
    }

    /// The age of the last commit of a branch of the branch rows: now, 12m, 2h04m, 3d.
    pub fn branch_age(&self, name: &str) -> Option<String> {
        let branches = if self.filter == Filter::New {
            Some(&self.create.branches)
        } else {
            self.current_branches.as_ref()
        }?;
        let time = branches.iter().find(|branch| branch.name == name)?.time?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let age = Duration::from_secs(now.saturating_sub(time));
        Some(if age.as_secs() < 60 {
            "now".to_string()
        } else {
            times::short(age)
        })
    }

    /// The default branch of the project of the branch rows, without "origin/": "main".
    pub fn base_branch(&self) -> Option<&str> {
        let base = if self.filter == Filter::New {
            self.create.base.as_deref()
        } else {
            self.current_base.as_deref()
        }?;
        Some(base.strip_prefix("origin/").unwrap_or(base))
    }

    /// The place of the current workspace.
    pub fn current_place(&self) -> Option<&Place> {
        let current = self.current.as_deref()?;
        self.open
            .iter()
            .find(|place| place.workspace_id.as_deref() == Some(current))
    }

    /// Build the lines again. The selection stays on the row with this key, if it is still there.
    fn rebuild(&mut self, keep: Option<String>) {
        self.lines = match (self.filter, self.create.step) {
            (Filter::New, Step::Where) => rows::where_lines(
                &self.open,
                &self.others,
                self.current.as_deref(),
                &self.create.filter,
            ),
            (Filter::New, Step::What) => match &self.create.place {
                Some(place) => rows::what_lines(
                    place,
                    create::choices(place, &self.create.name, &self.create.branches, &self.open),
                ),
                None => Vec::new(),
            },
            (Filter::Worktrees, _) => {
                let project = self
                    .current_place()
                    .map(|place| place.project.clone())
                    .unwrap_or_default();
                let branches = self.current_branches();
                rows::worktree_lines(&branches, &self.open, &project, &self.query)
            }
            _ => {
                if self.filter == Filter::Actions && self.actions.is_none() {
                    self.actions = Some(herdr::plugin_actions().unwrap_or_default());
                }
                let branches = if self.filter == Filter::All {
                    self.current_branches()
                } else {
                    Vec::new()
                };
                let project = self
                    .current_place()
                    .map(|place| place.project.clone())
                    .unwrap_or_default();
                let sources = rows::Sources {
                    model: &self.model,
                    others: &self.others,
                    actions: self.actions.as_deref().unwrap_or_default(),
                    open: &self.open,
                    project: &project,
                    branches: &branches,
                };
                rows::build(&sources, self.filter, &self.query)
            }
        };
        let rows = self.row_indices();
        let mut selected = keep
            .and_then(|key| {
                rows.iter().copied().find(
                    |&index| matches!(&self.lines[index], Line::Row { item, .. } if item.key() == key),
                )
            })
            .or_else(|| rows.first().copied());
        // A name that is not a branch name selects the next choice: the new tab.
        if let (Some(index), true) = (selected, self.filter == Filter::New) {
            if let Line::Row {
                item: Item::Choice(Choice::NewWorktree { branch }),
                ..
            } = &self.lines[index]
            {
                if !branch.is_empty() && !create::is_branch_name(branch) {
                    selected = rows.get(1).copied().or(selected);
                }
            }
        }
        self.selected = selected;
    }

    /// The branches of the current project, read one time.
    fn current_branches(&mut self) -> Vec<Branch> {
        if self.current_branches.is_none() {
            let main = self.current_place().and_then(|place| place.main.clone());
            self.current_branches = Some(main.as_deref().map(places::branches).unwrap_or_default());
            self.current_base = main.as_deref().map(places::base_branch);
            let worktrees = self
                .current_branches
                .iter()
                .flatten()
                .filter_map(|branch| branch.worktree.clone())
                .collect::<Vec<_>>();
            self.changes.ask(worktrees);
        }
        self.current_branches.clone().unwrap_or_default()
    }

    fn row_indices(&self) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| matches!(line, Line::Row { .. }))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected_item(&self) -> Option<&Item> {
        match self.selected.and_then(|index| self.lines.get(index)) {
            Some(Line::Row { item, .. }) => Some(item),
            _ => None,
        }
    }

    /// The running time of the foreground process of a pane, as a short text: 47s, 12m, 2h04m.
    pub fn running_time(&self, pane: &str) -> Option<String> {
        self.times.as_ref()?.of(pane).map(times::short)
    }

    /// Ask the thread of the running times for the panes of the model.
    pub fn ask_times(&mut self) {
        if let Some(times) = self.times.as_mut() {
            times.ask(self.model.panes.iter().map(|pane| pane.pane_id.clone()));
        }
    }

    /// Take a new model: the snapshot changed.
    pub fn update_model(&mut self, model: Model) {
        let key = self.selected_item().map(Item::key);
        self.open = places::workspaces(&model);
        self.model = model;
        self.ask_times();
        self.previews.clear();
        self.rebuild(key);
    }

    fn move_by(&mut self, delta: i32) {
        let rows = self.row_indices();
        if rows.is_empty() {
            return;
        }
        let at = self
            .selected
            .and_then(|selected| rows.iter().position(|&index| index == selected))
            .unwrap_or(0) as i32;
        let next = (at + delta).clamp(0, rows.len() as i32 - 1) as usize;
        self.selected = Some(rows[next]);
    }

    fn set_filter(&mut self, filter: Filter) {
        if filter == Filter::New && self.filter != Filter::New {
            self.create = Create::new();
        }
        self.filter = filter;
        self.rebuild(None);
    }

    fn cycle_filter(&mut self, delta: i32) {
        let count = self.filters.len() as i32;
        let at = self
            .filters
            .iter()
            .position(|filter| *filter == self.filter)
            .unwrap_or(0) as i32;
        self.set_filter(self.filters[(at + delta).rem_euclid(count) as usize]);
    }

    fn edit(&mut self, change: impl FnOnce(&mut String)) {
        change(self.text_mut());
        // A new name of a tab does not filter the list.
        if self.rename.is_none() {
            self.rebuild(None);
        }
    }

    /// The agent of the creation: the agent that the user chose, else the last agent of the
    /// place, else the agent of its tabs, else the first agent on the PATH.
    pub fn agent(&self) -> String {
        if let Some(agent) = &self.create.agent {
            return agent.clone();
        }
        self.agent_for(self.create.place.as_ref().or_else(|| self.current_place()))
    }

    /// The agent of a place: its last agent, else the agent of its tabs, else the first agent on
    /// the PATH.
    fn agent_for(&self, place: Option<&Place>) -> String {
        let remembered = place.and_then(|place| create::remembered_agent(&place.folder));
        let from_tabs = place.and_then(|place| place.tabs.iter().find_map(|tab| tab.agent.clone()));
        remembered
            .or(from_tabs)
            .filter(|agent| self.agents.contains(agent))
            .or_else(|| self.agents.first().cloned())
            .unwrap_or_else(|| "shell".to_string())
    }

    fn cycle_agent(&mut self) {
        let current = self.agent();
        let at = self
            .agents
            .iter()
            .position(|agent| *agent == current)
            .unwrap_or(0);
        self.create.agent = self
            .agents
            .get((at + 1) % self.agents.len().max(1))
            .cloned();
    }

    /// Choose a place: go to the step "what". The name stays.
    fn choose_place(&mut self, place: Place) {
        self.create.branches = place
            .main
            .as_deref()
            .map(places::branches)
            .unwrap_or_default();
        self.create.base = place.main.as_deref().map(places::base_branch);
        let worktrees = self
            .create
            .branches
            .iter()
            .filter_map(|branch| branch.worktree.clone())
            .collect::<Vec<_>>();
        self.changes.ask(worktrees);
        self.create.place = Some(place);
        self.create.step = Step::What;
        self.create.agent = None;
        self.rebuild(None);
    }

    fn back_to_where(&mut self) {
        self.create.step = Step::Where;
        self.create.filter.clear();
        let key = self
            .create
            .place
            .as_ref()
            .map(|place| format!("place:{}", place.key()));
        self.rebuild(key);
    }

    /// The plan of a choice for a place.
    fn plan(&self, place: &Place, choice: &Choice, name: &str) -> Option<Plan> {
        create::plan(place, choice, name, &self.agent(), self.create.no_prompts)
    }

    /// Enter on the selected row.
    fn enter(&mut self) -> Outcome {
        let Some(item) = self.selected_item().cloned() else {
            return Outcome::Continue;
        };
        match (self.filter, item) {
            (_, Item::Place(place)) => {
                if self.filter != Filter::New {
                    self.filter = Filter::New;
                    self.create = Create::new();
                }
                self.choose_place(place);
                Outcome::Continue
            }
            (Filter::New, Item::Choice(Choice::AnotherPlace)) => {
                self.back_to_where();
                Outcome::Continue
            }
            (Filter::New, Item::Choice(choice)) => {
                let Some(place) = self.create.place.clone() else {
                    return Outcome::Continue;
                };
                match self.plan(&place, &choice, &self.create.name.clone()) {
                    Some(plan) => Outcome::Create(plan),
                    None => Outcome::Continue,
                }
            }
            (_, Item::Choice(choice)) => {
                // The filter Worktrees: the place is the current workspace.
                let Some(place) = self.current_place().cloned() else {
                    return Outcome::Continue;
                };
                let name = match &choice {
                    Choice::NewWorktree { branch } | Choice::Existing { branch, .. } => {
                        branch.clone()
                    }
                    _ => String::new(),
                };
                match self.plan(&place, &choice, &name) {
                    Some(plan) => Outcome::Create(plan),
                    None => Outcome::Continue,
                }
            }
            (_, Item::Action(action)) => Outcome::Invoke(action),
            (_, Item::Create(name)) => {
                // The step "what" in the current place, with the query as the name.
                self.filter = Filter::New;
                self.create = Create::new();
                self.create.name = name;
                match self.current_place().cloned() {
                    Some(place) => self.choose_place(place),
                    None => self.rebuild(None),
                }
                Outcome::Continue
            }
            (_, item) => Outcome::Go(item),
        }
    }

    /// The text of a view of the preview of a branch of the current project: the diff since the
    /// default branch, the log, or the changes not committed. With preview = "summary", one view
    /// gives all.
    pub fn branch_view(&mut self, branch: &str, worktree: Option<&PathBuf>) -> String {
        let view = if self.config.preview == Preview::Summary {
            3
        } else {
            self.view % 3
        };
        let key = (branch.to_string(), view);
        if let Some(text) = self.views.get(&key) {
            return text.clone();
        }
        let Some(main) = self.current_place().and_then(|place| place.main.clone()) else {
            return String::new();
        };
        let base = places::base_branch(&main);
        let range = format!("{base}..{branch}");
        let text = match view {
            0 => places::git(&main, &["diff", "--stat", &format!("{base}...{branch}")]),
            1 => places::git(&main, &["log", "--oneline", "-n", "40", &range]),
            2 => match worktree {
                Some(folder) => {
                    let status = places::git(folder, &["status", "--short"]);
                    if status.trim().is_empty() {
                        "no change".to_string()
                    } else {
                        status
                    }
                }
                None => "no worktree".to_string(),
            },
            _ => {
                let count = places::git(&main, &["rev-list", "--count", &range]);
                let stat = places::git(
                    &main,
                    &["diff", "--shortstat", &format!("{base}...{branch}")],
                );
                let log = places::git(&main, &["log", "--oneline", "-n", "8", &range]);
                format!(
                    "{} commits ahead of {base}\n{}\n\n{log}",
                    count.trim(),
                    stat.trim()
                )
            }
        };
        let text = if text.trim().is_empty() {
            format!(
                "no commit ahead of {}",
                base.strip_prefix("origin/").unwrap_or(&base)
            )
        } else {
            text
        };
        self.views.insert(key, text.clone());
        text
    }

    /// True if ↵ on the selected row opens a worktree outside the creation: ⌃A then changes its
    /// agent.
    pub fn opens_worktree(&self) -> bool {
        if self.filter == Filter::New {
            return false;
        }
        match self.selected_item() {
            Some(Item::Choice(Choice::NewWorktree { branch })) => create::is_branch_name(branch),
            Some(Item::Choice(Choice::Existing { workspace_id, .. })) => workspace_id.is_none(),
            _ => false,
        }
    }

    /// The workspace of the selected row, as a place: the workspace of an agent or of a tab, a
    /// workspace, the open workspace of a worktree, or an open place.
    fn place_of_selected(&self) -> Option<Place> {
        let workspace_id = match self.selected_item()? {
            Item::Agent(agent) => agent.workspace_id.clone(),
            Item::Tab(tab) => tab.workspace_id.clone(),
            Item::Workspace(space) => space.workspace.workspace_id.clone(),
            Item::Choice(Choice::Existing {
                workspace_id: Some(id),
                ..
            }) => id.clone(),
            Item::Place(place) => place.workspace_id.clone()?,
            Item::Pane(pane) => pane.workspace_id.clone(),
            _ => return None,
        };
        self.open
            .iter()
            .find(|place| place.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .cloned()
    }

    /// The label of the workspace where ctrl+Enter adds a tab.
    pub fn new_tab_place(&self) -> Option<String> {
        self.place_of_selected().map(|place| place.label)
    }

    /// True if ctrl+Enter adds a tab next to the selected row: the row has an open workspace.
    pub fn adds_tab_here(&self) -> bool {
        self.filter != Filter::New && self.place_of_selected().is_some()
    }

    /// ctrl+Enter: a new tab, with its agent, next to the selected row.
    fn new_tab_here(&mut self) -> Outcome {
        let Some(place) = self.place_of_selected() else {
            return Outcome::Continue;
        };
        let agent = self.agent_for(Some(&place));
        match create::plan(&place, &Choice::NewTab, "", &agent, false) {
            Some(plan) => Outcome::Create(plan),
            None => Outcome::Continue,
        }
    }

    /// The tab of the selected agent or tab: its id and its name.
    fn selected_tab(&self) -> Option<(String, String)> {
        match self.selected_item()? {
            Item::Agent(agent) => Some((agent.tab_id.clone(), agent.name.clone())),
            Item::Tab(tab) => Some((tab.tab_id.clone(), tab.name.clone())),
            _ => None,
        }
    }

    /// The main checkout of the project of the branch rows: the place of the step "what", else
    /// the current place.
    fn project_main(&self) -> Option<PathBuf> {
        self.create
            .place
            .as_ref()
            .filter(|_| self.filter == Filter::New)
            .or_else(|| self.current_place())
            .and_then(|place| place.main.clone())
    }

    /// True if ctrl+Backspace on the selected row removes a worktree: a branch with a worktree
    /// that is not the main checkout.
    pub fn can_remove_worktree(&self) -> bool {
        match self.selected_item() {
            Some(Item::Choice(Choice::Existing {
                worktree: Some(folder),
                ..
            })) => self.project_main().as_ref() != Some(folder),
            _ => false,
        }
    }

    /// ctrl+Backspace: ask before the close of the selected tab, or before the removal of the
    /// selected worktree.
    fn ask_close(&mut self) {
        if let Some((tab_id, name)) = self.selected_tab() {
            self.confirm = Some(Confirm::CloseTab { tab_id, name });
            return;
        }
        let Some(Item::Choice(Choice::Existing {
            branch,
            worktree,
            workspace_id,
        })) = self.selected_item().cloned()
        else {
            return;
        };
        let Some(folder) = worktree else {
            self.message = Some(format!("the branch {branch} has no worktree to remove"));
            return;
        };
        if self.project_main().as_ref() == Some(&folder) {
            self.message = Some(format!(
                "{branch} is the main checkout of the project: it stays"
            ));
            return;
        }
        // The agents of the worktree: in its workspace, or in its folder. herdr-worktrunk
        // closes the idle agents, and refuses an agent that works or waits for an answer.
        let agents: Vec<&AgentRow> = self
            .model
            .agents
            .iter()
            .filter(|agent| {
                workspace_id.as_ref() == Some(&agent.workspace_id)
                    || agent
                        .cwd
                        .as_ref()
                        .is_some_and(|cwd| cwd.starts_with(&folder))
            })
            .collect();
        if agents
            .iter()
            .any(|agent| !matches!(agent.status, Status::Idle | Status::Done))
        {
            self.message = Some(format!(
                "an agent works in the worktree {branch}, or waits for you: stop it first"
            ));
            return;
        }
        if !agents.is_empty() && remove::worktrunk_remover().is_none() {
            self.message = Some(
                "only herdr-worktrunk 2026.11.33 or later closes the agents of a worktree: \
                 update it, or stop the agents first"
                    .to_string(),
            );
            return;
        }
        self.confirm = Some(Confirm::RemoveWorktree {
            folder,
            branch,
            agents: agents.len(),
        });
    }

    fn on_confirm_key(&mut self, key: KeyEvent) -> Outcome {
        let Some(confirm) = self.confirm.take() else {
            return Outcome::Continue;
        };
        if key.code != KeyCode::Enter {
            return Outcome::Continue;
        }
        match confirm {
            Confirm::CloseTab { tab_id, name } => {
                self.message = Some(match herdr::close_tab(&tab_id) {
                    Ok(()) => format!("closed the tab {name}"),
                    Err(error) => format!("cannot close the tab {name}: {error}"),
                });
                self.reload = true;
            }
            // The removal can close the workspace of the popup: it runs after the popup closes.
            Confirm::RemoveWorktree { folder, .. } => return Outcome::Remove(folder),
        }
        Outcome::Continue
    }

    /// ctrl+R: rename the tab of the selected row, in the search line.
    fn start_rename(&mut self) {
        if let Some((tab_id, name)) = self.selected_tab() {
            self.rename = Some(Rename {
                tab_id,
                old: name.clone(),
                text: name,
            });
        }
    }

    fn finish_rename(&mut self, save: bool) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        let text = rename.text.trim();
        if save && !text.is_empty() && text != rename.old {
            self.message = Some(match herdr::rename_tab(&rename.tab_id, text) {
                Ok(()) => format!("renamed the tab {} to {text}", rename.old),
                Err(error) => format!("cannot rename the tab: {error}"),
            });
            self.reload = true;
        }
    }

    /// A click selects a row, a double click runs it, the wheel moves the selection.
    pub fn on_mouse(&mut self, mouse: MouseEvent) -> Outcome {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.move_by(1),
            MouseEventKind::ScrollUp => self.move_by(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                let at = |area: &Rect| {
                    mouse.row >= area.y
                        && mouse.row < area.y + area.height
                        && mouse.column >= area.x
                        && mouse.column < area.x + area.width
                };
                // A key of the footer runs, as if the user pressed it.
                if let Some(key) = self
                    .hint_areas
                    .iter()
                    .find(|(area, _)| at(area))
                    .and_then(|(_, key)| key_of_hint(key))
                {
                    return self.on_key(key);
                }
                // A chip opens its filter.
                if let Some(filter) = self
                    .chip_areas
                    .iter()
                    .find(|(area, _)| at(area))
                    .map(|(_, filter)| *filter)
                {
                    if self.confirm.is_none() && self.rename.is_none() {
                        self.set_filter(filter);
                    }
                    return Outcome::Continue;
                }
                let area = self.list_area;
                let inside = mouse.row >= area.y
                    && mouse.row < area.y + area.height
                    && mouse.column >= area.x
                    && mouse.column < area.x + area.width;
                if !inside || self.confirm.is_some() || self.rename.is_some() {
                    return Outcome::Continue;
                }
                let index = self.list_offset + (mouse.row - area.y) as usize;
                if matches!(self.lines.get(index), Some(Line::Row { .. })) {
                    let double = self.last_click.is_some_and(|(line, time)| {
                        line == index && time.elapsed() < Duration::from_millis(400)
                    });
                    self.selected = Some(index);
                    self.last_click = Some((index, Instant::now()));
                    if double {
                        return self.enter();
                    }
                }
            }
            _ => {}
        }
        Outcome::Continue
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let creating = self.filter == Filter::New;
        let what = creating && self.create.step == Step::What;
        if self.help {
            self.help = false;
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1)) {
                return Outcome::Continue;
            }
        }
        if self.confirm.is_some() {
            return self.on_confirm_key(key);
        }
        self.message = None;
        let renaming = self.rename.is_some();
        if renaming && matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            self.finish_rename(key.code == KeyCode::Enter);
            return Outcome::Continue;
        }
        match key.code {
            KeyCode::Left if ctrl => self.view = (self.view + 2) % 3,
            KeyCode::Right if ctrl => self.view = (self.view + 1) % 3,
            KeyCode::Enter if ctrl && !creating => return self.new_tab_here(),
            KeyCode::Backspace if ctrl && !renaming => self.ask_close(),
            KeyCode::Char('r') if ctrl && !creating && self.config.has("rename_tab") => {
                self.start_rename()
            }
            KeyCode::Esc if what => self.back_to_where(),
            KeyCode::Esc => return Outcome::Quit,
            KeyCode::Char('a') if ctrl && self.opens_worktree() => self.cycle_agent(),
            KeyCode::Char('c' | 'g') if ctrl => return Outcome::Quit,
            KeyCode::Enter => return self.enter(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Char('p' | 'k') if ctrl => self.move_by(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.move_by(1),
            KeyCode::PageUp => self.move_by(-8),
            KeyCode::PageDown => self.move_by(8),
            KeyCode::Left if what && self.create.name.is_empty() => self.back_to_where(),
            KeyCode::Tab if what => self.cycle_agent(),
            KeyCode::Tab if creating && rows::is_path(&self.create.filter) => {
                if let Some(completed) = rows::complete_path(&self.create.filter, &self.lines) {
                    self.create.filter = completed;
                    self.rebuild(None);
                }
            }
            KeyCode::Tab => self.cycle_filter(1),
            KeyCode::BackTab => self.cycle_filter(-1),
            KeyCode::Char('y') if ctrl && what => self.create.no_prompts = !self.create.no_prompts,
            KeyCode::Char('o') if ctrl => self.preview = !self.preview,
            KeyCode::Char('u') if ctrl => self.edit(String::clear),
            KeyCode::Char('w') if ctrl => self.edit(delete_word),
            KeyCode::Backspace => self.edit(|text| {
                text.pop();
            }),
            KeyCode::F(1) => self.help = true,
            KeyCode::Char(character) if !ctrl && !alt => {
                // On an empty text, a shortcut opens its filter from any filter, also from the
                // creation. The shortcut of the current filter goes back to All.
                let shortcut =
                    Filter::of_shortcut(character).filter(|filter| self.filters.contains(filter));
                let empty = self.text().is_empty() && !renaming;
                if empty && character == '?' {
                    self.help = true;
                } else if let (true, Some(filter)) = (empty, shortcut) {
                    if filter == self.filter {
                        self.set_filter(Filter::All);
                    } else {
                        self.set_filter(filter);
                    }
                } else {
                    self.edit(|text| text.push(character));
                }
            }
            _ => {}
        }
        Outcome::Continue
    }

    /// The text of the pane of the selected row, for the preview. herdr reads it one time for each
    /// pane and each snapshot.
    pub fn preview_text(&mut self, lines: usize) -> Option<String> {
        let pane = match self.selected_item()? {
            Item::Agent(agent) => Some(agent.pane_id.clone()),
            Item::Tab(tab) => tab.pane_id.clone(),
            Item::Pane(pane) => Some(pane.pane_id.clone()),
            _ => None,
        }?;
        let text = self.previews.entry(pane.clone()).or_insert_with(|| {
            herdr::read_pane(&pane, lines).unwrap_or_else(|error| error.to_string())
        });
        Some(text.clone())
    }
}

/// The key event of a key of the footer.
fn key_of_hint(hint: &str) -> Option<KeyEvent> {
    let (code, modifiers) = match hint {
        "↵" => (KeyCode::Enter, KeyModifiers::NONE),
        "⌃↵" => (KeyCode::Enter, KeyModifiers::CONTROL),
        "⇥" => (KeyCode::Tab, KeyModifiers::NONE),
        "⌃⌫" => (KeyCode::Backspace, KeyModifiers::CONTROL),
        "esc" => (KeyCode::Esc, KeyModifiers::NONE),
        "?" => (KeyCode::F(1), KeyModifiers::NONE),
        _ => {
            // ⌃A, ⌃O, ⌃Y: ctrl and a letter.
            let letter = hint.strip_prefix('⌃')?.chars().next()?;
            (
                KeyCode::Char(letter.to_ascii_lowercase()),
                KeyModifiers::CONTROL,
            )
        }
    };
    Some(KeyEvent::new(code, modifiers))
}

/// Delete the last word of a text, and the spaces after it.
fn delete_word(text: &mut String) {
    let trimmed = text.trim_end().len();
    let start = text[..trimmed]
        .rfind(char::is_whitespace)
        .map(|index| index + 1)
        .unwrap_or(0);
    text.truncate(start);
}

/// Run the popup on a filter, in the context of the action.
pub fn run(filter: Filter, context: Context) -> Result<(), String> {
    // The colors of the herdr theme, read at each opening.
    crate::theme::init(crate::theme::load());
    let config: Config = config::current();
    let mut cache: HashMap<PathBuf, Option<Checkout>> = HashMap::new();
    let model = model::load_cached(&mut cache).map_err(|error| error.to_string())?;
    let open = places::workspaces(&model);
    let others = places::others(&config.roots, &open);
    let current = context
        .workspace_id
        .clone()
        .or(model.focused_workspace_id.clone());
    let mouse = config.has("mouse");
    let mut app = App::new(
        model,
        filter,
        others,
        current,
        create::installed_agents(),
        config,
    );
    if app.config.has("running_time") {
        app.times = Some(Times::start());
        app.ask_times();
    }
    app.logos = app.config.has("agent_logos") && crate::icons::font_installed();
    let mut terminal = ratatui::init();
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(crate::keys::flags())
        );
    }
    if mouse {
        let _ = execute!(io::stdout(), EnableMouseCapture);
    }
    let outcome = event_loop(&mut terminal, &mut app, &mut cache);
    if mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    match outcome? {
        Outcome::Go(item) => go(&item).map_err(|error| error.to_string()),
        Outcome::Create(plan) => create::execute_detached(&plan),
        Outcome::Invoke(action) => {
            create::spawn_detached(&["invoke", &action.plugin_id, &action.action_id], &[])
        }
        Outcome::Remove(folder) => {
            create::spawn_detached(&["remove", &folder.to_string_lossy()], &[])
        }
        Outcome::Quit | Outcome::Continue => Ok(()),
    }
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    cache: &mut HashMap<PathBuf, Option<Checkout>>,
) -> Result<Outcome, String> {
    let mut last_read = Instant::now();
    loop {
        terminal
            .draw(|frame| ui::draw(frame, app))
            .map_err(|error| error.to_string())?;
        let timeout = REFRESH.saturating_sub(last_read.elapsed()).min(REDRAW);
        if event::poll(timeout).map_err(|error| error.to_string())? {
            let outcome = match event::read().map_err(|error| error.to_string())? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.on_key(key),
                Event::Mouse(mouse) => app.on_mouse(mouse),
                _ => Outcome::Continue,
            };
            if !matches!(outcome, Outcome::Continue) {
                return Ok(outcome);
            }
            // A command changed herdr: read the snapshot now.
            if app.reload {
                app.reload = false;
                if let Ok(model) = model::load_cached(cache) {
                    app.update_model(model);
                }
            }
        } else if last_read.elapsed() >= REFRESH {
            if let Ok(model) = model::load_cached(cache) {
                app.update_model(model);
            }
            last_read = Instant::now();
        }
    }
}

/// Go to a row: focus its agent, its tab or its workspace.
fn go(item: &Item) -> Result<(), herdr::Error> {
    match item {
        Item::Agent(agent) => {
            herdr::focus_tab(&agent.tab_id)?;
            herdr::focus_agent(&agent.pane_id)
        }
        Item::Tab(tab) => herdr::focus_tab(&tab.tab_id),
        Item::Workspace(space) => herdr::focus_workspace(&space.workspace.workspace_id),
        // herdr has no focus of a pane by its id: agent focus works for an agent pane. Another
        // pane gets the focus of its tab.
        Item::Pane(pane) => {
            herdr::focus_tab(&pane.tab_id)?;
            if pane.agent.is_some() {
                herdr::focus_agent(&pane.pane_id)?;
            }
            Ok(())
        }
        Item::Place(_) | Item::Choice(_) | Item::Action(_) | Item::Create(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentRow, Status};
    use crate::places::Kind;

    fn place(label: &str, workspace: Option<&str>, main: Option<&str>) -> Place {
        Place {
            kind: if workspace.is_some() {
                Kind::Workspace
            } else {
                Kind::Project
            },
            label: label.into(),
            project: label.into(),
            folder: format!("/work/{label}").into(),
            workspace_id: workspace.map(str::to_string),
            main: main.map(PathBuf::from),
            tabs: Vec::new(),
        }
    }

    fn app(filter: Filter) -> App {
        let agents = ["fix the login", "omnibar design", "release notes"]
            .iter()
            .map(|name| AgentRow {
                pane_id: format!("p-{name}"),
                tab_id: format!("t-{name}"),
                workspace_id: "w1".into(),
                name: name.to_string(),
                kind: "claude".into(),
                status: Status::Idle,
                place: "atlas".into(),
                cwd: None,
            })
            .collect();
        let model = Model {
            agents,
            ..Model::default()
        };
        let mut app = App::new(
            model,
            filter,
            vec![place("prompts", None, Some("/work/prompts"))],
            Some("w1".into()),
            vec!["claude".into(), "codex".into(), "shell".into()],
            Config::default(),
        );
        app.open = vec![
            place("atlas", Some("w1"), None),
            place("pac", Some("w2"), None),
        ];
        app.rebuild(None);
        app
    }

    fn press(app: &mut App, code: KeyCode) -> Outcome {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            press(app, KeyCode::Char(character));
        }
    }

    fn selected_name(app: &App) -> String {
        app.selected_item().map(Item::name).unwrap_or_default()
    }

    #[test]
    fn the_first_row_is_selected_and_down_moves() {
        let mut app = app(Filter::All);
        assert_eq!(selected_name(&app), "fix the login");
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_name(&app), "omnibar design");
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(selected_name(&app), "fix the login");
    }

    #[test]
    fn typing_filters_and_enter_goes_to_the_selected_row() {
        let mut app = app(Filter::All);
        type_text(&mut app, "rel");
        assert_eq!(selected_name(&app), "release notes");
        assert!(
            matches!(press(&mut app, KeyCode::Enter), Outcome::Go(Item::Agent(agent)) if agent.name == "release notes")
        );
    }

    #[test]
    fn the_create_row_goes_to_the_step_what_in_the_current_place() {
        let mut app = app(Filter::All);
        type_text(&mut app, "zzz");
        assert_eq!(selected_name(&app), "create “zzz”…");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.filter, Filter::New);
        assert_eq!(app.create.step, Step::What);
        assert_eq!(app.create.name, "zzz");
        assert_eq!(
            app.create.place.as_ref().map(|place| place.label.as_str()),
            Some("atlas")
        );
        assert_eq!(selected_name(&app), "new tab");
    }

    #[test]
    fn ctrl_a_changes_the_agent_of_a_row_that_opens_a_worktree() {
        let mut app = app(Filter::All);
        app.open[0].main = Some("/work/atlas".into());
        app.current_branches = Some(vec![Branch {
            name: "fix-login".into(),
            worktree: None,
            time: None,
        }]);
        app.rebuild(None);
        type_text(&mut app, "fix-l");
        assert_eq!(selected_name(&app), "fix-login");
        assert!(app.opens_worktree());
        assert_eq!(app.agent(), "claude");
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(app.agent(), "codex");
        match press(&mut app, KeyCode::Enter) {
            Outcome::Create(plan) => {
                assert_eq!(plan.agent, "codex");
                assert_eq!(plan.name, "fix-login");
            }
            outcome => panic!("unexpected outcome {outcome:?}"),
        }
    }

    #[test]
    fn a_shortcut_works_from_any_filter_and_its_second_press_goes_back_to_all() {
        let mut app = app(Filter::New);
        press(&mut app, KeyCode::Char('@'));
        assert_eq!(app.filter, Filter::Agents);
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.filter, Filter::Actions);
        press(&mut app, KeyCode::Char('#'));
        assert_eq!(app.filter, Filter::Folders);
        press(&mut app, KeyCode::Char('#'));
        assert_eq!(app.filter, Filter::All);
        // Without the feature pane_rows, % is a character of the text.
        press(&mut app, KeyCode::Char('%'));
        assert_eq!((app.filter, app.query.as_str()), (Filter::All, "%"));
    }

    #[test]
    fn tab_changes_the_filter_and_at_on_an_empty_query_shows_the_agents() {
        let mut app = app(Filter::All);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.filter, Filter::Agents);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.filter, Filter::All);
        press(&mut app, KeyCode::Char('@'));
        assert_eq!(app.filter, Filter::Agents);
        assert!(app.query.is_empty());
    }

    #[test]
    fn the_creation_starts_at_the_current_place_then_keeps_the_name() {
        let mut app = app(Filter::New);
        assert_eq!(app.create.step, Step::Where);
        assert_eq!(selected_name(&app), "atlas");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.create.step, Step::What);
        type_text(&mut app, "review");
        assert_eq!(app.create.name, "review");
        assert_eq!(selected_name(&app), "new tab");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.create.step, Step::Where);
        assert_eq!(app.create.name, "review");
        type_text(&mut app, "pac");
        assert_eq!(selected_name(&app), "pac");
    }

    #[test]
    fn in_a_project_a_branch_name_selects_the_worktree_and_a_sentence_the_tab() {
        let mut app = app(Filter::New);
        type_text(&mut app, "prompts");
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "fix-login");
        assert_eq!(selected_name(&app), "new worktree fix-login");
        press(&mut app, KeyCode::Char(' '));
        type_text(&mut app, "now");
        assert_eq!(selected_name(&app), "new workspace");
    }

    #[test]
    fn tab_changes_the_agent_in_the_step_what_and_enter_gives_the_plan() {
        let mut app = app(Filter::New);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.agent(), "claude");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.agent(), "codex");
        app.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        type_text(&mut app, "notes");
        match press(&mut app, KeyCode::Enter) {
            Outcome::Create(plan) => {
                assert_eq!(plan.agent, "codex");
                assert!(plan.no_prompts);
                assert_eq!(plan.name, "notes");
                assert_eq!(
                    plan.action,
                    create::Action::NewTab {
                        workspace_id: "w1".into(),
                        folder: "/work/atlas".into()
                    }
                );
            }
            other => panic!("no plan: {other:?}"),
        }
    }

    #[test]
    fn ctrl_r_renames_the_tab_in_the_search_line_and_escape_keeps_the_name() {
        let mut app = app(Filter::All);
        app.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(app.text(), "fix the login");
        press(&mut app, KeyCode::Backspace);
        type_text(&mut app, "?x");
        assert_eq!(app.text(), "fix the logi?x");
        assert!(!app.help);
        assert_eq!(selected_name(&app), "fix the login");
        press(&mut app, KeyCode::Esc);
        assert!(app.rename.is_none());
        assert_eq!(app.text(), "");
    }

    #[test]
    fn ctrl_backspace_asks_and_escape_cancels() {
        let mut app = app(Filter::All);
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert_eq!(
            app.confirm,
            Some(Confirm::CloseTab {
                tab_id: "t-fix the login".to_string(),
                name: "fix the login".to_string()
            })
        );
        press(&mut app, KeyCode::Esc);
        assert!(app.confirm.is_none());
        assert!(!app.reload);
    }

    #[test]
    fn ctrl_backspace_on_a_worktree_asks_then_removes_it_and_keeps_the_main_checkout() {
        let mut app = app(Filter::Worktrees);
        app.open[0].main = Some("/work/atlas".into());
        app.current_branches = Some(vec![
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
        ]);
        app.rebuild(None);
        assert_eq!(selected_name(&app), "main");
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert!(app.confirm.is_none());
        assert!(app
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("main checkout"));
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_name(&app), "fix-login");
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert!(matches!(app.confirm, Some(Confirm::RemoveWorktree { .. })));
        match press(&mut app, KeyCode::Enter) {
            Outcome::Remove(folder) => assert_eq!(folder, PathBuf::from("/work/atlas.fix-login")),
            outcome => panic!("unexpected outcome {outcome:?}"),
        }
    }

    #[test]
    fn ctrl_backspace_refuses_a_worktree_where_an_agent_works() {
        let mut app = app(Filter::Worktrees);
        app.open[0].main = Some("/work/atlas".into());
        app.model.agents[0].status = Status::Working;
        app.model.agents[0].cwd = Some("/work/atlas.fix-login/src".into());
        app.current_branches = Some(vec![Branch {
            name: "fix-login".into(),
            worktree: Some("/work/atlas.fix-login".into()),
            time: None,
        }]);
        app.rebuild(None);
        assert_eq!(selected_name(&app), "fix-login");
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert!(app.confirm.is_none());
        assert!(app
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("stop it first"));
    }

    #[test]
    fn ctrl_enter_creates_a_tab_next_to_the_agent() {
        let mut app = app(Filter::All);
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) {
            Outcome::Create(plan) => assert_eq!(
                plan.action,
                create::Action::NewTab {
                    workspace_id: "w1".into(),
                    folder: "/work/atlas".into()
                }
            ),
            other => panic!("no plan: {other:?}"),
        }
    }

    #[test]
    fn ctrl_enter_creates_a_tab_in_the_open_workspace_of_a_worktree() {
        let mut app = app(Filter::All);
        app.open[0].main = Some("/work/atlas".into());
        app.current_branches = Some(vec![Branch {
            name: "main".into(),
            worktree: Some("/work/atlas".into()),
            time: None,
        }]);
        app.rebuild(None);
        // The 3 agents, then the group of the worktrees.
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(selected_name(&app), "main");
        assert!(app.adds_tab_here());
        match app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) {
            Outcome::Create(plan) => assert_eq!(
                plan.action,
                create::Action::NewTab {
                    workspace_id: "w1".into(),
                    folder: "/work/atlas".into()
                }
            ),
            other => panic!("no plan: {other:?}"),
        }
    }

    #[test]
    fn a_click_selects_and_a_double_click_runs() {
        let mut app = app(Filter::All);
        app.list_area = Rect::new(0, 2, 80, 10);
        let click = |row| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row,
            modifiers: KeyModifiers::NONE,
        };
        // Line 0 is the group, line 2 is the second agent.
        assert!(matches!(app.on_mouse(click(4)), Outcome::Continue));
        assert_eq!(selected_name(&app), "omnibar design");
        assert!(matches!(app.on_mouse(click(4)), Outcome::Go(_)));
    }

    #[test]
    fn a_click_on_a_chip_opens_its_filter_and_a_click_on_a_key_runs_it() {
        let mut app = app(Filter::All);
        app.chip_areas = vec![(Rect::new(50, 0, 8, 1), Filter::Agents)];
        app.hint_areas = vec![(Rect::new(10, 30, 9, 1), "⌃O".to_string())];
        let click = |column, row| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        app.on_mouse(click(53, 0));
        assert_eq!(app.filter, Filter::Agents);
        assert!(app.preview);
        app.on_mouse(click(12, 30));
        assert!(!app.preview);
        assert_eq!(
            key_of_hint("⌃A"),
            Some(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn escape_quits_and_question_mark_shows_the_help() {
        let mut app = app(Filter::All);
        press(&mut app, KeyCode::Char('?'));
        assert!(app.help);
        assert!(matches!(press(&mut app, KeyCode::Esc), Outcome::Continue));
        assert!(!app.help);
        assert!(matches!(press(&mut app, KeyCode::Esc), Outcome::Quit));
    }

    #[test]
    fn ctrl_w_deletes_the_last_word() {
        let mut text = String::from("fix the log");
        delete_word(&mut text);
        assert_eq!(text, "fix the ");
        let mut text = String::from("fix the ");
        delete_word(&mut text);
        assert_eq!(text, "fix ");
    }
}
