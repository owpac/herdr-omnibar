//! The running time of the agents and of the commands (feature running_time).
//!
//! herdr gives the foreground process of each pane (`herdr pane process-info`), and `ps` gives the
//! time since its start. A thread reads them in the background: the popup opens at once, and the
//! times show when they are ready. A start time never changes: the thread reads each pane one time.

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::herdr;

/// The start time of the foreground process of each pane, read in the background.
pub struct Times {
    starts: Arc<Mutex<HashMap<String, SystemTime>>>,
    /// The panes that the thread read, or reads now.
    asked: HashSet<String>,
    sender: Sender<Vec<String>>,
}

impl Times {
    /// Start the thread.
    pub fn start() -> Times {
        let starts = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::clone(&starts);
        thread::spawn(move || read_forever(&receiver, &shared));
        Times {
            starts,
            asked: HashSet::new(),
            sender,
        }
    }

    /// Ask for the panes that the thread did not read.
    pub fn ask(&mut self, panes: impl IntoIterator<Item = String>) {
        // The demo gives its times: herdr and ps know only the real panes.
        if let Some(times) = crate::demo::times() {
            let now = SystemTime::now();
            if let Ok(mut starts) = self.starts.lock() {
                for (pane, seconds) in times {
                    starts.insert(pane, now - Duration::from_secs(seconds));
                }
            }
            return;
        }
        let new: Vec<String> = panes
            .into_iter()
            .filter(|pane| self.asked.insert(pane.clone()))
            .collect();
        if !new.is_empty() {
            let _ = self.sender.send(new);
        }
    }

    /// The running time of the foreground process of a pane, if the thread read it.
    pub fn of(&self, pane: &str) -> Option<Duration> {
        let start = *self.starts.lock().ok()?.get(pane)?;
        SystemTime::now().duration_since(start).ok()
    }
}

fn read_forever(receiver: &Receiver<Vec<String>>, starts: &Mutex<HashMap<String, SystemTime>>) {
    while let Ok(panes) = receiver.recv() {
        let processes: Vec<(String, u32)> = panes
            .into_iter()
            .filter_map(|pane| Some((pane.clone(), command_of(&pane)?)))
            .collect();
        if processes.is_empty() {
            continue;
        }
        let ages = ages_of(processes.iter().map(|(_, pid)| *pid));
        let now = SystemTime::now();
        if let Ok(mut starts) = starts.lock() {
            for (pane, pid) in processes {
                if let Some(age) = ages.get(&pid) {
                    starts.insert(pane, now - *age);
                }
            }
        }
    }
}

/// The foreground process group of a pane, if a command runs: not the shell that waits.
fn command_of(pane: &str) -> Option<u32> {
    let info = herdr::process_info(pane).ok()?;
    let group = info["foreground_process_group_id"].as_u64()? as u32;
    let shell = info["shell_pid"].as_u64().map(|pid| pid as u32);
    (Some(group) != shell).then_some(group)
}

/// The time since the start of each process, from `ps`.
fn ages_of(pids: impl Iterator<Item = u32>) -> HashMap<u32, Duration> {
    let list: Vec<String> = pids.map(|pid| pid.to_string()).collect();
    let Ok(output) = Command::new("ps")
        .args(["-o", "pid=,etime=", "-p", &list.join(",")])
        .output()
    else {
        return HashMap::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            Some((pid, parse_elapsed(parts.next()?)?))
        })
        .collect()
}

/// The elapsed time of `ps`: `[[dd-]hh:]mm:ss`.
fn parse_elapsed(text: &str) -> Option<Duration> {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let mut seconds = 0;
    for part in rest.split(':') {
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(Duration::from_secs(days * 86_400 + seconds))
}

/// The time now, in seconds since 1970.
pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|now| now.as_secs())
        .unwrap_or_default()
}

/// The age of a time in seconds since 1970, as a short text: "now" under 1 minute, else 12m,
/// 2h04m, 3d.
pub fn ago(time: u64) -> String {
    let age = now_seconds().saturating_sub(time);
    if age < 60 {
        "now".to_string()
    } else {
        short(Duration::from_secs(age))
    }
}

/// A short running time: 47s, 12m, 2h04m, 3d.
pub fn short(time: Duration) -> String {
    let seconds = time.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h{:02}m", seconds / 3_600, seconds % 3_600 / 60),
        _ => format!("{}d", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_elapsed_time_of_ps_is_parsed() {
        assert_eq!(parse_elapsed("05:07"), Some(Duration::from_secs(307)));
        assert_eq!(parse_elapsed("02:00:01"), Some(Duration::from_secs(7_201)));
        assert_eq!(
            parse_elapsed("3-01:00:00"),
            Some(Duration::from_secs(3 * 86_400 + 3_600))
        );
        assert_eq!(parse_elapsed("x"), None);
    }

    #[test]
    fn a_running_time_is_short() {
        assert_eq!(short(Duration::from_secs(47)), "47s");
        assert_eq!(short(Duration::from_secs(12 * 60 + 5)), "12m");
        assert_eq!(short(Duration::from_secs(2 * 3_600 + 4 * 60)), "2h04m");
        assert_eq!(short(Duration::from_secs(3 * 86_400 + 5)), "3d");
    }

    #[test]
    fn an_age_under_1_minute_is_now() {
        assert_eq!(ago(now_seconds()), "now");
        assert_eq!(ago(now_seconds() - 3 * 3_600), "3h00m");
        assert_eq!(ago(now_seconds() + 60), "now");
    }

    #[test]
    fn ps_gives_the_age_of_this_process() {
        let ages = ages_of(std::iter::once(std::process::id()));
        assert!(ages.contains_key(&std::process::id()));
    }
}
