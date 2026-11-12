//! The preview of the selected row, on the right of the list.

use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::create::{self, Choice};
use crate::model::TabSummary;
use crate::places::{Kind, Place};
use crate::rows::{self, Item};

use super::{creating, style::*, text::*};

pub(super) fn draw_preview(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(rule_color()));
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
    let title = |text: &str| {
        Line::from(Span::styled(
            text.to_string(),
            Style::new().fg(dim_color()).add_modifier(Modifier::BOLD),
        ))
    };
    let muted = |text: String| Line::from(Span::styled(text, Style::new().fg(muted_color())));
    let mut lines: Vec<Line> = Vec::new();
    match &item {
        Item::Workspace(space) => {
            lines.push(title("WORKSPACE"));
            if let Some(folder) = &space.folder {
                lines.push(muted(fit(&rows::home(folder), width)));
            }
            lines.push(Line::raw(""));
            let tabs = app
                .model
                .workspace_tabs
                .get(&space.workspace.workspace_id)
                .cloned()
                .unwrap_or_default();
            lines.extend(tab_lines(&tabs, width, None));
        }
        Item::Agent(_) | Item::Tab(_) | Item::Pane(_) => {
            lines.push(title("PANE"));
            let text = app.preview_text(height.max(10)).unwrap_or_default();
            let content: Vec<&str> = text.trim_end().lines().collect();
            let start = content.len().saturating_sub(height.saturating_sub(1));
            for text_line in &content[start..] {
                lines.push(muted(fit(text_line, width)));
            }
        }
        Item::Action(action) => {
            lines.push(title("ACTION"));
            lines.push(muted(format!(
                "{} · {}",
                action.plugin_id, action.action_id
            )));
            lines.push(Line::raw(""));
            for part in wrap(&action.description, width) {
                lines.push(Line::raw(part));
            }
        }
        Item::Choice(Choice::Existing {
            branch, worktree, ..
        }) if !creating(app) => {
            let views = if app.config.preview == crate::config::Preview::Summary {
                vec![Span::styled(
                    "SUMMARY",
                    Style::new().fg(dim_color()).add_modifier(Modifier::BOLD),
                )]
            } else {
                let mut spans = Vec::new();
                for (index, name) in ["DIFF", "LOG", "CHANGES"].iter().enumerate() {
                    let style = if index == app.view % 3 {
                        Style::new().fg(accent_color()).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(dim_color())
                    };
                    spans.push(Span::styled(*name, style));
                    spans.push(Span::styled(" · ", Style::new().fg(rule_color())));
                }
                spans.pop();
                spans.push(Span::styled("  ⌃← ⌃→", Style::new().fg(match_color())));
                spans
            };
            lines.push(Line::from(views));
            let text = app.branch_view(branch, worktree.as_ref());
            for text_line in text.lines().take(height.saturating_sub(1)) {
                lines.push(muted(fit(text_line, width)));
            }
        }
        Item::Create(name) => {
            lines.push(title("CREATE"));
            match app.current_place() {
                Some(place) => {
                    lines.push(Line::from(round(vec![pill_span(true, &place.label)], app)));
                    lines.push(Line::raw(""));
                    let text = format!(
                        "↵ creates “{name}” in the current place. Then choose a new worktree, \
                         a new tab, or another place."
                    );
                    for part in wrap(&text, width) {
                        lines.push(Line::from(Span::styled(part, Style::new().fg(dim_color()))));
                    }
                }
                None => {
                    let text = format!("↵ creates “{name}”. Then choose its place.");
                    for part in wrap(&text, width) {
                        lines.push(Line::from(Span::styled(part, Style::new().fg(dim_color()))));
                    }
                }
            }
        }
        Item::Place(place) => {
            lines.push(title("THE PLACE"));
            lines.push(Line::from(round(
                vec![pill_span(place.kind == Kind::Workspace, &place.label)],
                app,
            )));
            lines.push(muted(fit(&rows::home(&place.folder), width)));
            lines.push(Line::raw(""));
            if place.tabs.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    no tab yet",
                    Style::new().fg(dim_color()),
                )));
            } else {
                lines.extend(tab_lines(&place.tabs, width, None));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "↵ chooses this place. Then type the name of the tab or of the branch.",
                Style::new().fg(dim_color()),
            )));
        }
        Item::Choice(choice) => {
            lines.push(title("WHERE IT GOES"));
            let agent = app.agent();
            let name = app.create.name.trim().to_string();
            let tab_name = if name.is_empty() {
                "(auto name)".to_string()
            } else {
                name.clone()
            };
            let place = app.target_place().cloned();
            match (choice, place) {
                (Choice::NewTab, Some(place)) => {
                    lines.push(Line::from(round(vec![pill_span(true, &place.label)], app)));
                    lines.extend(tab_lines(&place.tabs, width, Some((&tab_name, &agent))));
                }
                (Choice::NewWorkspace, Some(place)) => {
                    lines.push(Line::from(round(vec![pill_span(false, &place.label)], app)));
                    lines.push(muted(fit(&rows::home(&place.folder), width)));
                    lines.extend(tab_lines(&[], width, Some((&tab_name, &agent))));
                }
                (Choice::NewWorktree { branch }, Some(place)) if create::is_branch_name(branch) => {
                    lines.extend(project_lines(app, &place, branch, &agent, width));
                }
                (Choice::NewWorktree { branch }, _) if !branch.is_empty() => {
                    lines.push(Line::from(Span::styled(
                        format!("“{branch}” is not a branch name: choose + new tab."),
                        Style::new().fg(red_color()),
                    )));
                }
                (Choice::NewWorktree { .. }, _) => {
                    lines.push(Line::from(Span::styled(
                        "Type the branch name: wt creates the branch, its worktree and its workspace.",
                        Style::new().fg(dim_color()),
                    )));
                }
                (
                    Choice::Existing {
                        branch,
                        worktree,
                        workspace_id,
                    },
                    _,
                ) => {
                    let what = match (workspace_id, worktree) {
                        (Some(_), _) => "Its workspace is open: ↵ goes there.",
                        (None, Some(_)) => "Its worktree has no workspace: ↵ opens it.",
                        (None, None) => "The branch has no worktree: ↵ creates it with wt switch.",
                    };
                    lines.push(muted(format!("branch {branch}")));
                    if let Some(folder) = worktree {
                        lines.push(muted(fit(&rows::home(folder), width)));
                    }
                    lines.push(Line::raw(""));
                    lines.push(Line::from(Span::styled(what, Style::new().fg(dim_color()))));
                }
                _ => lines.push(Line::from(Span::styled(
                    "Choose a project, one of its workspaces, or a folder. The name stays.",
                    Style::new().fg(dim_color()),
                ))),
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The lines of the tabs of a workspace, with a new tab in green at the end if `new` gives its
/// name and its agent.
fn tab_lines(tabs: &[TabSummary], width: usize, new: Option<(&str, &str)>) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let mut spans = vec![
                Span::styled(format!("{:>3} ", index + 1), Style::new().fg(dim_color())),
                Span::styled(
                    fit(&tab.name, width.saturating_sub(18)),
                    Style::new().fg(muted_color()),
                ),
            ];
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
            Line::from(spans)
        })
        .collect();
    if let Some((name, agent)) = new {
        lines.push(Line::from(Span::styled(
            format!(
                "+{:>2} {}  {agent}",
                tabs.len() + 1,
                fit(name, width.saturating_sub(18))
            ),
            Style::new().fg(green_color()),
        )));
    }
    lines
}

