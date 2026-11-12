# herdr-omnibar

**One bar to go to an agent, a tab or a workspace of [herdr](https://herdr.dev), and to create a tab or a worktree.**

The work on this plugin has started. The popup does not work yet: this README follows the code.

## The design

- 2 doors. `open` goes to what exists: the agents, the tabs, the worktrees and the folders. `new` creates a tab or a worktree.
- A creation has 2 steps: the place (a project, a workspace or a folder), then what to create (a new tab or a new worktree, with its name).
- The typed text is a filter in the step "where", and a name in the step "what". It is never both.
- A worktree always gets its own workspace, under its project: `herdr-worktrunk ▸ fix-restore`.

## Actions

| Action | Opens the popup on |
|--------|--------------------|
| `herdr-omnibar.open` | All: agents, tabs, worktrees and folders |
| `herdr-omnibar.new` | The creation of a tab or a worktree |
| `herdr-omnibar.agents`, `.tabs`, `.worktrees`, `.folders`, `.actions` | One filter |
| `herdr-omnibar.keys` | A tool that shows the keys that the popup receives |

Bind a key to an action in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr-omnibar.open"
```

## Configuration

`config.toml` in the config folder of the plugin (`~/.config/herdr/plugins/config/herdr-omnibar/`):

```toml
size = "default"   # "default": 74 % × 62 % of the window, like the other popups. "big": 90 % × 80 %.
```

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

## License

[MIT](LICENSE)
