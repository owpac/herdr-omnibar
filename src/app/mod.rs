//! The popup: its state, and the queries that the drawing and the keys use.
//!
//! input.rs handles the keys, the mouse and Enter. run.rs starts the popup and runs its loop: the
//! loop draws the popup, waits for a key, and reads the herdr snapshot again every 1.5 s. Enter
//! closes the popup, then goes to the selected row, or runs the creation.
//!
//! The filter New is the creation, in 2 steps. The step "where" chooses a place: the text is a
//! filter. The step "what" chooses what to create there: the text is the name of the tab or of the
//! branch. The text is never both.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use ratatui::layout::Rect;

use crate::changes::Changes;
use crate::config::{Config, Preview};
use crate::create::{self, Choice, Plan};
use crate::filter::Filter;
use crate::herdr::{self, PluginAction};
use crate::model::Model;
use crate::places::{self, Place, Project};
use crate::pulls::{Lookup, Pulls};
use crate::rank::Ranker;
use crate::recent::Recent;
use crate::rows::{self, Item, Line};
use crate::times::{self, Times};

mod input;
mod run;

pub use run::run;

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
    /// The git project of the place, with its local branches and its default branch.
    pub project: Option<Project>,
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
            project: None,
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

/// A view of the preview of a worktree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    /// The diff stat since the default branch.
    Diff,
    /// The commits since the default branch.
    Log,
    /// The changes that are not committed.
    Changes,
    /// The pull request of the branch (feature pull_requests).
    Pull,
    /// One view with the commits and the diff stat (preview = "summary").
    Summary,
}

impl View {
    /// The title of the view in the preview.
    pub fn title(self) -> &'static str {
        match self {
            View::Diff => "DIFF",
            View::Log => "LOG",
            View::Changes => "CHANGES",
            View::Pull => "PR",
            View::Summary => "SUMMARY",
        }
    }
}

/// The git projects of the branch rows, read one time.
#[derive(Debug, Default)]
struct Projects {
    /// True after the read of the current project.
    current_read: bool,
    /// The project of the current workspace: the filters All and Worktrees. None outside a git
    /// project.
    current: Option<Project>,
    /// The other projects with an open workspace, with their worktrees only: the filter
    /// Worktrees. None before the read.
    others: Option<Vec<Project>>,
}

