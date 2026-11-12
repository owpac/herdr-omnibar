//! The life of the popup: its start, its loop, and what it does after it closes.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal};

use crate::config::{self, Config};
use crate::context::Context;
use crate::create;
use crate::filter::Filter;
use crate::herdr;
use crate::model::{self, Checkout};
use crate::places;
use crate::recent::{self, Recent};
use crate::rows::Item;
use crate::times::Times;
use crate::ui;

use super::{App, Outcome};

/// The time between 2 reads of the herdr snapshot.
const REFRESH: Duration = Duration::from_millis(1500);
/// The time between 2 drawings without a key: the threads of the running times and of the git
/// states give their results in the background.
const REDRAW: Duration = Duration::from_millis(250);

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
    let mouse = config.has("mouse");
    // The order of use: the tab of the popup gets the time now, then the rows of the model take
    // the order of use (feature recent_first).
    let state_dir = config::state_dir();
    let recent_first = config.has("recent_first");
    if let (true, Some(tab)) = (recent_first, context.tab_id.as_deref()) {
        recent::touch(&state_dir, tab);
    }
    let recent = if recent_first {
        recent::load(&state_dir)
    } else {
        Recent::new()
    };
    let mut model = model;
    if recent_first {
        recent::order(&mut model, &recent, context.tab_id.as_deref());
    }
    let mut app = App::new(
        model,
        filter,
        others,
        current,
        create::installed_agents(),
        config,
    );
    app.recent = recent;
    app.current_tab = context.tab_id.clone();
    if app.config.has("running_time") {
        app.times = Some(Times::start());
        app.ask_times();
    }
    app.logos = app.config.has("agent_logos") && crate::icons::font_installed();
    app.remembered = create::remembered_agents();
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
