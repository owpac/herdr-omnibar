//! The pull requests of the branches (feature pull_requests). One `gh pr list` for each project
//! reads its last pull requests, in a background thread: the network takes about 1 to 2 s. The
//! rows and the preview show the pull requests when they are ready.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

/// The number of pull requests that `gh pr list` reads for a project.
const LIMIT: &str = "50";
const FIELDS: &str = "number,title,state,isDraft,headRefName,mergeStateStatus,reviewDecision,\
statusCheckRollup,additions,deletions,updatedAt,baseRefName";

/// A pull request of a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    pub number: u64,
    pub title: String,
    /// OPEN, MERGED or CLOSED.
    pub state: String,
    pub draft: bool,
    pub base: String,
    pub additions: u64,
    pub deletions: u64,
    pub checks: Checks,
    /// APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED, or empty.
    pub review: String,
    /// The merge state of GitHub: CLEAN, BLOCKED, DIRTY, BEHIND, UNSTABLE, UNKNOWN…
    pub merge: String,
    /// The time of the last update, in seconds since 1970.
    pub updated: Option<u64>,
}

/// The checks of a pull request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Checks {
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
}

/// What the omnibar knows about the pull request of a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// The thread reads the pull requests of the project.
    Reading,
    /// gh failed: not installed, not logged in, or not a GitHub repository.
    Failed(String),
    /// The branch has no pull request.
    None,
    Found(Pull),
}

enum State {
    Reading,
    Failed(String),
    Ready(HashMap<String, Pull>),
}

/// The pull requests of each project, by main checkout.
#[derive(Default)]
pub struct Pulls {
    states: Arc<Mutex<HashMap<PathBuf, State>>>,
    asked: HashSet<PathBuf>,
}

impl Pulls {
    /// Read the pull requests of a project in the background, one time.
    pub fn ask(&mut self, main: &Path) {
        if !self.asked.insert(main.to_path_buf()) {
            return;
        }
        let states = Arc::clone(&self.states);
        let main = main.to_path_buf();
        if let Ok(mut states) = states.lock() {
            states.insert(main.clone(), State::Reading);
        }
        thread::spawn(move || {
            let state = match read(&main) {
                Ok(pulls) => State::Ready(pulls),
                Err(error) => State::Failed(error),
            };
            if let Ok(mut states) = states.lock() {
                states.insert(main, state);
            }
        });
    }

    /// The pull request of a branch of a project.
    pub fn of(&self, main: &Path, branch: &str) -> Lookup {
        let Ok(states) = self.states.lock() else {
            return Lookup::Reading;
        };
        match states.get(main) {
            None | Some(State::Reading) => Lookup::Reading,
            Some(State::Failed(error)) => Lookup::Failed(error.clone()),
            Some(State::Ready(pulls)) => pulls
                .get(branch)
                .cloned()
                .map_or(Lookup::None, Lookup::Found),
        }
    }
}

/// The last pull requests of a project, by branch. A branch keeps its open pull request, else its
/// last one: gh gives the newest first.
fn read(main: &Path) -> Result<HashMap<String, Pull>, String> {
    let output = Command::new("gh")
        .current_dir(main)
        .args(["pr", "list", "--state", "all", "--limit", LIMIT])
        .args(["--json", FIELDS])
        .output()
        .map_err(|error| format!("cannot run gh: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(error.lines().next().unwrap_or("gh failed").to_string());
    }
    let list: Vec<Value> =
        serde_json::from_slice(&output.stdout).map_err(|error| format!("gh: {error}"))?;
    let mut pulls: HashMap<String, Pull> = HashMap::new();
    for value in &list {
        let branch = value["headRefName"].as_str().unwrap_or_default();
        keep(&mut pulls, branch, pull_of(value));
    }
    Ok(pulls)
}

/// Keep a pull request for its branch: the open pull request, else the first one (the newest).
fn keep(pulls: &mut HashMap<String, Pull>, branch: &str, pull: Pull) {
    let known_wins = pulls
        .get(branch)
        .is_some_and(|known| known.state == "OPEN" || pull.state != "OPEN");
    if !known_wins {
        pulls.insert(branch.to_string(), pull);
    }
}

fn pull_of(value: &Value) -> Pull {
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    Pull {
        number: value["number"].as_u64().unwrap_or_default(),
        title: text("title"),
        state: text("state"),
        draft: value["isDraft"].as_bool().unwrap_or_default(),
        base: text("baseRefName"),
        additions: value["additions"].as_u64().unwrap_or_default(),
        deletions: value["deletions"].as_u64().unwrap_or_default(),
        checks: checks_of(&value["statusCheckRollup"]),
        review: text("reviewDecision"),
        merge: text("mergeStateStatus"),
        updated: value["updatedAt"].as_str().and_then(parse_time),
    }
}

/// Count the checks: a check run has a conclusion when it is complete, a status context has a
/// state.
fn checks_of(rollup: &Value) -> Checks {
    let mut checks = Checks::default();
    for check in rollup.as_array().into_iter().flatten() {
        let result = check["conclusion"]
            .as_str()
            .filter(|conclusion| !conclusion.is_empty())
            .or_else(|| check["state"].as_str())
            .unwrap_or_default();
        match result {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => checks.passed += 1,
            "FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "ERROR"
            | "STARTUP_FAILURE" => checks.failed += 1,
            _ => checks.pending += 1,
        }
    }
    checks
}

/// A time of GitHub, `2026-11-12T13:22:01Z`, in seconds since 1970.
fn parse_time(text: &str) -> Option<u64> {
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    // The days since 1970 of a date of the Gregorian calendar.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_of_github_gives_its_seconds() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_time("2026-11-12T13:22:01Z"), Some(1_794_489_721));
        assert_eq!(parse_time("x"), None);
    }

    #[test]
    fn the_checks_are_counted_by_their_result() {
        let rollup = serde_json::json!([
            {"conclusion": "SUCCESS"},
            {"conclusion": "SKIPPED"},
            {"conclusion": "FAILURE"},
            {"status": "IN_PROGRESS", "conclusion": ""},
            {"state": "PENDING"},
            {"state": "SUCCESS"}
        ]);
        assert_eq!(
            checks_of(&rollup),
            Checks {
                passed: 3,
                failed: 1,
                pending: 2
            }
        );
    }

    #[test]
    fn a_branch_keeps_its_open_pull_request() {
        let closed = serde_json::json!({"number": 2, "state": "CLOSED", "headRefName": "fix"});
        let open = serde_json::json!({"number": 1, "state": "OPEN", "headRefName": "fix"});
        let mut pulls: HashMap<String, Pull> = HashMap::new();
        keep(&mut pulls, "fix", pull_of(&closed));
        keep(&mut pulls, "fix", pull_of(&open));
        keep(&mut pulls, "fix", pull_of(&closed));
        assert_eq!(pulls["fix"].number, 1);
    }
}
