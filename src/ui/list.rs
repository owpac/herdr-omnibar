//! The list: the rows, with their glyph, their name, their context and their right side.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Step};
use crate::create::{self, Choice};
use crate::model::Status;
use crate::places::{Kind, Place};
use crate::rows::{self, Item, Line as ListLine};

use super::{creating, style::*, text::*};

pub(super) fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    if app.lines.is_empty() {
        let text = match (creating(app), app.create.step) {
            (true, Step::Where) => "No place. Type ~ or / for a folder of the disk.",
            _ => "No result. ⇥ changes the filter.",
        };
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::new().fg(dim_color()))),
            area,
        );
        return;
    }
    let width = area.width as usize;
    let height = area.height as usize;
    // One pass over the rows: the name and the right side of each row, for the widths of the
    // columns and for the drawing.
    let cells: Vec<Option<(String, Vec<Span<'static>>)>> = app
        .lines
        .iter()
        .map(|line| match line {
            ListLine::Row { item, .. } => Some((display_name(item, app), right_side(item, app))),
            _ => None,
        })
        .collect();
    // The widths of the columns: the same for each row.
    let right_width = cells
        .iter()
        .flatten()
        .map(|(_, right)| spans_width(right))
        .max()
        .unwrap_or(0)
        .min(width / 3);
    let room = width.saturating_sub(4 + right_width + 4);
    let name_width = cells
        .iter()
        .flatten()
        .map(|(name, _)| name.width())
        .max()
        .unwrap_or(0)
        .min(room * 3 / 5)
        .max(8.min(room));
    let context_width = room.saturating_sub(name_width);
    let offset = app.list_offset;
    let lines: Vec<Line> = app
        .lines
        .iter()
        .zip(cells)
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(index, (line, cell))| match (line, cell) {
            (ListLine::Gap, _) => Line::raw(""),
            (ListLine::Group(title), _) => Line::from(Span::styled(
                format!(" {}", title.to_uppercase()),
                Style::new().fg(dim_color()).add_modifier(Modifier::BOLD),
            )),
            (ListLine::Row { .. }, None) => Line::raw(""),
            (ListLine::Row { item, positions }, Some((name, right))) => {
                let is_selected = Some(index) == app.selected;
                let mut spans = vec![
                    if is_selected {
                        Span::styled("▌", Style::new().fg(accent_color()))
                    } else {
                        Span::raw(" ")
                    },
                    row_glyph(item, app),
                    Span::raw(" "),
                ];
                spans.extend(highlighted(&name, positions, name_width, name_style(item)));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    pad(&fit(&context_of(item, app), context_width), context_width),
                    Style::new().fg(dim_color()),
                ));
                spans.push(Span::raw("  "));
                spans.extend(fit_spans(right, right_width));
                let used = spans_width(&spans);
                spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
                let line = Line::from(spans);
                if is_selected {
                    line.patch_style(Style::new().bg(selected_color()))
                } else {
                    line
                }
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// The right part of a row.
fn right_side(item: &Item, app: &App) -> Vec<Span<'static>> {
    let mut spans = right_spans(item, app);
    // The running time of the agent or of the command (feature running_time).
    let pane = match item {
        Item::Agent(agent) => Some(agent.pane_id.as_str()),
        Item::Tab(tab) => tab.pane_id.as_deref(),
        Item::Pane(pane) => Some(pane.pane_id.as_str()),
        _ => None,
    };
    if let Some(time) = pane.and_then(|pane| app.running_time(pane)) {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("{time:>5}"),
            Style::new().fg(dim_color()),
        ));
    }
    spans
}

