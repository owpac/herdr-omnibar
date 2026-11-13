//! The action line: what Enter does, and where.
//!
//! Before the action of the selected row, the line shows a question (close or remove), the rename
//! of a tab, or the message of the last command.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, Confirm};
use crate::create::{self, Choice};
use crate::places::{Kind, Place};
use crate::rows::Item;

use super::{creating, style::*, text::*};

/// What Enter does on a row: a verb, the workspace where it goes, and the agent field of a
/// creation. A warning (a wrong branch name) shows in red.
struct Action {
    verb: String,
    place: Option<Span<'static>>,
    agent_field: bool,
    warning: bool,
}

impl Action {
    /// An action that goes to a place.
    fn go(verb: String, place: Span<'static>) -> Action {
        Action {
            verb,
            place: Some(place),
            agent_field: false,
            warning: false,
        }
    }

    /// An action without a place.
    fn bare(verb: impl Into<String>) -> Action {
        Action {
            verb: verb.into(),
            place: None,
            agent_field: false,
            warning: false,
        }
    }

    /// An action that creates in a place, with the agent field.
    fn create(verb: String, place: Span<'static>) -> Action {
        Action {
            verb,
            place: Some(place),
            agent_field: true,
            warning: false,
        }
    }

    /// A wrong text: the line tells it in red.
    fn warning(verb: String) -> Action {
        Action {
            verb,
            place: None,
            agent_field: false,
            warning: true,
        }
    }
}

pub(super) fn draw_action(frame: &mut Frame, area: Rect, app: &App) {
    let line = match (&app.confirm, &app.rename, &app.message) {
        (Some(confirm), _, _) => arrow_line(red_color(), question(confirm), red_color()),
        (None, Some(rename), _) => arrow_line(
            accent_color(),
            format!(
                "rename the {} “{}” to “{}”",
                rename.target.noun(),
                rename.old,
                rename.text.trim()
            ),
            Color::Reset,
        ),
        (None, None, Some(message)) => arrow_line(dim_color(), message.clone(), muted_color()),
        (None, None, None) => match app.selected_item() {
            Some(item) => action_line(app, item, area.width as usize),
            None => Line::from(Span::styled(
                "→ nothing selected",
                Style::new().fg(dim_color()),
            )),
        },
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// A line with an arrow and a text.
fn arrow_line(arrow: Color, text: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("→ ", Style::new().fg(arrow).add_modifier(Modifier::BOLD)),
        Span::styled(text, Style::new().fg(color)),
    ])
}

/// The question before a close or a removal.
fn question(confirm: &Confirm) -> String {
    match confirm {
        Confirm::CloseTab { name, .. } => format!("close the tab {name}? ↵ yes · esc no"),
        Confirm::ClosePane { name, .. } => format!("close the pane {name}? ↵ yes · esc no"),
        Confirm::RemoveWorktree { branch, agents, .. } => {
            let agents = match agents {
                0 => String::new(),
                1 => " Its agent closes.".to_string(),
                count => format!(" Its {count} agents close."),
            };
            // The notification of the result tells if the action restore of herdr-worktrunk can
            // bring the worktree back: wt alone keeps no record.
            format!("remove the worktree {branch}?{agents} ↵ yes · esc no")
        }
    }
}

/// The line of the action of a row: the verb, its place, and the agent field on the right.
fn action_line(app: &App, item: &Item, width: usize) -> Line<'static> {
    let action = action_of(app, item);
    let color = if action.warning {
        red_color()
    } else {
        accent_color()
    };
    let mut spans = vec![
        Span::styled("→ ", Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(
            action.verb,
            Style::new().fg(if action.warning {
                red_color()
            } else {
                Color::Reset
            }),
        ),
    ];
    if let Some(place) = action.place {
        spans.push(Span::styled("  in ", Style::new().fg(dim_color())));
        spans.push(place);
    }
    if action.agent_field {
        let field = agent_field(app);
        let used = spans_width(&spans) + spans_width(&field);
        spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
        spans.extend(field);
    }
    Line::from(round(spans, app))
}

