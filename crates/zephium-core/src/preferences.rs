//! Validated UI preferences shared by desktop admission and the application actor.
pub const KEYS: &[&str] = &[
    "appearance",
    "search.engine",
    "search.custom-url",
    "search.suggestions",
    "sidebar.mode",
    "tools.presentation",
    "ui.accent",
    "ui.reduce-motion",
    "ui.newtab-logo",
    "ui.newtab-shortcuts",
];

pub fn value_allowed(key: &str, value: &str) -> bool {
    match key {
        "search.custom-url" => value.is_empty() || crate::search::valid_template(value),
        "search.engine" => crate::search::SearchEngine::from_id(value).is_some(),
        "search.suggestions" => matches!(value, "true" | "false"),
        "appearance" => matches!(value, "system" | "light" | "dark"),
        "tools.presentation" => matches!(value, "follow_layout" | "floating"),
        "sidebar.mode" => matches!(value, "default" | "compact"),
        "ui.accent" => matches!(value, "graphite" | "sky" | "sage" | "rose"),
        "ui.reduce-motion" | "ui.newtab-logo" | "ui.newtab-shortcuts" => {
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
        assert!(value_allowed("ui.newtab-logo", "false"));
        for (key, value) in [
            ("ui.custom-css", "body{}"),
            ("ui.accent", "url(evil)"),
            ("ui.newtab-logo", "yes"),
            ("appearance", "sepia"),
        ] {
            assert!(!value_allowed(key, value));
        }
    }
}
