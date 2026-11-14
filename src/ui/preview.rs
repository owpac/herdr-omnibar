//! The preview of the selected row, on the right of the list.

use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use std::path::{Path, PathBuf};

use crate::app::{App, View};
use crate::create::{self, Choice};
use crate::herdr::PluginAction;
use crate::model::{Space, TabSummary};
use crate::places::{self, Kind, Place};
use crate::pulls::Lookup;
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
    // The preview of a pane or of a branch reads herdr or git, and keeps the text in the app.
    let Some(item) = app.selected_item().cloned() else {
        return;
    };
    let width = inner.width as usize;
    let height = inner.height as usize;
    let lines = match &item {
        Item::Workspace(space) => workspace_lines(app, space, width),
        Item::Agent(_) | Item::Tab(_) | Item::Pane(_) => pane_lines(app, width, height),
        Item::Action(action) => action_lines(action, width),
        Item::Choice(Choice::Existing {
            branch,
            worktree,
            main,
            ..
        }) if !creating(app) => branch_lines(app, main, branch, worktree.as_ref(), width, height),
        Item::Create(name) => create_lines(app, name, width),
        Item::NewHomeWorkspace(_) => new_workspace_lines(app, width),
        Item::Place(place) => place_lines(app, place, width),
        Item::Choice(choice) => choice_lines(app, choice, width),
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The title of a part of the preview.
fn title(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::new().fg(dim_color()).add_modifier(Modifier::BOLD),
    ))
}

/// A line of secondary text.
fn muted(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::new().fg(muted_color())))
}

/// A line of text that helps, cut into lines of the width.
fn help_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    wrap(text, width)
        .into_iter()
        .map(|part| Line::from(Span::styled(part, Style::new().fg(dim_color()))))
        .collect()
}

/// A workspace: its folder and its tabs.
fn workspace_lines(app: &App, space: &Space, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![title("WORKSPACE")];
    if let Some(folder) = &space.folder {
        lines.push(muted(fit(&rows::home(folder), width)));
    }
    lines.push(Line::raw(""));
    let tabs = app
        .model
        .workspace_tabs
        .get(&space.workspace.workspace_id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    lines.extend(tab_lines(tabs, width, None));
    lines
}

/// An agent, a tab or a pane: the last lines of its screen.
fn pane_lines(app: &mut App, width: usize, height: usize) -> Vec<Line<'static>> {
    let mut lines = vec![title("PANE")];
    let text = app.preview_text(height.max(10)).unwrap_or_default();
    let content: Vec<&str> = text.trim_end().lines().collect();
    let start = content.len().saturating_sub(height.saturating_sub(1));
    lines.extend(
        content[start..]
            .iter()
            .map(|text_line| muted(fit(text_line, width))),
    );
    lines
}

/// An action of a plugin: its plugin, its id and its description.
fn action_lines(action: &PluginAction, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        title("ACTION"),
        muted(format!("{} · {}", action.plugin_id, action.action_id)),
        Line::raw(""),
    ];
    lines.extend(wrap(&action.description, width).into_iter().map(Line::raw));
    lines
}

/// A branch of the current project: the view of its diff, its log or its changes.
fn branch_lines(
    app: &mut App,
    main: &Path,
    branch: &str,
    worktree: Option<&PathBuf>,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let views = app.views();
    let current = app.current_view();
    // The titles of the views: the current view in the accent color. A single view (the summary)
    // has no key to change it.
    let mut titles = Vec::new();
    for view in views {
        let style = match (views.len(), *view == current) {
            (1, _) => Style::new().fg(dim_color()).add_modifier(Modifier::BOLD),
            (_, true) => Style::new().fg(accent_color()).add_modifier(Modifier::BOLD),
            _ => Style::new().fg(dim_color()),
        };
        titles.push(Span::styled(view.title(), style));
        titles.push(Span::styled(" · ", Style::new().fg(rule_color())));
    }
    titles.pop();
    if views.len() > 1 {
        titles.push(Span::styled("  ⌃← ⌃→", Style::new().fg(match_color())));
    }
    let mut lines = vec![Line::from(titles)];
    let pull = app.pull(main, branch);
    // The view PR (feature pull_requests) comes from gh, not from git.
    if current == View::Pull {
        if let Some(pull) = &pull {
            lines.extend(super::pull::lines(pull, width));
        }
        return lines;
    }
    // The summary starts with the pull request, when the branch has one.
    if let (View::Summary, Some(found @ Lookup::Found(_))) = (current, &pull) {
        lines.extend(super::pull::lines(found, width).into_iter().take(1));
    }
    let text = app.view_text(main, branch, worktree, current);
    lines.extend(
        text.lines()
            .take(height.saturating_sub(1))
            .map(|text_line| muted(fit(text_line, width))),
    );
    lines
}

/// The row "create": the creation in the current place.
fn create_lines(app: &App, name: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![title("CREATE")];
    match app.current_place() {
        Some(place) => {
            lines.push(Line::from(round(vec![pill_span(true, &place.label)], app)));
            lines.push(Line::raw(""));
            lines.extend(help_lines(
                &format!(
                    "↵ creates “{name}” in the current place. Then choose a new worktree, a new \
                     tab, or another place."
                ),
                width,
            ));
        }
        None => lines.extend(help_lines(
            &format!("↵ creates “{name}”. Then choose its place."),
            width,
        )),
    }
    lines
}

