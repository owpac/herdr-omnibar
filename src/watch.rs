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
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config;
use crate::create;
use crate::herdr::{self, Plugin};
use crate::model::{self, Snapshot};
use crate::recent;

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
/// The binary in the folder of a version of the plugin.
const BINARY_IN_ROOT: &str = "bin/herdr-omnibar";
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
        let owned = tab.has_default_label() || written.get(&tab.tab_id) == Some(&tab.label);
        if !owned {
            continue;
        }
        if let Some(title) = auto_title(snapshot, &tab.tab_id).filter(|title| *title != tab.label) {
            renames.push((tab.tab_id.clone(), title));
        }
    }
    renames
}

/// The automatic title of a tab: the useful title of the terminal of its one agent. None for a
/// tab with no agent or with 2 agents, or if the title tells nothing.
pub fn auto_title(snapshot: &Snapshot, tab_id: &str) -> Option<String> {
    let agents: Vec<_> = snapshot
        .panes
        .iter()
        .filter(|pane| pane.tab_id == tab_id && pane.agent.is_some())
        .collect();
    let [pane] = agents.as_slice() else {
        return None;
    };
    useful_title(
        pane.terminal_title_stripped.as_deref()?,
        pane.agent.as_deref().unwrap_or_default(),
        snapshot.agent_name(&pane.pane_id),
        pane.cwd.as_deref(),
    )
}

/// The title of the terminal of an agent, if it tells the task. A prompt, the name of the agent
/// the name of the folder, the herdr name of the agent (`name`) and the command line of the agent
/// (`claude --name x`) tell nothing. Codex adds ` | <folder>` to its title: this part goes.
pub fn useful_title(
    title: &str,
    agent: &str,
    name: Option<&str>,
    folder: Option<&Path>,
) -> Option<String> {
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
        || model::is_agent_noise(title, agent, name)
    {
        return None;
    }
    Some(title.to_string())
}

