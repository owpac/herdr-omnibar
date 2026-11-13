//! The footer: the keys of the selected row, then "? help" on the right.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Step};
use crate::create::Choice;
use crate::rows::{self, Item};

use super::{creating, style::*, text::*};

/// A key of the footer, and its label.
type Hint = (&'static str, String);

/// Draw the footer. Return the area of each key, with its key, for the mouse.
pub(super) fn draw_footer(frame: &mut Frame, area: Rect, app: &App) -> Vec<(Rect, String)> {
    let mut spans = Vec::new();
    let mut areas = Vec::new();
    let mut x = area.x;
    for (key, label) in hints(app) {
        let text = format!(" {label}   ");
        let width = (key.width() + label.width() + 1) as u16;
        areas.push((Rect::new(x, area.y, width, 1), key.to_string()));
        x += (key.width() + text.width()) as u16;
        spans.push(Span::styled(
            key,
            Style::new().fg(match_color()).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(text, Style::new().fg(muted_color())));
    }
    let help = vec![
        Span::styled(
            "?",
            Style::new().fg(match_color()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" help", Style::new().fg(muted_color())),
    ];
    let used = spans_width(&spans) + spans_width(&help);
    spans.push(Span::raw(
        " ".repeat((area.width as usize).saturating_sub(used)),
    ));
    spans.extend(help);
    if used <= area.width as usize {
        areas.push((Rect::new(area.right() - 6, area.y, 6, 1), "?".to_string()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    areas
}

/// A key and its label.
fn hint(key: &'static str, label: &str) -> Hint {
    (key, label.to_string())
}

/// The keys of the footer, for the state of the popup.
fn hints(app: &App) -> Vec<Hint> {
    if app.confirm.is_some() {
        return vec![hint("↵", "yes"), hint("esc", "no")];
    }
    if app.rename.is_some() {
        return vec![hint("↵", "save"), hint("esc", "keep")];
    }
    if creating(app) {
        return creation_hints(app);
    }
    // ctrl+Enter names the workspace of its new tab.
    let new_tab = match app.new_tab_place() {
        Some(label) => format!("new tab in {}", fit(&label, 28)),
        None => "new tab here".to_string(),
    };
    match app.selected_item() {
        // An agent that herdr-fork can fork shows ⌃F in place of ⇥: the chips show the filters.
        Some(Item::Agent(agent)) if matches!(agent.kind.as_str(), "claude" | "codex") => vec![
            hint("↵", "go"),
            ("⌃↵", new_tab),
            hint("⌃F", "fork"),
            hint("⌃⌫", "close tab"),
            hint("esc", "close"),
        ],
        Some(Item::Agent(_) | Item::Tab(_)) => vec![
            hint("↵", "go"),
            ("⌃↵", new_tab),
            hint("⇥", "filter"),
            hint("⌃⌫", "close tab"),
            hint("esc", "close"),
        ],
        Some(Item::Pane(_)) => vec![
            hint("↵", "go"),
            ("⌃↵", new_tab),
            hint("⇥", "filter"),
            hint("⌃⌫", "close pane"),
            hint("esc", "close"),
        ],
        Some(Item::Workspace(_)) => vec![
            hint("↵", "go"),
            ("⌃↵", new_tab),
            hint("⇥", "filter"),
            hint("⌃O", "preview"),
            hint("esc", "close"),
        ],
        selected => {
            let primary = match selected {
                Some(Item::Place(place)) if place.kind == crate::places::Kind::Home => "create",
                Some(Item::Place(_)) => "create in",
                Some(Item::Action(_)) => "run",
                Some(Item::Create(_)) => "create…",
                Some(Item::Choice(Choice::Existing {
                    workspace_id: Some(_),
                    ..
                })) => "go",
                Some(Item::Choice(Choice::NewWorktree { .. })) => "create",
                Some(Item::Choice(_)) => "open",
                _ => "go",
            };
            let mut hints = vec![hint("↵", primary)];
            if app.adds_tab_here() {
                hints.push(("⌃↵", new_tab));
            }
            if app.opens_worktree() {
                hints.push(hint("⌃A", "agent"));
            }
            hints.push(hint("⇥", "filter"));
            // A short footer: ⌃O shows only when the row has no ⌃↵, no ⌃A and no ⌃⌫.
            if app.can_remove_worktree() {
                hints.push(hint("⌃⌫", "remove"));
            } else if hints.len() < 3 {
                hints.push(hint("⌃O", "preview"));
            }
            hints.push(hint("esc", "close"));
            hints
        }
    }
}

/// The keys of the creation: the step "where" or the step "what".
fn creation_hints(app: &App) -> Vec<Hint> {
    if app.create.step == Step::Where {
        let path = rows::is_path(&app.create.filter);
        return vec![
            hint("↵", "choose"),
            hint("⇥", if path { "complete" } else { "filter" }),
            hint("⌃O", "preview"),
            hint("esc", "close"),
        ];
    }
    let primary = match app.selected_item() {
        Some(Item::Choice(Choice::AnotherPlace)) => "place",
        Some(Item::Choice(Choice::Existing {
            workspace_id: Some(_),
            ..
        })) => "go",
        _ => "create",
    };
    let prompts = if app.create.no_prompts {
        "prompts on"
    } else {
        "prompts off"
    };
    vec![
        hint("↵", primary),
        hint("⇥", "agent"),
        hint("⌃Y", prompts),
        hint("esc", "place"),
    ]
}
