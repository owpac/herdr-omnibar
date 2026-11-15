//! The herdr shortcuts of the omnibar actions, in the open popup.
//!
//! herdr sends all the keys to an open popup, the prefix included. So with the popup open, a
//! shortcut such as `prefix+k` arrives as 2 keys: the prefix, then `k`. This module reads the
//! prefixes and the omnibar commands from the config of herdr, and finds the filter of a key that
//! follows a prefix.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

use crate::filter::Filter;

/// A key: its code and its modifiers, as `normalize` gives them.
type Combo = (KeyCode, KeyModifiers);

/// The prefix of herdr when the config gives none.
const DEFAULT_PREFIX: &str = "ctrl+b";
/// The id of the plugin in the key `command` of herdr.
const PLUGIN: &str = "herdr-omnibar";

/// The prefix keys and the keys of the omnibar actions of the herdr config.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcuts {
    prefixes: Vec<Combo>,
    commands: Vec<(Combo, Filter)>,
}

impl Shortcuts {
    /// The shortcuts of the herdr config file. A missing or wrong file gives no shortcut.
    pub fn load() -> Shortcuts {
        let text = fs::read_to_string(crate::theme::config_path()).unwrap_or_default();
        Shortcuts::parse(&text)
    }

    /// The shortcuts of the text of a herdr config.
    pub fn parse(text: &str) -> Shortcuts {
        let Ok(config) = text.parse::<toml::Table>() else {
            return Shortcuts::default();
        };
        let keys = config.get("keys").and_then(toml::Value::as_table);
        let mut prefixes = Vec::new();
        for name in ["prefix", "extra_prefixes"] {
            for label in keys.map(|keys| labels(keys.get(name))).unwrap_or_default() {
                if let Some(combo) = parse_combo(&label) {
                    if !prefixes.contains(&combo) {
                        prefixes.push(combo);
                    }
                }
            }
        }
        if prefixes.is_empty() {
            prefixes.extend(parse_combo(DEFAULT_PREFIX));
        }
        let mut commands = Vec::new();
        let entries = keys
            .and_then(|keys| keys.get("command"))
            .and_then(toml::Value::as_array);
        for entry in entries.into_iter().flatten() {
            let text_of = |name: &str| entry.get(name).and_then(toml::Value::as_str);
            if text_of("type") != Some("plugin_action") {
                continue;
            }
            let filter = text_of("command")
                .and_then(|command| command.strip_prefix(PLUGIN)?.strip_prefix('.'))
                .and_then(filter_of_action);
            let combo = text_of("key")
                .and_then(|key| key.trim().strip_prefix("prefix+"))
                .and_then(parse_combo);
            if let (Some(filter), Some(combo)) = (filter, combo) {
                commands.push((combo, filter));
            }
        }
        Shortcuts { prefixes, commands }
    }

    /// True if the key is a prefix of herdr.
    pub fn is_prefix(&self, key: &KeyEvent) -> bool {
        !self.commands.is_empty() && self.prefixes.contains(&normalize(key.code, key.modifiers))
    }

    /// The filter of the omnibar action that a key runs after a prefix.
    pub fn filter_of(&self, key: &KeyEvent) -> Option<Filter> {
        let combo = normalize(key.code, key.modifiers);
        self.commands
            .iter()
            .find(|(candidate, _)| *candidate == combo)
            .map(|(_, filter)| *filter)
    }
}

/// The filter of an action id of herdr-plugin.toml: `open` is All. The actions without a filter
/// (sort, keys) give None.
fn filter_of_action(id: &str) -> Option<Filter> {
    match id {
        "open" => Some(Filter::All),
        "sort" | "keys" => None,
        other => Filter::parse(other),
    }
}

