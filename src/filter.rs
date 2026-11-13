//! The filters of the popup. Each action of herdr-plugin.toml opens the popup on one filter.

/// A filter of the popup: the kind of rows that it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Agents, tabs, worktrees and folders: the bar to go somewhere.
    All,
    /// The creation of a tab or a worktree: the place, then what to create.
    New,
    Agents,
    Tabs,
    Worktrees,
    Folders,
    Actions,
    /// One row for each pane. Off by default: add "panes" to the filters of config.toml.
    Panes,
}

impl Filter {
    /// The filter of a name, as the actions and the variable OMNIBAR_FILTER give it.
    pub fn parse(name: &str) -> Option<Filter> {
        match name {
            "all" => Some(Filter::All),
            "new" => Some(Filter::New),
            "agents" => Some(Filter::Agents),
            "tabs" => Some(Filter::Tabs),
            "worktrees" => Some(Filter::Worktrees),
            "folders" => Some(Filter::Folders),
            "actions" => Some(Filter::Actions),
            "panes" => Some(Filter::Panes),
            _ => None,
        }
    }

    /// The label of the filter in the popup.
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::New => "New",
            Filter::Agents => "Agents",
            Filter::Tabs => "Tabs",
            Filter::Worktrees => "Worktrees",
            Filter::Folders => "Folders",
            Filter::Actions => "Actions",
            Filter::Panes => "Panes",
        }
    }

    /// The key that opens the filter on an empty search.
    pub fn shortcut(self) -> Option<char> {
        match self {
            Filter::Agents => Some('@'),
            Filter::Folders => Some('#'),
            Filter::New => Some('+'),
            Filter::Actions => Some('>'),
            Filter::Panes => Some('%'),
            _ => None,
        }
    }

    /// The filter of a key on an empty search.
    pub fn of_shortcut(key: char) -> Option<Filter> {
        [
            Filter::Agents,
            Filter::Folders,
            Filter::New,
            Filter::Actions,
            Filter::Panes,
        ]
        .into_iter()
        .find(|filter| filter.shortcut() == Some(key))
    }

    /// The name of the filter, for the actions and the variable OMNIBAR_FILTER.
    pub fn name(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::New => "new",
            Filter::Agents => "agents",
            Filter::Tabs => "tabs",
            Filter::Worktrees => "worktrees",
            Filter::Folders => "folders",
            Filter::Actions => "actions",
            Filter::Panes => "panes",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_gives_its_filter_back() {
        for name in [
            "all",
            "new",
            "agents",
            "tabs",
            "worktrees",
            "folders",
            "actions",
            "panes",
        ] {
            assert_eq!(Filter::parse(name).map(Filter::name), Some(name));
        }
    }

    #[test]
    fn an_unknown_name_gives_no_filter() {
        assert_eq!(Filter::parse("everything"), None);
    }

    #[test]
    fn each_shortcut_gives_its_filter_back() {
        for key in ['@', '#', '+', '>', '%'] {
            assert_eq!(
                Filter::of_shortcut(key).and_then(Filter::shortcut),
                Some(key)
            );
        }
        assert_eq!(Filter::of_shortcut('?'), None);
    }
}
