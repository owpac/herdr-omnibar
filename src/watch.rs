//! The automatic titles of the tabs (feature `auto_titles`).
//!
//! A tab with one agent takes the title that the agent gives to its terminal: Claude Code and Codex
//! write their task there. The watcher renames only a tab with the name that herdr gives (its
//! number), or with the last title that the watcher wrote. A name that the user gives stays.
//!
//! One watcher runs for each herdr server. herdr runs `watch` at its start and when it detects an
//! agent (`[[startup]]` and `[[events]]` of herdr-plugin.toml): `watch` starts the detached process
//! `watch --run`, which takes a lock, then returns at once.
//!
//! herdr-bar also writes the titles of the tabs. While herdr-bar is enabled with its own automatic
//! titles, the watcher waits: 2 writers on the same tab fight.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config;
use crate::create;
use crate::herdr::{self, Plugin};
use crate::model::{self, Snapshot};

/// The time between 2 reads of the snapshot.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// The time between 2 reads of the list of the plugins.
const PLUGINS_INTERVAL: Duration = Duration::from_secs(10);
/// After this number of failed snapshots in a row, herdr is stopped: the watcher stops too.
const MAX_SNAPSHOT_FAILURES: u32 = 5;
/// A rebuilt binary must not change for this time before the watcher runs it: a copy can write
/// the binary for a moment.
const REBUILD_SETTLE_TIME: Duration = Duration::from_secs(2);
const MAX_LOG_SIZE_BYTES: u64 = 200_000;

/// The id of this plugin, and of the other plugin that writes the titles of the tabs.
const PLUGIN_ID: &str = "herdr-omnibar";
const HERDR_BAR: &str = "herdr-bar";

/// The renames for a snapshot: the id of each tab, and its new title.
///
/// `written` gives the last title that the watcher wrote on each tab. A tab changes only if:
/// - it has one agent, with a useful title,
/// - its name is its number, or the last title of the watcher,
/// - its name is not already this title.
pub fn decide(snapshot: &Snapshot, written: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut renames = Vec::new();
    for tab in &snapshot.tabs {
        let agents: Vec<_> = snapshot
            .panes
            .iter()
            .filter(|pane| pane.tab_id == tab.tab_id && pane.agent.is_some())
            .collect();
        let [pane] = agents.as_slice() else {
            continue;
        };
        let owned = tab.has_default_label() || written.get(&tab.tab_id) == Some(&tab.label);
        if !owned {
            continue;
        }
        let title = pane.terminal_title_stripped.as_deref().and_then(|title| {
            useful_title(
                title,
                pane.agent.as_deref().unwrap_or_default(),
                pane.cwd.as_deref(),
            )
        });
        if let Some(title) = title.filter(|title| *title != tab.label) {
            renames.push((tab.tab_id.clone(), title));
        }
    }
    renames
}

/// The title of the terminal of an agent, if it tells the task. A prompt, the name of the agent
/// or the name of the folder tells nothing. Codex adds ` | <folder>` to its title: this part goes.
pub fn useful_title(title: &str, agent: &str, folder: Option<&Path>) -> Option<String> {
    let folder_name = folder
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let mut title = title.trim();
    if let (Some(name), Some((head, tail))) = (&folder_name, title.rsplit_once(" | ")) {
        if tail.trim() == name {
            title = head.trim();
        }
    }
    let lower = title.to_lowercase();
    let agent_names = [agent, "claude", "claude code", "codex", "opencode"];
    if title.is_empty()
        || model::is_prompt_title(title)
        || agent_names.iter().any(|name| name.to_lowercase() == lower)
        || folder_name.as_deref() == Some(title)
    {
        return None;
    }
    Some(title.to_string())
}

/// The reason to wait, if another plugin writes the titles of the tabs: herdr-bar is enabled, and
/// its config.json does not set `"auto_titles": false`.
fn other_writer(plugins: &[Plugin], bar_config: Option<&str>) -> Option<String> {
    let bar_enabled = plugins
        .iter()
        .any(|plugin| plugin.plugin_id == HERDR_BAR && plugin.enabled);
    if !bar_enabled {
        return None;
    }
    let bar_titles = bar_config
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|config| config["auto_titles"].as_bool())
        .unwrap_or(true);
    bar_titles.then(|| format!("{HERDR_BAR} writes the titles"))
}

