//! The places of a creation: the open workspaces, the git projects without a workspace, and the
//! other folders.
//!
//! The projects come from the roots of config.toml (2 levels deep) and from zoxide. A project is
//! a folder with a `.git` folder: its main checkout. A linked worktree has a `.git` file: it is
//! not a project, it belongs to its project.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{Checkout, Model, TabSummary};

/// The kind of a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An open workspace: a creation adds a tab to it.
    Workspace,
    /// A git project without a workspace: a creation opens a workspace.
    Project,
    /// A folder outside git, without a workspace.
    Folder,
    /// The home folder, for a new workspace that the typed text labels: the first row of the
    /// step "where" of the filter New. ↵ creates the workspace at once.
    Home,
}

/// A place where the omnibar can create a tab or a worktree.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub kind: Kind,
    /// The name: the workspace (`project ▸ branch` for a worktree), or the folder.
    pub label: String,
    /// The name of the project, without the branch.
    pub project: String,
    pub folder: PathBuf,
    pub workspace_id: Option<String>,
    /// The main checkout of the git repository of the place.
    pub main: Option<PathBuf>,
    /// The tabs of the workspace.
    pub tabs: Vec<TabSummary>,
}

/// The message of the row "new workspace…" when no text is typed.
pub const NAME_MISSING: &str = "type the name of the new workspace";

impl Place {
    /// The place of the row "new workspace…": the home folder, with the typed text as its label.
    /// The home folder can already have a workspace: the new workspace is another one.
    pub fn home(name: &str) -> Place {
        Place {
            kind: Kind::Home,
            label: name.trim().to_string(),
            project: "~".to_string(),
            folder: env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
            workspace_id: None,
            main: None,
            tabs: Vec::new(),
        }
    }

    /// True for the row "new workspace…".
    pub fn is_home(&self) -> bool {
        self.kind == Kind::Home
    }

    /// True for the row "new workspace “text”": ↵ creates the workspace at once.
    pub fn creates_at_once(&self) -> bool {
        self.is_home() && !self.label.is_empty()
    }

    /// True if the typed name labels the new workspace of this place, and not its first tab: the
    /// place already has a workspace, or it is the row "new workspace…".
    pub fn labels_new_workspace(&self) -> bool {
        self.workspace_id.is_some() || self.is_home()
    }

    /// A key that stays the same between 2 refreshes.
    pub fn key(&self) -> String {
        match &self.workspace_id {
            Some(id) => format!("workspace:{id}"),
            None => format!("folder:{}", self.folder.display()),
        }
    }
}

