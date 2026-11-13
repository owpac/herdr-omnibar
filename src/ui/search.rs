//! The search line: the crumb of the creation, the prompt, the text and the chips.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Step};
use crate::filter::Filter;
use crate::rows::{self};

use super::{creating, style::*, text::*};

/// Draw the search line. Return the area of each chip, for the mouse.
pub(super) fn draw_search(frame: &mut Frame, area: Rect, app: &App) -> Vec<(Rect, Filter)> {
    // A narrow popup shows only the chip of the current filter.
    let filters = app.shown_filters();
    let all_width: u16 = filters.iter().map(|filter| chip_width(*filter) + 1).sum();
    let all_fit = area.width > all_width + 30;
    let shown: Vec<Filter> = filters
        .into_iter()
        .filter(|filter| all_fit || *filter == app.filter)
        .collect();
    let chips: Vec<Span> = shown
        .iter()
        .flat_map(|filter| chip(*filter, *filter == app.filter))
        .collect();
    let chips_width: u16 = chips.iter().map(|span| span.width() as u16).sum();
    let show_chips = area.width > chips_width + 30;
    let [field, chips_area] = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(if show_chips { chips_width } else { 0 }),
    ])
    .areas(area);
    // The creation shows its step before the prompt: "new › where", or "new › <place>".
    let mut spans = Vec::new();
    let (prompt, placeholder) = match (creating(app), app.create.step) {
        _ if app.rename.is_some() => {
            spans.extend([
                Span::styled(
                    " rename › tab ",
                    Style::new().fg(muted_color()).bg(pill_color()),
                ),
                gap(),
            ]);
            ("name ❯ ", "the new name of the tab")
        }
        (true, Step::Where) => {
            spans.extend([crumb(None), gap()]);
            ("where ❯ ", "project, workspace or folder · ~ for a path")
        }
        (true, Step::What) => {
            let place = app
                .create
                .place
                .as_ref()
                .map(|place| place.label.clone())
                .unwrap_or_default();
            spans.extend([crumb(Some(&place)), gap()]);
            ("name ❯ ", "tab or branch name")
        }
        // Each filter tells what its text finds.
        _ => (
            "❯ ",
            match app.filter {
                Filter::Agents => "agent or workspace",
                Filter::Tabs => "tab or workspace",
                Filter::Workspaces => "workspace or folder",
                Filter::Worktrees => "branch: go to it, or type a new name to create it",
                Filter::Folders => "project or folder",
                Filter::Actions => "action or plugin",
                Filter::Panes => "pane, tab or workspace",
                _ => "search",
            },
        ),
    };
    spans.push(Span::styled(
        prompt,
        Style::new().fg(accent_color()).add_modifier(Modifier::BOLD),
    ));
    let before: usize = spans.iter().map(Span::width).sum();
    let text = app.text();
    if text.is_empty() {
        spans.push(Span::styled(
            fit(placeholder, (field.width as usize).saturating_sub(before)),
            Style::new().fg(dim_color()),
        ));
    } else {
        spans.push(Span::raw(text.to_string()));
        // The completion of a path, in grey after the text.
        if creating(app) && app.create.step == Step::Where && rows::is_path(text) {
            if let Some(completed) = rows::complete_path(text, &app.lines) {
                if let Some(rest) = completed.strip_prefix(text) {
                    spans.push(Span::styled(rest.to_string(), Style::new().fg(dim_color())));
                }
            }
        }
    }
    frame.render_widget(Paragraph::new(Line::from(round(spans, app))), field);
    let mut areas = Vec::new();
    if show_chips {
        let mut x = chips_area.x;
        for filter in &shown {
            let width = chip_width(*filter);
            areas.push((Rect::new(x, chips_area.y, width, 1), *filter));
            x += width + 1;
        }
        frame.render_widget(Paragraph::new(Line::from(round(chips, app))), chips_area);
    }
    let cursor = field.x + (before + text.width()) as u16;
    frame.set_cursor_position((cursor.min(field.right().saturating_sub(1)), field.y));
    areas
}

/// The spans of a chip: the shortcut of its filter, then its label, then a space. The current
/// filter is one span with a background: `round` gives it its ends.
fn chip(filter: Filter, current: bool) -> Vec<Span<'static>> {
    let key = filter
        .shortcut()
        .map(|key| format!("{key} "))
        .unwrap_or_default();
    let label = filter.label();
    if current {
        let style = Style::new()
            .fg(on_accent_color())
            .bg(accent_color())
            .add_modifier(Modifier::BOLD);
        return vec![
            Span::styled(format!(" {key}{label} "), style),
            Span::raw(" "),
        ];
    }
    vec![
        Span::raw(" "),
        Span::styled(key, Style::new().fg(match_color())),
        Span::styled(format!("{label} "), Style::new().fg(dim_color())),
        Span::raw(" "),
    ]
}

/// The width of a chip, without the space after it.
fn chip_width(filter: Filter) -> u16 {
    let key = filter.shortcut().map_or(0, |_| 2);
    (filter.label().width() + 2 + key) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chip_shows_the_shortcut_of_its_filter_and_has_its_width() {
        for filter in [Filter::All, Filter::Agents, Filter::Worktrees] {
            for current in [true, false] {
                let spans = chip(filter, current);
                let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
                assert_eq!(text.width(), chip_width(filter) as usize + 1);
            }
        }
        let text = |filter| {
            chip(filter, false)
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };
        assert_eq!(text(Filter::Agents), " @ Agents  ");
        assert_eq!(text(Filter::All), " All  ");
    }
}
