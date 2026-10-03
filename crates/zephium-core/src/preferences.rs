//! Validated UI preferences shared by desktop admission and the application actor.
pub const KEYS: &[&str] = &[
    "appearance",
    "search.engine",
    "search.custom-url",
    "search.suggestions",
    "sidebar.mode",
    "ui.accent",
    "ui.reduce-motion",
    "ui.newtab-greeting",
    "ui.newtab-name",
    "ui.newtab-clock",
    "ui.newtab-clock-format",
    "ui.newtab-tasks",
    "ui.tab-layout",
    "onboarding",
    "ai.enabled",
    "work.enabled",
    "ui.language",
    "ui.density",
    "ui.text-size",
    "ui.contrast",
    "search.history",
    "tabs.new-position",
    "tabs.after-close",
    "tabs.switch-to-open",
    "tabs.startup",
    "time.track",
    "time.retention",
    "focus.minutes",
    "focus.breaks",
    "focus.goal",
    "focus.blocked",
];

/// Interface languages a person may choose, by BCP 47 tag. One without a
/// translation yet shows English; a translation lands by adding its locale to
/// `project.inlang`. Mirrors `INTERFACE_LANGUAGES` in the frame.
pub const LANGUAGES: &[&str] = &[
    "system", "en", "ar", "bg", "bn", "ca", "cs", "da", "de", "el", "es", "es-419", "et", "fa",
    "fi", "fil", "fr", "he", "hi", "hr", "hu", "id", "it", "ja", "ko", "lt", "lv", "ms", "nb",
    "nl", "pl", "pt-BR", "pt-PT", "ro", "ru", "sk", "sl", "sr", "sv", "sw", "ta", "te", "th", "tr",
    "uk", "vi", "zh-CN", "zh-TW",
];

pub fn value_allowed(key: &str, value: &str) -> bool {
    match key {
        "search.custom-url" => value.is_empty() || crate::search::valid_template(value),
        "search.engine" => crate::search::SearchEngine::from_id(value).is_some(),
        "appearance" => matches!(value, "system" | "light" | "dark"),
        "sidebar.mode" => matches!(value, "default" | "compact"),
        "ui.accent" => matches!(
            value,
            "graphite" | "sky" | "sage" | "rose" | "amber" | "teal" | "lavender" | "orchid"
        ),
        "ui.newtab-clock-format" => matches!(value, "system" | "12h" | "24h"),
        "ui.tab-layout" => matches!(value, "vertical" | "horizontal"),
        "ui.language" => LANGUAGES.contains(&value),
        "ui.density" => matches!(value, "comfortable" | "compact"),
        "ui.text-size" => matches!(value, "small" | "default" | "large"),
        "tabs.new-position" => matches!(value, "end" | "after-current"),
        "tabs.after-close" => matches!(value, "next" | "previous" | "recent"),
        // A new tab in front at launch; everything restored stays in the sidebar.
        "tabs.startup" => matches!(value, "continue" | "new-tab"),
        // Native writes `pending` on a fresh install; chrome only ever
        // finishes it, or asks for it again to replay.
        "onboarding" => matches!(value, "pending" | "done"),
        "time.retention" => matches!(value, "30" | "90" | "365"),
        "focus.minutes" => value.parse::<u16>().is_ok_and(|minutes| {
            value == minutes.to_string()
                && (crate::time::MIN_FOCUS_MINUTES..=crate::time::MAX_FOCUS_MINUTES)
                    .contains(&minutes)
        }),
        "focus.goal" => matches!(value, "30" | "60" | "120" | "180" | "240" | "360"),
        // One site per line, each already in the form the shell matches on.
        "focus.blocked" => {
            let sites: Vec<&str> = value.lines().collect();
            sites.len() <= crate::time::MAX_BLOCKED_SITES
                && !value.ends_with('\n')
                && sites.iter().enumerate().all(|(index, site)| {
                    crate::time::normalize_site(site).as_deref() == Some(*site)
                        && !sites[..index].contains(site)
                })
        }
        "search.suggestions"
        | "ui.reduce-motion"
        | "ui.newtab-greeting"
        | "ui.newtab-name"
        | "ui.newtab-clock"
        | "ui.newtab-tasks"
        | "ai.enabled"
        | "work.enabled"
        | "ui.contrast"
        | "search.history"
        | "tabs.switch-to-open"
        | "time.track"
        | "focus.breaks" => {
            matches!(value, "true" | "false")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_settings_hold_normalized_sites_and_bounded_numbers() {
        assert!(value_allowed("focus.blocked", ""));
        assert!(value_allowed("focus.blocked", "x.com\nyoutube.com"));
        for value in [
            "x.com\n",
            "x.com\nx.com",
            "https://x.com",
            "www.x.com",
            "X.com",
            "x",
        ] {
            assert!(!value_allowed("focus.blocked", value), "{value:?}");
        }
        assert!(value_allowed("focus.minutes", "25"));
        assert!(value_allowed("focus.minutes", "180"));
        for value in ["4", "181", "025", "25.0", ""] {
            assert!(!value_allowed("focus.minutes", value), "{value:?}");
        }
        assert!(value_allowed("time.retention", "365"));
        assert!(!value_allowed("time.retention", "7"));
        assert!(value_allowed("time.track", "false"));
    }

    #[test]
    fn settings_are_closed_and_values_are_bounded() {
        assert!(value_allowed("sidebar.mode", "compact"));
        assert!(value_allowed("ui.accent", "sage"));
        assert!(value_allowed("ui.accent", "orchid"));
        assert!(!value_allowed("ui.accent", "purple"));
        assert!(value_allowed("ui.newtab-clock-format", "24h"));
        assert!(value_allowed("ui.newtab-tasks", "false"));
        assert!(value_allowed("ui.language", "en"));
        assert!(value_allowed("tabs.after-close", "recent"));
        assert!(value_allowed("tabs.startup", "new-tab"));
        assert!(!value_allowed("tabs.startup", "pages"));
        assert!(value_allowed("ui.text-size", "large"));
        assert!(value_allowed("ui.tab-layout", "horizontal"));
        assert!(value_allowed("onboarding", "done"));
        for key in ["ai.enabled", "work.enabled"] {
            assert!(KEYS.contains(&key));
            assert!(value_allowed(key, "true"));
            assert!(value_allowed(key, "false"));
            for value in ["", "0", "1", "TRUE", " false", "false "] {
                assert!(!value_allowed(key, value));
            }
        }
        for (key, value) in [
            ("ui.custom-css", "body{}"),
            ("ui.accent", "url(evil)"),
            ("ui.newtab-style", "clock"),
            ("ui.newtab-clock-format", "12-hour"),
            ("ui.newtab-greeting", "yes"),
            ("ui.newtab-logo", "true"),
            ("appearance", "sepia"),
            ("ui.language", "klingon"),
            ("ui.density", "tight"),
            ("tabs.new-position", "start"),
            ("tabs.switch-to-open", "maybe"),
            ("ui.tab-layout", "diagonal"),
            ("onboarding", "skipped"),
        ] {
            assert!(!value_allowed(key, value));
        }
    }
}