/// The open workspaces, as places.
pub fn workspaces(model: &Model) -> Vec<Place> {
    model
        .spaces
        .iter()
        .filter_map(|space| {
            let folder = space.folder.clone()?;
            Some(Place {
                kind: Kind::Workspace,
                label: space.label(),
                project: space.project(),
                main: space.checkout.as_ref().map(Checkout::main),
                folder,
                workspace_id: Some(space.workspace.workspace_id.clone()),
                tabs: model
                    .workspace_tabs
                    .get(&space.workspace.workspace_id)
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// The roots of the projects, by default.
pub const DEFAULT_ROOTS: [&str; 7] = [
    "~/Workspace",
    "~/workspace",
    "~/projects",
    "~/src",
    "~/code",
    "~/dev",
    "~/repos",
];

/// A path with ~ for the home folder, as an absolute path.
pub fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix('~'), env::var_os("HOME")) {
        (Some(rest), Some(home)) => {
            PathBuf::from(format!("{}{rest}", PathBuf::from(home).display()))
        }
        _ => PathBuf::from(path),
    }
}

/// The places without a workspace: the projects of the roots and of zoxide, then the folders of
/// zoxide. A folder of an open workspace is not in the list.
pub fn others(roots: &[String], open: &[Place]) -> Vec<Place> {
    // A folder is known by its inode: on macOS, ~/Workspace and ~/workspace are the same folder.
    let mut seen: HashSet<(u64, u64)> = open
        .iter()
        .flat_map(|place| [Some(place.folder.clone()), place.main.clone()])
        .flatten()
        .filter_map(|folder| identity(&folder))
        .collect();
    if let Some(home) = env::var_os("HOME").and_then(|home| identity(Path::new(&home))) {
        seen.insert(home);
    }
    let mut projects = Vec::new();
    let mut folders = Vec::new();
    let mut add = |folder: PathBuf, from_zoxide: bool| {
        if !identity(&folder).is_some_and(|key| seen.insert(key)) {
            return;
        }
        if is_project(&folder) {
            projects.push(place_of(Kind::Project, folder));
        } else if from_zoxide {
            folders.push(place_of(Kind::Folder, folder));
        }
    };
    for folder in zoxide() {
        add(folder, true);
    }
    for root in roots {
        for folder in projects_in(&expand(root)) {
            add(folder, false);
        }
    }
    projects.extend(folders);
    projects
}

/// The device and the inode of a folder: 2 paths of the same folder give the same identity.
fn identity(folder: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(folder).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

fn place_of(kind: Kind, folder: PathBuf) -> Place {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string());
    Place {
        kind,
        label: name.clone(),
        project: name,
        main: (kind == Kind::Project).then(|| folder.clone()),
        folder,
        workspace_id: None,
        tabs: Vec::new(),
    }
}

/// A place for a folder that the user typed.
pub fn folder_place(folder: PathBuf) -> Place {
    let kind = if is_project(&folder) {
        Kind::Project
    } else {
        Kind::Folder
    };
    place_of(kind, folder)
}

/// True for the main checkout of a git repository: the folder has a `.git` folder.
pub fn is_project(folder: &Path) -> bool {
    folder.join(".git").is_dir()
}

/// The git projects in a root: its folders, and the folders of its folders.
fn projects_in(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for folder in subfolders(root) {
        if is_project(&folder) {
            found.push(folder);
        } else {
            found.extend(
                subfolders(&folder)
                    .into_iter()
                    .filter(|sub| is_project(sub)),
            );
        }
    }
    found
}

/// The visible subfolders of a folder, sorted by name.
pub fn subfolders(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    folders
}

/// The folders of zoxide, the most used first. Empty without zoxide.
fn zoxide() -> Vec<PathBuf> {
    // The folders of zoxide are real folders of the machine.
    if crate::demo::active() {
        return Vec::new();
    }
    let Ok(output) = Command::new("zoxide").args(["query", "--list"]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .take(60)
        .map(PathBuf::from)
        .filter(|folder| folder.is_dir())
        .collect()
}

/// A local branch of a project, with its worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    /// The folder of its worktree, if it has one.
    pub worktree: Option<PathBuf>,
    /// The time of its last commit, in seconds since 1970.
    pub time: Option<u64>,
}

/// A git project of the branch rows: its main checkout, its default branch and its branches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    /// The name of the project, without the branch.
    pub name: String,
    pub main: PathBuf,
    /// The default branch: a new branch starts there. None if the omnibar did not read it.
    pub base: Option<String>,
    pub branches: Vec<Branch>,
}

impl Project {
    /// Read the branches and the default branch of a project.
    pub fn read(name: &str, main: &Path) -> Project {
        Project {
            name: name.to_string(),
            main: main.to_path_buf(),
            base: Some(base_branch(main)),
            branches: branches(main),
        }
    }

    /// Read only the branches with a worktree, and the default branch: the projects that are not
    /// current show only their worktrees.
    pub fn read_worktrees(name: &str, main: &Path) -> Project {
        let mut project = Project::read(name, main);
        project.branches.retain(|branch| branch.worktree.is_some());
        project
    }

    /// A project in /work/<name> for the tests: its branches, and the folders of their worktrees.
    #[cfg(test)]
    pub fn sample(name: &str, branches: &[(&str, Option<&str>)]) -> Project {
        Project {
            name: name.into(),
            main: format!("/work/{name}").into(),
            base: None,
            branches: branches
                .iter()
                .map(|(branch, worktree)| Branch {
                    name: branch.to_string(),
                    worktree: worktree.map(PathBuf::from),
                    time: None,
                })
                .collect(),
        }
    }

    /// The folders of the worktrees of the branches.
    pub fn worktrees(&self) -> Vec<PathBuf> {
        self.branches
            .iter()
            .filter_map(|branch| branch.worktree.clone())
            .collect()
    }
}

/// The local branches of a git repository, with their worktrees. The branch of the main checkout
/// comes first.
pub fn branches(main: &Path) -> Vec<Branch> {
    let names = git(
        main,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(committerdate:unix)",
            "refs/heads",
        ],
    );
    let worktrees = git(main, &["worktree", "list", "--porcelain"]);
    let mut by_branch = Vec::new();
    let mut path: Option<PathBuf> = None;
    for line in worktrees.lines() {
        if let Some(folder) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(folder));
        } else if let Some(reference) = line.strip_prefix("branch refs/heads/") {
            if let Some(folder) = path.take() {
                by_branch.push((reference.to_string(), folder));
            }
        }
    }
    let mut branches: Vec<Branch> = names
        .lines()
        .filter_map(|line| {
            let (name, time) = line.split_once('\t').unwrap_or((line, ""));
            (!name.is_empty()).then(|| Branch {
                name: name.to_string(),
                worktree: by_branch
                    .iter()
                    .find(|(branch, _)| branch == name)
                    .map(|(_, folder)| folder.clone()),
                time: time.parse().ok(),
            })
        })
        .collect();
    branches.sort_by_key(|branch| branch.worktree.as_deref() != Some(main));
    branches
}

