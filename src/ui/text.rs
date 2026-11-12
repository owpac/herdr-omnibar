//! The text helpers of the popup: cut, wrap and pad a text to a width.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::style::*;

/// A name with the letters of the query in color, cut and padded to a width.
pub(super) fn highlighted(
    name: &str,
    positions: &[usize],
    width: usize,
    base: Style,
) -> Vec<Span<'static>> {
    let shown = fit(name, width);
    let mut spans = Vec::new();
    for (index, character) in shown.chars().enumerate() {
        let style = if positions.contains(&index) && character != '…' {
            Style::new().fg(match_color()).add_modifier(Modifier::BOLD)
        } else {
            base
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

/// A text cut into lines of a width, at the spaces.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.width() + 1 + word.width() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// A text padded with spaces to a width.
pub(super) fn pad(text: &str, width: usize) -> String {
    let used = text.width();
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

pub(super) fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Spans cut to a width: the last spans lose their end first.
pub(super) fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
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

pub(super) fn tabs_text(count: usize) -> String {
    format!("{count} tab{}", if count == 1 { "" } else { "s" })
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
    fn a_description_wraps_at_the_spaces() {
        assert_eq!(
            wrap("open the cleaner of the worktrees", 16),
            ["open the cleaner", "of the worktrees"]
        );
    }

    #[test]
    fn the_spans_are_cut_to_the_width() {
        let spans = fit_spans(vec![Span::raw("claude"), Span::raw(" needs you")], 10);
        assert_eq!(spans_width(&spans), 10);
    }
}
