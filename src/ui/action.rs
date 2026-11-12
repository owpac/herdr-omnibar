//! The action line: what Enter does, and where.

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

pub(super) fn draw_action(frame: &mut Frame, area: Rect, app: &App) {
    let arrow =
        |color: Color| Span::styled("→ ", Style::new().fg(color).add_modifier(Modifier::BOLD));
    if let Some(confirm) = &app.confirm {
        let question = match confirm {
            Confirm::CloseTab { name, .. } => format!("close the tab {name}? ↵ yes · esc no"),
            Confirm::RemoveWorktree { branch, agents, .. } => {
                let agents = match agents {
                    0 => String::new(),
                    1 => " Its agent closes.".to_string(),
                    count => format!(" Its {count} agents close."),
                };
                // The notification of the result tells if the action restore of herdr-worktrunk
                // can bring the worktree back: wt alone keeps no record.
                format!("remove the worktree {branch}?{agents} ↵ yes · esc no")
            }
        };
        let line = Line::from(vec![
            arrow(red_color()),
            Span::styled(question, Style::new().fg(red_color())),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    if let Some(rename) = &app.rename {
        let line = Line::from(vec![
            arrow(accent_color()),
            Span::raw(format!(
                "rename the tab “{}” to “{}”",
                rename.old,
                rename.text.trim()
            )),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    if let Some(message) = &app.message {
        let line = Line::from(vec![
            arrow(dim_color()),
            Span::styled(message.clone(), Style::new().fg(muted_color())),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    let Some(item) = app.selected_item() else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "→ nothing selected",
                Style::new().fg(dim_color()),
            )),
            area,
        );
        return;
    };
    let name = app.create.name.trim();
    let tab = if name.is_empty() {
        "new tab (auto name)".to_string()
    } else {
        format!("new tab “{name}”")
    };
    let place = app.target_place();
    let place_label = place
        .as_ref()
        .map(|place| place.label.clone())
        .unwrap_or_default();
    // The verb, the place (open or new), and the agent field for a creation.
    let (verb, where_, agent_field, warning): (String, Option<Span>, bool, bool) = match item {
        Item::Agent(agent) => (
            format!("go to the agent {}", agent.name),
            Some(pill_span(true, &agent.place)),
            false,
            false,
        ),
        Item::Tab(tab_row) => (
            format!("go to the tab {}", tab_row.name),
            Some(pill_span(true, &tab_row.place)),
            false,
            false,
        ),
        Item::Pane(pane) => (
            if pane.name == pane.tab {
                format!("go to the pane {}", pane.name)
            } else {
                format!("go to the pane {} of the tab {}", pane.name, pane.tab)
            },
            Some(pill_span(true, &pane.place)),
            false,
            false,
        ),
        Item::Workspace(space) => (
            "go to".to_string(),
            Some(pill_span(true, &space.label())),
            false,
            false,
        ),
        Item::Action(action) => (
            format!("run the action {}", action.title),
            None,
            false,
            false,
        ),
        Item::Place(place) => (
            "create".to_string(),
            Some(pill_span(place.kind == Kind::Workspace, &place.label)),
            false,
            false,
        ),
        Item::Create(name) => (
            format!("create “{name}”: choose a tab or a worktree"),
            place.map(|place| pill_span(true, &place.label)),
            false,
            false,
        ),
        Item::Choice(choice) => match choice {
            Choice::NewWorktree { branch } if branch.is_empty() => (
                "type the branch name of the new worktree".into(),
                None,
                false,
                true,
            ),
            Choice::NewWorktree { branch } if !create::is_branch_name(branch) => (
                format!("“{branch}” is not a branch name"),
                None,
                false,
                true,
            ),
            Choice::NewWorktree { branch } => (
                format!("new worktree {branch}"),
                Some(pill_span(
                    false,
                    &format!(
                        "{} ▸ {branch}",
                        place.map(|place| place.project.as_str()).unwrap_or("")
                    ),
                )),
                true,
                false,
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
                (
                    format!("go to the worktree {branch}"),
                    Some(pill_span(true, &label)),
                    false,
                    false,
                )
            }
            Choice::Existing { branch, .. } => (
                format!("open the worktree {branch}"),
                Some(pill_span(false, &worktree_label(place, branch))),
                true,
                false,
            ),
            Choice::NewTab => (tab, Some(pill_span(true, &place_label)), true, false),
            Choice::NewWorkspace => (tab, Some(pill_span(false, &place_label)), true, false),
            Choice::AnotherPlace => ("choose another place".into(), None, false, false),
        },
    };
    let arrow_color = if warning { red_color() } else { accent_color() };
    let mut spans = vec![
        Span::styled(
            "→ ",
            Style::new().fg(arrow_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            verb,
            Style::new().fg(if warning { red_color() } else { Color::Reset }),
        ),
    ];
    if let Some(pill) = where_ {
        spans.push(Span::styled("  in ", Style::new().fg(dim_color())));
        spans.push(pill);
    }
    if agent_field {
        let agent = app.agent();
        let mut field = vec![
            Span::styled("agent ", Style::new().fg(dim_color())),
            Span::styled(
                format!(" {agent} "),
                Style::new().fg(agent_color(&agent)).bg(pill_color()),
            ),
        ];
        let key = if creating(app) { " ⇥" } else { " ⌃A" };
        field.push(Span::styled(key, Style::new().fg(match_color())));
        if app.create.no_prompts && agent != "shell" {
            field.push(Span::styled("  prompts off", Style::new().fg(red_color())));
        }
        let used = spans_width(&spans) + spans_width(&field);
        spans.push(Span::raw(
            " ".repeat((area.width as usize).saturating_sub(used)),
        ));
        spans.extend(field);
    }
    frame.render_widget(Paragraph::new(Line::from(round(spans, app))), area);
}

/// The label of the workspace of a worktree: "project ▸ branch".
fn worktree_label(place: Option<&Place>, branch: &str) -> String {
    match place {
        Some(place) => format!("{} ▸ {branch}", place.project),
        None => branch.to_string(),
    }
}
