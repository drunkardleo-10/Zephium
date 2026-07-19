#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

const MAX_ACCELERATOR_BYTES: usize = 128;

pub(crate) const LAUNCHER_COMMAND_ID: &str = "launcher.toggle";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxLauncherKey {
    Space,
    Tab,
    AsciiAlphanumeric(u8),
}

/// The one Linux launcher-accelerator representation consumed by the portal,
/// direct X11 grabs, and focused GTK fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxLauncherShortcut {
    ctrl: bool,
    shift: bool,
    alt: bool,
    key: LinuxLauncherKey,
}

impl LinuxLauncherShortcut {
    pub(crate) fn parse(accelerator: &str) -> Option<Self> {
        if accelerator.is_empty() || accelerator.len() > MAX_ACCELERATOR_BYTES {
            return None;
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;
        for raw in accelerator.split('+') {
            let token = raw.trim();
            match token {
                "CmdOrCtrl" | "Ctrl" | "Control" => {
                    if key.is_some() || ctrl {
                        return None;
                    }
                    ctrl = true;
                }
                "Shift" => {
                    if key.is_some() || shift {
                        return None;
                    }
                    shift = true;
                }
                "Alt" | "Option" => {
                    if key.is_some() || alt {
                        return None;
                    }
                    alt = true;
                }
                // The core/GTK representation has no Meta bit. Reject it so
                // the three Linux backends can never bind different keys.
                "Cmd" | "Command" | "Super" => return None,
                "" => return None,
                token if key.is_none() => key = Some(parse_key(token)?),
                _ => return None,
            }
        }
        Some(Self {
            ctrl,
            shift,
            alt,
            key: key?,
        })
        // The focused GTK handler intentionally ignores unmodified/Shift-only
        // keys; reject them globally too instead of creating split semantics.
        .filter(|shortcut| shortcut.ctrl || shortcut.alt)
    }

    pub(crate) fn ctrl(self) -> bool {
        self.ctrl
    }

    pub(crate) fn shift(self) -> bool {
        self.shift
    }

    pub(crate) fn alt(self) -> bool {
        self.alt
    }

    pub(crate) fn key(self) -> LinuxLauncherKey {
        self.key
    }

    pub(crate) fn focused_shortcut(self) -> zephium_core::ports::engine::Shortcut {
        let key = match self.key {
            LinuxLauncherKey::Space => 0x20,
            LinuxLauncherKey::Tab => 0x09,
            LinuxLauncherKey::AsciiAlphanumeric(key) if key.is_ascii_alphabetic() => {
                u32::from(key.to_ascii_uppercase())
            }
            LinuxLauncherKey::AsciiAlphanumeric(key) => u32::from(key),
        };
        zephium_core::ports::engine::Shortcut {
            id: LAUNCHER_COMMAND_ID.to_owned(),
            ctrl: self.ctrl,
            shift: self.shift,
            alt: self.alt,
            key,
        }
    }

    pub(crate) fn portal_trigger(self) -> String {
        let mut parts = Vec::with_capacity(4);
        if self.ctrl {
            parts.push("CTRL".to_owned());
        }
        if self.alt {
            parts.push("ALT".to_owned());
        }
        if self.shift {
            parts.push("SHIFT".to_owned());
        }
        let key = match self.key {
            LinuxLauncherKey::Space => "space".to_owned(),
            LinuxLauncherKey::Tab => "Tab".to_owned(),
            LinuxLauncherKey::AsciiAlphanumeric(key) => char::from(key).to_string(),
        };
        parts.push(key);
        parts.join("+")
    }
}

fn parse_key(token: &str) -> Option<LinuxLauncherKey> {
    if token.eq_ignore_ascii_case("space") {
        return Some(LinuxLauncherKey::Space);
    }
    if token.eq_ignore_ascii_case("tab") {
        return Some(LinuxLauncherKey::Tab);
    }
    if token.len() == 1 && token.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(LinuxLauncherKey::AsciiAlphanumeric(
            token.as_bytes()[0].to_ascii_lowercase(),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerator_has_one_strict_linux_meaning() {
        let shortcut = LinuxLauncherShortcut::parse("CmdOrCtrl+Shift+Space")
            .expect("valid Linux launcher shortcut");
        assert!(shortcut.ctrl());
        assert!(shortcut.shift());
        assert!(!shortcut.alt());
        assert_eq!(shortcut.key(), LinuxLauncherKey::Space);
        assert_eq!(shortcut.portal_trigger(), "CTRL+SHIFT+space");
        assert_eq!(shortcut.focused_shortcut().key, 0x20);

        let shortcut =
            LinuxLauncherShortcut::parse("Ctrl+Alt+T").expect("valid alphanumeric shortcut");
        assert_eq!(shortcut.portal_trigger(), "CTRL+ALT+t");
        assert_eq!(shortcut.focused_shortcut().key, u32::from(b'T'));
    }

    #[test]
    fn accelerator_rejects_divergent_or_ambiguous_forms() {
        for invalid in [
            "",
            "Shift+Space",
            "Ctrl+Shift",
            "Ctrl+T+X",
            "Ctrl+Escape",
            "Ctrl+Control+T",
            "Ctrl+Ctrl+T",
            "Ctrl+Alt+Option+T",
            "T+Ctrl",
            "Cmd+Space",
            "Command+Space",
            "Super+Space",
            "Ctrl++T",
            "Ctrl+é",
        ] {
            assert!(
                LinuxLauncherShortcut::parse(invalid).is_none(),
                "accepted {invalid:?}"
            );
        }
        let oversized = format!("Ctrl+{}", "x".repeat(MAX_ACCELERATOR_BYTES));
        assert!(LinuxLauncherShortcut::parse(&oversized).is_none());
    }
}
