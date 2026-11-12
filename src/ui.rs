//! The drawing of the popup.
//!
//! From top to bottom: the search line with the filters, a rule, the list (with the preview on
//! the right), a rule, the action line, and the footer with the keys. The list uses all the free
//! height.
//!
//! The colors that carry a meaning are ANSI colors: they follow the theme of the terminal. The
//! greys are fixed 256 colors, as in the popups of herdr-worktrunk.

use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::model::Status;
use crate::rows::{self, Item, Line as ListLine, FILTERS};

/// The prompt, the pointer of the selected row, the current filter.
const ACCENT: Color = Color::Indexed(12);
/// The letters of the query in a name, and the keys of the footer.
const MATCH: Color = Color::Indexed(14);
/// The secondary text: the context of a row, the labels of the footer.
const MUTED: Color = Color::Indexed(249);
/// The titles of the groups, the text that helps.
const DIM: Color = Color::Indexed(246);
/// The rules.
const RULE: Color = Color::Indexed(239);
/// The background of the selected row.
const SELECTED: Color = Color::Indexed(237);
/// The background of the place on the action line.
const PILL: Color = Color::Indexed(236);

/// The width of the popup under which the preview hides.
const PREVIEW_MIN_WIDTH: u16 = 70;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area().inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    let [search, rule_top, body, rule_bottom, action, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_search(frame, search, app);
    draw_rule(frame, rule_top);
    if app.preview && body.width >= PREVIEW_MIN_WIDTH {
        let [list, preview] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(body);
        draw_list(frame, list, app);
        draw_preview(frame, preview, app);
    } else {
        draw_list(frame, body, app);
    }
    draw_rule(frame, rule_bottom);
    draw_action(frame, action, app);
    draw_footer(frame, footer);
    if app.help {
        draw_help(frame, frame.area());
    }
}

fn draw_rule(frame: &mut Frame, area: Rect) {
    let rule = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(rule, Style::new().fg(RULE))),
        area,
    );
}

fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    let chips: Vec<Span> = FILTERS
        .iter()
        .flat_map(|filter| {
            let style = if *filter == app.filter {
                Style::new()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(DIM)
            };
            [
                Span::styled(format!(" {} ", filter.label()), style),
                Span::raw(" "),
            ]
        })
        .collect();
    let chips_width: u16 = chips.iter().map(|span| span.width() as u16).sum();
    let show_chips = area.width > chips_width + 20;
    let [field, chips_area] = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(if show_chips { chips_width } else { 0 }),
    ])
    .areas(area);
    let prompt = Span::styled("❯ ", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD));
    let text = if app.query.is_empty() {
        Span::styled(
            fit(
                "search · @ agents · + new · > actions",
                field.width as usize - 2,
            ),
            Style::new().fg(DIM),
        )
    } else {
        Span::raw(app.query.clone())
    };
    frame.render_widget(Paragraph::new(Line::from(vec![prompt, text])), field);
    if show_chips {
        frame.render_widget(Paragraph::new(Line::from(chips)), chips_area);
    }
    let cursor = field.x + 2 + app.query.width() as u16;
    frame.set_cursor_position((cursor.min(field.right().saturating_sub(1)), field.y));
}

/// The right part of a row: the agent and its status, the command of a tab, or the tabs of a
/// workspace.
fn right_side(item: &Item, app: &App) -> Vec<Span<'static>> {
    match item {
        Item::Agent(agent) => vec![
            Span::styled(
                agent.kind.clone(),
                Style::new().fg(agent_color(&agent.kind)),
            ),
            Span::raw(" "),
            Span::styled(
                agent.status.label().to_string(),
                Style::new().fg(status_color(agent.status)),
            ),
        ],
        Item::Tab(tab) => vec![Span::styled(tab.command.clone(), Style::new().fg(DIM))],
        Item::Workspace(space) => {
            let count = app
                .model
                .workspace_tabs
                .get(&space.workspace.workspace_id)
                .map(Vec::len)
                .unwrap_or(0);
            vec![Span::styled(
                format!("{count} tab{}", if count == 1 { "" } else { "s" }),
                Style::new().fg(DIM),
            )]
        }
    }
}