/// The head of the preview of a place: the title, the pill of the place, its folder, then an empty
/// line.
fn place_head(
    app: &App,
    heading: &str,
    pill: Span<'static>,
    folder: &Path,
    width: usize,
) -> Vec<Line<'static>> {
    vec![
        title(heading),
        Line::from(round(vec![pill], app)),
        muted(fit(&rows::home(folder), width)),
        Line::raw(""),
    ]
}

/// A place of the step "where": its workspace and its tabs.
fn place_lines(app: &App, place: &Place, width: usize) -> Vec<Line<'static>> {
    let pill = pill_span(place.kind == Kind::Workspace, &place.label);
    let mut lines = place_head(app, "THE PLACE", pill, &place.folder, width);
    if place.tabs.is_empty() {
        lines.push(Line::from(Span::styled(
            "    no tab yet",
            Style::new().fg(dim_color()),
        )));
    } else {
        lines.extend(tab_lines(&place.tabs, width, None));
    }
    lines.push(Line::raw(""));
    lines.extend(help_lines(
        "↵ chooses this place. Then type the name of the tab or of the branch.",
        width,
    ));
    lines
}

/// The row "new workspace…": a workspace in ~, with the typed text as its label.
fn new_workspace_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let pill = pill_span(false, "~");
    let mut lines = place_head(app, "NEW WORKSPACE", pill, &places::home_folder(), width);
    lines.extend(help_lines(
        "↵ creates a workspace in ~. Its label is the text that you typed.",
        width,
    ));
    lines
}

/// A choice of the step "what", or a worktree row: where the creation goes.
fn choice_lines(app: &App, choice: &Choice, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![title("WHERE IT GOES")];
    let agent = app.agent();
    let name = app.create.name.trim();
    let tab_name = if name.is_empty() { "(auto name)" } else { name };
    match (choice, app.target_place()) {
        (Choice::NewTab, Some(place)) => {
            lines.push(Line::from(round(vec![pill_span(true, &place.label)], app)));
            lines.extend(tab_lines(&place.tabs, width, Some((tab_name, &agent))));
        }
        (Choice::NewSplit { .. }, Some(place)) => {
            lines.push(Line::from(round(vec![pill_span(true, &place.label)], app)));
            let text = format!(
                "A new pane next to the current pane, with {agent}: to the right of a wide pane, \
                 below a tall pane."
            );
            lines.extend(wrap(&text, width).into_iter().map(muted));
        }
        (Choice::NewWorkspace | Choice::LabelledWorkspace, Some(place)) => {
            // The names of the plan: with a label, the typed name is the label.
            let labelled = *choice == Choice::LabelledWorkspace;
            let (label, first_tab) = create::new_workspace_names(place, name, labelled);
            let first_tab = if first_tab.is_empty() {
                "(auto name)"
            } else {
                &first_tab
            };
            lines.push(Line::from(round(vec![pill_span(false, &label)], app)));
            lines.push(muted(fit(&rows::home(&place.folder), width)));
            lines.extend(tab_lines(&[], width, Some((first_tab, &agent))));
        }
        (Choice::NewWorktree { branch }, Some(place)) if create::is_branch_name(branch) => {
            lines.extend(project_lines(app, place, branch, &agent, width));
        }
        (Choice::NewWorktree { branch }, _) if !branch.is_empty() => {
            lines.push(Line::from(Span::styled(
                format!("“{branch}” is not a branch name: choose + new tab."),
                Style::new().fg(red_color()),
            )));
        }
        (Choice::NewWorktree { .. }, _) => lines.extend(help_lines(
            "Type the branch name: wt creates the branch, its worktree and its workspace.",
            width,
        )),
        (
            Choice::Existing {
                branch,
                worktree,
                workspace_id,
                ..
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
            lines.extend(help_lines(what, width));
        }
        _ => lines.extend(help_lines(
            "Choose a project, one of its workspaces, or a folder. The name stays.",
            width,
        )),
    }
    lines
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::app;
    use crate::app::Step;
    use crate::filter::Filter;

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_preview_of_a_new_workspace_shows_the_typed_name_as_its_label() {
        let mut app = app(Filter::New);
        app.create.step = Step::What;
        app.create.place = Some(Place::sample("atlas", Some("w1"), None));
        app.create.name = "my label".into();
        let lines = text(&choice_lines(&app, &Choice::LabelledWorkspace, 60));
        // The pill holds the label, and the automatic titles name the first tab.
        assert!(
            lines.iter().any(|line| line.contains("my label")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("(auto name)")),
            "{lines:?}"
        );
        // A place without a workspace keeps the name for its first tab.
        app.create.place = Some(Place::sample("prompts", None, Some("/work/prompts")));
        let lines = text(&choice_lines(&app, &Choice::NewWorkspace, 60));
        assert!(
            lines.iter().any(|line| line.contains("prompts")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("(auto name)")),
            "{lines:?}"
        );
    }
}
