//! The input of the popup: the keys, the mouse, Enter, and the questions of the action line.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::create::{self, Choice, NAME_MISSING};
use crate::filter::Filter;
use crate::herdr;
use crate::model::{AgentRow, Status};
use crate::order;
use crate::remove;
use crate::rows::{self, Item, Line};
use crate::watch;

use super::{App, Confirm, Create, Outcome, Rename, RenameTarget, Step};

impl App {
    /// Enter on the selected row.
    fn enter(&mut self) -> Outcome {
        let Some(item) = self.selected_item().cloned() else {
            return Outcome::Continue;
        };
        match (self.filter, item) {
            // The row "new workspace…": the typed text is the label, and ↵ creates it at once.
            (_, Item::NewWorkspace(text)) => {
                match create::new_workspace_plan(&text, &self.home_agent(), self.create.no_prompts)
                {
                    Some(plan) => Outcome::Create(plan),
                    None => {
                        self.message = Some(NAME_MISSING.to_string());
                        Outcome::Continue
                    }
                }
            }
            (_, Item::Place(place)) => {
                if self.filter != Filter::New {
                    self.filter = Filter::New;
                    self.create = Create::new();
                }
                self.choose_place(place);
                Outcome::Continue
            }
            (Filter::New, Item::Choice(Choice::AnotherPlace)) => {
                self.back_to_where();
                Outcome::Continue
            }
            (Filter::New, Item::Choice(choice)) => {
                let Some(place) = self.create.place.clone() else {
                    return Outcome::Continue;
                };
                match self.plan(&place, &choice, &self.create.name.clone()) {
                    Some(plan) => Outcome::Create(plan),
                    None => Outcome::Continue,
                }
            }
            (_, Item::Choice(choice)) => {
                // The filter Worktrees: a new worktree goes into the current project. A branch
                // goes into its own project.
                let Some(place) = self.place_of_choice(&choice).cloned() else {
                    return Outcome::Continue;
                };
                let name = match &choice {
                    Choice::NewWorktree { branch } | Choice::Existing { branch, .. } => {
                        branch.clone()
                    }
                    _ => String::new(),
                };
                match self.plan(&place, &choice, &name) {
                    Some(plan) => Outcome::Create(plan),
                    None => Outcome::Continue,
                }
            }
            (_, Item::Action(action)) => Outcome::Invoke(action),
            (_, Item::Create(name)) => {
                // The step "what" in the current place, with the query as the name.
                self.filter = Filter::New;
                self.create = Create::new();
                self.create.name = name;
                match self.current_place().cloned() {
                    Some(place) => self.choose_place(place),
                    None => self.rebuild(None),
                }
                Outcome::Continue
            }
            (_, item) => Outcome::Go(item),
        }
    }

    /// ctrl+Enter: a new tab, with its agent, next to the selected row.
    fn new_tab_here(&mut self) -> Outcome {
        let Some(place) = self.place_of_selected() else {
            return Outcome::Continue;
        };
        let agent = self.agent_for(Some(place));
        match create::plan(place, &Choice::NewTab, "", &agent, false) {
            Some(plan) => Outcome::Create(plan),
            None => Outcome::Continue,
        }
    }

    /// ctrl+F: fork the session of the selected agent into a new tab, with herdr-fork. herdr-fork
    /// forks only Claude Code and Codex.
    fn fork(&mut self) -> Outcome {
        let (tab_id, pane_id, kind) = match self.selected_item() {
            Some(Item::Agent(agent)) => (&agent.tab_id, &agent.pane_id, agent.kind.as_str()),
            Some(Item::Pane(pane)) => match &pane.agent {
                Some(kind) => (&pane.tab_id, &pane.pane_id, kind.as_str()),
                None => return Outcome::Continue,
            },
            _ => return Outcome::Continue,
        };
        if !matches!(kind, "claude" | "codex") {
            self.message = Some(format!(
                "herdr-fork forks only Claude Code and Codex, not {kind}"
            ));
            return Outcome::Continue;
        }
        let (tab_id, pane_id) = (tab_id.clone(), pane_id.clone());
        if herdr::enabled_plugin(create::FORK_PLUGIN).is_none() {
            self.message = Some("install herdr-fork to fork an agent".to_string());
            return Outcome::Continue;
        }
        Outcome::Fork { tab_id, pane_id }
    }

