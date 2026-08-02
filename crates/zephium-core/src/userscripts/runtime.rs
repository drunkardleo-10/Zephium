//! Pure preparation of the initial userscript runtime subset.
//!
//! A prepared value is safe input for a future platform adapter, but is not a
//! claim that the application requested activation or that native settlement
//! succeeded. No JavaScript wrapper or URL guard is generated here: adapters
//! receive the exact source and the precompiled, web-only matcher.

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use url::Url;

use crate::ids::{ScriptId, UserscriptId};
use crate::injection::{
    MatchOptions, MatchPattern, MatchPatternComponents, MatchPatternList, MatchSet, MatchSetError,
    MAX_MATCH_PATTERNS_PER_SET, MAX_MATCH_PATTERN_BYTES,
};
use crate::ports::engine::{
    RunAt, ScriptOwner, ScriptPrincipal, UserScript, World, MAX_USER_SCRIPT_BYTES,
};

use super::{
    assess_userscript_runtime_eligibility, parse_userscript_metadata, DeclaredRunAt, Userscript,
    UserscriptMetadataError, UserscriptRevision, UserscriptRuntimeEligibility,
    UserscriptSourceDigest, CURRENT_USERSCRIPT_METADATA_FORMAT,
};

/// Fixed registration charge shared with the core engine admission policy.
const PREPARED_RUNTIME_FIXED_RETAINED_BYTES: usize = 256;

/// Owner-local registration slot for the unchanged userscript source.
///
/// The durable userscript id is the security principal. Keeping the native
/// registration id fixed and owner-local makes source updates retain the same
/// diff key while delete-and-reinstall obtains a distinct key through its new
/// principal. Future implementation-owned wrappers must receive separate,
/// explicitly assigned slots instead of deriving authority from a caller.
const USERSCRIPT_SOURCE_REGISTRATION_ID: u128 = 1;

/// Mathematical upper bound for one valid prepared userscript registration.
///
/// The compiled matcher charges 512 bytes plus sixteen times canonical input
/// per pattern, and valid metadata admits at most the injection module's fixed
/// pattern count and byte limits. This is a policy ceiling, not an allocator
/// measurement.
pub const MAX_PREPARED_USERSCRIPT_RETAINED_BYTES: usize = MAX_USER_SCRIPT_BYTES
    + std::mem::size_of::<MatchSet>()
    + MAX_MATCH_PATTERNS_PER_SET * (512 + MAX_MATCH_PATTERN_BYTES * 16)
    + PREPARED_RUNTIME_FIXED_RETAINED_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparedUserscriptRunAt {
    DocumentStart,
    DocumentEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparedUserscriptFrameScope {
    TopFrameOnly,
    AllFrames,
}

/// The only execution environment admitted by the initial subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparedUserscriptEnvironment {
    IsolatedNoApi,
}

/// Exact logical input accounting plus a conservative retained-memory charge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedUserscriptAccounting {
    /// Exact UTF-8 bytes in the unchanged source passed to the adapter.
    pub source_bytes: usize,
    /// Exact positive plus excluded registration count after file filtering.
    pub pattern_count: usize,
    /// Exact canonical UTF-8 bytes in those retained patterns.
    pub canonical_pattern_bytes: usize,
    /// Conservative source + matcher + fixed registration admission charge.
    pub retained_budget_bytes: usize,
}

/// Bounded, web-only descriptor for a potentially eligible userscript.
///
/// Fields are private so callers cannot weaken the HTTP(S)-only match set,
/// isolated environment, direct-frame policy, or supported run-at modes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedUserscriptRuntime {
    id: UserscriptId,
    revision: UserscriptRevision,
    metadata_format: u32,
    source: Arc<str>,
    source_digest: UserscriptSourceDigest,
    matches: MatchSet,
    run_at: PreparedUserscriptRunAt,
    frame_scope: PreparedUserscriptFrameScope,
    environment: PreparedUserscriptEnvironment,
    accounting: PreparedUserscriptAccounting,
}

impl PreparedUserscriptRuntime {
    pub const fn id(&self) -> UserscriptId {
        self.id
    }

    pub const fn revision(&self) -> UserscriptRevision {
        self.revision
    }

