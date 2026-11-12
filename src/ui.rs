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
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Confirm, Step};
use crate::create::{self, Choice};
use crate::filter::Filter;
use crate::model::{Status, TabSummary};
use crate::places::{Kind, Place};
use crate::rows::{self, Item, Line as ListLine};
use crate::theme;

// The colors come from the herdr theme (see theme.rs). Each role of the omnibar takes a color of
// the palette of herdr, like the mockup on the theme dracula.

/// The prompt, the pointer of the selected row, the current filter, an open workspace.
fn accent_color() -> Color {
    theme::palette().accent
}

/// The text on the accent: the current filter.
fn on_accent_color() -> Color {
    match theme::palette().panel_bg {
        Color::Reset => Color::Black,
        color => color,
    }
}

/// The letters of the query in a name, and the keys of the footer.
fn match_color() -> Color {
    theme::palette().teal
}

/// A creation: a new tab, a new worktree, a new workspace.
fn green_color() -> Color {
    theme::palette().green
}

/// A name that cannot be a branch, and the flag without prompts.
fn red_color() -> Color {
    theme::palette().red
}

/// The secondary text: the labels of the footer.
fn muted_color() -> Color {
    theme::palette().overlay1
}

/// The titles of the groups, the context of a row, the text that helps.
fn dim_color() -> Color {
    theme::palette().overlay0
}

/// The rules.
fn rule_color() -> Color {
    theme::palette().surface0
}

/// The background of the selected row.
fn selected_color() -> Color {
    theme::palette().active_row_bg
}

/// The background of a place on the action line.
fn pill_color() -> Color {
    theme::palette().surface0
}

/// The colors of the agents and of the states.
fn orange_color() -> Color {
    theme::palette().peach
}

fn pink_color() -> Color {
    theme::palette().mauve
}

fn yellow_color() -> Color {
    theme::palette().yellow
}

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

/// Draw the search line. Return the area of each chip, for the mouse.
fn draw_search(frame: &mut Frame, area: Rect, app: &App) -> Vec<(Rect, Filter)> {
    // A narrow popup shows only the chip of the current filter.
    let all_width: u16 = app
        .filters
        .iter()
        .map(|filter| filter.label().len() as u16 + 3)
        .sum();
    let all_fit = area.width > all_width + 30;
    let shown: Vec<Filter> = app
        .filters
        .iter()
        .copied()
        .filter(|filter| all_fit || *filter == app.filter)
        .collect();
    let chips: Vec<Span> = shown
        .iter()
        .flat_map(|filter| {
            let style = if *filter == app.filter {
                Style::new()
                    .fg(on_accent_color())
                    .bg(accent_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(dim_color())
            };
            [
                Span::styled(format!(" {} ", filter.label()), style),
                Span::raw(" "),
            ]
        })
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
                Filter::Worktrees => "branch: go to it, or type a new name to create it",
                Filter::Folders => "project or folder",
                Filter::Actions => "action or plugin",
                Filter::Panes => "pane, tab or workspace",
                _ => "search · @ agents · # folders · + new · > actions",
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
    frame.render_widget(Paragraph::new(Line::from(spans)), field);
    let mut areas = Vec::new();
    if show_chips {
        let mut x = chips_area.x;
        for filter in &shown {
            let width = filter.label().width() as u16 + 2;
            areas.push((Rect::new(x, chips_area.y, width, 1), *filter));
            x += width + 1;
        }
        frame.render_widget(Paragraph::new(Line::from(chips)), chips_area);
    }
    let cursor = field.x + (before + text.width()) as u16;
    frame.set_cursor_position((cursor.min(field.right().saturating_sub(1)), field.y));
    areas
}

fn crumb(place: Option<&str>) -> Span<'static> {
    let text = match place {
        Some(place) => format!(" new › {} ", fit(place, 30)),
        None => " new ".to_string(),
    };
    Span::styled(text, Style::new().fg(muted_color()).bg(pill_color()))
}

/// The space after the crumb.
fn gap() -> Span<'static> {
    Span::raw(" ")
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