fn right_spans(item: &Item, app: &App) -> Vec<Span<'static>> {
    match item {
        Item::Pane(pane) => match &pane.agent {
            Some(kind) => vec![
                Span::styled(kind_text(kind, app), Style::new().fg(agent_color(kind))),
                Span::raw(" "),
                Span::styled(
                    status_text(pane.status, app),
                    Style::new().fg(status_color(pane.status)),
                ),
            ],
            None => Vec::new(),
        },
        Item::Agent(agent) => vec![
            Span::styled(
                kind_text(&agent.kind, app),
                Style::new().fg(agent_color(&agent.kind)),
            ),
            Span::raw(" "),
            Span::styled(
                status_text(agent.status, app),
                Style::new().fg(status_color(agent.status)),
            ),
        ],
        Item::Tab(tab) => vec![Span::styled(
            tab.command.clone(),
            Style::new().fg(dim_color()),
        )],
        Item::Workspace(space) => {
            let count = app
                .model
                .workspace_tabs
                .get(&space.workspace.workspace_id)
                .map(Vec::len)
                .unwrap_or(0);
            vec![Span::styled(tabs_text(count), Style::new().fg(dim_color()))]
        }
        Item::Place(place) => place_right(place, app),
        Item::Action(_) | Item::Create(_) => Vec::new(),
        Item::Choice(choice) => match choice {
            Choice::Existing {
                branch,
                workspace_id,
                worktree,
            } => {
                let (text, color) = match (workspace_id, worktree) {
                    (Some(_), _) => ("workspace", accent_color()),
                    (None, Some(_)) => ("no workspace", dim_color()),
                    (None, None) => ("no worktree", dim_color()),
                };
                let mut spans = vec![Span::styled(text, Style::new().fg(color))];
                // The age of the last commit of the branch.
                if let Some(age) = app.branch_age(branch) {
                    spans.push(Span::styled(
                        format!(" {age:>5}"),
                        Style::new().fg(dim_color()),
                    ));
                }
                spans
            }
            Choice::AnotherPlace => Vec::new(),
            _ => {
                let agent = app.agent();
                vec![Span::styled(
                    agent.clone(),
                    Style::new().fg(agent_color(&agent)),
                )]
            }
        },
    }
}

/// The kind of an agent, after its logo if the font is installed. With running times, it has a
/// fixed width: the times make a column.
fn kind_text(kind: &str, app: &App) -> String {
    let kind = match crate::icons::logo(kind).filter(|_| app.logos) {
        Some(logo) => format!("{logo} {kind}"),
        None => kind.to_string(),
    };
    if app.times.is_some() {
        let width = if app.logos { 10 } else { 8 };
        format!("{kind:<width$}")
    } else {
        kind
    }
}

/// The label of a status. With running times, it has a fixed width: the times make a column.
fn status_text(status: Status, app: &App) -> String {
    if app.times.is_some() {
        format!("{:<9}", status.label())
    } else {
        status.label().to_string()
    }
}

fn place_right(place: &Place, app: &App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if place.workspace_id.is_some() && place.workspace_id == app.current {
        spans.push(Span::styled("current ", Style::new().fg(accent_color())));
    }
    match place.kind {
        Kind::Workspace => {
            for tab in &place.tabs {
                spans.push(match &tab.agent {
                    Some(_) => Span::styled(
                        format!("{} ", tab.status.glyph()),
                        Style::new().fg(status_color(tab.status)),
                    ),
                    None => Span::styled("▪ ", Style::new().fg(dim_color())),
                });
            }
        }
        Kind::Project => spans.push(Span::styled("no workspace", Style::new().fg(dim_color()))),
        Kind::Folder => spans.push(Span::styled("folder", Style::new().fg(dim_color()))),
    }
    spans
}

fn glyph(item: &Item) -> Span<'static> {
    match item {
        Item::Agent(agent) => Span::styled(
            agent.status.glyph(),
            Style::new().fg(status_color(agent.status)),
        ),
        Item::Tab(_) => Span::styled("▪", Style::new().fg(dim_color())),
        Item::Pane(pane) if pane.agent.is_some() => Span::styled(
            pane.status.glyph(),
            Style::new().fg(status_color(pane.status)),
        ),
        Item::Pane(_) => Span::styled("▫", Style::new().fg(dim_color())),
        Item::Workspace(_) => Span::styled("▣", Style::new().fg(accent_color())),
        Item::Action(_) => Span::styled("›", Style::new().fg(accent_color())),
        Item::Create(_) => Span::styled("+", Style::new().fg(green_color())),
        Item::Place(place) => match place.kind {
            Kind::Workspace => Span::styled("▣", Style::new().fg(accent_color())),
            Kind::Project => Span::styled("□", Style::new().fg(muted_color())),
            Kind::Folder => Span::styled("▸", Style::new().fg(dim_color())),
        },
        Item::Choice(choice) => match choice {
            Choice::Existing { .. } => Span::styled("↪", Style::new().fg(accent_color())),
            Choice::AnotherPlace => Span::styled("‹", Style::new().fg(dim_color())),
            _ => Span::styled("+", Style::new().fg(green_color())),
        },
    }
}