    pub const fn metadata_format(&self) -> u32 {
        self.metadata_format
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn shared_source(&self) -> Arc<str> {
        self.source.clone()
    }

    pub const fn source_digest(&self) -> UserscriptSourceDigest {
        self.source_digest
    }

    /// Returns an HTTP(S)-only direct-URL matcher with no related-frame
    /// fallback options. File patterns have already been removed and
    /// `<all_urls>` has been narrowed to `*://*/*`.
    pub fn matches(&self) -> &MatchSet {
        &self.matches
    }

    pub fn matches_url(&self, url: &Url) -> bool {
        self.matches.matches_url(url)
    }

    pub const fn run_at(&self) -> PreparedUserscriptRunAt {
        self.run_at
    }

    pub const fn frame_scope(&self) -> PreparedUserscriptFrameScope {
        self.frame_scope
    }

    pub const fn environment(&self) -> PreparedUserscriptEnvironment {
        self.environment
    }

    pub const fn accounting(&self) -> PreparedUserscriptAccounting {
        self.accounting
    }

    /// Consumes this revalidated descriptor and binds every engine-facing
    /// authority field to its durable userscript identity.
    ///
    /// Callers cannot choose a different owner, isolated world, registration
    /// id, match set, timing, or frame scope. This conversion is still only a
    /// desired registration: native adapters may refuse it, and callers must
    /// wait for the matching generation settlement before reporting it active.
    pub fn into_engine_user_script(self) -> UserScript {
        let principal = ScriptPrincipal::Userscript(self.id);
        UserScript {
            id: ScriptId::from(USERSCRIPT_SOURCE_REGISTRATION_ID),
            owner: ScriptOwner::Principal(principal),
            source: self.source,
            world: World::Isolated(principal),
            matches: self.matches,
            run_at: match self.run_at {
                PreparedUserscriptRunAt::DocumentStart => RunAt::DocumentStart,
                PreparedUserscriptRunAt::DocumentEnd => RunAt::DocumentEnd,
            },
            all_frames: matches!(self.frame_scope, PreparedUserscriptFrameScope::AllFrames),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptRuntimePreparationError {
    UnsupportedMetadataFormat {
        found: u32,
        supported: u32,
    },
    DigestMismatch {
        expected: UserscriptSourceDigest,
        actual: UserscriptSourceDigest,
    },
    InvalidStoredSource(UserscriptMetadataError),
    Ineligible(UserscriptRuntimeEligibility),
    InvalidEffectiveMatches(MatchSetError),
    AccountingOverflow,
    RetainedBudgetExceeded {
        bytes: usize,
        max: usize,
    },
}

impl fmt::Display for UserscriptRuntimePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMetadataFormat { found, supported } => write!(
                formatter,
                "userscript metadata format {found} is unsupported; supported format is {supported}"
            ),
            Self::DigestMismatch { .. } => {
                formatter.write_str("userscript source digest mismatches its stored authority")
            }
            Self::InvalidStoredSource(error) => {
                write!(formatter, "userscript stored source is invalid: {error}")
            }
            Self::Ineligible(eligibility) => write!(
                formatter,
                "userscript is outside the initial runtime subset ({:?})",
                eligibility.issues
            ),
            Self::InvalidEffectiveMatches(error) => {
                write!(
                    formatter,
                    "userscript effective matches are invalid: {error}"
                )
            }
            Self::AccountingOverflow => {
                formatter.write_str("userscript runtime accounting overflowed")
            }
            Self::RetainedBudgetExceeded { bytes, max } => write!(
                formatter,
                "userscript runtime retains {bytes} budget bytes; limit is {max}"
            ),
        }
    }
}

impl Error for UserscriptRuntimePreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidStoredSource(error) => Some(error),
            Self::InvalidEffectiveMatches(error) => Some(error),
            Self::UnsupportedMetadataFormat { .. }
            | Self::DigestMismatch { .. }
            | Self::Ineligible(_)
            | Self::AccountingOverflow
            | Self::RetainedBudgetExceeded { .. } => None,
        }
    }
}

