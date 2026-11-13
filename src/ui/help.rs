//! The help: the keys of the popup.

use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use super::{style::*, text::*};

pub(super) fn draw_help(frame: &mut Frame, area: Rect) {
    let keys = [
        ("↵", "Go to the selected row, choose a place, or create."),
        (
            "↑ ↓  ⌃P ⌃N",
            "Move in the list. PageUp and PageDown move by 8 rows.",
        ),
        (
            "⇥ ⇧⇥",
            "The next or the previous filter. In the step what: the next agent.",
        ),
        (
            "@ & * # + > %",
            "On an empty text: agents, workspaces, worktrees, folders, creation, actions, panes, if the config shows the filter. Again: All.",
        ),
        (
            "~  /",
            "In the step where: the folders of the disk. ⇥ completes.",
        ),
        ("⌃A", "On a row that opens a worktree: the next agent."),
        (
            "⌃F",
            "Fork the session of the selected agent into a new tab (herdr-fork).",
        ),
        (
            "⌃Y",
            "In the step what: start the agent without its permission prompts.",
        ),
        (
            "⌃↵",
            "A new tab, with its agent, in the open workspace of the selected row.",
        ),
        (
            "⌃⌫",
            "Close the tab or the pane of the selected row, or remove its worktree, after a question.",
        ),
        (
            "⌃R",
            "Rename the tab of the selected row. ↵ saves, esc keeps the name.",
        ),
        (
            "mouse",
            "Click a row, a chip or a key. Double click runs a row. The wheel moves.",
        ),
        (
            "⌃← ⌃→",
            "The view of the preview of a worktree: diff, log, changes.",
        ),
        ("⌃O", "Show or hide the preview."),
        ("⌃U  ⌃W", "Clear the text, or delete its last word."),
        (
            "✓ ● ○",
            "A branch: worktree without change, with changes, no worktree.",
        ),
        ("esc", "Close, or go back to the step where."),
        ("? F1", "This help."),
    ];
    // The help covers the list and the preview, like the mockup.
    let box_area = area;
    frame.render_widget(Clear, box_area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(rule_color()))
        .title(Span::styled(
            " herdr-omnibar keys ",
            Style::new().fg(accent_color()),
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
                    Style::new().fg(match_color()).add_modifier(Modifier::BOLD),
                ),
                Span::styled(*text, Style::new().fg(muted_color())),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}
