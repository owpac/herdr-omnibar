//! The logos of the agents (feature agent_logos): the font Herdr Agent Icons Max of herdr-radar,
//! like herdr-bar. The font gives one logo for each vendor, from U+E1A0. The omnibar shows the
//! logos only if the font is installed: it never installs a font.
//!
//! Font: https://github.com/hhdebb/herdr-radar (dist/HerdrAgentIconsMax-Regular.ttf).

use std::env;
use std::fs;
use std::path::PathBuf;

/// The vendors of the font, in the order of their codepoints.
const VENDORS: [&str; 24] = [
    "claude",
    "codex",
    "opencode",
    "omp",
    "cline",
    "mastracode",
    "kimi",
    "kilo",
    "maki",
    "pi",
    "hermes",
    "cursor",
    "copilot",
    "deepseek",
    "gemini",
    "gpt",
    "qwen",
    "grok",
    "agy",
    "kiro",
    "amp",
    "devin",
    "qodercli",
    "glm",
];

/// The logo of a kind of agent, as herdr names it: claude, codex, opencode…
pub fn logo(kind: &str) -> Option<char> {
    let kind = kind.trim().to_lowercase();
    let kind = kind.split(':').next().unwrap_or_default();
    let kind = kind.strip_suffix(" code").unwrap_or(kind);
    let kind = match kind {
        "cursor-agent" => "cursor",
        "open-code" => "opencode",
        other => other,
    };
    let index = VENDORS.iter().position(|vendor| *vendor == kind)?;
    char::from_u32(0xE1A0 + index as u32)
}

/// True if the font is in a font folder of the user or of the system.
pub fn font_installed() -> bool {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let folders = if cfg!(target_os = "macos") {
        vec![home.join("Library/Fonts"), PathBuf::from("/Library/Fonts")]
    } else {
        let data = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        vec![data.join("fonts"), home.join(".fonts")]
    };
    folders.iter().any(|folder| {
        fs::read_dir(folder).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("HerdrAgentIconsMax") && name.ends_with(".ttf")
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_known_agent_has_its_logo() {
        assert_eq!(logo("claude"), Some('\u{E1A0}'));
        assert_eq!(logo("Claude Code"), Some('\u{E1A0}'));
        assert_eq!(logo("codex"), Some('\u{E1A1}'));
        assert_eq!(logo("open-code"), Some('\u{E1A2}'));
        assert_eq!(logo("shell"), None);
    }
}
