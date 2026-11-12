//! The pull request of a branch (feature pull_requests): a badge on its row, and a view of the
//! preview.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::pulls::{Lookup, Pull};
use crate::times;

use super::style::*;
use super::text::fit;

/// The state of a pull request, as a word and a color.
fn state(pull: &Pull) -> (&'static str, Color) {
    match pull.state.as_str() {
        "MERGED" => ("merged", pink_color()),
        "CLOSED" => ("closed", red_color()),
        _ if pull.draft => ("draft", dim_color()),
        _ => ("open", green_color()),
    }
}

/// The glyph of the checks: ✗ a check failed, ◐ a check runs, ✓ all the checks passed.
fn checks_glyph(pull: &Pull) -> Option<Span<'static>> {
    let checks = pull.checks;
    if checks.failed > 0 {
        Some(Span::styled("✗", Style::new().fg(red_color())))
    } else if checks.pending > 0 {
        Some(Span::styled("◐", Style::new().fg(yellow_color())))
    } else if checks.passed > 0 {
        Some(Span::styled("✓", Style::new().fg(green_color())))
    } else {
        None
    }
}

/// The badge of a branch row: the number of its pull request in the color of its state, and the
/// glyph of its checks.
pub(super) fn badge(pull: &Pull) -> Vec<Span<'static>> {
    let (_, color) = state(pull);
    let mut spans = vec![Span::styled(
        format!("#{}", pull.number),
        Style::new().fg(color),
    )];
    if let Some(glyph) = checks_glyph(pull) {
        spans.push(Span::raw(" "));
        spans.push(glyph);
    }
    spans.push(Span::raw("  "));
    spans
}

/// A line of the view: a label in grey, then its spans.
fn row(label: &str, spans: Vec<Span<'static>>) -> Line<'static> {
    let mut line = vec![Span::styled(
        format!("{label:<9}"),
        Style::new().fg(dim_color()),
    )];
    line.extend(spans);
    Line::from(line)
}

fn muted(text: String) -> Span<'static> {
    Span::styled(text, Style::new().fg(muted_color()))
}

/// The view of the pull request of a branch.
pub(super) fn lines(lookup: &Lookup, width: usize) -> Vec<Line<'static>> {
    let pull = match lookup {
        Lookup::Reading => return vec![Line::from(muted("reading the pull requests…".into()))],
        Lookup::Failed(error) => {
            return vec![Line::from(muted(fit(&format!("gh: {error}"), width)))]
        }
        Lookup::None => return vec![Line::from(muted("no pull request for this branch".into()))],
        Lookup::Found(pull) => pull,
    };
    let (state_word, state_color) = state(pull);
    let mut lines = vec![
        row(
            &format!("PR #{}", pull.number),
            vec![Span::styled(
                fit(&pull.title, width.saturating_sub(9)),
                Style::new().add_modifier(Modifier::BOLD),
            )],
        ),
        row(
            "",
            vec![
                Span::styled(state_word, Style::new().fg(state_color)),
                muted(format!(" · base {}", pull.base)),
                Span::styled(
                    format!(" · +{}", pull.additions),
                    Style::new().fg(green_color()),
                ),
                Span::styled(
                    format!(" −{}", pull.deletions),
                    Style::new().fg(red_color()),
                ),
            ],
        ),
    ];
    // The counts of the checks, without the counts at 0.
    let checks = pull.checks;
    let counts: Vec<Span<'static>> = [
        (checks.passed, "✓", "passed", green_color()),
        (checks.failed, "✗", "failed", red_color()),
        (checks.pending, "◐", "running", yellow_color()),
    ]
    .into_iter()
    .filter(|(count, ..)| *count > 0)
    .enumerate()
    .map(|(index, (count, glyph, word, color))| {
        let gap = if index == 0 { "" } else { "   " };
        Span::styled(
            format!("{gap}{glyph} {count} {word}"),
            Style::new().fg(color),
        )
    })
    .collect();
    if counts.is_empty() {
        lines.push(row("checks", vec![muted("no check".into())]));
    } else {
        lines.push(row("checks", counts));
    }
    let review = match pull.review.as_str() {
        "APPROVED" => "approved",
        "CHANGES_REQUESTED" => "changes requested",
        "REVIEW_REQUIRED" => "review required",
        _ => "no review",
    };
    lines.push(row("review", vec![muted(review.into())]));
    if pull.state == "OPEN" {
        let merge = match pull.merge.as_str() {
            "CLEAN" => "clean: ready to merge",
            "BLOCKED" => "blocked by a rule or a check",
            "DIRTY" => "conflicts with the base branch",
            "BEHIND" => "behind the base branch",
            "UNSTABLE" => "a check that is not required failed",
            "DRAFT" => "draft",
            _ => "unknown",
        };
        lines.push(row("merge", vec![muted(merge.into())]));
    }
    if let Some(updated) = pull.updated {
        let age = crate::pulls::age(updated);
        let text = if age < 60 {
            "now".to_string()
        } else {
            format!("{} ago", times::short(std::time::Duration::from_secs(age)))
        };
        lines.push(row("updated", vec![muted(text)]));
    }
    lines
}