/// The kind of an agent. With running times, it has a fixed width: the times make a column.
fn kind_text(kind: &str, app: &App) -> String {
    if app.times.is_some() {
        format!("{kind:<8}")
    } else {
        kind.to_string()
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

fn tabs_text(count: usize) -> String {
    format!("{count} tab{}", if count == 1 { "" } else { "s" })
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
        .create
        .place
        .as_ref()
        .filter(|_| creating(app))
        .or_else(|| app.current_place())
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
                .open
                .iter()
                .find(|place| place.workspace_id.as_deref() == Some(id.as_str()))
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

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
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
    // The widths of the columns: the same for each row.
    let right_width = app
        .lines
        .iter()
        .filter_map(|line| match line {
            ListLine::Row { item, .. } => Some(spans_width(&right_side(item, app))),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .min(width / 3);
    let room = width.saturating_sub(4 + right_width + 4);
    let name_width = app
        .lines
        .iter()
        .filter_map(|line| match line {
            ListLine::Row { item, .. } => Some(display_name(item, app).width()),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .min(room * 3 / 5)
        .max(8.min(room));
    let context_width = room.saturating_sub(name_width);
    let offset = app.list_offset;
    let lines: Vec<Line> = app
        .lines
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(index, line)| match line {
            ListLine::Gap => Line::raw(""),
            ListLine::Group(title) => Line::from(Span::styled(
                format!(" {}", title.to_uppercase()),
                Style::new().fg(dim_color()).add_modifier(Modifier::BOLD),
            )),
            ListLine::Row { item, positions } => {
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
                spans.extend(highlighted(
                    &display_name(item, app),
                    positions,
                    name_width,
                    name_style(item),
                ));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    pad(&fit(&context_of(item, app), context_width), context_width),
                    Style::new().fg(dim_color()),
                ));
                spans.push(Span::raw("  "));
                spans.extend(fit_spans(right_side(item, app), right_width));
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

fn draw_preview(frame: &mut Frame, area: Rect, app: &mut App) {
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
                    lines.push(Line::from(pill_span(true, &place.label)));
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
            lines.push(Line::from(pill_span(
                place.kind == Kind::Workspace,
                &place.label,
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
            let place = app
                .create
                .place
                .clone()
                .or_else(|| app.current_place().cloned());
            match (choice, place) {
                (Choice::NewTab, Some(place)) => {
                    lines.push(Line::from(pill_span(true, &place.label)));
                    lines.extend(tab_lines(&place.tabs, width, Some((&tab_name, &agent))));
                }
                (Choice::NewWorkspace, Some(place)) => {
                    lines.push(Line::from(pill_span(false, &place.label)));
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

/// A place on the action line: violet for an open workspace, green for a new one.
fn pill_span(open: bool, label: &str) -> Span<'static> {
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

fn draw_action(frame: &mut Frame, area: Rect, app: &App) {
    let arrow =
        |color: Color| Span::styled("→ ", Style::new().fg(color).add_modifier(Modifier::BOLD));
    if let Some(confirm) = &app.confirm {
        let question = match confirm {
            Confirm::CloseTab { name, .. } => format!("close the tab {name}? ↵ yes · esc no"),
            Confirm::RemoveWorktree { branch, agents, .. } => {
                let agents = match agents {
                    0 => String::new(),
                    1 => " Its agent closes.".to_string(),
                    count => format!(" Its {count} agents close."),
                };
                // The notification of the result tells if the action restore of herdr-worktrunk
                // can bring the worktree back: wt alone keeps no record.
                format!("remove the worktree {branch}?{agents} ↵ yes · esc no")
            }
        };
        let line = Line::from(vec![
            arrow(red_color()),
            Span::styled(question, Style::new().fg(red_color())),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    if let Some(rename) = &app.rename {
        let line = Line::from(vec![
            arrow(accent_color()),
            Span::raw(format!(
                "rename the tab “{}” to “{}”",
                rename.old,
                rename.text.trim()
            )),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    if let Some(message) = &app.message {
        let line = Line::from(vec![
            arrow(dim_color()),
            Span::styled(message.clone(), Style::new().fg(muted_color())),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    let Some(item) = app.selected_item() else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "→ nothing selected",
                Style::new().fg(dim_color()),
            )),
            area,
        );
        return;
    };
    let name = app.create.name.trim();
    let tab = if name.is_empty() {
        "new tab (auto name)".to_string()
    } else {
        format!("new tab “{name}”")
    };
    let place = app
        .create
        .place
        .clone()
        .or_else(|| app.current_place().cloned());
    let place_label = place
        .as_ref()
        .map(|place| place.label.clone())
        .unwrap_or_default();
    // The verb, the place (open or new), and the agent field for a creation.
    let (verb, where_, agent_field, warning): (String, Option<Span>, bool, bool) = match item {
        Item::Agent(agent) => (
            format!("go to the agent {}", agent.name),
            Some(pill_span(true, &agent.place)),
            false,
            false,
        ),
        Item::Tab(tab_row) => (
            format!("go to the tab {}", tab_row.name),
            Some(pill_span(true, &tab_row.place)),
            false,
            false,
        ),
        Item::Pane(pane) => (
            if pane.name == pane.tab {
                format!("go to the pane {}", pane.name)
            } else {
                format!("go to the pane {} of the tab {}", pane.name, pane.tab)
            },
            Some(pill_span(true, &pane.place)),
            false,
            false,
        ),
        Item::Workspace(space) => (
            "go to".to_string(),
            Some(pill_span(true, &space.label())),
            false,
            false,
        ),
        Item::Action(action) => (
            format!("run the action {}", action.title),
            None,
            false,
            false,
        ),
        Item::Place(place) => (
            "create".to_string(),
            Some(pill_span(place.kind == Kind::Workspace, &place.label)),
            false,
            false,
        ),
        Item::Create(name) => (
            format!("create “{name}”: choose a tab or a worktree"),
            place.as_ref().map(|place| pill_span(true, &place.label)),
            false,
            false,
        ),
        Item::Choice(choice) => match choice {
            Choice::NewWorktree { branch } if branch.is_empty() => (
                "type the branch name of the new worktree".into(),
                None,
                false,
                true,
            ),
            Choice::NewWorktree { branch } if !create::is_branch_name(branch) => (
                format!("“{branch}” is not a branch name"),
                None,
                false,
                true,
            ),
            Choice::NewWorktree { branch } => (
                format!("new worktree {branch}"),
                Some(pill_span(
                    false,
                    &format!(
                        "{} ▸ {branch}",
                        place
                            .as_ref()
                            .map(|place| place.project.as_str())
                            .unwrap_or("")
                    ),
                )),
                true,
                false,
            ),
            Choice::Existing {
                branch,
                workspace_id: Some(id),
                ..
            } => {
                let label = app
                    .open
                    .iter()
                    .find(|open| open.workspace_id.as_deref() == Some(id.as_str()))
                    .map(|open| open.label.clone())
                    .unwrap_or_else(|| worktree_label(place.as_ref(), branch));
                (
                    format!("go to the worktree {branch}"),
                    Some(pill_span(true, &label)),
                    false,
                    false,
                )
            }
            Choice::Existing { branch, .. } => (
                format!("open the worktree {branch}"),
                Some(pill_span(false, &worktree_label(place.as_ref(), branch))),
                true,
                false,
            ),
            Choice::NewTab => (tab, Some(pill_span(true, &place_label)), true, false),
            Choice::NewWorkspace => (tab, Some(pill_span(false, &place_label)), true, false),
            Choice::AnotherPlace => ("choose another place".into(), None, false, false),
        },
    };
    let arrow_color = if warning { red_color() } else { accent_color() };
    let mut spans = vec![
        Span::styled(
            "→ ",
            Style::new().fg(arrow_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            verb,
            Style::new().fg(if warning { red_color() } else { Color::Reset }),
        ),
    ];
    if let Some(pill) = where_ {
        spans.push(Span::styled("  in ", Style::new().fg(dim_color())));
        spans.push(pill);
    }
    if agent_field {
        let agent = app.agent();
        let mut field = vec![
            Span::styled("agent ", Style::new().fg(dim_color())),
            Span::styled(
                format!(" {agent} "),
                Style::new().fg(agent_color(&agent)).bg(pill_color()),
            ),
        ];
        let key = if creating(app) { " ⇥" } else { " ⌃A" };
        field.push(Span::styled(key, Style::new().fg(match_color())));
        if app.create.no_prompts && agent != "shell" {
            field.push(Span::styled("  prompts off", Style::new().fg(red_color())));
        }
        let used = spans_width(&spans) + spans_width(&field);
        spans.push(Span::raw(
            " ".repeat((area.width as usize).saturating_sub(used)),
        ));
        spans.extend(field);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The label of the workspace of a worktree: "project ▸ branch".
fn worktree_label(place: Option<&Place>, branch: &str) -> String {
    match place {
        Some(place) => format!("{} ▸ {branch}", place.project),
        None => branch.to_string(),
    }
}

/// Draw the footer. Return the area of each key, with its key, for the mouse.
fn draw_footer(frame: &mut Frame, area: Rect, app: &App) -> Vec<(Rect, String)> {
    // ctrl+Enter names the workspace of its new tab.
    let new_tab = match app.new_tab_place() {
        Some(label) => format!("new tab in {}", fit(&label, 28)),
        None => "new tab here".to_string(),
    };
    let what = creating(app) && app.create.step == Step::What;
    let path = creating(app) && app.create.step == Step::Where && rows::is_path(&app.create.filter);
    let hints: Vec<(&str, String)> = if app.confirm.is_some() {
        vec![("↵", "yes".to_string()), ("esc", "no".to_string())]
    } else if app.rename.is_some() {
        vec![("↵", "save".to_string()), ("esc", "keep".to_string())]
    } else if what {
        let selected = app.selected_item();
        let primary = match selected {
            Some(Item::Choice(Choice::AnotherPlace)) => "place",
            Some(Item::Choice(Choice::Existing {
                workspace_id: Some(_),
                ..
            })) => "go",
            _ => "create",
        };
        let prompts = if app.create.no_prompts {
            "prompts on"
        } else {
            "prompts off"
        };
        vec![
            ("↵", primary.to_string()),
            ("⇥", "agent".to_string()),
            ("⌃Y", prompts.to_string()),
            ("esc", "place".to_string()),
        ]
    } else if creating(app) {
        vec![
            ("↵", "choose".to_string()),
            ("⇥", if path { "complete" } else { "filter" }.to_string()),
            ("⌃O", "preview".to_string()),
            ("esc", "close".to_string()),
        ]
    } else {
        match app.selected_item() {
            Some(Item::Agent(_) | Item::Tab(_)) => vec![
                ("↵", "go".to_string()),
                ("⌃↵", new_tab.clone()),
                ("⇥", "filter".to_string()),
                ("⌃⌫", "close tab".to_string()),
                ("esc", "close".to_string()),
            ],
            Some(Item::Workspace(_)) => vec![
                ("↵", "go".to_string()),
                ("⌃↵", new_tab.clone()),
                ("⇥", "filter".to_string()),
                ("⌃O", "preview".to_string()),
                ("esc", "close".to_string()),
            ],
            selected => {
                let primary = match selected {
                    Some(Item::Place(_)) => "create in",
                    Some(Item::Action(_)) => "run",
                    Some(Item::Create(_)) => "create…",
                    Some(Item::Choice(Choice::Existing {
                        workspace_id: Some(_),
                        ..
                    })) => "go",
                    Some(Item::Choice(Choice::NewWorktree { .. })) => "create",
                    Some(Item::Choice(_)) => "open",
                    _ => "go",
                };
                let mut hints = vec![("↵", primary.to_string())];
                if app.adds_tab_here() {
                    hints.push(("⌃↵", new_tab.clone()));
                }
                if app.opens_worktree() {
                    hints.push(("⌃A", "agent".to_string()));
                }
                hints.push(("⇥", "filter".to_string()));
                // A short footer: ⌃O shows only when the row has no ⌃↵, no ⌃A and no ⌃⌫.
                if app.can_remove_worktree() {
                    hints.push(("⌃⌫", "remove".to_string()));
                } else if hints.len() < 3 {
                    hints.push(("⌃O", "preview".to_string()));
                }
                hints.push(("esc", "close".to_string()));
                hints
            }
        }
    };
    let mut spans = Vec::new();
    let mut areas = Vec::new();
    let mut x = area.x;
    for (key, label) in hints {
        let text = format!(" {label}   ");
        let width = (key.width() + label.width() + 1) as u16;
        areas.push((Rect::new(x, area.y, width, 1), key.to_string()));
        x += (key.width() + text.width()) as u16;
        spans.push(Span::styled(
            key,
            Style::new().fg(match_color()).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(text, Style::new().fg(muted_color())));
    }
    let help = vec![
        Span::styled(
            "?",
            Style::new().fg(match_color()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" help", Style::new().fg(muted_color())),
    ];
    let used = spans_width(&spans) + spans_width(&help);
    spans.push(Span::raw(
        " ".repeat((area.width as usize).saturating_sub(used)),
    ));
    spans.extend(help);
    if used <= area.width as usize {
        areas.push((Rect::new(area.right() - 6, area.y, 6, 1), "?".to_string()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    areas
}

fn draw_help(frame: &mut Frame, area: Rect) {
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
            "@ # + > %",
            "On an empty text: agents, folders, creation, actions, panes. Again: All.",
        ),
        (
            "~  /",
            "In the step where: the folders of the disk. ⇥ completes.",
        ),
        ("⌃A", "On a row that opens a worktree: the next agent."),
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
            "Close the tab of the selected row, or remove its worktree, after a question.",
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

/// The color of an agent kind.
fn agent_color(kind: &str) -> Color {
    match kind {
        "claude" => orange_color(),
        "codex" => match_color(),
        "opencode" => pink_color(),
        _ => muted_color(),
    }
}

/// The color of a status, like the glyphs of herdr.
fn status_color(status: Status) -> Color {
    match status {
        Status::Blocked => pink_color(),
        Status::Working => yellow_color(),
        Status::Done => green_color(),
        Status::Idle => dim_color(),
        Status::Unknown => rule_color(),
    }
}

/// A name with the letters of the query in color, cut and padded to a width.
fn highlighted(name: &str, positions: &[usize], width: usize, base: Style) -> Vec<Span<'static>> {
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
fn wrap(text: &str, width: usize) -> Vec<String> {
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
    fn a_description_wraps_at_the_spaces() {
        assert_eq!(
            wrap("open the cleaner of the worktrees", 16),
            ["open the cleaner", "of the worktrees"]
        );
    }

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

    #[test]
    fn the_spans_are_cut_to_the_width() {
        let spans = fit_spans(vec![Span::raw("claude"), Span::raw(" needs you")], 10);
        assert_eq!(spans_width(&spans), 10);
    }
}