fn glyph(item: &Item) -> Span<'static> {
    match item {
        Item::Agent(agent) => Span::styled(
            agent.status.glyph(),
            Style::new().fg(status_color(agent.status)),
        ),
        Item::Tab(_) => Span::styled("▪", Style::new().fg(DIM)),
        Item::Workspace(_) => Span::styled("▣", Style::new().fg(ACCENT)),
    }
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    if !rows::is_ready(app.filter) {
        let text = format!(
            "The filter {} comes in a next version. ⇥ changes the filter.",
            app.filter.label()
        );
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::new().fg(DIM))),
            area,
        );
        return;
    }
    if app.lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No result. ⇥ changes the filter.",
                Style::new().fg(DIM),
            )),
            area,
        );
        return;
    }
    let width = area.width as usize;
    let height = area.height as usize;
    // The widths of the columns: the same for each row.
    let rights: Vec<usize> = app
        .lines
        .iter()
        .filter_map(|line| match line {
            ListLine::Row { item, .. } => Some(spans_width(&right_side(item, app))),
            ListLine::Group(_) => None,
        })
        .collect();
    let right_width = rights.iter().copied().max().unwrap_or(0).min(width / 3);
    let names = app.lines.iter().filter_map(|line| match line {
        ListLine::Row { item, .. } => Some(item.name().width()),
        ListLine::Group(_) => None,
    });
    let room = width.saturating_sub(4 + right_width + 4);
    let name_width = names.max().unwrap_or(0).min(room * 3 / 5).max(8.min(room));
    let context_width = room.saturating_sub(name_width);
    // Scroll: the selected row stays visible.
    let selected = app.selected.unwrap_or(0);
    let offset = selected.saturating_sub(height.saturating_sub(1));
    let lines: Vec<Line> = app
        .lines
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(index, line)| match line {
            ListLine::Group(title) => Line::from(Span::styled(
                format!(" {}", title.to_uppercase()),
                Style::new().fg(DIM).add_modifier(Modifier::BOLD),
            )),
            ListLine::Row { item, positions } => {
                let is_selected = Some(index) == app.selected;
                let mut spans = vec![
                    if is_selected {
                        Span::styled("▌", Style::new().fg(ACCENT))
                    } else {
                        Span::raw(" ")
                    },
                    glyph(item),
                    Span::raw(" "),
                ];
                spans.extend(highlighted(&item.name(), positions, name_width));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    pad(&fit(&item.context(), context_width), context_width),
                    Style::new().fg(DIM),
                ));
                spans.push(Span::raw("  "));
                spans.extend(fit_spans(right_side(item, app), right_width));
                let used = spans_width(&spans);
                spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
                let line = Line::from(spans);
                if is_selected {
                    line.patch_style(Style::new().bg(SELECTED))
                } else {
                    line
                }
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_preview(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(RULE));
    let inner = block.inner(area).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    frame.render_widget(block, area);
    let Some(item) = app.selected_item().cloned() else {
        return;
    };
    let height = inner.height as usize;
    let width = inner.width as usize;
    let title_style = Style::new().fg(DIM).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();
    match &item {
        Item::Workspace(space) => {
            lines.push(Line::from(Span::styled("WORKSPACE", title_style)));
            if let Some(folder) = &space.folder {
                lines.push(Line::from(Span::styled(
                    fit(&rows::home(folder), width),
                    Style::new().fg(MUTED),
                )));
            }
            lines.push(Line::raw(""));
            let tabs = app
                .model
                .workspace_tabs
                .get(&space.workspace.workspace_id)
                .cloned()
                .unwrap_or_default();
            for (index, tab) in tabs.iter().enumerate() {
                let mut spans = vec![Span::styled(
                    format!("{:>2} ", index + 1),
                    Style::new().fg(DIM),
                )];
                spans.push(Span::raw(fit(&tab.name, width.saturating_sub(16))));
                if let Some(agent) = &tab.agent {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        agent.clone(),
                        Style::new().fg(agent_color(agent)),
                    ));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        tab.status.glyph(),
                        Style::new().fg(status_color(tab.status)),
                    ));
                }
                lines.push(Line::from(spans));
            }
        }
        Item::Agent(_) | Item::Tab(_) => {
            lines.push(Line::from(Span::styled("PANE", title_style)));
            let text = app.preview_text(height.max(10)).unwrap_or_default();
            let content: Vec<&str> = text.trim_end().lines().collect();
            let room = height.saturating_sub(1);
            let start = content.len().saturating_sub(room);
            for text_line in &content[start..] {
                lines.push(Line::from(Span::styled(
                    fit(text_line, width),
                    Style::new().fg(MUTED),
                )));
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_action(frame: &mut Frame, area: Rect, app: &App) {
    let Some(item) = app.selected_item() else {
        frame.render_widget(
            Paragraph::new(Span::styled("→ nothing selected", Style::new().fg(DIM))),
            area,
        );
        return;
    };
    let (verb, place) = match item {
        Item::Agent(agent) => (
            format!("go to the agent {}", agent.name),
            agent.place.clone(),
        ),
        Item::Tab(tab) => (format!("go to the tab {}", tab.name), tab.place.clone()),
        Item::Workspace(space) => ("go to the workspace".to_string(), space.label()),
    };
    let line = Line::from(vec![
        Span::styled("→ ", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw(fit(
            &verb,
            (area.width as usize).saturating_sub(place.width() + 16),
        )),
        Span::styled("  in ", Style::new().fg(DIM)),
        Span::styled(
            format!(" workspace {place} "),
            Style::new().fg(Color::Reset).bg(PILL),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    let mut spans = Vec::new();
    for (key, label) in [
        ("↵", "go"),
        ("⇥", "filter"),
        ("⌃O", "preview"),
        ("esc", "close"),
    ] {
        spans.push(Span::styled(
            key,
            Style::new().fg(MATCH).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {label}   "), Style::new().fg(MUTED)));
    }
    let help = vec![
        Span::styled("?", Style::new().fg(MATCH).add_modifier(Modifier::BOLD)),
        Span::styled(" help", Style::new().fg(MUTED)),
    ];
    let used = spans_width(&spans) + spans_width(&help);
    spans.push(Span::raw(
        " ".repeat((area.width as usize).saturating_sub(used)),
    ));
    spans.extend(help);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let keys = [
        ("↵", "Go to the selected row, and close the popup."),
        (
            "↑ ↓  ⌃P ⌃N",
            "Move in the list. PageUp and PageDown move by 8 rows.",
        ),
        ("⇥ ⇧⇥", "The next or the previous filter."),
        (
            "@  +  >",
            "On an empty search: the agents, the creation, the actions.",
        ),
        ("⌃O", "Show or hide the preview."),
        ("⌃U  ⌃W", "Clear the search, or delete its last word."),
        ("esc", "Close the popup, or this help."),
        ("? F1", "This help."),
    ];
    let width = (area.width * 7 / 10).max(40).min(area.width);
    let height = (keys.len() as u16 + 4).min(area.height);
    let box_area = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, box_area);
    let block = Block::bordered()
        .border_style(Style::new().fg(RULE))
        .title(Span::styled(
            " herdr-omnibar keys ",
            Style::new().fg(ACCENT),
        ));
    let inner = block.inner(box_area).inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.render_widget(block, box_area);
    let lines: Vec<Line> = keys
        .iter()
        .map(|(key, text)| {
            Line::from(vec![
                Span::styled(
                    pad(key, 12),
                    Style::new().fg(MATCH).add_modifier(Modifier::BOLD),
                ),
                Span::styled(*text, Style::new().fg(MUTED)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The color of an agent kind.
fn agent_color(kind: &str) -> Color {
    match kind {
        "claude" => Color::Indexed(215),
        "codex" => Color::Indexed(14),
        "opencode" => Color::Indexed(13),
        _ => MUTED,
    }
}

/// The color of a status, like the glyphs of herdr.
fn status_color(status: Status) -> Color {
    match status {
        Status::Blocked => Color::Indexed(13),
        Status::Working => Color::Indexed(11),
        Status::Done => Color::Indexed(10),
        Status::Idle => DIM,
        Status::Unknown => RULE,
    }
}

/// A name with the letters of the query in color, cut and padded to a width.
fn highlighted(name: &str, positions: &[usize], width: usize) -> Vec<Span<'static>> {
    let shown = fit(name, width);
    let mut spans = Vec::new();
    for (index, character) in shown.chars().enumerate() {
        let style = if positions.contains(&index) && character != '…' {
            Style::new().fg(MATCH).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        spans.push(Span::styled(character.to_string(), style));
    }
    let used = shown.width();
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

/// A text cut to a width, with … at the end if it is too long.
pub fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let size = character.width().unwrap_or(0);
        if used + size > width - 1 {
            break;
        }
        out.push(character);
        used += size;
    }
    out.push('…');
    out
}

/// A text padded with spaces to a width.
fn pad(text: &str, width: usize) -> String {
    let used = text.width();
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Spans cut to a width: the last spans lose their end first.
fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    for span in spans {
        let size = span.width();
        if used + size <= width {
            used += size;
            out.push(span);
        } else {
            out.push(Span::styled(fit(&span.content, width - used), span.style));
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_text_ends_with_an_ellipsis() {
        assert_eq!(fit("omnibar design", 8), "omnibar…");
        assert_eq!(fit("omnibar", 8), "omnibar");
        assert_eq!(fit("omnibar", 0), "");
    }

    #[test]
    fn the_width_counts_wide_characters() {
        assert_eq!(fit("日本語のテキスト", 5), "日本…");
    }

    #[test]
    fn the_spans_are_cut_to_the_width() {
        let spans = fit_spans(vec![Span::raw("claude"), Span::raw(" needs you")], 10);
        assert_eq!(spans_width(&spans), 10);
    }
}