    /// ctrl+Backspace: ask before the close of the selected tab or pane, or before the removal of
    /// the selected worktree.
    fn ask_close(&mut self) {
        // A pane row closes its pane only: the other panes of its tab stay.
        if let Some(Item::Pane(pane)) = self.selected_item() {
            self.confirm = Some(Confirm::ClosePane {
                pane_id: pane.pane_id.clone(),
                name: pane.name.clone(),
            });
            return;
        }
        if let Some((tab_id, name)) = self.selected_tab() {
            self.confirm = Some(Confirm::CloseTab { tab_id, name });
            return;
        }
        let Some(Item::Choice(Choice::Existing {
            branch,
            worktree,
            workspace_id,
            main,
        })) = self.selected_item().cloned()
        else {
            return;
        };
        let Some(folder) = worktree else {
            self.message = Some(format!("the branch {branch} has no worktree to remove"));
            return;
        };
        if folder == main {
            self.message = Some(format!(
                "{branch} is the main checkout of the project: it stays"
            ));
            return;
        }
        // The agents of the worktree: in its workspace, or in its folder. herdr-worktrunk
        // closes the idle agents, and refuses an agent that works or waits for an answer.
        let agents: Vec<&AgentRow> = self
            .model
            .agents
            .iter()
            .filter(|agent| {
                workspace_id.as_ref() == Some(&agent.workspace_id)
                    || agent
                        .cwd
                        .as_ref()
                        .is_some_and(|cwd| cwd.starts_with(&folder))
            })
            .collect();
        if agents
            .iter()
            .any(|agent| !matches!(agent.status, Status::Idle | Status::Done))
        {
            self.message = Some(format!(
                "an agent works in the worktree {branch}, or waits for you: stop it first"
            ));
            return;
        }
        if !agents.is_empty() && remove::worktrunk_remover().is_none() {
            self.message = Some(
                "only herdr-worktrunk 2026.11.33 or later closes the agents of a worktree: \
                 update it, or stop the agents first"
                    .to_string(),
            );
            return;
        }
        self.confirm = Some(Confirm::RemoveWorktree {
            folder,
            branch,
            agents: agents.len(),
        });
    }

    fn on_confirm_key(&mut self, key: KeyEvent) -> Outcome {
        let Some(confirm) = self.confirm.take() else {
            return Outcome::Continue;
        };
        if key.code != KeyCode::Enter {
            return Outcome::Continue;
        }
        match confirm {
            Confirm::CloseTab { tab_id, name } => {
                self.message = Some(match herdr::close_tab(&tab_id) {
                    Ok(()) => format!("closed the tab {name}"),
                    Err(error) => format!("cannot close the tab {name}: {error}"),
                });
                self.reload = true;
            }
            Confirm::ClosePane { pane_id, name } => {
                self.message = Some(match herdr::close_pane(&pane_id) {
                    Ok(()) => format!("closed the pane {name}"),
                    Err(error) => format!("cannot close the pane {name}: {error}"),
                });
                self.reload = true;
            }
            // The removal can close the workspace of the popup: it runs after the popup closes.
            Confirm::RemoveWorktree { folder, .. } => return Outcome::Remove(folder),
        }
        Outcome::Continue
    }

    /// ctrl+R: rename the target of the selected row, in the search line: the tab of an agent or
    /// of a tab row, a workspace, or a pane. Other rows do nothing.
    fn start_rename(&mut self) {
        if let Some((target, name)) = self.selected_rename() {
            self.rename = Some(Rename {
                target,
                old: name.clone(),
                text: name,
            });
        }
    }

    /// True if alt+Up and alt+Down move the selected workspace: the feature move_workspaces is on,
    /// and the selected row is a workspace of the filter Workspaces. A typed text sorts the rows by
    /// the score of the match, not in the order of the sidebar: then the keys do not move.
    pub fn moves_workspace(&self) -> bool {
        self.config.has("move_workspaces")
            && self.filter == Filter::Workspaces
            && self.query.is_empty()
            && self.rename.is_none()
            && matches!(self.selected_item(), Some(Item::Workspace(_)))
    }

