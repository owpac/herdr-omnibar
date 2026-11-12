//! The footer: the keys of the selected row.

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

/// Draw the footer. Return the area of each key, with its key, for the mouse.
pub(super) fn draw_footer(frame: &mut Frame, area: Rect, app: &App) -> Vec<(Rect, String)> {
    // ctrl+Enter names the workspace of its new tab.
    let new_tab = match app.new_tab_place() {
        Some(label) => format!("new tab in {}", fit(&label, 28)),
        None => "new tab here".to_string(),
    };
    let what = creating(app) && app.create.step == Step::What;
    let path = creating(app) && app.create.step == Step::Where && rows::is_path(&app.create.filter);
    let hints: Vec<(&str, String)> = if app.confirm.is_some() {
        vec![("↵", "yes".to_string()), ("esc", "no".to_string())]
    } else if app.rename.is_some() {
        vec![("↵", "save".to_string()), ("esc", "keep".to_string())]
    } else if what {
        let selected = app.selected_item();
        let primary = match selected {
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
            ("↵", primary.to_string()),
            ("⇥", "agent".to_string()),
            ("⌃Y", prompts.to_string()),
            ("esc", "place".to_string()),
        ]
    } else if creating(app) {
        vec![
            ("↵", "choose".to_string()),
            ("⇥", if path { "complete" } else { "filter" }.to_string()),
            ("⌃O", "preview".to_string()),
            ("esc", "close".to_string()),
        ]
    } else {
        match app.selected_item() {
            Some(Item::Agent(_) | Item::Tab(_)) => vec![
                ("↵", "go".to_string()),
                ("⌃↵", new_tab.clone()),
                ("⇥", "filter".to_string()),
                ("⌃⌫", "close tab".to_string()),
                ("esc", "close".to_string()),
            ],
            Some(Item::Workspace(_)) => vec![
                ("↵", "go".to_string()),
                ("⌃↵", new_tab.clone()),
                ("⇥", "filter".to_string()),
                ("⌃O", "preview".to_string()),
                ("esc", "close".to_string()),
            ],
            selected => {
                let primary = match selected {
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
                let mut hints = vec![("↵", primary.to_string())];
                if app.adds_tab_here() {
                    hints.push(("⌃↵", new_tab.clone()));
                }
                if app.opens_worktree() {
                    hints.push(("⌃A", "agent".to_string()));
                }
                hints.push(("⇥", "filter".to_string()));
                // A short footer: ⌃O shows only when the row has no ⌃↵, no ⌃A and no ⌃⌫.
                if app.can_remove_worktree() {
                    hints.push(("⌃⌫", "remove".to_string()));
                } else if hints.len() < 3 {
                    hints.push(("⌃O", "preview".to_string()));
                }
                hints.push(("esc", "close".to_string()));
                hints
            }
        }
    };
    let mut spans = Vec::new();
    let mut areas = Vec::new();
    let mut x = area.x;
    for (key, label) in hints {
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