/// Revalidates and prepares one durable userscript for the initial isolated,
/// no-API subset.
///
/// This function deliberately ignores the catalog's enabled flag. Enabled is
/// desired application state, while preparation is a pure compatibility step;
/// the future activation projection must require both and must still wait for
/// native settlement before claiming execution.
pub fn prepare_userscript_runtime(
    script: &Userscript,
) -> Result<PreparedUserscriptRuntime, UserscriptRuntimePreparationError> {
    // `Userscript` predates this runtime boundary and currently exposes public
    // fields. Rebind preparation to the exact durable source instead of
    // trusting that a caller did not desynchronize its derived metadata.
    if script.metadata_format != CURRENT_USERSCRIPT_METADATA_FORMAT {
        return Err(
            UserscriptRuntimePreparationError::UnsupportedMetadataFormat {
                found: script.metadata_format,
                supported: CURRENT_USERSCRIPT_METADATA_FORMAT,
            },
        );
    }
    let actual_digest = UserscriptSourceDigest::for_source(&script.source);
    if actual_digest != script.digest {
        return Err(UserscriptRuntimePreparationError::DigestMismatch {
            expected: script.digest,
            actual: actual_digest,
        });
    }
    let metadata = parse_userscript_metadata(&script.source)
        .map_err(UserscriptRuntimePreparationError::InvalidStoredSource)?;
    let eligibility = assess_userscript_runtime_eligibility(&metadata);
    if !eligibility.potentially_eligible {
        return Err(UserscriptRuntimePreparationError::Ineligible(eligibility));
    }

    let matches = web_only_matches(&metadata.matches)?;
    let run_at = match metadata.run_at {
        None | Some(DeclaredRunAt::Supported(RunAt::DocumentEnd)) => {
            PreparedUserscriptRunAt::DocumentEnd
        }
        Some(DeclaredRunAt::Supported(RunAt::DocumentStart)) => {
            PreparedUserscriptRunAt::DocumentStart
        }
        Some(DeclaredRunAt::Supported(RunAt::DocumentIdle))
        | Some(DeclaredRunAt::Unsupported(_)) => {
            // The eligibility pass above must retain these as blockers. Keep a
            // fail-closed return rather than relying on an unreachable panic.
            return Err(UserscriptRuntimePreparationError::Ineligible(
                assess_userscript_runtime_eligibility(&metadata),
            ));
        }
    };
    let retained_budget_bytes = script
        .source
        .len()
        .checked_add(matches.retained_budget_bytes())
        .and_then(|bytes| bytes.checked_add(PREPARED_RUNTIME_FIXED_RETAINED_BYTES))
        .ok_or(UserscriptRuntimePreparationError::AccountingOverflow)?;
    if retained_budget_bytes > MAX_PREPARED_USERSCRIPT_RETAINED_BYTES {
        return Err(UserscriptRuntimePreparationError::RetainedBudgetExceeded {
            bytes: retained_budget_bytes,
            max: MAX_PREPARED_USERSCRIPT_RETAINED_BYTES,
        });
    }
    let accounting = PreparedUserscriptAccounting {
        source_bytes: script.source.len(),
        pattern_count: matches.pattern_count(),
        canonical_pattern_bytes: matches.canonical_pattern_bytes(),
        retained_budget_bytes,
    };

    Ok(PreparedUserscriptRuntime {
        id: script.id,
        revision: script.revision,
        metadata_format: script.metadata_format,
        source: script.source.clone(),
        source_digest: script.digest,
        matches,
        run_at,
        frame_scope: if metadata.all_frames {
            PreparedUserscriptFrameScope::AllFrames
        } else {
            PreparedUserscriptFrameScope::TopFrameOnly
        },
        environment: PreparedUserscriptEnvironment::IsolatedNoApi,
        accounting,
    })
}

fn web_only_matches(declared: &MatchSet) -> Result<MatchSet, UserscriptRuntimePreparationError> {
    let includes = web_only_patterns(declared.includes(), MatchPatternList::Matches)?;
    let excludes = web_only_patterns(declared.excludes(), MatchPatternList::ExcludeMatches)?;
    MatchSet::new(includes, excludes, MatchOptions::default())
        .map_err(UserscriptRuntimePreparationError::InvalidEffectiveMatches)
}

