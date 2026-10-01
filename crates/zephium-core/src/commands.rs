//! Commands as data: one registry feeds native menus, the palette, the
//! launcher and the keymap. Execution stays in the shell.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    File,
    View,
    History,
    Window,
    /// Not placed in any menu; bound to a system-wide shortcut.
    Global,
    /// Work pane bindings: menu-hosted so page keystrokes reach them, enabled
    /// only while the pane is shown.
    Work,
}

pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub accelerator: Option<&'static str>,
    pub group: Group,
}

pub const REGISTRY: &[CommandSpec] = &[
    CommandSpec {
        id: "tab.new",
        title: "New Tab",
        accelerator: Some("CmdOrCtrl+T"),
        group: Group::File,
    },
    CommandSpec {
        id: "tab.close",
        title: "Close Tab",
        accelerator: Some("CmdOrCtrl+W"),
        group: Group::File,
    },
    CommandSpec {
        id: "tab.reopen",
        title: "Reopen Closed Tab",
        accelerator: Some("CmdOrCtrl+Shift+T"),
        group: Group::History,
    },
    CommandSpec {
        id: "split.choose",
        title: "Split View…",
        accelerator: None,
        group: Group::File,
    },
    CommandSpec {
        id: "sidebar.toggleCompact",
        title: "Compact Mode",
        accelerator: Some("CmdOrCtrl+Shift+S"),
        group: Group::View,
    },
    CommandSpec {
        id: "nav.reload",
        title: "Reload Page",
        accelerator: Some("CmdOrCtrl+R"),
        group: Group::View,
    },
    CommandSpec {
        id: "nav.stop",
        title: "Stop Loading",
        accelerator: Some("CmdOrCtrl+."),
        group: Group::View,
    },
    CommandSpec {
        id: "url.focus",
        title: "Open Location",
        accelerator: Some("CmdOrCtrl+L"),
        group: Group::View,
    },
    CommandSpec {
        id: "zoom.in",
        title: "Zoom In",
        accelerator: Some("CmdOrCtrl+="),
        group: Group::View,
    },
    CommandSpec {
        id: "zoom.out",
        title: "Zoom Out",
        accelerator: Some("CmdOrCtrl+-"),
        group: Group::View,
    },
    CommandSpec {
        id: "zoom.reset",
        title: "Actual Size",
        accelerator: Some("CmdOrCtrl+0"),
        group: Group::View,
    },
    CommandSpec {
        id: "nav.back",
        title: "Back",
        accelerator: Some("CmdOrCtrl+["),
        group: Group::History,
    },
    CommandSpec {
        id: "nav.forward",
        title: "Forward",
        accelerator: Some("CmdOrCtrl+]"),
        group: Group::History,
    },
    CommandSpec {
        id: "tab.next",
        title: "Next Tab",
        accelerator: Some("Ctrl+Tab"),
        group: Group::Window,
    },
    CommandSpec {
        id: "tab.previous",
        title: "Previous Tab",
        accelerator: Some("Ctrl+Shift+Tab"),
        group: Group::Window,
    },
    CommandSpec {
        id: "work.pane.close",
        title: "Close Pane",
        accelerator: Some("Escape"),
        group: Group::Work,
    },
    CommandSpec {
        id: "work.pane.openInBrowse",
        title: "Open Pane in Browse",
        accelerator: Some("CmdOrCtrl+Shift+Return"),
        group: Group::Work,
    },
    CommandSpec {
        id: "launcher.toggle",
        title: "Toggle Launcher",
        accelerator: Some("CmdOrCtrl+Shift+Space"),
        group: Group::Global,
    },
    CommandSpec {
        id: "theme.system",
        title: "Appearance: System",
        accelerator: None,
        group: Group::Global,
    },
    CommandSpec {
        id: "theme.light",
        title: "Appearance: Light",
        accelerator: None,
        group: Group::Global,
    },
    CommandSpec {
        id: "theme.dark",
        title: "Appearance: Dark",
        accelerator: None,
        group: Group::Global,
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedCommand {
    pub id: &'static str,
    pub title: &'static str,
    pub accelerator: Option<String>,
    pub group: Group,
}

pub fn get(id: &str) -> Option<&'static CommandSpec> {
    REGISTRY.iter().find(|c| c.id == id)
}

/// Applies keymap overrides onto the defaults. An override with an empty
/// string removes the shortcut; unknown ids are ignored.
pub fn resolve(overrides: &HashMap<String, String>) -> Vec<ResolvedCommand> {
    REGISTRY
        .iter()
        .map(|c| {
            let accelerator = match overrides.get(c.id) {
                Some(a) if a.is_empty() => None,
                Some(a) => Some(a.clone()),
                None => c.accelerator.map(Into::into),
            };
            ResolvedCommand {
                id: c.id,
                title: c.title,
                accelerator,
                group: c.group,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in REGISTRY {
            assert!(seen.insert(c.id), "duplicate command id {}", c.id);
        }
    }

    #[test]
    fn overrides_replace_and_remove_shortcuts() {
        let mut overrides = HashMap::new();
        overrides.insert("tab.new".into(), "CmdOrCtrl+N".into());
        overrides.insert("tab.close".into(), "".into());
        overrides.insert("bogus.id".into(), "CmdOrCtrl+X".into());

        let resolved = resolve(&overrides);
        let find = |id: &str| resolved.iter().find(|c| c.id == id).unwrap();
        assert_eq!(find("tab.new").accelerator.as_deref(), Some("CmdOrCtrl+N"));
        assert_eq!(find("tab.close").accelerator, None);
        assert_eq!(
            find("nav.reload").accelerator.as_deref(),
            Some("CmdOrCtrl+R")
        );
        assert!(!resolved.iter().any(|c| c.id == "bogus.id"));
    }

    #[test]
    fn compact_sidebar_is_a_registered_view_command() {
        let command = get("sidebar.toggleCompact").expect("compact sidebar command");
        assert_eq!(command.title, "Compact Mode");
        assert_eq!(command.group, Group::View);
    }

    #[test]
    fn split_selection_is_a_registered_ui_command_without_an_accelerator() {
        let command = get("split.choose").expect("split selection command");
        assert_eq!(command.title, "Split View…");
        assert_eq!(command.accelerator, None);
        assert_eq!(command.group, Group::File);
    }
}