    /// alt+Up, alt+Down: move the selected workspace up or down in the sidebar. At the first or the
    /// last position, nothing happens. The selection stays on the workspace.
    fn move_workspace(&mut self, delta: i32) {
        let Some(Item::Workspace(space)) = self.selected_item() else {
            return;
        };
        let name = space.label();
        let Some(plan) = order::plan_move(
            &self.model.flat_workspaces,
            &space.workspace.workspace_id,
            delta,
        ) else {
            return;
        };
        self.message = Some(
            match herdr::move_workspace_block(&plan.ids, Some(&plan.before)) {
                Ok(()) => format!(
                    "moved the workspace {name} {}",
                    if delta < 0 { "up" } else { "down" }
                ),
                Err(error) => format!("cannot move the workspace {name}: {error}"),
            },
        );
        self.reload = true;
    }

    /// ctrl+T: give the tab of the selected row back to the automatic titles (feature auto_titles).
    /// The tab takes its number, then the watcher writes the title of its agent.
    fn auto_title(&mut self) {
        let Some((tab_id, name)) = self.selected_tab() else {
            return;
        };
        if !self.config.has("auto_titles") {
            self.message = Some("turn on the feature auto_titles to name a tab by itself".into());
            return;
        }
        self.message = Some(match watch::give_back(&tab_id) {
            Ok(Some(_)) => format!("the tab {name} takes the title of its agent"),
            Ok(None) => "no automatic title for this tab".to_string(),
            Err(error) => format!("cannot name the tab: {error}"),
        });
        self.reload = true;
    }

    fn finish_rename(&mut self, save: bool) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        let text = rename.text.trim();
        if save && !text.is_empty() && text != rename.old {
            let noun = rename.target.noun();
            let result = match &rename.target {
                RenameTarget::Tab(id) => herdr::rename_tab(id, text),
                RenameTarget::Workspace(id) => herdr::rename_workspace(id, text),
                RenameTarget::Pane(id) => herdr::rename_pane(id, text),
            };
            self.message = Some(match result {
                Ok(()) => format!("renamed the {noun} {} to {text}", rename.old),
                Err(error) => format!("cannot rename the {noun}: {error}"),
            });
            self.reload = true;
        }
    }

    /// A click selects a row, a double click runs it, the wheel moves the selection.
    pub fn on_mouse(&mut self, mouse: MouseEvent) -> Outcome {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.move_by(1),
            MouseEventKind::ScrollUp => self.move_by(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                let at = |area: &Rect| {
                    mouse.row >= area.y
                        && mouse.row < area.y + area.height
                        && mouse.column >= area.x
                        && mouse.column < area.x + area.width
                };
                // A key of the footer runs, as if the user pressed it.
                if let Some(key) = self
                    .hint_areas
                    .iter()
                    .find(|(area, _)| at(area))
                    .and_then(|(_, key)| key_of_hint(key))
                {
                    return self.on_key(key);
                }
                // A chip opens its filter.
                if let Some(filter) = self
                    .chip_areas
                    .iter()
                    .find(|(area, _)| at(area))
                    .map(|(_, filter)| *filter)
                {
                    if self.confirm.is_none() && self.rename.is_none() {
                        self.set_filter(filter);
                    }
                    return Outcome::Continue;
                }
                let area = self.list_area;
                let inside = mouse.row >= area.y
                    && mouse.row < area.y + area.height
                    && mouse.column >= area.x
                    && mouse.column < area.x + area.width;
                if !inside || self.confirm.is_some() || self.rename.is_some() {
                    return Outcome::Continue;
                }
                let index = self.list_offset + (mouse.row - area.y) as usize;
                if matches!(self.lines.get(index), Some(Line::Row { .. })) {
                    let double = self.last_click.is_some_and(|(line, time)| {
                        line == index && time.elapsed() < Duration::from_millis(400)
                    });
                    self.selected = Some(index);
                    self.last_click = Some((index, Instant::now()));
                    if double {
                        return self.enter();
                    }
                }
            }
            _ => {}
        }
        Outcome::Continue
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let creating = self.filter == Filter::New;
        let what = creating && self.create.step == Step::What;
        if self.help {
            self.help = false;
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1)) {
                return Outcome::Continue;
            }
        }
        if self.confirm.is_some() {
            return self.on_confirm_key(key);
        }
        self.message = None;
        let renaming = self.rename.is_some();
        if renaming && matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            self.finish_rename(key.code == KeyCode::Enter);
            return Outcome::Continue;
        }
        match key.code {
            KeyCode::Left if ctrl => {
                let count = self.views().len();
                self.view = (self.view + count - 1) % count
            }
            KeyCode::Right if ctrl => self.view = (self.view + 1) % self.views().len(),
            KeyCode::Enter if ctrl && !creating => return self.new_tab_here(),
            KeyCode::Backspace if ctrl && !renaming => self.ask_close(),
            KeyCode::Char('t') if ctrl && !creating && !renaming => self.auto_title(),
            KeyCode::Char('r') if ctrl && !creating && self.config.has("rename_tab") => {
                self.start_rename()
            }
            KeyCode::Up if alt && self.moves_workspace() => self.move_workspace(-1),
            KeyCode::Down if alt && self.moves_workspace() => self.move_workspace(1),
            KeyCode::Esc if what => self.back_to_where(),
            KeyCode::Esc => return Outcome::Quit,
            KeyCode::Char('a') if ctrl && self.opens_worktree() => self.cycle_agent(),
            KeyCode::Char('f') if ctrl && !creating => return self.fork(),
            KeyCode::Char('c' | 'g') if ctrl => return Outcome::Quit,
            KeyCode::Enter => return self.enter(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Char('p' | 'k') if ctrl => self.move_by(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.move_by(1),
            KeyCode::PageUp => self.move_by(-8),
            KeyCode::PageDown => self.move_by(8),
            KeyCode::Left if what && self.create.name.is_empty() => self.back_to_where(),
            KeyCode::Tab if what => self.cycle_agent(),
            KeyCode::Tab if creating && rows::is_path(&self.create.filter) => {
                if let Some(completed) = rows::complete_path(&self.create.filter, &self.lines) {
                    self.create.filter = completed;
                    self.rebuild(None);
                }
            }
            KeyCode::Tab => self.cycle_filter(1),
            KeyCode::BackTab => self.cycle_filter(-1),
            KeyCode::Char('y') if ctrl && what => self.create.no_prompts = !self.create.no_prompts,
            KeyCode::Char('o') if ctrl => self.preview = !self.preview,
            KeyCode::Char('u') if ctrl => self.edit(String::clear),
            KeyCode::Char('w') if ctrl => self.edit(delete_word),
            KeyCode::Backspace => self.edit(|text| {
                text.pop();
            }),
            KeyCode::F(1) => self.help = true,
            KeyCode::Char(character) if !ctrl && !alt => {
                // On an empty text, a shortcut opens its filter from any filter, also from the
                // creation. The shortcut of the current filter goes back to All.
                let shortcut = Filter::of_shortcut(character).filter(|filter| {
                    // The current filter can be hidden: an action opened it.
                    self.filters.contains(filter) || *filter == self.filter
                });
                let empty = self.text().is_empty() && !renaming;
                if empty && character == '?' {
                    self.help = true;
                } else if let (true, Some(filter)) = (empty, shortcut) {
                    if filter == self.filter {
                        self.set_filter(Filter::All);
                    } else {
                        self.set_filter(filter);
                    }
                } else {
                    self.edit(|text| text.push(character));
                }
            }
            _ => {}
        }
        Outcome::Continue
    }
}

