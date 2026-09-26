//! Sites the agent works on as the person: the profile's standing answers.
use serde::{Deserialize, Serialize};

use super::WorkError;

pub const MAX_WORK_SITE_ACCESS: usize = 512;

/// A standing answer for one site. Absent means the agent asks once per run.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkSiteAccessV1 {
    /// Work in the person's session without asking.
    Always,
    /// Ask once per run, including on a site Zephium treats as sensitive.
    Ask,
    /// Never use the person's session here; pages open privately.
    Never,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSiteEntryV1 {
    /// Registrable domain, such as slack.com.
    pub site: String,
    pub access: WorkSiteAccessV1,
}

/// A registrable domain as the store keeps it: lowercase host characters only.
pub fn validate_site(site: &str) -> Result<(), WorkError> {
    if site.is_empty()
        || site.len() > 253
        || site.starts_with(['.', '-'])
        || site.ends_with(['.', '-'])
        || !site
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
    {
        return Err(WorkError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sites_are_bare_lowercase_domains() {
        for valid in ["slack.com", "notion.so", "bbc.co.uk", "127.0.0.1"] {
            assert!(validate_site(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "Slack.com",
            "https://slack.com",
            "slack.com/",
            ".slack.com",
            "a b",
        ] {
            assert!(validate_site(invalid).is_err(), "{invalid}");
        }
    }
}