/// The glyph of a row of the list. Outside the creation, a branch shows the git state of its
/// worktree, like the mockup: ✓ no change, ● changes not committed, ○ no worktree. The right
/// column tells its workspace.
fn row_glyph(item: &Item, app: &App) -> Span<'static> {
    match item {
        Item::Choice(Choice::Existing { worktree, .. }) if !creating(app) => match worktree {
            None => Span::styled("○", Style::new().fg(dim_color())),
            Some(folder) => match app.changes.of(folder) {
                Some(false) => Span::styled("✓", Style::new().fg(green_color())),
                Some(true) => Span::styled("●", Style::new().fg(yellow_color())),
                // The thread did not read the worktree yet.
                None => Span::styled("·", Style::new().fg(dim_color())),
            },
        },
        _ => glyph(item),
    }
}

/// The style of the name of a row: green for a creation, red for a wrong branch name.
fn name_style(item: &Item) -> Style {
    match item {
        Item::Choice(Choice::NewWorktree { branch })
            if !branch.is_empty() && !create::is_branch_name(branch) =>
        {
            Style::new().fg(red_color())
        }
        Item::Choice(Choice::NewWorktree { .. } | Choice::NewTab | Choice::NewWorkspace)
        | Item::Create(_) => Style::new().fg(green_color()),
        _ => Style::new(),
    }
}

/// The name of a row on the screen. A new tab or workspace shows the typed name.
fn display_name(item: &Item, app: &App) -> String {
    let name = app.create.name.trim();
    match item {
        Item::Choice(Choice::NewTab) if !name.is_empty() => format!("new tab “{name}”"),
        Item::Choice(Choice::NewWorkspace) if !name.is_empty() => {
            format!("new workspace “{name}”")
        }
        _ => item.name(),
    }
}

/// The context of a row. A choice tells what it creates.
fn context_of(item: &Item, app: &App) -> String {
    let name = app.create.name.trim();
    let place = app
        .target_place()
        .map(|place| place.label.clone())
        .unwrap_or_default();
    let auto = if name.is_empty() { "auto name, " } else { "" };
    match item {
        // An open workspace tells its number of tabs, like its preview.
        Item::Place(place) if place.kind == Kind::Workspace => tabs_text(place.tabs.len()),
        // Outside the creation, a branch tells where its worktree is.
        Item::Choice(Choice::Existing {
            branch,
            worktree,
            workspace_id,
        }) if !creating(app) => match (workspace_id, worktree) {
            (Some(id), _) => app
                .open_place(id)
                .map(|place| place.label.clone())
                .unwrap_or_else(|| branch.clone()),
            (None, Some(folder)) => rows::home(folder),
            (None, None) => "branch without worktree".into(),
        },
        Item::Choice(choice) => match choice {
            Choice::NewWorktree { branch } if branch.is_empty() => "type its branch name".into(),
            Choice::NewWorktree { branch } if !create::is_branch_name(branch) => {
                "not a branch name".into()
            }
            Choice::NewWorktree { .. } => format!(
                "branch from {}, in its own workspace",
                app.base_branch().unwrap_or("the default branch")
            ),
            Choice::Existing {
                workspace_id: Some(_),
                ..
            } => "go to its workspace".into(),
            Choice::Existing {
                worktree: Some(_), ..
            } => "open its worktree".into(),
            Choice::Existing { .. } => "create its worktree".into(),
            Choice::NewTab => format!("{auto}in {place}"),
            Choice::NewWorkspace => format!("{auto}in a new workspace {place}"),
            Choice::AnotherPlace => "a project, a workspace or a folder".into(),
        },
        Item::Create(_) => "a tab or a worktree: choose in the next step".into(),
        _ => item.context(),
    }
}