fn web_only_patterns(
    patterns: &[MatchPattern],
    list: MatchPatternList,
) -> Result<Vec<MatchPattern>, UserscriptRuntimePreparationError> {
    let mut effective = Vec::with_capacity(patterns.len());
    for (index, pattern) in patterns.iter().enumerate() {
        match pattern.components() {
            MatchPatternComponents::AllUrls => {
                // This is an implementation-owned canonical transform, but
                // propagate even an invariant failure. Silently dropping a
                // transformed exclusion would widen runtime authority.
                let web = MatchPattern::parse("*://*/*").map_err(|error| {
                    UserscriptRuntimePreparationError::InvalidEffectiveMatches(
                        MatchSetError::InvalidPattern { list, index, error },
                    )
                })?;
                effective.push(web);
            }
            MatchPatternComponents::Standard { scheme, .. } if scheme.includes_http_or_https() => {
                effective.push(pattern.clone());
            }
            MatchPatternComponents::Standard { .. } => {}
        }
    }
    Ok(effective)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::ids::UserscriptId;
    use crate::injection::{MatchOptions, MatchSet};
    use crate::ports::engine::UserContent;
    use crate::userscripts::{UserscriptRevision, MAX_USERSCRIPT_DIRECTIVES};

    use super::*;

    fn source(metadata: &str, body: &str) -> Arc<str> {
        format!("// ==UserScript==\n// @name Runtime test\n{metadata}// ==/UserScript==\n{body}")
            .into()
    }

    fn script(metadata: &str, body: &str) -> Userscript {
        Userscript::from_source(
            UserscriptId::from(1),
            UserscriptRevision::INITIAL,
            true,
            source(metadata, body),
        )
        .unwrap()
    }

    #[test]
    fn omitted_grant_and_run_at_prepare_only_isolated_no_api_at_document_end() {
        let candidate = script("// @match https://example.com/*\n", "window.test = true;");
        let prepared = prepare_userscript_runtime(&candidate).unwrap();
        assert_eq!(prepared.id(), candidate.id);
        assert_eq!(prepared.revision(), candidate.revision);
        assert_eq!(
            prepared.metadata_format(),
            CURRENT_USERSCRIPT_METADATA_FORMAT
        );
        assert_eq!(
            prepared.environment(),
            PreparedUserscriptEnvironment::IsolatedNoApi
        );
        assert_eq!(prepared.run_at(), PreparedUserscriptRunAt::DocumentEnd);
        assert_eq!(
            prepared.frame_scope(),
            PreparedUserscriptFrameScope::AllFrames
        );

        let top_only = prepare_userscript_runtime(&script(
            "// @match https://example.com/*\n// @noframes\n// @run-at document-start\n",
            "void 0;",
        ))
        .unwrap();
        assert_eq!(top_only.run_at(), PreparedUserscriptRunAt::DocumentStart);
        assert_eq!(
            top_only.frame_scope(),
            PreparedUserscriptFrameScope::TopFrameOnly
        );
    }

    #[test]
    fn every_capability_or_main_world_request_is_refused() {
        for declaration in ["// @grant none\n", "// @grant GM.xmlHttpRequest\n"] {
            let candidate = script(
                &format!("// @match https://example.com/*\n{declaration}"),
                "void 0;",
            );
            assert!(matches!(
                prepare_userscript_runtime(&candidate),
                Err(UserscriptRuntimePreparationError::Ineligible(_))
            ));
        }
        let unsupported = script(
            "// @match https://example.com/*\n// @require https://example.com/a.js\n",
            "void 0;",
        );
        assert!(matches!(
            prepare_userscript_runtime(&unsupported),
            Err(UserscriptRuntimePreparationError::Ineligible(_))
        ));
    }

    #[test]
    fn document_idle_and_unknown_run_at_are_refused() {
        for run_at in ["document-idle", "document-beforeload"] {
            let candidate = script(
                &format!("// @match https://example.com/*\n// @run-at {run_at}\n"),
                "void 0;",
            );
            assert!(matches!(
                prepare_userscript_runtime(&candidate),
                Err(UserscriptRuntimePreparationError::Ineligible(_))
            ));
        }
    }

    #[test]
    fn all_urls_is_narrowed_to_http_and_https_and_file_only_is_inactive() {
        let prepared =
            prepare_userscript_runtime(&script("// @match <all_urls>\n", "void 0;")).unwrap();
        assert_eq!(prepared.matches().includes()[0].as_str(), "*://*/*");
        assert!(prepared.matches_url(&Url::parse("http://example.com/").unwrap()));
        assert!(prepared.matches_url(&Url::parse("https://example.com/").unwrap()));
        assert!(!prepared.matches_url(&Url::parse("file:///tmp/private").unwrap()));

        let file_only = script("// @match file:///*\n", "void 0;");
        assert!(matches!(
            prepare_userscript_runtime(&file_only),
            Err(UserscriptRuntimePreparationError::Ineligible(
                UserscriptRuntimeEligibility {
                    potentially_eligible: false,
                    ..
                }
            ))
        ));

        let mixed = prepare_userscript_runtime(&script(
            "// @match file:///*\n// @match https://example.com/*\n",
            "void 0;",
        ))
        .unwrap();
        assert_eq!(mixed.matches().includes().len(), 1);
        assert!(!mixed.matches_url(&Url::parse("file:///tmp/private").unwrap()));
        assert!(mixed.matches_url(&Url::parse("https://example.com/page").unwrap()));
    }

    #[test]
    fn all_urls_exclusion_is_transformed_and_never_silently_dropped() {
        let prepared = prepare_userscript_runtime(&script(
            "// @match <all_urls>\n// @exclude-match <all_urls>\n",
            "void 0;",
        ))
        .unwrap();
        assert_eq!(prepared.matches().includes()[0].as_str(), "*://*/*");
        assert_eq!(prepared.matches().excludes()[0].as_str(), "*://*/*");
        assert!(!prepared.matches_url(&Url::parse("https://example.com/").unwrap()));
    }

    #[test]
    fn prepared_matcher_preserves_ports_queries_unicode_and_exclusions() {
        let prepared = prepare_userscript_runtime(&script(
            "// @match https://BÜCHER.example:8443/search?q=*\n// @exclude-match https://xn--bcher-kva.example:8443/search?q=private*\n",
            "void 0;",
        ))
        .unwrap();
        assert!(prepared
            .matches_url(&Url::parse("https://bücher.example:8443/search?q=public").unwrap()));
        assert!(!prepared.matches_url(
            &Url::parse("https://bücher.example:8443/search?q=private-notes").unwrap()
        ));
        assert!(
            !prepared.matches_url(&Url::parse("https://bücher.example/search?q=public").unwrap())
        );
        assert!(!prepared.matches_url(&Url::parse("https://bücher.example:8443/search").unwrap()));
    }

    #[test]
    fn reparsing_source_discards_mutated_related_frame_fallback() {
        let mut candidate = script("// @match https://example.com/*\n", "void 0;");
        candidate.metadata.matches = MatchSet::parse(
            ["https://example.com/*"],
            std::iter::empty::<&str>(),
            MatchOptions {
                match_about_blank: true,
                match_origin_as_fallback: false,
            },
        )
        .unwrap();
        // Runtime preparation reparses the source authority, so an in-memory
        // mutation of derived metadata cannot add a related-frame capability.
        let prepared = prepare_userscript_runtime(&candidate).unwrap();
        assert_eq!(prepared.matches().options(), MatchOptions::default());
    }

    #[test]
    fn hostile_public_field_desynchronization_cannot_weaken_source_policy() {
        let mut main_world = script(
            "// @match https://example.com/*\n// @grant none\n",
            "void 0;",
        );
        main_world.metadata.grants.clear();
        assert!(matches!(
            prepare_userscript_runtime(&main_world),
            Err(UserscriptRuntimePreparationError::Ineligible(_))
        ));

        let mut changed_source = script("// @match https://example.com/*\n", "void 0;");
        changed_source.source = source("// @match https://other.example/*\n", "void 0;");
        assert!(matches!(
            prepare_userscript_runtime(&changed_source),
            Err(UserscriptRuntimePreparationError::DigestMismatch { .. })
        ));

        let mut malformed = script("// @match https://example.com/*\n", "void 0;");
        malformed.source = "not userscript metadata".into();
        malformed.digest = UserscriptSourceDigest::for_source(&malformed.source);
        assert!(matches!(
            prepare_userscript_runtime(&malformed),
            Err(UserscriptRuntimePreparationError::InvalidStoredSource(
                UserscriptMetadataError::MissingHeader
            ))
        ));

        let mut unsupported_format = script("// @match https://example.com/*\n", "void 0;");
        unsupported_format.metadata_format = CURRENT_USERSCRIPT_METADATA_FORMAT + 1;
        assert!(matches!(
            prepare_userscript_runtime(&unsupported_format),
            Err(
                UserscriptRuntimePreparationError::UnsupportedMetadataFormat {
                    found,
                    supported: CURRENT_USERSCRIPT_METADATA_FORMAT,
                }
            ) if found == CURRENT_USERSCRIPT_METADATA_FORMAT + 1
        ));
    }

    #[test]
    fn prepared_identity_binds_durable_id_revision_and_source_authority() {
        let id = UserscriptId::from(42);
        let initial = Userscript::from_source(
            id,
            UserscriptRevision::INITIAL,
            true,
            source("// @match https://example.com/*\n", "window.version = 1;"),
        )
        .unwrap();
        let next_revision = initial.revision.next().unwrap();
        let updated = Userscript::from_source(
            id,
            next_revision,
            true,
            source("// @match https://example.com/*\n", "window.version = 2;"),
        )
        .unwrap();
        let initial_prepared = prepare_userscript_runtime(&initial).unwrap();
        let updated_prepared = prepare_userscript_runtime(&updated).unwrap();
        assert_eq!(initial_prepared.id(), updated_prepared.id());
        assert_eq!(initial_prepared.id(), id);
        assert_eq!(initial_prepared.revision(), UserscriptRevision::INITIAL);
        assert_eq!(updated_prepared.revision(), next_revision);
        assert_ne!(
            initial_prepared.source_digest(),
            updated_prepared.source_digest()
        );

        let other_owner = Userscript::from_source(
            UserscriptId::from(43),
            UserscriptRevision::INITIAL,
            true,
            initial.source.clone(),
        )
        .unwrap();
        let other_owner = prepare_userscript_runtime(&other_owner).unwrap();
        assert_eq!(
            initial_prepared.source_digest(),
            other_owner.source_digest()
        );
        assert_ne!(initial_prepared.id(), other_owner.id());
    }

    #[test]
    fn engine_conversion_binds_owner_world_and_registration_to_one_identity() {
        let candidate = Userscript::from_source(
            UserscriptId::from(42),
            UserscriptRevision::INITIAL,
            true,
            source(
                "// @match https://example.com/*\n// @run-at document-start\n",
                "window.bound = true;",
            ),
        )
        .unwrap();
        let prepared = prepare_userscript_runtime(&candidate).unwrap();
        let expected_source = prepared.shared_source();
        let expected_matches = prepared.matches().clone();
        let engine = prepared.into_engine_user_script();
        let principal = ScriptPrincipal::Userscript(candidate.id);

        assert_eq!(engine.id, ScriptId::from(USERSCRIPT_SOURCE_REGISTRATION_ID));
        assert_eq!(engine.owner, ScriptOwner::Principal(principal));
        assert_eq!(engine.world, World::Isolated(principal));
        assert_eq!(engine.source, expected_source);
        assert_eq!(engine.matches, expected_matches);
        assert_eq!(engine.run_at, RunAt::DocumentStart);
        assert!(engine.all_frames);
        assert!(UserContent {
            scripts: vec![engine],
            styles: Vec::new(),
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn updates_retain_engine_key_but_reinstall_mints_a_distinct_principal() {
        let durable_id = UserscriptId::from(7);
        let initial = Userscript::from_source(
            durable_id,
            UserscriptRevision::INITIAL,
            true,
            source("// @match https://example.com/*\n", "window.version = 1;"),
        )
        .unwrap();
        let updated = Userscript::from_source(
            durable_id,
            initial.revision.next().unwrap(),
            true,
            source("// @match https://example.com/*\n", "window.version = 2;"),
        )
        .unwrap();
        let reinstalled = Userscript::from_source(
            UserscriptId::from(8),
            UserscriptRevision::INITIAL,
            true,
            initial.source.clone(),
        )
        .unwrap();

        let initial = prepare_userscript_runtime(&initial)
            .unwrap()
            .into_engine_user_script();
        let updated = prepare_userscript_runtime(&updated)
            .unwrap()
            .into_engine_user_script();
        let reinstalled = prepare_userscript_runtime(&reinstalled)
            .unwrap()
            .into_engine_user_script();

        assert_eq!(initial.key(), updated.key());
        assert_ne!(initial.source, updated.source);
        assert_ne!(initial.key(), reinstalled.key());
        assert_eq!(initial.id, reinstalled.id);
    }

    #[test]
    fn engine_conversion_maps_top_frame_document_end_without_caller_input() {
        let prepared = prepare_userscript_runtime(&script(
            "// @match https://example.com/*\n// @noframes\n",
            "void 0;",
        ))
        .unwrap();
        let engine = prepared.into_engine_user_script();

        assert_eq!(engine.run_at, RunAt::DocumentEnd);
        assert!(!engine.all_frames);
    }

    #[test]
    fn accounting_is_exact_and_boundary_bounded() {
        let metadata = "// @match <all_urls>\n// @exclude-match file:///*\n";
        let candidate = script(metadata, "console.log('exact');");
        let prepared = prepare_userscript_runtime(&candidate).unwrap();
        let accounting = prepared.accounting();
        assert_eq!(accounting.source_bytes, candidate.source.len());
        assert_eq!(accounting.pattern_count, 1);
        assert_eq!(accounting.canonical_pattern_bytes, "*://*/*".len());
        assert_eq!(
            accounting.retained_budget_bytes,
            candidate.source.len()
                + prepared.matches().retained_budget_bytes()
                + PREPARED_RUNTIME_FIXED_RETAINED_BYTES
        );
        assert!(accounting.retained_budget_bytes <= MAX_PREPARED_USERSCRIPT_RETAINED_BYTES);

        let prefix = source("// @match https://example.com/*\n", "");
        let mut exact_max = String::from(&*prefix);
        exact_max.push_str(&"x".repeat(MAX_USER_SCRIPT_BYTES - exact_max.len()));
        let max_script = Userscript::from_source(
            UserscriptId::from(2),
            UserscriptRevision::INITIAL,
            true,
            exact_max.into(),
        )
        .unwrap();
        let max_prepared = prepare_userscript_runtime(&max_script).unwrap();
        assert_eq!(
            max_prepared.accounting().source_bytes,
            MAX_USER_SCRIPT_BYTES
        );
        assert!(
            max_prepared.accounting().retained_budget_bytes
                <= MAX_PREPARED_USERSCRIPT_RETAINED_BYTES
        );

        let mut match_boundary = String::new();
        for index in 0..(MAX_USERSCRIPT_DIRECTIVES - 1) {
            match_boundary.push_str(&format!("// @match https://example.com/path-{index}*\n"));
        }
        let match_boundary = script(&match_boundary, "void 0;");
        let match_boundary = prepare_userscript_runtime(&match_boundary).unwrap();
        assert_eq!(
            match_boundary.accounting().pattern_count,
            MAX_USERSCRIPT_DIRECTIVES - 1
        );
        assert_eq!(
            match_boundary.accounting().canonical_pattern_bytes,
            match_boundary.matches().canonical_pattern_bytes()
        );
        assert!(
            match_boundary.accounting().retained_budget_bytes
                <= MAX_PREPARED_USERSCRIPT_RETAINED_BYTES
        );
    }

    proptest! {
        #[test]
        fn all_urls_preparation_never_admits_any_file_url(path in "[a-zA-Z0-9_./-]{0,128}") {
            let prepared = prepare_userscript_runtime(&script("// @match <all_urls>\n", "void 0;"))
                .unwrap();
            let encoded = path.replace(' ', "%20");
            let candidate = Url::parse(&format!("file:///tmp/{encoded}")).unwrap();
            prop_assert!(!prepared.matches_url(&candidate));
        }

        #[test]
        fn every_named_grant_is_ineligible(name in "[A-Za-z][A-Za-z0-9._]{0,31}") {
            prop_assume!(name != "none");
            let candidate = script(
                &format!("// @match https://example.com/*\n// @grant {name}\n"),
                "void 0;",
            );
            prop_assert!(matches!(
                prepare_userscript_runtime(&candidate),
                Err(UserscriptRuntimePreparationError::Ineligible(_))
            ));
        }

        #[test]
        fn accounting_tracks_exact_source_length(body in "[a-zA-Z0-9 ;()]{0,8192}") {
            let candidate = script("// @match https://example.com/*\n", &body);
            let prepared = prepare_userscript_runtime(&candidate).unwrap();
            prop_assert_eq!(prepared.accounting().source_bytes, candidate.source.len());
            prop_assert!(
                prepared.accounting().retained_budget_bytes
                    <= MAX_PREPARED_USERSCRIPT_RETAINED_BYTES
            );
        }
    }
}