/// The config.json of herdr-bar, next to the config folder of this plugin.
fn bar_config() -> Option<String> {
    let folder = config::config_dir();
    fs::read_to_string(folder.parent()?.join(HERDR_BAR).join("config.json")).ok()
}

/// Start the watcher if the feature auto_titles is on and no watcher runs. Return at once.
pub fn start() -> Result<(), String> {
    if !config::current().has("auto_titles") {
        return Ok(());
    }
    let state_dir = config::state_dir();
    let lock = open_lock(&state_dir)?;
    let executable =
        std::env::current_exe().map_err(|error| format!("cannot find the binary: {error}"))?;
    let written = write_wanted_binary(&state_dir, &canonical(&executable));
    if lock.try_lock().is_err() {
        // A watcher runs: the file gives it this binary.
        return written;
    }
    // The detached process takes the lock itself.
    drop(lock);
    create::spawn_detached(&["watch", "--run"], &[])
}

/// Run the watcher until herdr stops, until the feature auto_titles is off, or until the plugin
/// is disabled. If another watcher has the lock, return at once.
pub fn run() -> Result<(), String> {
    let state_dir = config::state_dir();
    let lock = open_lock(&state_dir)?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let log = Log::open(&state_dir.join("watch.log"));
    log.write("watcher started");
    let mut written = read_written(&state_dir);
    let mut binary = std::env::current_exe()
        .ok()
        .map(|binary| BinaryWatch::new(canonical(&binary)));
    let mut failures = 0;
    let mut plugins_read: Option<Instant> = None;
    let mut waiting: Option<String> = Some("start".to_string());
    loop {
        if !config::current().has("auto_titles") {
            log.write("auto_titles is off: watcher stopped");
            return Ok(());
        }
        if plugins_read.is_none_or(|time| time.elapsed() >= PLUGINS_INTERVAL) {
            plugins_read = Some(Instant::now());
            let reason = match herdr::plugins() {
                Ok(plugins) => {
                    let enabled = plugins
                        .iter()
                        .any(|plugin| plugin.plugin_id == PLUGIN_ID && plugin.enabled);
                    if !enabled {
                        log.write("the plugin is disabled: watcher stopped");
                        return Ok(());
                    }
                    other_writer(&plugins, bar_config().as_deref())
                }
                Err(error) => Some(format!("no list of the plugins: {error}")),
            };
            if reason != waiting {
                log.write(&match &reason {
                    Some(reason) => format!("{reason}: the watcher waits"),
                    None => "the watcher writes the titles".to_string(),
                });
            }
            waiting = reason;
        }
        match herdr::snapshot() {
            Ok(snapshot) => {
                failures = 0;
                if waiting.is_none() {
                    rename(&snapshot, &mut written, &state_dir, &log);
                }
            }
            Err(error) => {
                failures += 1;
                if failures >= MAX_SNAPSHOT_FAILURES {
                    log.write(&format!("watcher stopped: {error}"));
                    return Ok(());
                }
            }
        }
        if let Some(watch) = binary.as_mut() {
            if let Some(next) = watch.binary_to_run(&state_dir) {
                log.write(&format!("new binary: {}", next.display()));
                // The lock file closes at the exec (close-on-exec): the new process takes the
                // lock. If the exec fails, this process keeps the lock and continues.
                let error = Command::new(&next).args(["watch", "--run"]).exec();
                log.write(&format!("cannot run {}: {error}", next.display()));
                watch.refuse(next);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Rename the tabs of a snapshot, then save the titles that the watcher wrote.
fn rename(snapshot: &Snapshot, written: &mut HashMap<String, String>, state_dir: &Path, log: &Log) {
    let mut changed = false;
    for (tab_id, title) in decide(snapshot, written) {
        match herdr::rename_tab(&tab_id, &title) {
            Ok(()) => {
                log.write(&format!("{tab_id}: {title}"));
                written.insert(tab_id, title);
                changed = true;
            }
            Err(error) => log.write(&format!("{tab_id}: cannot rename: {error}")),
        }
    }
    // A closed tab does not come back: its title goes.
    let before = written.len();
    written.retain(|tab_id, _| snapshot.tabs.iter().any(|tab| &tab.tab_id == tab_id));
    if changed || written.len() != before {
        save_written(state_dir, written);
    }
}

/// Print what the watcher does now: the renames, or the reason to wait.
pub fn print() -> Result<(), String> {
    let plugins = herdr::plugins().map_err(|error| error.to_string())?;
    let feature = config::current().has("auto_titles");
    println!("auto_titles: {}", if feature { "on" } else { "off" });
    if let Some(reason) = other_writer(&plugins, bar_config().as_deref()) {
        println!("{reason}: the watcher waits");
    }
    let snapshot = herdr::snapshot().map_err(|error| error.to_string())?;
    let renames = decide(&snapshot, &read_written(&config::state_dir()));
    if renames.is_empty() {
        println!("no tab to rename");
    }
    for (tab_id, title) in renames {
        let label = snapshot
            .tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .map(|tab| tab.label.as_str())
            .unwrap_or_default();
        println!("{tab_id}: “{label}” → “{title}”");
    }
    Ok(())
}

fn written_path(state_dir: &Path) -> PathBuf {
    state_dir.join("titles.json")
}

fn read_written(state_dir: &Path) -> HashMap<String, String> {
    fs::read_to_string(written_path(state_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_written(state_dir: &Path, written: &HashMap<String, String>) {
    if let Ok(text) = serde_json::to_string_pretty(written) {
        // The titles are only a memory: a failed write must not stop the watcher.
        let _ = fs::write(written_path(state_dir), text);
    }
}

fn open_lock(state_dir: &Path) -> Result<File, String> {
    fs::create_dir_all(state_dir)
        .map_err(|error| format!("cannot create {}: {error}", state_dir.display()))?;
    let path = state_dir.join("watch.lock");
    File::create(&path).map_err(|error| format!("cannot create {}: {error}", path.display()))
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The file with the path of the binary of the last `watch` command.
///
/// herdr installs each version of the plugin in a new folder. herdr runs `watch` of the active
/// version at its start and when it detects an agent: this file gives the running watcher the
/// binary of this version.
fn wanted_binary_path(state_dir: &Path) -> PathBuf {
    state_dir.join("watch.binary")
}

/// Write the path of a binary for the running watcher: a temporary file, then a rename. The
/// watcher never reads a partial path.
fn write_wanted_binary(state_dir: &Path, binary: &Path) -> Result<(), String> {
    let path = wanted_binary_path(state_dir);
    let temporary = state_dir.join(format!("watch.binary.{}.tmp", process::id()));
    let result = fs::write(&temporary, binary.as_os_str().as_bytes())
        .and_then(|()| fs::rename(&temporary, &path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn read_wanted_binary(state_dir: &Path) -> Option<PathBuf> {
    let bytes = fs::read(wanted_binary_path(state_dir)).ok()?;
    (!bytes.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(&bytes)))
}

fn modification_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// What the watcher knows about its binary, to run a new binary.
struct BinaryWatch {
    /// The canonical path of the binary of the watcher, saved at the start.
    current: PathBuf,
    /// The modification time of `current` at the start, or after a failed `exec`.
    time: Option<SystemTime>,
    /// A wanted binary that `exec` refused: the watcher does not try it again.
    refused: Option<PathBuf>,
}

impl BinaryWatch {
    fn new(current: PathBuf) -> Self {
        let time = modification_time(&current);
        BinaryWatch {
            current,
            time,
            refused: None,
        }
    }

    /// The binary that the watcher runs now: the binary of the last `watch` command, or its own
    /// binary after a rebuild. None keeps the process.
    fn binary_to_run(&self, state_dir: &Path) -> Option<PathBuf> {
        let wanted = read_wanted_binary(state_dir)
            .map(|path| canonical(&path))
            .filter(|path| path.is_file() && self.refused.as_ref() != Some(path));
        if let Some(wanted) = wanted.filter(|wanted| *wanted != self.current) {
            return Some(wanted);
        }
        let rebuilt = modification_time(&self.current).is_some_and(|time| {
            Some(time) != self.time
                && SystemTime::now()
                    .duration_since(time)
                    .is_ok_and(|age| age >= REBUILD_SETTLE_TIME)
        });
        rebuilt.then(|| self.current.clone())
    }

    fn refuse(&mut self, binary: PathBuf) {
        if binary == self.current {
            // Try again only after the next rebuild.
            self.time = modification_time(&self.current);
        } else {
            self.refused = Some(binary);
        }
    }
}

/// The log of the watcher: each rename, and each reason to wait or to stop.
struct Log {
    file: Option<File>,
}

impl Log {
    fn open(path: &Path) -> Self {
        let too_big = fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_LOG_SIZE_BYTES);
        let file = OpenOptions::new()
            .create(true)
            .append(!too_big)
            .write(true)
            .truncate(too_big)
            .open(path)
            .ok();
        Log { file }
    }

    fn write(&self, message: &str) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        if let Some(mut file) = self.file.as_ref() {
            // The log is only for a diagnosis: a failed write must not stop the watcher.
            let _ = writeln!(file, "{seconds} {message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(tabs: &str, panes: &str) -> Snapshot {
        serde_json::from_str(&format!(r#"{{"tabs": [{tabs}], "panes": [{panes}]}}"#)).unwrap()
    }

    fn tab(id: &str, label: &str, number: u32) -> String {
        format!(
            r#"{{"tab_id": "{id}", "workspace_id": "w1", "label": "{label}", "number": {number}}}"#
        )
    }

    fn pane(id: &str, tab: &str, agent: Option<&str>, title: &str) -> String {
        let agent = agent
            .map(|agent| format!(r#""agent": "{agent}","#))
            .unwrap_or_default();
        format!(
            r#"{{"pane_id": "{id}", "tab_id": "{tab}", "workspace_id": "w1", "cwd": "/work/atlas", {agent} "terminal_title_stripped": "{title}"}}"#
        )
    }

    #[test]
    fn a_tab_with_its_number_takes_the_title_of_its_agent() {
        let snapshot = snapshot(
            &tab("w1:t1", "1", 1),
            &pane("w1:p1", "w1:t1", Some("claude"), "Fix the login"),
        );
        assert_eq!(
            decide(&snapshot, &HashMap::new()),
            [("w1:t1".to_string(), "Fix the login".to_string())]
        );
    }

    #[test]
    fn a_name_of_the_user_stays_and_a_title_of_the_watcher_follows_the_agent() {
        let snapshot = snapshot(
            &tab("w1:t1", "review", 1),
            &pane("w1:p1", "w1:t1", Some("claude"), "Fix the login"),
        );
        assert!(decide(&snapshot, &HashMap::new()).is_empty());
        let written = HashMap::from([("w1:t1".to_string(), "review".to_string())]);
        assert_eq!(decide(&snapshot, &written).len(), 1);
    }

    #[test]
    fn a_tab_with_2_agents_or_without_an_agent_stays() {
        let panes = [
            pane("w1:p1", "w1:t1", Some("claude"), "Fix the login"),
            pane("w1:p2", "w1:t1", Some("codex"), "Review"),
            pane("w1:p3", "w1:t2", None, "cargo test"),
        ]
        .join(",");
        let tabs = [tab("w1:t1", "1", 1), tab("w1:t2", "2", 2)].join(",");
        assert!(decide(&snapshot(&tabs, &panes), &HashMap::new()).is_empty());
    }

    #[test]
    fn a_title_without_the_task_is_not_useful() {
        let folder = Some(Path::new("/work/atlas"));
        assert_eq!(useful_title("  ", "claude", folder), None);
        assert_eq!(useful_title("OpenCode", "opencode", folder), None);
        assert_eq!(useful_title("Claude Code", "claude", folder), None);
        assert_eq!(useful_title("atlas", "claude", folder), None);
        assert_eq!(useful_title("thomas@mac:~/work", "claude", folder), None);
        assert_eq!(
            useful_title("Fix the login | atlas", "codex", folder).as_deref(),
            Some("Fix the login")
        );
        assert_eq!(
            useful_title("A | B", "codex", folder).as_deref(),
            Some("A | B")
        );
    }

    #[test]
    fn herdr_bar_writes_the_titles_unless_its_auto_titles_is_false() {
        let plugins = |enabled: bool| {
            vec![Plugin {
                plugin_id: HERDR_BAR.to_string(),
                enabled,
                plugin_root: None,
            }]
        };
        assert!(other_writer(&plugins(true), None).is_some());
        assert!(other_writer(&plugins(true), Some(r#"{"auto_titles": true}"#)).is_some());
        assert!(other_writer(&plugins(true), Some(r#"{"auto_titles": false}"#)).is_none());
        assert!(other_writer(&plugins(false), None).is_none());
        assert!(other_writer(&[], None).is_none());
    }
}
