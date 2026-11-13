//! `keys`: show each key that the popup receives.
//!
//! The omnibar asks the terminal for the keyboard protocol of kitty: herdr then forwards the
//! modifiers of Enter, Backspace and the arrows, if the terminal sends them to herdr. This tool
//! shows if ctrl+Enter, ctrl+Backspace, ctrl+Left, alt+Up and alt+Down arrive as different keys.

use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal};
use std::io::{self, Write};

/// The keyboard flags of the omnibar: modified keys come as different keys.
pub fn flags() -> KeyboardEnhancementFlags {
    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
}

/// Show the keys until q is pressed 2 times in a row.
pub fn run() -> io::Result<()> {
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(out, PushKeyboardEnhancementFlags(flags()))?;
    }
    let result = show(&mut out, enhanced);
    if enhanced {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    terminal::disable_raw_mode()?;
    result
}

fn show(out: &mut impl Write, enhanced: bool) -> io::Result<()> {
    write!(
        out,
        "Keyboard protocol of kitty: {}\r\nPress keys: ctrl+Enter, ctrl+Backspace, ctrl+Left, alt+Up, alt+Down, Tab.\r\nPress q 2 times to quit.\r\n\r\n",
        if enhanced { "on" } else { "off: modified keys can look like plain keys" }
    )?;
    out.flush()?;
    let mut last_was_q = false;
    loop {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            writeln!(out, "{}\r", describe(key.code, key.modifiers))?;
            out.flush()?;
            let is_q = key.code == KeyCode::Char('q') && key.modifiers.is_empty();
            if is_q && last_was_q {
                return Ok(());
            }
            last_was_q = is_q;
        }
    }
}

/// A key as a text: the modifiers, then the key. Example: "ctrl+Enter".
pub fn describe(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mut parts = Vec::new();
    for (flag, name) in [
        (KeyModifiers::CONTROL, "ctrl"),
        (KeyModifiers::ALT, "alt"),
        (KeyModifiers::SHIFT, "shift"),
        (KeyModifiers::SUPER, "super"),
    ] {
        if modifiers.contains(flag) {
            parts.push(name.to_string());
        }
    }
    parts.push(match code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(character) => character.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        other => format!("{other:?}"),
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_with_modifiers_names_each_modifier() {
        assert_eq!(
            describe(KeyCode::Enter, KeyModifiers::CONTROL),
            "ctrl+Enter"
        );
        assert_eq!(
            describe(
                KeyCode::Char('g'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            "ctrl+shift+g"
        );
        assert_eq!(describe(KeyCode::Tab, KeyModifiers::NONE), "Tab");
    }
}
