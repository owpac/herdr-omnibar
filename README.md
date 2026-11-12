# herdr-omnibar

**One bar to go to an agent, a tab or a workspace of [herdr](https://herdr.dev), and to create a tab or a worktree.**

## The 2 doors

- **`open`** goes to what exists. The list has the agents that need you first, then the other agents, the tabs without an agent, the workspaces and the git projects. `↵` goes to the selected row.
- **`new`** creates a tab or a worktree, in 2 steps:
  1. **Where:** the current place first, then the open workspaces, the git projects without a workspace and the other folders. Type `~` or `/` for any folder of the disk: `⇥` completes it.
  2. **What:** type the name. In a git project, `new worktree <name>` comes first: wt creates the branch, its worktree and its own workspace. `new tab` adds a tab to the workspace of the place. A name that is not a branch name selects `new tab`.

The typed text is a filter in the step "where", and a name in the step "what". It is never both. A worktree always gets its own workspace, under its project: `herdr-worktrunk ▸ fix-login`.

After `↵`, the popup closes at once. A detached process creates the tab, the workspace or the worktree, then starts the agent. If the creation fails, a herdr notification gives the error.

## Actions

Each filter has its own action. Bind a key to the action of your choice in `~/.config/herdr/config.toml`:

| Action | Opens the popup on |
|--------|--------------------|
| `herdr-omnibar.open` | All: agents, tabs, workspaces and projects |
| `herdr-omnibar.new` | The creation of a tab or a worktree |
| `herdr-omnibar.agents` | The agents |
| `herdr-omnibar.tabs` | The tabs without an agent |
| `herdr-omnibar.worktrees` | The branches of the current project, and a new worktree |
| `herdr-omnibar.folders` | The projects and the folders |
| `herdr-omnibar.actions` | The actions of the herdr plugins |
| `herdr-omnibar.keys` | A tool that shows the keys that the popup receives |

```toml
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr-omnibar.open"

[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-omnibar.new"
```

## Keys

| Key | Result |
|-----|--------|
| `↵` | Go to the selected row, choose a place, create, or run an action. |
| `⇥`, `⇧⇥` | The next or the previous filter. In the step "what": the next agent (claude, codex, opencode, shell). In a path: complete the folder. |
| `@`, `+` | On an empty search: the agents, the creation. |
| `⌃↵` | A new tab, with its agent, in the workspace of the selected row. |
| `⌃⌫` | Close the tab of the selected row, after a question on the action line. |
| `⌃R` | Rename the tab of the selected row. `↵` saves, `esc` keeps the old name. |
| `⌃Y` | In the step "what": start the agent without its permission prompts, for this tab only. |
| `⌃←`, `⌃→` | The view of the preview of a worktree: diff, log, changes. |
| `⌃O` | Show or hide the preview. |
| `⌃U`, `⌃W` | Clear the text, or delete its last word. |
| `esc` | Close the popup, or go back to the step "where". |
| `?`, `F1` | The help. |

The mouse works too: a click selects a row, a double click runs it, and the wheel moves the selection.

The omnibar remembers the last agent of each folder, in `agents.json` of its state folder.

## Configuration

`config.toml` in the config folder of the plugin (`~/.config/herdr/plugins/config/herdr-omnibar/`):

```toml
size = "default"   # "default": 74 % × 62 % of the window, like the other popups. "big": 90 % × 80 %.
roots = ["~/Workspace", "~/projects", "~/src", "~/code", "~/dev", "~/repos"]   # the git projects, 2 levels deep
preview = "full"   # "full": the 3 views of a worktree. "summary": one view.
# The optional features. Remove a name to turn its feature off.
features = ["mouse", "rename_tab", "auto_titles", "agent_logos"]
```

zoxide adds its folders to the projects and the folders, when it is installed.

## The keyboard

The omnibar asks the terminal for the keyboard protocol of kitty. herdr then forwards the modifiers of Enter, Backspace and the arrows to the popup, if your terminal sends them to herdr. The action `herdr-omnibar.keys` shows each key that the popup receives: use it to check `ctrl+Enter`, `ctrl+Backspace` and `ctrl+Left`.

## Development

```sh
mise install                                        # Rust and shellcheck of mise.toml
HERDR_OMNIBAR_BUILD=source sh scripts/install.sh    # build bin/herdr-omnibar with cargo
herdr plugin link "$PWD"
```

`herdr plugin link` does not run the build step: run `scripts/install.sh` again after each change. The checks:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
shellcheck scripts/*.sh
```

`herdr-omnibar list` prints the workspaces, the agents and the tabs, as the popup sees them.

## License

[MIT](LICENSE)