/// The agent of a creation, with its key: ⇥ in the creation, ⌃A on a row that opens a worktree.
fn agent_field(app: &App) -> Vec<Span<'static>> {
    let agent = app.agent();
    let key = if creating(app) { " ⇥" } else { " ⌃A" };
    let mut field = vec![
        Span::styled("agent ", Style::new().fg(dim_color())),
        Span::styled(
            format!(" {agent} "),
            Style::new().fg(agent_color(&agent)).bg(pill_color()),
        ),
        Span::styled(key, Style::new().fg(match_color())),
    ];
    if app.create.no_prompts && agent != "shell" {
        field.push(Span::styled("  prompts off", Style::new().fg(red_color())));
    }
    field
}

/// What Enter does on a row.
fn action_of(app: &App, item: &Item) -> Action {
    match item {
        Item::Agent(agent) => Action::go(
            format!("go to the agent {}", agent.name),
            pill_span(true, &agent.place),
        ),
        Item::Tab(tab) => Action::go(
            format!("go to the tab {}", tab.name),
            pill_span(true, &tab.place),
        ),
        Item::Pane(pane) => {
            let verb = if pane.name == pane.tab {
                format!("go to the pane {}", pane.name)
            } else {
                format!("go to the pane {} of the tab {}", pane.name, pane.tab)
            };
            Action::go(verb, pill_span(true, &pane.place))
        }
        Item::Workspace(space) => Action::go("go to".to_string(), pill_span(true, &space.label())),
        Item::Action(action) => Action::bare(format!("run the action {}", action.title)),
        Item::Place(place) if place.kind == Kind::Home && place.label.is_empty() => {
            Action::warning("type the name of the new workspace".into())
        }
        Item::Place(place) if place.kind == Kind::Home => Action::create(
            format!("new workspace “{}”", place.label),
            pill_span(false, "~"),
        ),
        Item::Place(place) => Action::go(
            "create".to_string(),
            pill_span(place.kind == Kind::Workspace, &place.label),
        ),
        Item::Create(name) => Action {
            place: app
                .target_place()
                .map(|place| pill_span(true, &place.label)),
            ..Action::bare(format!("create “{name}”: choose a tab or a worktree"))
        },
        Item::Choice(choice) => choice_action(app, choice),
    }
}

/// What Enter does on a choice: a creation, or a worktree to open.
fn choice_action(app: &App, choice: &Choice) -> Action {
    let place = app.place_of_choice(choice);
    let place_label = place.map(|place| place.label.as_str()).unwrap_or_default();
    let name = app.create.name.trim();
    // "new tab “name”", "new split (auto name)": the text of a new tab or of a new split.
    let new = |what: &str| {
        if name.is_empty() {
            format!("new {what} (auto name)")
        } else {
            format!("new {what} “{name}”")
        }
    };
    let tab = new("tab");
    match choice {
        Choice::NewWorktree { branch } if branch.is_empty() => {
            Action::warning("type the branch name of the new worktree".into())
        }
        Choice::NewWorktree { branch } if !create::is_branch_name(branch) => {
            Action::warning(format!("“{branch}” is not a branch name"))
        }
        Choice::NewWorktree { branch } => Action::create(
            format!("new worktree {branch}"),
            pill_span(false, &worktree_label(place, branch)),
        ),
        Choice::Existing {
            branch,
            workspace_id: Some(id),
            ..
        } => {
            let label = app
                .open_place(id)
                .map(|open| open.label.clone())
                .unwrap_or_else(|| worktree_label(place, branch));
            Action::go(
                format!("go to the worktree {branch}"),
                pill_span(true, &label),
            )
        }
        Choice::Existing { branch, .. } => Action::create(
            format!("open the worktree {branch}"),
            pill_span(false, &worktree_label(place, branch)),
        ),
        Choice::NewTab => Action::create(tab, pill_span(true, place_label)),
        Choice::NewSplit { .. } => Action::create(new("split"), pill_span(true, place_label)),
        Choice::NewWorkspace if place.is_some_and(|p| p.kind == Kind::Home) && name.is_empty() => {
            Action::warning("type the name of the new workspace".into())
        }
        Choice::NewWorkspace if place.is_some_and(Place::labels_new_workspace) => {
            Action::create(new("workspace"), pill_span(false, place_label))
        }
        Choice::NewWorkspace => Action::create(tab, pill_span(false, place_label)),
        Choice::AnotherPlace => Action::bare("choose another place"),
    }
}

/// The label of the workspace of a worktree: "project ▸ branch".
fn worktree_label(place: Option<&Place>, branch: &str) -> String {
    match place {
        Some(place) => format!("{} ▸ {branch}", place.project),
        None => branch.to_string(),
    }
}