/// The question of the action line, before a close or a removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Close a tab: its id and its name.
    CloseTab { tab_id: String, name: String },
    /// Close a pane (filter Panes): its id and its name.
    ClosePane { pane_id: String, name: String },
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
    /// The filters of the chips, of Tab and of the shortcuts, from config.toml. The action of a
    /// filter outside this list still opens it: see `shown_filters`.
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
    /// The index of the current view of the preview of a worktree, in `views()`.
    pub view: usize,
    /// The text of each git view of each branch, by main checkout, read one time.
    view_texts: HashMap<(PathBuf, String, View), String>,
    /// The git projects of the branch rows.
    projects: Projects,
    /// The text of each pane that the preview has read, by pane id.
    previews: HashMap<String, String>,
    /// The running times of the panes (feature running_time).
    pub times: Option<Times>,
    /// The git state of the worktrees of the branch rows.
    pub changes: Changes,
    /// The pull requests of the projects (feature pull_requests).
    pub pulls: Pulls,
    /// Show the logos of the agents: the feature agent_logos, and the font is installed.
    pub logos: bool,
    /// The focused pane of the current workspace, when the popup opens: a split goes next to it.
    pub focused_pane: Option<String>,
    /// The order of the lists: the ranks of the rows (key `order` of the config).
    ranker: Ranker,
    /// The times of use of the tabs, and the tab of the popup (criterion visit).
    recent: Recent,
    current_tab: Option<String>,
    /// The last agent of each folder (agents.json), read one time when the popup opens. Empty in
    /// the tests: they do not read the state of the machine.
    remembered: HashMap<String, String>,
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
    /// Fork the agent of this pane with herdr-fork, after the popup closes.
    Fork {
        tab_id: String,
        pane_id: String,
    },
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
        let filters = rows::filters(&config.filters);
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
            view_texts: HashMap::new(),
            projects: Projects::default(),
            previews: HashMap::new(),
            times: None,
            changes: Changes::default(),
            pulls: Pulls::default(),
            logos: false,
            remembered: HashMap::new(),
            recent: Recent::new(),
            current_tab: None,
            ranker: Ranker::default(),
            focused_pane: None,
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

    /// The project of the new branches: the project of the step "what" in the creation, else the
    /// current project.
    fn target_project(&self) -> Option<&Project> {
        if self.filter == Filter::New {
            self.create.project.as_ref()
        } else {
            self.projects.current.as_ref()
        }
    }

    /// The project of the branch rows with this main checkout.
    fn project_of(&self, main: &Path) -> Option<&Project> {
        if self.filter == Filter::New {
            return self
                .create
                .project
                .as_ref()
                .filter(|project| project.main == main);
        }
        self.projects
            .current
            .iter()
            .chain(self.projects.others.iter().flatten())
            .find(|project| project.main == main)
    }

    /// The age of the last commit of a branch of a project: now, 12m, 2h04m, 3d.
    pub fn branch_age(&self, main: &Path, name: &str) -> Option<String> {
        let project = self.project_of(main)?;
        let time = project
            .branches
            .iter()
            .find(|branch| branch.name == name)?
            .time?;
        Some(times::ago(time))
    }

    /// The default branch of the project of the new branches, without "origin/": "main".
    pub fn base_branch(&self) -> Option<&str> {
        let base = self.target_project()?.base.as_deref()?;
        Some(base.strip_prefix("origin/").unwrap_or(base))
    }

    /// The place of the current workspace.
    pub fn current_place(&self) -> Option<&Place> {
        self.open_place(self.current.as_deref()?)
    }

    /// The place of the rows that create or open: the place of the step "what" in the creation,
    /// else the current place. Outside the creation, the place of an old step "what" does not
    /// count: a shortcut can leave the creation.
    pub fn target_place(&self) -> Option<&Place> {
        self.create
            .place
            .as_ref()
            .filter(|_| self.filter == Filter::New)
            .or_else(|| self.current_place())
    }

    /// The place of a choice: for a branch outside the creation, an open workspace of its project.
    /// The current place comes first if it is in this project. Else the target place.
    pub fn place_of_choice(&self, choice: &Choice) -> Option<&Place> {
        match choice {
            Choice::Existing { main, .. } if self.filter != Filter::New => {
                let in_project = |place: &&Place| place.main.as_deref() == Some(main.as_path());
                self.current_place()
                    .filter(in_project)
                    .or_else(|| self.open.iter().find(|place| place.folder == *main))
                    .or_else(|| self.open.iter().find(in_project))
            }
            _ => self.target_place(),
        }
    }

    /// The open workspace with this id, as a place.
    pub fn open_place(&self, workspace_id: &str) -> Option<&Place> {
        self.open
            .iter()
            .find(|place| place.workspace_id.as_deref() == Some(workspace_id))
    }

    /// Build the lines again. The selection stays on the row with this key, if it is still there.
    fn rebuild(&mut self, keep: Option<String>) {
        if matches!(self.filter, Filter::All | Filter::Worktrees) {
            self.load_current_project();
        }
        if self.filter == Filter::Worktrees {
            self.load_other_projects();
        }
        if self.filter == Filter::Actions && self.actions.is_none() {
            self.actions = Some(herdr::plugin_actions().unwrap_or_default());
        }
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
                    create::choices(
                        place,
                        &self.create.name,
                        self.create
                            .project
                            .as_ref()
                            .map(|project| project.branches.as_slice())
                            .unwrap_or_default(),
                        &self.open,
                        self.split_pane(place),
                    ),
                ),
                None => Vec::new(),
            },
            (Filter::Worktrees, _) => rows::worktree_lines(
                self.projects.current.as_ref(),
                &self.ranker.projects(
                    self.projects.others.as_deref().unwrap_or_default(),
                    &self.open,
                ),
                &self.open,
                &self.query,
            ),
            _ => {
                let sources = rows::Sources {
                    model: &self.model,
                    others: &self.others,
                    actions: self.actions.as_deref().unwrap_or_default(),
                    open: &self.open,
                    project: self
                        .projects
                        .current
                        .as_ref()
                        .filter(|_| self.filter == Filter::All),
                    in_sidebar_order: self.config.has("move_workspaces"),
                    attention: crate::rank::has(&self.config.order, "attention"),
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

    /// Read the branches of the current project, one time, and ask for the git state of their
    /// worktrees and for the pull requests.
    fn load_current_project(&mut self) {
        if self.projects.current_read {
            return;
        }
        self.projects.current_read = true;
        let project = self.current_place().and_then(|place| {
            let main = place.main.as_deref()?;
            Some(Project::read(&place.project, main))
        });
        if let Some(project) = &project {
            self.ask_git_states(project);
        }
        self.projects.current = project;
    }

    /// Read the worktrees of the other projects with an open workspace, one time, in the order of
    /// the workspaces. Several workspaces of a project give one project.
    fn load_other_projects(&mut self) {
        if self.projects.others.is_some() {
            return;
        }
        let current = self
            .projects
            .current
            .as_ref()
            .map(|project| project.main.as_path());
        let mut mains: Vec<(&str, &Path)> = Vec::new();
        for place in &self.open {
            let Some(main) = place.main.as_deref() else {
                continue;
            };
            if Some(main) != current && mains.iter().all(|(_, seen)| *seen != main) {
                mains.push((&place.project, main));
            }
        }
        // Each project runs its git commands in its own thread: the popup waits for the slowest
        // project only.
        let others: Vec<Project> = thread::scope(|scope| {
            let reads: Vec<_> = mains
                .iter()
                .map(|(name, main)| scope.spawn(move || Project::read_worktrees(name, main)))
                .collect();
            reads
                .into_iter()
                .filter_map(|read| read.join().ok())
                .collect()
        });
        for project in &others {
            self.ask_git_states(project);
        }
        self.projects.others = Some(others);
    }

    /// Ask for the git state of the worktrees of a project, and for its pull requests.
    fn ask_git_states(&mut self, project: &Project) {
        self.changes.ask(project.worktrees());
        if self.config.has("pull_requests") {
            self.pulls.ask(&project.main);
        }
    }

    /// The pull request of a branch of a project (feature pull_requests).
    pub fn pull(&self, main: &Path, branch: &str) -> Option<Lookup> {
        if !self.config.has("pull_requests") {
            return None;
        }
        Some(self.pulls.of(main, branch))
    }

    /// The views of the preview of a worktree, in the order of ⌃← and ⌃→: diff, log, changes, and
    /// the pull request with the feature pull_requests. With preview = "summary", one view.
    pub fn views(&self) -> &'static [View] {
        match (self.config.preview, self.config.has("pull_requests")) {
            (Preview::Summary, _) => &[View::Summary],
            (Preview::Full, true) => &[View::Diff, View::Log, View::Changes, View::Pull],
            (Preview::Full, false) => &[View::Diff, View::Log, View::Changes],
        }
    }

    /// The current view of the preview of a worktree.
    pub fn current_view(&self) -> View {
        let views = self.views();
        views[self.view % views.len()]
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

    /// The pane that a split of this place divides: the focused pane, if the place is the current
    /// workspace.
    fn split_pane(&self, place: &Place) -> Option<&str> {
        let current = place.workspace_id.is_some() && place.workspace_id == self.current;
        self.focused_pane.as_deref().filter(|_| current)
    }

    /// Take the times of use of the tabs, and the tab of the popup: the rows take their order of
    /// use (criterion visit).
    /// The selection goes to the first row.
    pub fn use_recent(&mut self, recent: Recent, current_tab: Option<String>) {
        self.recent = recent;
        self.current_tab = current_tab;
        let mut model = std::mem::take(&mut self.model);
        self.order(&mut model);
        self.open = places::workspaces(&model);
        self.model = model;
        self.rebuild(None);
    }

    /// Sort the rows of a model with the key `order` of the config, then keep the ranks for the
    /// groups of the filter Worktrees.
    fn order(&mut self, model: &mut Model) {
        let ranker = Ranker::new(
            model,
            &self.config.order,
            &self.recent,
            self.current_tab.as_deref(),
            self.current.as_deref(),
        );
        ranker.sort(model);
        self.ranker = ranker;
    }

    /// Take the tab of the popup: the agent of this tab goes at the end of its part of the list.
    pub fn use_current_tab(&mut self, current_tab: Option<String>) {
        self.current_tab = current_tab;
        let mut model = std::mem::take(&mut self.model);
        self.order(&mut model);
        self.model = model;
        self.rebuild(None);
    }

    /// Take a new model: the snapshot changed.
    pub fn update_model(&mut self, mut model: Model) {
        let key = self.selected_item().map(Item::key);
        self.order(&mut model);
        let open = places::workspaces(&model);
        // A workspace of another project opened or closed: read the other projects again.
        let mains = |places: &[Place]| -> Vec<Option<PathBuf>> {
            places.iter().map(|place| place.main.clone()).collect()
        };
        if mains(&open) != mains(&self.open) {
            self.projects.others = None;
        }
        self.open = open;
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

    /// The filters of the chips and of Tab: the filters of the config, and the current filter at
    /// the end if the config hides it. An action can open a hidden filter.
    pub fn shown_filters(&self) -> Vec<Filter> {
        let mut filters = self.filters.clone();
        if !filters.contains(&self.filter) {
            filters.push(self.filter);
        }
        filters
    }

    fn cycle_filter(&mut self, delta: i32) {
        let filters = self.shown_filters();
        let count = filters.len() as i32;
        let at = filters
            .iter()
            .position(|filter| *filter == self.filter)
            .unwrap_or(0) as i32;
        self.set_filter(filters[(at + delta).rem_euclid(count) as usize]);
    }

    fn edit(&mut self, change: impl FnOnce(&mut String)) {
        change(self.text_mut());
        // A new name of a tab does not filter the list.
        if self.rename.is_none() {
            self.rebuild(None);
        }
    }

    /// The agent of the creation: the agent that the user chose, else the last agent of the
    /// place, else the agent of its tabs, else the first agent on the PATH. The place of a branch
    /// row is an open workspace of its project.
    pub fn agent(&self) -> String {
        if let Some(agent) = &self.create.agent {
            return agent.clone();
        }
        let place = match self.selected_item() {
            Some(Item::Choice(choice)) => self.place_of_choice(choice),
            _ => self.target_place(),
        };
        self.agent_for(place)
    }

    /// The agent of a place: its last agent, else the agent of its tabs, else the first agent on
    /// the PATH.
    fn agent_for(&self, place: Option<&Place>) -> String {
        let remembered = place.and_then(|place| {
            self.remembered
                .get(&place.folder.display().to_string())
                .cloned()
        });
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
        self.create.project = place
            .main
            .as_deref()
            .map(|main| Project::read(&place.project, main));
        if let Some(project) = &self.create.project {
            self.changes.ask(project.worktrees());
        }
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

    /// The text of a git view of the preview of a branch of a project: the diff since the default
    /// branch, the log, the changes not committed, or the summary of all. The view of the pull
    /// request comes from gh: it has no text here.
    pub fn view_text(
        &mut self,
        main: &Path,
        branch: &str,
        worktree: Option<&PathBuf>,
        view: View,
    ) -> String {
        if view == View::Pull {
            return String::new();
        }
        let key = (main.to_path_buf(), branch.to_string(), view);
        if let Some(text) = self.view_texts.get(&key) {
            return text.clone();
        }
        // The current project has its default branch. The other projects read it now.
        let base = self
            .project_of(main)
            .and_then(|project| project.base.clone())
            .unwrap_or_else(|| places::base_branch(main));
        let range = format!("{base}..{branch}");
        let text = match view {
            View::Diff => places::git(main, &["diff", "--stat", &format!("{base}...{branch}")]),
            View::Log => places::git(main, &["log", "--oneline", "-n", "40", &range]),
            View::Changes => match worktree {
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
            // The return at the start of the function: the view PR has no git text.
            View::Pull => String::new(),
            View::Summary => {
                let count = places::git(main, &["rev-list", "--count", &range]);
                let stat = places::git(
                    main,
                    &["diff", "--shortstat", &format!("{base}...{branch}")],
                );
                let log = places::git(main, &["log", "--oneline", "-n", "8", &range]);
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
        self.view_texts.insert(key, text.clone());
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
    fn place_of_selected(&self) -> Option<&Place> {
        let workspace_id = match self.selected_item()? {
            Item::Agent(agent) => &agent.workspace_id,
            Item::Tab(tab) => &tab.workspace_id,
            Item::Workspace(space) => &space.workspace.workspace_id,
            Item::Choice(Choice::Existing {
                workspace_id: Some(id),
                ..
            }) => id,
            Item::Place(place) => place.workspace_id.as_ref()?,
            Item::Pane(pane) => &pane.workspace_id,
            _ => return None,
        };
        self.open_place(workspace_id)
    }

    /// The label of the workspace where ctrl+Enter adds a tab.
    pub fn new_tab_place(&self) -> Option<String> {
        self.place_of_selected().map(|place| place.label.clone())
    }

    /// True if ctrl+Enter adds a tab next to the selected row: the row has an open workspace.
    pub fn adds_tab_here(&self) -> bool {
        self.filter != Filter::New && self.place_of_selected().is_some()
    }

    /// The tab of the selected agent or tab: its id and its name.
    fn selected_tab(&self) -> Option<(String, String)> {
        match self.selected_item()? {
            Item::Agent(agent) => Some((agent.tab_id.clone(), agent.name.clone())),
            Item::Tab(tab) => Some((tab.tab_id.clone(), tab.name.clone())),
            _ => None,
        }
    }

    /// True if ctrl+Backspace on the selected row removes a worktree: a branch with a worktree
    /// that is not the main checkout of its project.
    pub fn can_remove_worktree(&self) -> bool {
        match self.selected_item() {
            Some(Item::Choice(Choice::Existing {
                worktree: Some(folder),
                main,
                ..
            })) => folder != main,
            _ => false,
        }
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

#[cfg(test)]
mod tests {
    use super::input::{delete_word, key_of_hint};
    use super::*;
    use crate::model::{AgentRow, Status};
    use crate::places::Kind;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

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
                seq: None,
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

    /// Give these branches to the current project atlas. No other project.
    fn set_branches(app: &mut App, branches: &[(&str, Option<&str>)]) {
        app.projects = Projects {
            current_read: true,
            current: Some(Project::sample("atlas", branches)),
            others: Some(Vec::new()),
        };
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
    fn the_agent_of_a_place_comes_from_the_memory_of_the_popup() {
        let mut app = app(Filter::New);
        assert_eq!(app.agent(), "claude");
        app.remembered
            .insert("/work/atlas".to_string(), "codex".to_string());
        assert_eq!(app.agent(), "codex");
        // An agent that is not on the PATH gives the first agent.
        app.remembered
            .insert("/work/atlas".to_string(), "aider".to_string());
        assert_eq!(app.agent(), "claude");
    }

    #[test]
    fn the_place_of_an_old_step_what_does_not_count_after_a_shortcut() {
        let mut app = app(Filter::New);
        app.remembered
            .insert("/work/pac".to_string(), "codex".to_string());
        type_text(&mut app, "pac");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.create.step, Step::What);
        assert_eq!(app.agent(), "codex");
        // @ leaves the creation: the current place (atlas) counts again.
        press(&mut app, KeyCode::Char('@'));
        assert_eq!(app.filter, Filter::Agents);
        assert_eq!(
            app.target_place().map(|place| place.label.as_str()),
            Some("atlas")
        );
        assert_eq!(app.agent(), "claude");
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
        set_branches(&mut app, &[("fix-login", None)]);
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
        press(&mut app, KeyCode::Char('&'));
        assert_eq!(app.filter, Filter::Workspaces);
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(app.filter, Filter::Worktrees);
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(app.filter, Filter::All);
        // Without the filter panes in the config, % is a character of the text.
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
    fn the_action_of_a_hidden_filter_opens_it_until_tab_leaves_it() {
        // The action herdr-omnibar.new opens the filter New, but the config hides it.
        let mut app = app(Filter::New);
        app.filters = vec![Filter::All, Filter::Agents];
        assert_eq!(
            app.shown_filters(),
            [Filter::All, Filter::Agents, Filter::New]
        );
        // Its own shortcut goes back to All, like the shortcut of a filter of the config.
        press(&mut app, KeyCode::Char('+'));
        assert_eq!((app.filter, app.query.as_str()), (Filter::All, ""));
        assert_eq!(app.shown_filters(), [Filter::All, Filter::Agents]);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.filter, Filter::Agents);
        // The shortcut of a hidden filter is a character of the text.
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('+'));
        assert_eq!((app.filter, app.query.as_str()), (Filter::All, "+"));
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
    fn alt_up_and_down_move_a_workspace_only_with_the_feature_in_the_filter_workspaces() {
        let mut app = app(Filter::Workspaces);
        app.model.spaces = vec![crate::model::Space {
            workspace: crate::order::tests::workspace("w1", "atlas", None),
            folder: None,
            checkout: None,
        }];
        app.rebuild(None);
        assert!(!app.moves_workspace());
        app.config.features.push("move_workspaces".to_string());
        assert!(app.moves_workspace());
        app.filter = Filter::Agents;
        assert!(!app.moves_workspace());
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
        set_branches(
            &mut app,
            &[
                ("main", Some("/work/atlas")),
                ("fix-login", Some("/work/atlas.fix-login")),
            ],
        );
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
    fn the_agents_go_by_activity_and_the_agent_of_the_current_tab_goes_last() {
        let mut app = app(Filter::Agents);
        for (agent, seq) in app.model.agents.iter_mut().zip([5, 9, 7]) {
            agent.seq = Some(seq);
        }
        app.use_current_tab(Some("t-omnibar design".to_string()));
        let names: Vec<&str> = app
            .model
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        // release notes 7, fix the login 5, then the current tab (omnibar design, 9).
        assert_eq!(names, ["release notes", "fix the login", "omnibar design"]);
        assert_eq!(selected_name(&app), "release notes");
    }

    #[test]
    fn ctrl_backspace_on_a_pane_asks_to_close_the_pane_only() {
        let mut app = app(Filter::All);
        app.model.panes = vec![crate::model::PaneRow {
            pane_id: "w1:p2".into(),
            tab_id: "w1:t1".into(),
            workspace_id: "w1".into(),
            name: "cargo watch".into(),
            tab: "build".into(),
            place: "atlas".into(),
            agent: None,
            status: Status::Unknown,
        }];
        app.filters.push(Filter::Panes);
        app.set_filter(Filter::Panes);
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert_eq!(
            app.confirm,
            Some(Confirm::ClosePane {
                pane_id: "w1:p2".into(),
                name: "cargo watch".into()
            })
        );
    }

    #[test]
    fn ctrl_f_forks_only_claude_code_and_codex() {
        let mut app = app(Filter::Agents);
        app.model.agents[0].kind = "opencode".into();
        app.rebuild(None);
        let outcome = app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(matches!(outcome, Outcome::Continue));
        assert!(app
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("not opencode"));
    }

    #[test]
    fn a_branch_of_another_project_opens_and_keeps_its_own_project() {
        let mut app = app(Filter::Worktrees);
        app.open[0].main = Some("/work/atlas".into());
        app.open[1].main = Some("/work/pac".into());
        set_branches(&mut app, &[("main", Some("/work/atlas"))]);
        app.projects.others = Some(vec![Project::sample(
            "pac",
            &[("main", Some("/work/pac")), ("fix", Some("/work/pac.fix"))],
        )]);
        app.rebuild(None);
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_name(&app), "main");
        // The main checkout of pac stays, also from the current project atlas.
        assert!(!app.can_remove_worktree());
        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert!(app.confirm.is_none());
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_name(&app), "fix");
        assert!(app.can_remove_worktree());
        // The worktree has no workspace: wt opens it in the project pac.
        match press(&mut app, KeyCode::Enter) {
            Outcome::Create(plan) => {
                assert_eq!(
                    plan.action,
                    create::Action::Worktree {
                        main: "/work/pac".into(),
                        branch: "fix".into(),
                        create: false,
                    }
                );
                assert_eq!(plan.folder, PathBuf::from("/work/pac"));
            }
            outcome => panic!("unexpected outcome {outcome:?}"),
        }
    }

    #[test]
    fn ctrl_backspace_refuses_a_worktree_where_an_agent_works() {
        let mut app = app(Filter::Worktrees);
        app.open[0].main = Some("/work/atlas".into());
        app.model.agents[0].status = Status::Working;
        app.model.agents[0].cwd = Some("/work/atlas.fix-login/src".into());
        set_branches(&mut app, &[("fix-login", Some("/work/atlas.fix-login"))]);
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
        set_branches(&mut app, &[("main", Some("/work/atlas"))]);
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
    fn ctrl_left_and_right_go_round_the_views_of_the_config() {
        let mut app = app(Filter::All);
        let ctrl = |code| KeyEvent::new(code, KeyModifiers::CONTROL);
        assert_eq!(app.views(), [View::Diff, View::Log, View::Changes]);
        app.on_key(ctrl(KeyCode::Left));
        assert_eq!(app.current_view(), View::Changes);
        app.on_key(ctrl(KeyCode::Right));
        assert_eq!(app.current_view(), View::Diff);
        app.config.features.push("pull_requests".into());
        app.on_key(ctrl(KeyCode::Left));
        assert_eq!(app.current_view(), View::Pull);
        app.config.preview = Preview::Summary;
        assert_eq!(app.views(), [View::Summary]);
        assert_eq!(app.current_view(), View::Summary);
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
