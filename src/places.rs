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

use crate::model::{Model, TabSummary};

/// The kind of a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An open workspace: a creation adds a tab to it.
    Workspace,
    /// A git project without a workspace: a creation opens a workspace.
    Project,
    /// A folder outside git, without a workspace.
    Folder,
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

impl Place {
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
                main: space.checkout.as_ref().map(|checkout| {
                    if checkout.is_main() {
                        checkout.root.clone()
                    } else {
                        checkout
                            .common
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| checkout.root.clone())
                    }
                }),
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
    let mut seen: HashSet<PathBuf> = open
        .iter()
        .flat_map(|place| [Some(place.folder.clone()), place.main.clone()])
        .flatten()
        .collect();
    if let Some(home) = env::var_os("HOME") {
        seen.insert(PathBuf::from(home));
    }
    let mut projects = Vec::new();
    let mut folders = Vec::new();
    let mut add = |folder: PathBuf, from_zoxide: bool| {
        if !seen.insert(folder.clone()) {
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
}

/// The local branches of a git repository, with their worktrees.
pub fn branches(main: &Path) -> Vec<Branch> {
    let names = git(
        main,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
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
    names
        .lines()
        .filter(|name| !name.is_empty())
        .map(|name| Branch {
            name: name.to_string(),
            worktree: by_branch
                .iter()
                .find(|(branch, _)| branch == name)
                .map(|(_, folder)| folder.clone()),
        })
        .collect()
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
    fn this_repository_is_a_project_with_its_branch_main() {
        // The tests also run in a linked worktree: the project is the main checkout.
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let common = git(
            &here,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        let main = PathBuf::from(common.trim())
            .parent()
            .map(Path::to_path_buf)
            .unwrap();
        assert!(is_project(&main));
        let branches = branches(&main);
        let branch = branches.iter().find(|branch| branch.name == "main");
        assert_eq!(
            branch.and_then(|branch| branch.worktree.clone()),
            Some(main)
        );
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
