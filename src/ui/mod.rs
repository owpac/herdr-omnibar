//! The drawing of the popup.
//!
//! From top to bottom: the search line with the filters, a rule, the list (with the preview on
//! the right), a rule, the action line, and the footer with the keys. The list uses all the free
//! height.
//!
//! Each part has its module: search, list, preview, action and footer, with the help over the
//! list. style.rs gives the colors of the roles, from the herdr theme (theme.rs), and the pills.
//! text.rs cuts, wraps and pads the texts.

mod action;
mod footer;
mod help;
mod list;
mod preview;
mod search;
mod style;
mod text;

use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::filter::Filter;
use crate::rows::Line as ListLine;

use action::draw_action;
use footer::draw_footer;
use help::draw_help;
use list::draw_list;
use preview::draw_preview;
use search::draw_search;
use style::rule_color;

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
    // The mouse needs the places of the chips and of the keys of the footer.
    app.chip_areas = draw_search(frame, search, app);
    draw_rule(frame, rule_top);
    let (list, preview) = if app.preview && body.width >= PREVIEW_MIN_WIDTH {
        let [list, preview] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(body);
        (list, Some(preview))
    } else {
        (body, None)
    };
    // The mouse needs the place of the list and its first visible line.
    app.list_area = list;
    app.list_offset = scroll_offset(app.list_offset, app.selected, &app.lines, list.height);
    draw_list(frame, list, app);
    if let Some(preview) = preview {
        draw_preview(frame, preview, app);
    }
    draw_rule(frame, rule_bottom);
    draw_action(frame, action, app);
    app.hint_areas = draw_footer(frame, footer, app);
    if app.help {
        draw_help(frame, body);
    }
}

fn draw_rule(frame: &mut Frame, area: Rect) {
    let rule = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(rule, Style::new().fg(rule_color()))),
        area,
    );
}

fn creating(app: &App) -> bool {
    app.filter == Filter::New
}

/// The first visible line of the list. The list moves only when the selected row leaves the
/// view. The title of the group of the first visible row stays visible.
fn scroll_offset(
    previous: usize,
    selected: Option<usize>,
    lines: &[ListLine],
    height: u16,
) -> usize {
    let height = (height as usize).max(1);
    let Some(selected) = selected else {
        return 0;
    };
    let mut offset = previous;
    if selected < offset {
        offset = selected;
    } else if selected >= offset + height {
        offset = selected + 1 - height;
    }
    let group_above = selected > 0 && matches!(lines.get(selected - 1), Some(ListLine::Group(_)));
    if offset == selected && group_above && height > 1 {
        offset -= 1;
    }
    // No empty lines at the end of the list while lines above it are hidden.
    offset.min(lines.len().saturating_sub(height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::Item;

    /// A list with a group for each number, and this number of rows in the group.
    fn list(groups: &[usize]) -> Vec<ListLine> {
        let mut lines = Vec::new();
        for (index, rows) in groups.iter().enumerate() {
            lines.push(ListLine::Group(format!("group {index}")));
            for row in 0..*rows {
                lines.push(ListLine::Row {
                    item: Item::Create(format!("row {row}")),
                    positions: Vec::new(),
                });
            }
        }
        lines
    }

    #[test]
    fn the_list_moves_only_when_the_selected_row_leaves_the_view() {
        // The lines: group 0, rows 1 to 10, group 11, rows 12 to 21.
        let lines = list(&[10, 10]);
        assert_eq!(scroll_offset(0, Some(1), &lines, 5), 0);
        assert_eq!(scroll_offset(0, Some(7), &lines, 5), 3);
        // Up in the view: the list stays.
        assert_eq!(scroll_offset(3, Some(5), &lines, 5), 3);
        // Up out of the view, to the first row of a group: the title of the group shows.
        assert_eq!(scroll_offset(14, Some(12), &lines, 5), 11);
        // A new short list starts at its top.
        assert_eq!(scroll_offset(15, Some(1), &list(&[3]), 5), 0);
    }
}
