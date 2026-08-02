//! Runtime-policy assessment for syntactically valid userscript metadata.
//!
//! Parser acceptance, potential runtime eligibility, and actual product
//! execution are deliberately separate claims. A source may be safe to retain
//! but incompatible with the initial isolated runtime; a potentially eligible
//! source still remains non-executable until the application and native gates
//! activate it.

use crate::ports::engine::RunAt;

use super::{DeclaredGrant, DeclaredRunAt, ParsedUserscriptMetadata};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptCompatibilityIssue {
    /// The product has not connected prepared userscripts to a live native
    /// runtime. This is always present in the current compatibility result.
    RuntimeDisabled,
    UnsupportedDirective {
        name: Box<str>,
    },
    UnsupportedRunAt {
        value: Box<str>,
    },
    DocumentIdleUnavailable,
    ExplicitMainWorldGrant,
    RuntimeGrantUnavailable {
        name: Box<str>,
    },
    /// File declarations are retained but omitted from the initial web-only
    /// runtime until a separate per-script file grant exists.
    FileAccessGrantRequired,
    /// No positive HTTP(S) declaration remains after the default-off file
    /// policy is applied.
    NoEligibleWebMatches,
    /// The initial runtime only matches a frame's direct HTTP(S) URL.
    RelatedFrameMatchingUnavailable,
}

impl UserscriptCompatibilityIssue {
    /// Whether this issue prevents construction of the initial isolated,
    /// no-API runtime descriptor.
    ///
    /// `RuntimeDisabled` is a later product-activation gate, while file access
    /// is a disclosed reduction when a usable HTTP(S) subset remains.
    pub const fn prevents_initial_preparation(&self) -> bool {
        !matches!(self, Self::RuntimeDisabled | Self::FileAccessGrantRequired)
    }
}

/// Metadata-only assessment for the initial isolated, no-API runtime subset.
///
/// `potentially_eligible` means the source can be prepared without granting a
/// capability it did not request. It does not mean that the product executes
/// the script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserscriptRuntimeEligibility {
    pub potentially_eligible: bool,
    pub issues: Vec<UserscriptCompatibilityIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserscriptCompatibility {
    /// Always false until the app/native activation and settlement path exists.
    pub executable: bool,
    /// Metadata can be represented by the initial isolated, no-API subset.
    pub potentially_eligible: bool,
    pub issues: Vec<UserscriptCompatibilityIssue>,
}

/// Assesses only the bounded initial runtime contract:
///
/// - an omitted `@grant` selects Zephium's isolated, no-API environment;
/// - explicit `@grant none` requires a main-world runtime and is refused;
/// - every named grant and unsupported directive is refused;
/// - omitted `@run-at` defaults to `document-end`, while document-start and
///   document-end are supported and document-idle is not;
/// - only direct HTTP(S) matches are potentially active. File declarations are
///   retained but require a separate grant and a file-only script is inactive.
pub fn assess_userscript_runtime_eligibility(
    metadata: &ParsedUserscriptMetadata,
) -> UserscriptRuntimeEligibility {
    let mut issues = Vec::with_capacity(
        4_usize
            .saturating_add(metadata.unsupported_directives.len())
            .saturating_add(metadata.grants.len()),
    );

    issues.extend(metadata.unsupported_directives.iter().map(|directive| {
        UserscriptCompatibilityIssue::UnsupportedDirective {
            name: directive.name.clone(),
        }
    }));
    match &metadata.run_at {
        Some(DeclaredRunAt::Supported(RunAt::DocumentIdle)) => {
            issues.push(UserscriptCompatibilityIssue::DocumentIdleUnavailable);
        }
        Some(DeclaredRunAt::Unsupported(value)) => {
            issues.push(UserscriptCompatibilityIssue::UnsupportedRunAt {
                value: value.clone(),
            });
        }
        None | Some(DeclaredRunAt::Supported(RunAt::DocumentStart | RunAt::DocumentEnd)) => {}
    }
    for grant in &metadata.grants {
        match grant {
            DeclaredGrant::None => {
                issues.push(UserscriptCompatibilityIssue::ExplicitMainWorldGrant);
            }
            DeclaredGrant::Named(name) => {
                issues.push(UserscriptCompatibilityIssue::RuntimeGrantUnavailable {
                    name: name.clone(),
                });
            }
        }
    }

    if metadata.matches.options() != Default::default() {
        issues.push(UserscriptCompatibilityIssue::RelatedFrameMatchingUnavailable);
    }

    let includes_file = metadata
        .matches
        .includes()
        .iter()
        .any(|pattern| pattern.components().includes_file());
    let includes_web = metadata
        .matches
        .includes()
        .iter()
        .any(|pattern| pattern.components().includes_http_or_https());
    if includes_file {
        issues.push(UserscriptCompatibilityIssue::FileAccessGrantRequired);
    }
    if !includes_web {
        issues.push(UserscriptCompatibilityIssue::NoEligibleWebMatches);
    }

    let potentially_eligible = !issues
        .iter()
        .any(UserscriptCompatibilityIssue::prevents_initial_preparation);
    UserscriptRuntimeEligibility {
        potentially_eligible,
        issues,
    }
}