/// The key event of a key of the footer.
pub(super) fn key_of_hint(hint: &str) -> Option<KeyEvent> {
    let (code, modifiers) = match hint {
        "↵" => (KeyCode::Enter, KeyModifiers::NONE),
        "⌃↵" => (KeyCode::Enter, KeyModifiers::CONTROL),
        "⇥" => (KeyCode::Tab, KeyModifiers::NONE),
        "⌃⌫" => (KeyCode::Backspace, KeyModifiers::CONTROL),
        "esc" => (KeyCode::Esc, KeyModifiers::NONE),
        "?" => (KeyCode::F(1), KeyModifiers::NONE),
        _ => {
            // ⌃A, ⌃O, ⌃Y: ctrl and a letter.
            let letter = hint.strip_prefix('⌃')?.chars().next()?;
            (
                KeyCode::Char(letter.to_ascii_lowercase()),
                KeyModifiers::CONTROL,
            )
        }
    };
    Some(KeyEvent::new(code, modifiers))
}

/// Delete the last word of a text, and the spaces after it.
pub(super) fn delete_word(text: &mut String) {
    let trimmed = text.trim_end().len();
    let start = text[..trimmed]
        .rfind(char::is_whitespace)
        .map(|index| index + 1)
        .unwrap_or(0);
    text.truncate(start);
}
