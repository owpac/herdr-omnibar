//! The colors and the styles of the popup: the roles of the herdr theme, the pills.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::app::App;
use crate::config::Pills;
use crate::model::Status;
use crate::theme;

use super::text::*;

/// A color of the palette of the herdr theme.
fn palette() -> &'static theme::Palette {
    &theme::current().palette
}

/// A grey of the palette, or a fixed 256 color with the theme terminal.
fn grey(color: Color, terminal: u8) -> Color {
    if theme::current().terminal {
        Color::Indexed(terminal)
    } else {
        color
    }
}

/// The prompt, the pointer of the selected row, the current filter, an open workspace.
pub(super) fn accent_color() -> Color {
    palette().accent
}

/// The text on the accent: the background of the panels of herdr, dark on a dark theme and light
/// on a light theme, else black.
pub(super) fn on_accent_color() -> Color {
    match palette().panel_bg {
        Color::Reset => Color::Black,
        color => color,
    }
}

/// The letters of the query in a name, and the keys of the footer.
pub(super) fn match_color() -> Color {
    palette().teal
}

/// A creation: a new tab, a new worktree, a new workspace.
pub(super) fn green_color() -> Color {
    palette().green
}

/// A name that cannot be a branch, and the flag without prompts.
pub(super) fn red_color() -> Color {
    palette().red
}

/// The secondary text: the labels of the footer.
pub(super) fn muted_color() -> Color {
    grey(palette().overlay1, 249)
}

/// The titles of the groups, the context of a row, the text that helps.
pub(super) fn dim_color() -> Color {
    grey(palette().overlay0, 246)
}

/// The rules.
pub(super) fn rule_color() -> Color {
    grey(palette().surface0, 239)
}

/// The background of the selected row.
pub(super) fn selected_color() -> Color {
    grey(palette().active_row_bg, 237)
}

/// The background of a place on the action line.
pub(super) fn pill_color() -> Color {
    grey(palette().surface0, 236)
}

/// The colors of the agents and of the states.
fn orange_color() -> Color {
    palette().peach
}

fn pink_color() -> Color {
    palette().mauve
}

pub(super) fn yellow_color() -> Color {
    palette().yellow
}

/// The color of an agent kind.
pub(super) fn agent_color(kind: &str) -> Color {
    match kind {
        "claude" => orange_color(),
        "codex" => match_color(),
        "opencode" => pink_color(),
        _ => muted_color(),
    }
}

/// The color of a status, like the glyphs of herdr.
pub(super) fn status_color(status: Status) -> Color {
    match status {
        Status::Blocked => pink_color(),
        Status::Working => yellow_color(),
        Status::Done => green_color(),
        Status::Idle => dim_color(),
        Status::Unknown => rule_color(),
    }
}

/// The ends of each span with a background: the filter, the crumb, the places of the action line.
/// The first and the last space of the span become the ends of the pill: the width stays the
/// same. A cell gives only 2 forms of end: a half circle, or a straight side.
pub(super) fn round(spans: Vec<Span<'static>>, app: &App) -> Vec<Span<'static>> {
    let (left_cap, right_cap) = match app.config.pills {
        Pills::Flat => return spans,
        // Half blocks: a straight side at the middle of the cell.
        Pills::Square => ("▐", "▌"),
        // Half circles of a Nerd Font.
        Pills::Round => ("\u{e0b6}", "\u{e0b4}"),
    };
    let mut rounded = Vec::new();
    for span in spans {
        let background = span.style.bg.filter(|color| *color != Color::Reset);
        let text = span.content.to_string();
        match (
            background,
            text.strip_prefix(' ')
                .and_then(|rest| rest.strip_suffix(' ')),
        ) {
            (Some(color), Some(inner)) => {
                let cap = Style::new().fg(color);
                rounded.push(Span::styled(left_cap, cap));
                rounded.push(Span::styled(inner.to_string(), span.style));
                rounded.push(Span::styled(right_cap, cap));
            }
            _ => rounded.push(span),
        }
    }
    rounded
}

/// A place on the action line: violet for an open workspace, green for a new one.
pub(super) fn pill_span(open: bool, label: &str) -> Span<'static> {
    if open {
        Span::styled(
            format!(" workspace {label} · open "),
            Style::new().fg(Color::Reset).bg(pill_color()),
        )
    } else {
        Span::styled(
            format!(" ＋ new workspace {label} "),
            Style::new().fg(green_color()).bg(pill_color()),
        )
    }
}

pub(super) fn crumb(place: Option<&str>) -> Span<'static> {
    let text = match place {
        Some(place) => format!(" new › {} ", fit(place, 30)),
        None => " new ".to_string(),
    };
    Span::styled(text, Style::new().fg(muted_color()).bg(pill_color()))
}

/// The space after the crumb.
pub(super) fn gap() -> Span<'static> {
    Span::raw(" ")
}
