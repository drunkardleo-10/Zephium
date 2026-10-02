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
];

/// Interface languages, by the locale ids the frame's message catalog uses.
/// A translation lands by adding its locale here and to `project.inlang`.
pub const LANGUAGES: &[&str] = &["system", "en"];

pub fn value_allowed(key: &str, value: &str) -> bool {
    match key {
        "search.custom-url" => value.is_empty() || crate::search::valid_template(value),
        "search.engine" => crate::search::SearchEngine::from_id(value).is_some(),
        "appearance" => matches!(value, "system" | "light" | "dark"),
        "sidebar.mode" => matches!(value, "default" | "compact"),
        "ui.accent" => matches!(value, "graphite" | "sky" | "sage" | "rose"),
        "ui.newtab-clock-format" => matches!(value, "system" | "12h" | "24h"),
        "ui.tab-layout" => matches!(value, "vertical" | "horizontal"),
        "ui.language" => LANGUAGES.contains(&value),
        "ui.density" => matches!(value, "comfortable" | "compact"),
        "ui.text-size" => matches!(value, "small" | "default" | "large"),
        "tabs.new-position" => matches!(value, "end" | "after-current"),
        "tabs.after-close" => matches!(value, "next" | "previous" | "recent"),
        // Native writes `pending` on a fresh install; chrome only ever
        // finishes it, or asks for it again to replay.
        "onboarding" => matches!(value, "pending" | "done"),
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
        | "tabs.switch-to-open" => {
            matches!(value, "true" | "false")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_closed_and_values_are_bounded() {
        assert!(value_allowed("sidebar.mode", "compact"));
        assert!(value_allowed("ui.accent", "sage"));
        assert!(value_allowed("ui.newtab-clock-format", "24h"));
        assert!(value_allowed("ui.newtab-tasks", "false"));
        assert!(value_allowed("ui.language", "en"));
        assert!(value_allowed("tabs.after-close", "recent"));
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
