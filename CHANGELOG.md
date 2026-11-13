# Changelog

The changes of each release of herdr-omnibar. The workflow release-pr writes this file.

## [2026.11.5](https://github.com/owpac/herdr-omnibar/releases/tag/2026.11.5) - 2026-11-13

### Bug fixes

- the watcher follows the active version that herdr gives ([#12](https://github.com/owpac/herdr-omnibar/pull/12))

## [2026.11.4](https://github.com/owpac/herdr-omnibar/releases/tag/2026.11.4) - 2026-11-13

### Features

- the key filters chooses the filters, with a filter Workspaces and the worktrees of each open project ([#9](https://github.com/owpac/herdr-omnibar/pull/9))

### Bug fixes

- a shorter placeholder in the filter Worktrees, and new images of the README ([#11](https://github.com/owpac/herdr-omnibar/pull/11))

## [2026.11.3](https://github.com/owpac/herdr-omnibar/releases/tag/2026.11.3) - 2026-11-13

### Documentation

- the header of install.sh gives the 2 commits that can download the release binary ([#7](https://github.com/owpac/herdr-omnibar/pull/7))

## [2026.11.2](https://github.com/owpac/herdr-omnibar/releases/tag/2026.11.2) - 2026-11-12

### Bug fixes

- install.sh downloads the release binary when the checkout is the commit of the tag ([#5](https://github.com/owpac/herdr-omnibar/pull/5))

## [2026.11.1](https://github.com/owpac/herdr-omnibar/releases/tag/2026.11.1) - 2026-11-12

### Features

- the skeleton of herdr-omnibar: manifest, CLI, config and the keys tool
- the model reads the herdr snapshot, and finds the project of each workspace
- the popup searches the agents, the tabs and the workspaces, and goes to the selected row
- the creation in 2 steps: the place, then a new tab, a new workspace or a new worktree
- ctrl+Enter, ctrl+Backspace, ctrl+R and the mouse, with the option features
- the filter Actions runs an action of a herdr plugin, and the preview of a worktree has 3 views
- open shows the worktrees of the current project and a create row, and ctrl+A changes the agent of a worktree
- a watcher gives each tab with one agent the title of its task (feature auto_titles)
- ctrl+Backspace removes the worktree of a branch, with herdr-worktrunk if it is installed
- **ui:** the omnibar comes closer to the mockup
- the shortcuts work from any filter, # opens the folders, and % opens the panes (feature pane_rows)
- a click on a chip opens its filter, and a click on a key of the footer runs it
- the feature running_time shows the running time of each agent and command
- **ui:** the omnibar takes the colors, the spaces and the texts of the mockup
- **ui:** the omnibar takes the colors of the herdr theme, also its custom colors
- **ui:** a branch row shows the git state of its worktree, like the mockup
- **ui:** round pills with a Nerd Font (feature nerd_font), and the logos of the agents when their font is installed
- **ui:** the key pills chooses the ends of the pills: square by default, round with a Nerd Font, or flat
- the rows come in their order of use, the tab before first (feature recent_first)
- ctrl+F forks the session of the selected agent into a new tab, with herdr-fork
- ctrl+Backspace on a pane row closes this pane only, and an agent row shows ctrl+F in the footer
- the step what offers a new split next to the focused pane of the current workspace
- the pull request of each branch, as a badge and as a view of the preview (feature pull_requests)

### Bug fixes

- the popup closes at once, and a detached process runs the creation
- ctrl+Backspace asks only for a worktree that herdr-worktrunk or wt can remove, from the states of its agents
- a folder shows once in the places, also with 2 paths such as ~/Workspace and ~/workspace
- **ui:** the preview of a branch without commit says "no commit ahead of main", like the mockup
- **ui:** the theme terminal keeps fixed greys, and a key with a wrong type in the herdr config keeps the other keys
- after a shortcut out of the creation, the agent and the action line use the current place, not the old place of the step what
- the calls to wt share one function, and wt remove runs with stdin closed, like wt switch
- an action, a fork or a removal waits for the end of the popup only while the popup runs, not 2 s more
- mise installs git-cliff, because the release pull request needs it ([#3](https://github.com/owpac/herdr-omnibar/pull/3))

### Performance

- the popup reads the last agents of the folders one time, not at each drawing
- no clone of the places, of the branches and of the JSON of herdr, and one pass over the rows for each frame
- the context of a row reads the place of the creation only for a new tab or a new workspace
- an action after the popup starts 130 ms sooner: herdr removes a popup about 10 ms after the end of its process

### Refactoring

- the module ui splits into search, list, preview, action, footer, help, style and text
- the action line, the preview and the footer split into small functions
- the module app splits into the state, the input and the loop
- Checkout::main gives the main checkout, in place of 3 copies of this logic
- one function finds an enabled plugin, in place of 4 copies
- times gives the time now and the text of an age, in place of 3 copies
- the action line builds the text of a new split, in place of a replace in the text of a new tab
- App takes the order of use itself, and the popup reads and writes recent.json one time
- the views of the preview of a worktree are an enum, in place of the indexes 0 to 3

### Documentation

- the README gives the doors, the keys and the options of the omnibar
- the README tells which titles of an agent replace the name of a tab
- the README gives the limits of the colors of the herdr theme
- the comments of choices and update_model match their functions again
- the README shows the omnibar, with images that docs/make-images makes from a demo

### Tests

- the tests of the repository also pass in a linked worktree
- the direction of a split is a function with a test
- the branch test makes its own repository, because the CI checks out a pull request without the branch main ([#2](https://github.com/owpac/herdr-omnibar/pull/2))

### Build and CI

- the checks, the release pull request and the release of the binaries, like herdr-worktrunk
