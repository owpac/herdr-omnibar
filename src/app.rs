//! The popup: its state, its keys and its loop.
//!
//! The loop draws the popup, waits for a key, and reads the herdr snapshot again every 1.5 s. Enter
//! closes the popup, then goes to the selected row.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal};

use crate::filter::Filter;
use crate::herdr;
use crate::model::{self, Checkout, Model};
use crate::rows::{self, Item, Line};
use crate::ui;

/// The time between 2 reads of the herdr snapshot.
const REFRESH: Duration = Duration::from_millis(1500);

/// The state of the popup.
pub struct App {
    pub model: Model,
    pub filter: Filter,
    pub query: String,
    pub lines: Vec<Line>,
    /// The index in `lines` of the selected row.
    pub selected: Option<usize>,
    pub preview: bool,
    pub help: bool,
    /// The text of each pane that the preview has read, by pane id.
    previews: HashMap<String, String>,
}

/// What a key asks for.
#[derive(Debug)]
pub enum Outcome {
    Continue,
    Quit,
    Go(Item),
}

impl App {
    pub fn new(model: Model, filter: Filter) -> App {
        let mut app = App {
            model,
            filter,
            query: String::new(),
            lines: Vec::new(),
            selected: None,
            preview: true,
            help: false,
            previews: HashMap::new(),
        };
        app.rebuild(None);
        app
    }

    /// Build the lines again. The selection stays on the row with this key, if it is still there.
    fn rebuild(&mut self, keep: Option<String>) {
        self.lines = rows::build(&self.model, self.filter, &self.query);
        let rows = self.row_indices();
        self.selected = keep
            .and_then(|key| {
                rows.iter().copied().find(
                    |&index| matches!(&self.lines[index], Line::Row { item, .. } if item.key() == key),
                )
            })
            .or_else(|| rows.first().copied());
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
        change(&mut self.query);
        self.rebuild(None);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.help {
            self.help = false;
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1)) {
                return Outcome::Continue;
            }
        }
        match key.code {
            KeyCode::Esc => return Outcome::Quit,
            KeyCode::Char('c' | 'g') if ctrl => return Outcome::Quit,
            KeyCode::Enter => {
                return self
                    .selected_item()
                    .cloned()
                    .map(Outcome::Go)
                    .unwrap_or(Outcome::Continue)
            }
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Char('p' | 'k') if ctrl => self.move_by(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.move_by(1),
            KeyCode::PageUp => self.move_by(-8),
            KeyCode::PageDown => self.move_by(8),
            KeyCode::Tab => self.cycle_filter(1),
            KeyCode::BackTab => self.cycle_filter(-1),
            KeyCode::Char('o') if ctrl => self.preview = !self.preview,
            KeyCode::Char('u') if ctrl => self.edit(String::clear),
            KeyCode::Char('w') if ctrl => self.edit(delete_word),
            KeyCode::Backspace => self.edit(|query| {
                query.pop();
            }),
            KeyCode::F(1) => self.help = true,
            KeyCode::Char(character) if !ctrl && !alt => {
                let shortcut = match character {
                    '@' => Some(Filter::Agents),
                    '+' => Some(Filter::New),
                    '>' => Some(Filter::Actions),
                    _ => None,
                };
                if self.query.is_empty() && character == '?' {
                    self.help = true;
                } else if let (true, Some(filter)) = (self.query.is_empty(), shortcut) {
                    self.set_filter(filter);
                } else {
                    self.edit(|query| query.push(character));
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
            Item::Workspace(_) => None,
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

/// Run the popup on a filter.
pub fn run(filter: Filter) -> Result<(), String> {
    let mut cache: HashMap<PathBuf, Option<Checkout>> = HashMap::new();
    let model = model::load_cached(&mut cache).map_err(|error| error.to_string())?;
    let mut app = App::new(model, filter);
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
        Some(item) => go(&item).map_err(|error| error.to_string()),
        None => Ok(()),
    }
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    cache: &mut HashMap<PathBuf, Option<Checkout>>,
) -> Result<Option<Item>, String> {
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
                    Outcome::Quit => return Ok(None),
                    Outcome::Go(item) => return Ok(Some(item)),
                    Outcome::Continue => {}
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentRow, Status};

    fn app() -> App {
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
        App::new(model, Filter::All)
    }

    fn press(app: &mut App, code: KeyCode) -> Outcome {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn selected_name(app: &App) -> String {
        app.selected_item().map(Item::name).unwrap_or_default()
    }

    #[test]
    fn the_first_row_is_selected_and_down_moves() {
        let mut app = app();
        assert_eq!(selected_name(&app), "fix the login");
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_name(&app), "omnibar design");
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(selected_name(&app), "fix the login");
    }

    #[test]
    fn typing_filters_and_enter_goes_to_the_selected_row() {
        let mut app = app();
        for character in "rel".chars() {
            press(&mut app, KeyCode::Char(character));
        }
        assert_eq!(selected_name(&app), "release notes");
        assert!(
            matches!(press(&mut app, KeyCode::Enter), Outcome::Go(Item::Agent(agent)) if agent.name == "release notes")
        );
    }

    #[test]
    fn tab_changes_the_filter_and_at_on_an_empty_query_shows_the_agents() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.filter, Filter::Agents);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.filter, Filter::All);
        press(&mut app, KeyCode::Char('@'));
        assert_eq!(app.filter, Filter::Agents);
        assert!(app.query.is_empty());
    }

    #[test]
    fn a_refresh_keeps_the_selected_row() {
        let mut app = app();
        press(&mut app, KeyCode::Down);
        let model = app.model.clone();
        app.update_model(model);
        assert_eq!(selected_name(&app), "omnibar design");
    }

    #[test]
    fn escape_quits_and_question_mark_shows_the_help() {
        let mut app = app();
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
