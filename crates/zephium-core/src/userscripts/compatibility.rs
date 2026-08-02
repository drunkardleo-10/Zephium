//! Runtime-compatibility assessment for syntactically valid metadata.
//!
//! Parser acceptance is never capability admission. The initial policy is
//! deliberately disabled and records every semantic prerequisite separately.

use super::{DeclaredGrant, DeclaredRunAt, ParsedUserscriptMetadata};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptCompatibilityIssue {
    RuntimeDisabled,
    UnsupportedDirective { name: Box<str> },
    UnsupportedRunAt { value: Box<str> },
    ExplicitMainWorldGrant,
    RuntimeGrantUnavailable { name: Box<str> },
    FileAccessGrantRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserscriptCompatibility {
    pub executable: bool,
    pub issues: Vec<UserscriptCompatibilityIssue>,
}

/// Assesses the current product, which intentionally exposes no userscript
/// runtime. Keeping this separate lets storage preserve valid source without
/// presenting it as runnable or teaching the parser platform policy.
pub fn assess_userscript_compatibility(
    metadata: &ParsedUserscriptMetadata,
) -> UserscriptCompatibility {
    let mut issues = Vec::with_capacity(
        2_usize
            .saturating_add(metadata.unsupported_directives.len())
            .saturating_add(metadata.grants.len()),
    );
    issues.push(UserscriptCompatibilityIssue::RuntimeDisabled);
    issues.extend(metadata.unsupported_directives.iter().map(|directive| {
        UserscriptCompatibilityIssue::UnsupportedDirective {
            name: directive.name.clone(),
        }
    }));
    if let Some(DeclaredRunAt::Unsupported(value)) = &metadata.run_at {
        issues.push(UserscriptCompatibilityIssue::UnsupportedRunAt {
            value: value.clone(),
        });
    }
    for grant in &metadata.grants {
        match grant {
            DeclaredGrant::None => {
                issues.push(UserscriptCompatibilityIssue::ExplicitMainWorldGrant)
            }
            DeclaredGrant::Named(name) => issues
                .push(UserscriptCompatibilityIssue::RuntimeGrantUnavailable { name: name.clone() }),
        }
    }
    let requires_file =
        metadata.matches.includes().iter().any(|pattern| {
            pattern.as_str() == "<all_urls>" || pattern.as_str().starts_with("file://")
        });
    if requires_file {
        issues.push(UserscriptCompatibilityIssue::FileAccessGrantRequired);
    }
    UserscriptCompatibility {
        executable: false,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::userscripts::parse_userscript_metadata;

    #[test]
    fn parser_acceptance_never_enables_the_disabled_runtime() {
        let metadata = parse_userscript_metadata(
            "// ==UserScript==\n// @name A\n// @match https://example.com/*\n// ==/UserScript==\n",
        )
        .unwrap();
        let compatibility = assess_userscript_compatibility(&metadata);
        assert!(!compatibility.executable);
        assert_eq!(
            compatibility.issues,
            vec![UserscriptCompatibilityIssue::RuntimeDisabled]
        );
    }

    #[test]
    fn file_and_main_world_requirements_remain_explicit() {
        let metadata = parse_userscript_metadata(
            "// ==UserScript==\n// @name A\n// @match <all_urls>\n// @grant none\n// ==/UserScript==\n",
        )
        .unwrap();
        let compatibility = assess_userscript_compatibility(&metadata);
        assert!(compatibility
            .issues
            .contains(&UserscriptCompatibilityIssue::ExplicitMainWorldGrant));
        assert!(compatibility
            .issues
            .contains(&UserscriptCompatibilityIssue::FileAccessGrantRequired));
    }
}
