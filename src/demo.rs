//! The demo of the README (docs/make-images). With OMNIBAR_DEMO=<folder>, the omnibar reads in this
//! folder what it asks herdr for: the snapshot (snapshot.json), the screens of the panes
//! (panes/<pane id>.txt), the actions of the plugins (actions.json) and the running times
//! (times.json, in seconds by pane id). It reads no plugin, no process and no zoxide: the images
//! show no real project, and no real data of the machine enters the demo.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

/// The folder of the demo, if OMNIBAR_DEMO gives it.
fn folder() -> Option<PathBuf> {
    env::var_os("OMNIBAR_DEMO").map(PathBuf::from)
}

/// True in the demo.
pub fn active() -> bool {
    folder().is_some()
}

/// The text of a file of the demo. None outside the demo. An empty text if the file is missing.
pub fn read(name: &str) -> Option<String> {
    Some(fs::read_to_string(folder()?.join(name)).unwrap_or_default())
}

/// The running time of each pane, in seconds.
pub fn times() -> Option<HashMap<String, u64>> {
    read("times.json").map(|text| serde_json::from_str(&text).unwrap_or_default())
}