/// Assesses the current product. This intentionally adds the global runtime
/// gate to the metadata-only result instead of misrepresenting preparation as
/// execution.
pub fn assess_userscript_compatibility(
    metadata: &ParsedUserscriptMetadata,
) -> UserscriptCompatibility {
    let eligibility = assess_userscript_runtime_eligibility(metadata);
    let mut issues = Vec::with_capacity(eligibility.issues.len().saturating_add(1));
    issues.push(UserscriptCompatibilityIssue::RuntimeDisabled);
    issues.extend(eligibility.issues);
    UserscriptCompatibility {
        executable: false,
        potentially_eligible: eligibility.potentially_eligible,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::userscripts::parse_userscript_metadata;

    fn parse(lines: &str) -> ParsedUserscriptMetadata {
        parse_userscript_metadata(&format!(
            "// ==UserScript==\n// @name A\n{lines}// ==/UserScript==\n"
        ))
        .unwrap()
    }

    #[test]
    fn product_execution_stays_disabled_while_safe_metadata_is_potentially_eligible() {
        let metadata = parse("// @match https://example.com/*\n");
        let eligibility = assess_userscript_runtime_eligibility(&metadata);
        assert!(eligibility.potentially_eligible);
        assert!(eligibility.issues.is_empty());

        let compatibility = assess_userscript_compatibility(&metadata);
        assert!(!compatibility.executable);
        assert!(compatibility.potentially_eligible);
        assert_eq!(
            compatibility.issues,
            vec![UserscriptCompatibilityIssue::RuntimeDisabled]
        );
    }

    #[test]
    fn file_reduction_is_nonblocking_only_when_a_web_subset_remains() {
        let all = assess_userscript_runtime_eligibility(&parse("// @match <all_urls>\n"));
        assert!(all.potentially_eligible);
        assert_eq!(
            all.issues,
            vec![UserscriptCompatibilityIssue::FileAccessGrantRequired]
        );

        let file = assess_userscript_runtime_eligibility(&parse("// @match file:///*\n"));
        assert!(!file.potentially_eligible);
        assert_eq!(
            file.issues,
            vec![
                UserscriptCompatibilityIssue::FileAccessGrantRequired,
                UserscriptCompatibilityIssue::NoEligibleWebMatches,
            ]
        );
    }

    #[test]
    fn explicit_or_named_grants_and_idle_are_blocking() {
        let explicit_none = assess_userscript_runtime_eligibility(&parse(
            "// @match https://example.com/*\n// @grant none\n",
        ));
        assert!(!explicit_none.potentially_eligible);
        assert!(explicit_none
            .issues
            .contains(&UserscriptCompatibilityIssue::ExplicitMainWorldGrant));

        let named = assess_userscript_runtime_eligibility(&parse(
            "// @match https://example.com/*\n// @grant GM.getValue\n",
        ));
        assert!(!named.potentially_eligible);
        assert!(named
            .issues
            .contains(&UserscriptCompatibilityIssue::RuntimeGrantUnavailable {
                name: "GM.getValue".into(),
            }));

        let idle = assess_userscript_runtime_eligibility(&parse(
            "// @match https://example.com/*\n// @run-at document-idle\n",
        ));
        assert!(!idle.potentially_eligible);
        assert!(idle
            .issues
            .contains(&UserscriptCompatibilityIssue::DocumentIdleUnavailable));
    }
}