/// The reason to wait, if another plugin writes the titles of the tabs: herdr-bar is enabled, and
/// its config.json does not set `"auto_titles": false`.
fn other_writer(plugins: &[Plugin], bar_config: Option<&str>) -> Option<String> {
    herdr::enabled(plugins, HERDR_BAR)?;
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

/// True if the watcher has work: the titles of the tabs (feature auto_titles), or the order of
/// use of the tabs (feature recent_first).
fn has_work(config: &config::Config) -> bool {
    config.has("auto_titles") || config.has("recent_first")
}

/// Start the watcher if it has work and if no watcher runs. Return at once.
pub fn start() -> Result<(), String> {
    if !has_work(&config::current()) {
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

/// Run the watcher until herdr stops, until it has no work, or until the plugin is disabled. If
/// another watcher has the lock, return at once.
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
    let mut focused: Option<String> = None;
    loop {
        let config = config::current();
        if !has_work(&config) {
            log.write("auto_titles and recent_first are off: watcher stopped");
            return Ok(());
        }
        if plugins_read.is_none_or(|time| time.elapsed() >= PLUGINS_INTERVAL) {
            plugins_read = Some(Instant::now());
            let reason = match herdr::plugins() {
                Ok(plugins) => {
                    let Some(plugin) = herdr::enabled(&plugins, PLUGIN_ID) else {
                        log.write("the plugin is disabled: watcher stopped");
                        return Ok(());
                    };
                    // herdr installs each version in a new folder, and runs no command of the
                    // plugin after an install or a link: the list gives the active version.
                    if let Some(watch) = binary.as_mut() {
                        watch.active = plugin
                            .plugin_root
                            .as_ref()
                            .map(|root| canonical(&root.join(BINARY_IN_ROOT)));
                        watch.share_active_version(&state_dir);
                    }
                    other_writer(&plugins, bar_config().as_deref())
                }
                Err(error) => {
                    if let Some(watch) = binary.as_mut() {
                        watch.active = None;
                    }
                    Some(format!("no list of the plugins: {error}"))
                }
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
                if config.has("recent_first") {
                    recent::follow(&state_dir, &snapshot, &mut focused);
                }
                if waiting.is_none() && config.has("auto_titles") {
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

/// Give a tab back to the watcher: rename it to its number, the name that herdr gives. The
/// watcher then writes the title of its agent at its next read (2 seconds). Start the watcher if
/// it is stopped. Return the automatic title, or None if the tab has none: then the name stays.
pub fn give_back(tab_id: &str) -> Result<Option<String>, String> {
    let snapshot = herdr::snapshot().map_err(|error| error.to_string())?;
    let Some(title) = auto_title(&snapshot, tab_id) else {
        return Ok(None);
    };
    let number = snapshot
        .tabs
        .iter()
        .find(|tab| tab.tab_id == tab_id)
        .and_then(|tab| tab.number)
        .ok_or_else(|| format!("no tab {tab_id}"))?;
    herdr::rename_tab(tab_id, &number.to_string()).map_err(|error| error.to_string())?;
    start()?;
    Ok(Some(title))
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
/// herdr installs each version of the plugin in a new folder. The watcher takes the binary of the
/// active version from the list of the plugins of herdr, every 10 s, and writes it in this file for
/// an older binary that reads only this file. If herdr does not answer, this file gives the binary
/// of the last `watch` command: at the start of herdr, for a new agent, and at each opening of the
/// popup.
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

/// True for a binary that the watcher can run: a file with an execute bit, without a change for
/// REBUILD_SETTLE_TIME. During an install, the new binary can be partial, or still without its
/// execute bit: an `exec` fails, and the watcher refuses the binary until its end.
fn is_ready(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let settled = metadata
        .modified()
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age >= REBUILD_SETTLE_TIME);
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 && settled
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
    /// The canonical path of the binary of the active version, from the list of the plugins of
    /// herdr. None if herdr did not answer.
    active: Option<PathBuf>,
}

impl BinaryWatch {
    fn new(current: PathBuf) -> Self {
        let time = modification_time(&current);
        BinaryWatch {
            current,
            time,
            refused: None,
            active: None,
        }
    }

    /// Write the binary of the active version in watch.binary, if the file gives another binary.
    /// A binary older than this logic reads only this file: without this write, after a
    /// downgrade, the old binary and this binary run each other in a loop.
    fn share_active_version(&self, state_dir: &Path) {
        let Some(active) = self.active.as_ref().filter(|path| is_ready(path)) else {
            return;
        };
        let written = read_wanted_binary(state_dir).map(|path| canonical(&path));
        if written.as_ref() != Some(active) {
            let _ = write_wanted_binary(state_dir, active);
        }
    }

    /// The binary that the watcher runs now: the binary of the active version in herdr, else the
    /// binary of the last `watch` command, or its own binary after a rebuild. None keeps the
    /// process.
    fn binary_to_run(&self, state_dir: &Path) -> Option<PathBuf> {
        let usable = |path: &PathBuf| is_ready(path) && self.refused.as_ref() != Some(path);
        let wanted = self.active.clone().filter(usable).or_else(|| {
            read_wanted_binary(state_dir)
                .map(|path| canonical(&path))
                .filter(usable)
        });
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

    /// A binary that the watcher can run: an execute bit, and no change for 1 minute.
    fn ready_binary(path: &Path) {
        fs::write(path, "").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(60))
            .unwrap();
    }

    #[test]
    fn a_binary_in_writing_or_without_an_execute_bit_waits() {
        let folder =
            canonical(&std::env::temp_dir()).join(format!("omnibar-not-ready-{}", process::id()));
        fs::create_dir_all(&folder).unwrap();
        let current = folder.join("current");
        let active = folder.join("active");
        ready_binary(&current);
        // A new version during its install: just written, without an execute bit.
        fs::write(&active, "").unwrap();
        let mut watch = BinaryWatch::new(current.clone());
        watch.active = Some(active.clone());
        let during = watch.binary_to_run(&folder);
        ready_binary(&active);
        let after = watch.binary_to_run(&folder);
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(during, None);
        assert_eq!(after, Some(active));
    }

    #[test]
    fn the_active_version_of_herdr_comes_before_the_last_watch() {
        let folder =
            canonical(&std::env::temp_dir()).join(format!("omnibar-active-{}", process::id()));
        fs::create_dir_all(&folder).unwrap();
        let [current, active, old] = ["current", "active", "old"].map(|name| folder.join(name));
        for binary in [&current, &active, &old] {
            ready_binary(binary);
        }
        write_wanted_binary(&folder, &old).unwrap();
        let mut watch = BinaryWatch::new(current.clone());
        watch.active = Some(active.clone());
        let first = watch.binary_to_run(&folder);
        // The watcher runs the active version: the last watch changes nothing.
        let mut watch = BinaryWatch::new(active.clone());
        watch.active = Some(active.clone());
        let second = watch.binary_to_run(&folder);
        // Without an answer of herdr, the last watch gives the binary.
        watch.active = None;
        let third = watch.binary_to_run(&folder);
        // A downgrade: herdr gives the old binary, the file gives this binary. The file gets the
        // old binary: the old binary reads only the file, so it stays on itself.
        let mut watch = BinaryWatch::new(current.clone());
        write_wanted_binary(&folder, &current).unwrap();
        watch.active = Some(old.clone());
        watch.share_active_version(&folder);
        let shared = read_wanted_binary(&folder);
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(first, Some(active));
        assert_eq!(second, None);
        assert_eq!(third, Some(old.clone()));
        assert_eq!(shared, Some(old));
    }

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
        assert_eq!(useful_title("  ", "claude", None, folder), None);
        assert_eq!(useful_title("OpenCode", "opencode", None, folder), None);
        assert_eq!(useful_title("Claude Code", "claude", None, folder), None);
        assert_eq!(useful_title("atlas", "claude", None, folder), None);
        assert_eq!(
            useful_title("thomas@mac:~/work", "claude", None, folder),
            None
        );
        assert_eq!(
            useful_title("Fix the login | atlas", "codex", None, folder).as_deref(),
            Some("Fix the login")
        );
        assert_eq!(
            useful_title("A | B", "codex", None, folder).as_deref(),
            Some("A | B")
        );
    }

    #[test]
    fn a_tab_has_an_automatic_title_only_if_its_agent_gives_a_useful_one() {
        let tabs = tab("w1:t1", "1", 1);
        let useful = snapshot(
            &tabs,
            &pane("w1:p1", "w1:t1", Some("claude"), "Fix the login"),
        );
        assert_eq!(
            auto_title(&useful, "w1:t1").as_deref(),
            Some("Fix the login")
        );
        // The title of a brief is its herdr name: ctrl+T keeps the name of the tab.
        let mut brief = snapshot(&tabs, &pane("w1:p1", "w1:t1", Some("claude"), "task-a2p"));
        brief.agents = vec![crate::model::AgentInfo {
            pane_id: "w1:p1".into(),
            name: Some("task-a2p".into()),
            ..Default::default()
        }];
        assert_eq!(auto_title(&brief, "w1:t1"), None);
        // A tab without an agent, and an unknown tab.
        let shell = snapshot(&tabs, &pane("w1:p1", "w1:t1", None, "cargo test"));
        assert_eq!(auto_title(&shell, "w1:t1"), None);
        assert_eq!(auto_title(&useful, "w9:t9"), None);
    }

    #[test]
    fn the_herdr_name_and_the_command_line_of_an_agent_are_not_titles() {
        let folder = Some(Path::new("/work/atlas"));
        let named = Some("task-a2p");
        assert_eq!(useful_title("task-a2p", "claude", named, folder), None);
        assert_eq!(useful_title("Task-A2P", "claude", named, folder), None);
        assert_eq!(
            useful_title(
                "claude --model \"opus[1m]\" --permission-mode auto --name task-a2p --model",
                "claude",
                named,
                folder
            ),
            None
        );
        assert_eq!(
            useful_title("codex resume abc", "codex", None, folder),
            None
        );
        assert_eq!(useful_title("opencode run", "opencode", None, folder), None);
        // The same text is a title for another agent without this name.
        assert_eq!(
            useful_title("task-a2p", "claude", None, folder).as_deref(),
            Some("task-a2p")
        );
        // A task that only starts with the same letters is a title.
        assert_eq!(
            useful_title("Claudette login", "claude", None, folder).as_deref(),
            Some("Claudette login")
        );
    }

    #[test]
    fn a_named_agent_keeps_the_number_of_its_tab() {
        let panes = [
            r#"{"pane_id":"w1:p1","tab_id":"w1:t1","workspace_id":"w1","agent":"claude","terminal_title_stripped":"task-a2p"}"#,
        ]
        .join(",");
        let tabs = [tab("w1:t1", "1", 1)].join(",");
        let mut snapshot = snapshot(&tabs, &panes);
        snapshot.agents = vec![crate::model::AgentInfo {
            pane_id: "w1:p1".into(),
            name: Some("task-a2p".into()),
            ..Default::default()
        }];
        assert!(decide(&snapshot, &HashMap::new()).is_empty());
        // The name that the list shows is the number too.
        let model = model::Model::build(&snapshot, &HashMap::new());
        assert_eq!(model.agents[0].name, "1");
        // Without the herdr name, the title is the task.
        snapshot.agents.clear();
        assert_eq!(decide(&snapshot, &HashMap::new()).len(), 1);
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