/// The text of a key that is a string, or of the keys of a list.
fn labels(value: Option<&toml::Value>) -> Vec<String> {
    match value {
        Some(toml::Value::String(label)) => vec![label.clone()],
        Some(toml::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// A key as the popup receives it: an uppercase letter is the lowercase letter with shift, and
/// the modifiers that herdr does not know are removed.
fn normalize(code: KeyCode, modifiers: KeyModifiers) -> Combo {
    let mut modifiers = modifiers
        & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::SUPER);
    let code = match code {
        KeyCode::Char(character) if character.is_uppercase() => {
            modifiers |= KeyModifiers::SHIFT;
            KeyCode::Char(character.to_lowercase().next().unwrap_or(character))
        }
        // A symbol such as ? or ! has no shift in the herdr label, but a terminal can add it.
        KeyCode::Char(character) if !character.is_alphabetic() => {
            modifiers.remove(KeyModifiers::SHIFT);
            code
        }
        KeyCode::Tab if modifiers.contains(KeyModifiers::SHIFT) => {
            modifiers.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        KeyCode::BackTab => {
            modifiers.remove(KeyModifiers::SHIFT);
            code
        }
        other => other,
    };
    (code, modifiers)
}

/// A key of the herdr config, for example "ctrl+space", "k" or "shift+g".
fn parse_combo(label: &str) -> Option<Combo> {
    let label = label.trim();
    if label == "+" {
        return Some((KeyCode::Char('+'), KeyModifiers::empty()));
    }
    let (modifier_text, key) = match label.strip_suffix("++") {
        Some(rest) => (rest, "+"),
        None => label.rsplit_once('+').unwrap_or(("", label)),
    };
    let mut modifiers = KeyModifiers::empty();
    for part in modifier_text.split('+').filter(|part| !part.is_empty()) {
        modifiers |= match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "shift" => KeyModifiers::SHIFT,
            "alt" | "option" | "meta" => KeyModifiers::ALT,
            "cmd" | "command" | "super" => KeyModifiers::SUPER,
            _ => return None,
        };
    }
    let key = key.trim();
    let code = match key.to_lowercase().as_str() {
        "space" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" | "bs" => KeyCode::Backspace,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "minus" => KeyCode::Char('-'),
        "comma" => KeyCode::Char(','),
        "period" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "plus" => KeyCode::Char('+'),
        lower => {
            let mut characters = key.chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => KeyCode::Char(character),
                _ => KeyCode::F(lower.strip_prefix('f')?.parse().ok()?),
            }
        }
    };
    Some(normalize(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(character: char) -> KeyEvent {
        key(KeyCode::Char(character), KeyModifiers::NONE)
    }

    const CONFIG: &str = r#"
[keys]
prefix = "ctrl+space"
extra_prefixes = ["ctrl+b"]

[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr-omnibar.open"

[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-omnibar.new"

[[keys.command]]
key = "prefix+k"
type = "plugin_action"
command = "herdr-omnibar.actions"

[[keys.command]]
key = "prefix+s"
type = "plugin_action"
command = "herdr-omnibar.sort"

[[keys.command]]
key = "prefix+x"
type = "plugin_action"
command = "other-plugin.actions"

[[keys.command]]
key = "prefix+y"
type = "shell"
command = "herdr-omnibar.actions"
"#;

    #[test]
    fn the_prefixes_come_from_prefix_and_extra_prefixes() {
        let shortcuts = Shortcuts::parse(CONFIG);
        assert!(shortcuts.is_prefix(&key(KeyCode::Char(' '), KeyModifiers::CONTROL)));
        assert!(shortcuts.is_prefix(&key(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        assert!(!shortcuts.is_prefix(&plain('b')));
    }

    #[test]
    fn a_config_without_prefix_gives_the_prefix_of_herdr() {
        let shortcuts = Shortcuts::parse("[[keys.command]]\nkey = \"prefix+k\"\ntype = \"plugin_action\"\ncommand = \"herdr-omnibar.actions\"\n");
        assert!(shortcuts.is_prefix(&key(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        assert_eq!(shortcuts.filter_of(&plain('k')), Some(Filter::Actions));
    }

    #[test]
    fn the_omnibar_commands_give_their_filter() {
        let shortcuts = Shortcuts::parse(CONFIG);
        assert_eq!(shortcuts.filter_of(&plain('p')), Some(Filter::All));
        assert_eq!(shortcuts.filter_of(&plain('t')), Some(Filter::New));
        assert_eq!(shortcuts.filter_of(&plain('k')), Some(Filter::Actions));
    }

    #[test]
    fn the_other_commands_give_no_filter() {
        let shortcuts = Shortcuts::parse(CONFIG);
        // An action without a filter, another plugin, another type, an unknown key.
        for character in ['s', 'x', 'y', 'z'] {
            assert_eq!(shortcuts.filter_of(&plain(character)), None);
        }
    }

    #[test]
    fn a_shifted_key_matches_the_label_with_shift() {
        let shortcuts = Shortcuts::parse(
            "[[keys.command]]\nkey = \"prefix+shift+g\"\ntype = \"plugin_action\"\ncommand = \"herdr-omnibar.worktrees\"\n",
        );
        let event = key(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(shortcuts.filter_of(&event), Some(Filter::Worktrees));
        assert_eq!(shortcuts.filter_of(&plain('g')), None);
    }

    #[test]
    fn a_symbol_matches_with_or_without_shift() {
        let shortcuts = Shortcuts::parse(
            "[[keys.command]]\nkey = \"prefix+?\"\ntype = \"plugin_action\"\ncommand = \"herdr-omnibar.agents\"\n",
        );
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            let event = key(KeyCode::Char('?'), modifiers);
            assert_eq!(shortcuts.filter_of(&event), Some(Filter::Agents));
        }
    }

    #[test]
    fn a_wrong_prefix_gives_the_prefix_of_herdr() {
        let shortcuts = Shortcuts::parse(
            "[keys]\nprefix = \"nonsense+\"\n[[keys.command]]\nkey = \"prefix+k\"\ntype = \"plugin_action\"\ncommand = \"herdr-omnibar.actions\"\n",
        );
        assert!(shortcuts.is_prefix(&key(KeyCode::Char('b'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn a_config_without_command_takes_no_prefix() {
        let shortcuts = Shortcuts::parse("[keys]\nprefix = \"ctrl+a\"\n");
        assert!(!shortcuts.is_prefix(&key(KeyCode::Char('a'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn a_wrong_config_gives_no_shortcut() {
        assert_eq!(Shortcuts::parse("[keys\nprefix ="), Shortcuts::default());
        assert_eq!(Shortcuts::parse(""), Shortcuts::parse("[keys]"));
        let shortcuts = Shortcuts::parse("[keys]\ncommand = 3\nprefix = 4\n");
        assert_eq!(shortcuts.filter_of(&plain('k')), None);
        assert!(!shortcuts.is_prefix(&plain('k')));
    }
}