/// The default branch of a repository, as git can compare with it: origin/HEAD, else main.
pub fn base_branch(main: &Path) -> String {
    let remote = git(main, &["rev-parse", "--abbrev-ref", "origin/HEAD"]);
    let remote = remote.trim();
    if remote.is_empty() || remote == "origin/HEAD" {
        "main".to_string()
    } else {
        remote.to_string()
    }
}

/// The standard output of a git command, or an empty text.
pub fn git(folder: &Path, arguments: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilde_becomes_the_home_folder() {
        let home = env::var("HOME").unwrap();
        assert_eq!(
            expand("~/Workspace"),
            PathBuf::from(format!("{home}/Workspace"))
        );
        assert_eq!(expand("/tmp"), PathBuf::from("/tmp"));
    }

    #[test]
    fn each_branch_of_a_project_has_its_worktree_and_main_comes_first() {
        // A new repository: the checkout of the CI can be a detached HEAD, without a branch main.
        // The real path: on macOS, the temporary folder is a symbolic link, and git gives real
        // paths.
        let folder = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(format!("omnibar-places-{}", std::process::id()));
        let main = folder.join("main");
        let linked = folder.join("linked");
        fs::create_dir_all(&main).unwrap();
        // The git config of the user can sign the commits or run hooks: the test turns them off.
        let run = |arguments: &[&str]| {
            let options = [
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ];
            git(&main, &[&options[..], arguments].concat());
        };
        run(&["init", "--quiet", "--initial-branch", "main"]);
        run(&[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--quiet",
            "--allow-empty",
            "--message",
            "start",
        ]);
        let linked_text = linked.to_str().unwrap();
        run(&["worktree", "add", "--quiet", "-b", "feature", linked_text]);
        run(&["branch", "other"]);
        let found = branches(&main);
        let (main_is_project, linked_is_project) = (is_project(&main), is_project(&linked));
        let _ = fs::remove_dir_all(&folder);
        assert!(main_is_project && !linked_is_project);
        let worktrees: Vec<(&str, Option<&Path>)> = found
            .iter()
            .map(|branch| (branch.name.as_str(), branch.worktree.as_deref()))
            .collect();
        assert_eq!(
            worktrees,
            [
                ("main", Some(main.as_path())),
                ("feature", Some(linked.as_path())),
                ("other", None)
            ]
        );
    }

    #[test]
    fn two_paths_of_the_same_folder_have_the_same_identity() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(identity(&here), identity(&here.join("src/..")));
        assert_ne!(identity(&here), identity(&here.join("src")));
        assert_eq!(identity(&here.join("missing")), None);
    }

    #[test]
    fn the_projects_of_a_root_are_found_2_levels_deep() {
        let root = env::temp_dir().join(format!("omnibar-roots-{}", std::process::id()));
        for folder in ["atlas/.git", "owpac/pac/.git", "owpac/notes", "plain"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        let found = projects_in(&root);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, [root.join("atlas"), root.join("owpac/pac")]);
    }
}