/// A new worktree in its project: the open workspaces of the project, and the new one in green.
fn project_lines(
    app: &App,
    place: &Place,
    branch: &str,
    agent: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("project ", Style::new().fg(dim_color())),
        Span::raw(place.project.clone()),
    ])];
    // The open workspaces of the project: the main checkout, then "▸ branch" for a worktree.
    let column = 24.min(width.saturating_sub(12));
    for other in app
        .open
        .iter()
        .filter(|other| other.main == place.main && other.main.is_some())
    {
        let name = match other.label.split_once(" ▸ ") {
            Some((_, worktree)) => format!("▸ {worktree}"),
            None => other.label.clone(),
        };
        lines.push(Line::from(vec![
            Span::styled("  ▣ ", Style::new().fg(accent_color())),
            Span::styled(
                pad(&fit(&name, column), column),
                Style::new().fg(muted_color()),
            ),
            Span::styled(
                format!(" {}", tabs_text(other.tabs.len())),
                Style::new().fg(dim_color()),
            ),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("+ ▣ ", Style::new().fg(green_color())),
        Span::styled(
            pad(&fit(&format!("▸ {branch}"), column), column),
            Style::new().fg(green_color()),
        ),
        Span::styled(
            fit(
                &format!(" new workspace · 1 tab {agent}"),
                width.saturating_sub(column + 4),
            ),
            Style::new().fg(green_color()),
        ),
    ]));
    if let Some(main) = &place.main {
        // The folder of wt by default: <main>.<branch>, with - in place of /.
        let folder = format!("{}.{}", rows::home(main), branch.replace('/', "-"));
        let base = app.base_branch().unwrap_or("the default branch");
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("worktree ", Style::new().fg(dim_color())),
            Span::styled(
                fit(&folder, width.saturating_sub(9)),
                Style::new().fg(muted_color()),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("branch   ", Style::new().fg(dim_color())),
            Span::styled(
                fit(
                    &format!("{branch} (new, from {base})"),
                    width.saturating_sub(9),
                ),
                Style::new().fg(muted_color()),
            ),
        ]));
    }
    lines
}
