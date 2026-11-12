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
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal};

use crate::config::{self, Config};
use crate::context::Context;
use crate::create::{self, Choice, Plan};
use crate::filter::Filter;
use crate::herdr;
use crate::model::{self, Checkout, Model};
use crate::places::{self, Branch, Place};
use crate::rows::{self, Item, Line};
use crate::ui;

/// The time between 2 reads of the herdr snapshot.
const REFRESH: Duration = Duration::from_millis(1500);

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
        }
    }
}

/// The state of the popup.
pub struct App {
    pub model: Model,
    pub filter: Filter,
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
    /// The branches of the current project, for the filter Worktrees.
    current_branches: Option<Vec<Branch>>,
    /// The text of each pane that the preview has read, by pane id.
    previews: HashMap<String, String>,
}

/// What a key asks for.
#[derive(Debug)]
pub enum Outcome {
    Continue,
    Quit,
    Go(Item),
    Create(Plan),
}

impl App {
    pub fn new(
        model: Model,
        filter: Filter,
        others: Vec<Place>,
        current: Option<String>,
        agents: Vec<String>,
    ) -> App {
        let open = places::workspaces(&model);
        let mut app = App {
            model,
            filter,
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
            current_branches: None,
            previews: HashMap::new(),
        };
        app.rebuild(None);
        app
    }

    /// The text of the search line: the query, or the text of the step of the creation.
    pub fn text(&self) -> &str {
        match (self.filter, self.create.step) {
            (Filter::New, Step::Where) => &self.create.filter,
            (Filter::New, Step::What) => &self.create.name,
            _ => &self.query,
        }
    }

    fn text_mut(&mut self) -> &mut String {
        match (self.filter, self.create.step) {
            (Filter::New, Step::Where) => &mut self.create.filter,
            (Filter::New, Step::What) => &mut self.create.name,
            _ => &mut self.query,
        }
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
            _ => rows::build(&self.model, &self.others, self.filter, &self.query),
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

    /// Take a new model: the snapshot changed.
    pub fn update_model(&mut self, model: Model) {
        let key = self.selected_item().map(Item::key);
        self.open = places::workspaces(&model);
        self.model = model;
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
        let count = rows::FILTERS.len() as i32;
        let at = rows::FILTERS
            .iter()
            .position(|filter| *filter == self.filter)
            .unwrap_or(0) as i32;
        self.set_filter(rows::FILTERS[(at + delta).rem_euclid(count) as usize]);
    }

    fn edit(&mut self, change: impl FnOnce(&mut String)) {
        change(self.text_mut());
        self.rebuild(None);
    }

    /// The agent of the creation: the agent that the user chose, else the last agent of the
    /// place, else the agent of its tabs, else the first agent on the PATH.
    pub fn agent(&self) -> String {
        if let Some(agent) = &self.create.agent {
            return agent.clone();
        }
        let place = self.create.place.as_ref().or_else(|| self.current_place());
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
            (_, item) => Outcome::Go(item),
        }
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
        match key.code {
            KeyCode::Esc if what => self.back_to_where(),
            KeyCode::Esc => return Outcome::Quit,
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
                let shortcut = match character {
                    '@' => Some(Filter::Agents),
                    '+' => Some(Filter::New),
                    '>' => Some(Filter::Actions),
                    _ => None,
                };
                let empty = self.text().is_empty();
                if empty && character == '?' {
                    self.help = true;
                } else if let (true, false, Some(filter)) = (empty, creating, shortcut) {
                    self.set_filter(filter);
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
            _ => None,
        }?;
        let text = self.previews.entry(pane.clone()).or_insert_with(|| {
            herdr::read_pane(&pane, lines).unwrap_or_else(|error| error.to_string())
        });
        Some(text.clone())
    }
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
    let config: Config = config::current();
    let mut cache: HashMap<PathBuf, Option<Checkout>> = HashMap::new();
    let model = model::load_cached(&mut cache).map_err(|error| error.to_string())?;
    let open = places::workspaces(&model);
    let others = places::others(&config.roots, &open);
    let current = context
        .workspace_id
        .clone()
        .or(model.focused_workspace_id.clone());
    let mut app = App::new(model, filter, others, current, create::installed_agents());
    let mut terminal = ratatui::init();
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(crate::keys::flags())
        );
    }
    let outcome = event_loop(&mut terminal, &mut app, &mut cache);
    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    match outcome? {
        Outcome::Go(item) => go(&item).map_err(|error| error.to_string()),
        Outcome::Create(plan) => create::execute(&plan),
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
        let timeout = REFRESH.saturating_sub(last_read.elapsed());
        if event::poll(timeout).map_err(|error| error.to_string())? {
            if let Event::Key(key) = event::read().map_err(|error| error.to_string())? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                match app.on_key(key) {
                    Outcome::Continue => {}
                    outcome => return Ok(outcome),
                }
            }
        } else {
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
        Item::Place(_) | Item::Choice(_) => Ok(()),
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
