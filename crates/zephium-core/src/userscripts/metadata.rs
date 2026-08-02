//! Allocation-bounded parsing of userscript metadata blocks.

use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::injection::{MatchOptions, MatchSet, MatchSetError};
use crate::ports::engine::{RunAt, MAX_USER_SCRIPT_BYTES};

pub const MAX_USERSCRIPT_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_USERSCRIPT_METADATA_LINES: usize = 256;
pub const MAX_USERSCRIPT_METADATA_LINE_BYTES: usize = 4 * 1024;
pub const MAX_USERSCRIPT_DIRECTIVES: usize = 128;
pub const MAX_USERSCRIPT_GRANTS: usize = 64;
pub const MAX_USERSCRIPT_NAME_BYTES: usize = 256;
pub const MAX_USERSCRIPT_NAMESPACE_BYTES: usize = 512;
pub const MAX_USERSCRIPT_VERSION_BYTES: usize = 128;
pub const MAX_USERSCRIPT_DESCRIPTION_BYTES: usize = 2 * 1024;
const MAX_DIRECTIVE_NAME_BYTES: usize = 64;

const HEADER: &str = "// ==UserScript==";
const FOOTER: &str = "// ==/UserScript==";

/// Durable v1 integrity witness: SHA-256 of the fixed
/// `zephium-userscript-source-v1` domain followed by exact UTF-8 source bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserscriptSourceDigest([u8; 32]);

impl UserscriptSourceDigest {
    pub fn for_source(source: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"zephium-userscript-source-v1");
        digest.update(source.as_bytes());
        Self(digest.finalize().into())
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for UserscriptSourceDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UserscriptSourceDigest(")?;
        for byte in &self.0[..8] {
            write!(formatter, "{byte:02x}")?;
        }
        formatter.write_str("…)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredRunAt {
    Supported(RunAt),
    Unsupported(Box<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredGrant {
    None,
    Named(Box<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedDirective {
    pub name: Box<str>,
    pub line: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUserscriptMetadata {
    pub name: Box<str>,
    pub namespace: Option<Box<str>>,
    pub version: Option<Box<str>>,
    pub description: Option<Box<str>>,
    pub matches: MatchSet,
    /// Omission remains distinct because established userscript managers use
    /// different defaults. Runtime policy, not the parser, selects a default.
    pub run_at: Option<DeclaredRunAt>,
    /// Traditional metadata enables child frames unless `@noframes` exists.
    pub all_frames: bool,
    /// Absence remains distinct from an explicit `@grant none` declaration.
    pub grants: Vec<DeclaredGrant>,
    pub unsupported_directives: Vec<UnsupportedDirective>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptMetadataError {
    EmptySource,
    SourceTooLarge {
        length: usize,
        max: usize,
    },
    SourceContainsNul,
    MissingHeader,
    MissingFooter,
    MetadataTooLarge {
        length: usize,
        max: usize,
    },
    TooManyLines {
        count: usize,
        max: usize,
    },
    LineTooLong {
        line: usize,
        length: usize,
        max: usize,
    },
    InvalidLine {
        line: usize,
    },
    TooManyDirectives {
        count: usize,
        max: usize,
    },
    DirectiveNameTooLong {
        line: usize,
    },
    InvalidDirectiveName {
        line: usize,
    },
    MissingValue {
        line: usize,
        directive: &'static str,
    },
    FieldTooLong {
        line: usize,
        directive: &'static str,
        length: usize,
        max: usize,
    },
    ControlCharacter {
        line: usize,
    },
    DuplicateDirective {
        line: usize,
        directive: &'static str,
    },
    MissingName,
    MissingMatches,
    InvalidMatches(MatchSetError),
    TooManyGrants {
        count: usize,
        max: usize,
    },
    DuplicateGrant {
        line: usize,
    },
    ConflictingGrantNone {
        line: usize,
    },
    UnexpectedValue {
        line: usize,
        directive: &'static str,
    },
}

impl fmt::Display for UserscriptMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource => formatter.write_str("userscript source is empty"),
            Self::SourceTooLarge { length, max } => {
                write!(
                    formatter,
                    "userscript source is {length} bytes; limit is {max}"
                )
            }
            Self::SourceContainsNul => {
                formatter.write_str("userscript source contains an embedded NUL byte")
            }
            Self::MissingHeader => formatter.write_str("userscript metadata header is missing"),
            Self::MissingFooter => formatter.write_str("userscript metadata footer is missing"),
            Self::MetadataTooLarge { length, max } => {
                write!(
                    formatter,
                    "userscript metadata is {length} bytes; limit is {max}"
                )
            }
            Self::TooManyLines { count, max } => {
                write!(
                    formatter,
                    "userscript metadata has {count} lines; limit is {max}"
                )
            }
            Self::LineTooLong { line, length, max } => {
                write!(
                    formatter,
                    "userscript metadata line {line} is {length} bytes; limit is {max}"
                )
            }
            Self::InvalidLine { line } => write!(formatter, "invalid metadata line {line}"),
            Self::TooManyDirectives { count, max } => {
                write!(
                    formatter,
                    "userscript metadata has {count} directives; limit is {max}"
                )
            }
            Self::DirectiveNameTooLong { line } => {
                write!(
                    formatter,
                    "metadata directive name on line {line} is too long"
                )
            }
            Self::InvalidDirectiveName { line } => {
                write!(formatter, "invalid metadata directive name on line {line}")
            }
            Self::MissingValue { line, directive } => {
                write!(formatter, "@{directive} on line {line} requires a value")
            }
            Self::FieldTooLong {
                line,
                directive,
                length,
                max,
            } => write!(
                formatter,
                "@{directive} on line {line} is {length} bytes; limit is {max}"
            ),
            Self::ControlCharacter { line } => {
                write!(
                    formatter,
                    "metadata value on line {line} contains a control character"
                )
            }
            Self::DuplicateDirective { line, directive } => {
                write!(formatter, "duplicate @{directive} on line {line}")
            }
            Self::MissingName => formatter.write_str("userscript metadata has no @name"),
            Self::MissingMatches => formatter.write_str("userscript metadata has no @match"),
            Self::InvalidMatches(error) => write!(formatter, "invalid userscript matches: {error}"),
            Self::TooManyGrants { count, max } => {
                write!(
                    formatter,
                    "userscript metadata has {count} grants; limit is {max}"
                )
            }
            Self::DuplicateGrant { line } => write!(formatter, "duplicate @grant on line {line}"),
            Self::ConflictingGrantNone { line } => {
                write!(
                    formatter,
                    "@grant none conflicts with another grant on line {line}"
                )
            }
            Self::UnexpectedValue { line, directive } => {
                write!(formatter, "@{directive} on line {line} cannot have a value")
            }
        }
    }
}

impl Error for UserscriptMetadataError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMatches(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Builder {
    name: Option<Box<str>>,
    namespace: Option<Box<str>>,
    version: Option<Box<str>>,
    description: Option<Box<str>>,
    matches: Vec<Box<str>>,
    excludes: Vec<Box<str>>,
    run_at: Option<DeclaredRunAt>,
    noframes: bool,
    grants: Vec<DeclaredGrant>,
    unsupported: Vec<UnsupportedDirective>,
}

/// Parses only metadata syntax and bounded declarations. Runtime support is
/// assessed separately by `compatibility`, preventing parser acceptance from
/// becoming an accidental capability grant.
pub fn parse_userscript_metadata(
    source: &str,
) -> Result<ParsedUserscriptMetadata, UserscriptMetadataError> {
    if source.is_empty() {
        return Err(UserscriptMetadataError::EmptySource);
    }
    if source.len() > MAX_USER_SCRIPT_BYTES {
        return Err(UserscriptMetadataError::SourceTooLarge {
            length: source.len(),
            max: MAX_USER_SCRIPT_BYTES,
        });
    }
    if source.as_bytes().contains(&0) {
        return Err(UserscriptMetadataError::SourceContainsNul);
    }
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut lines = source.lines();
    if lines.next() != Some(HEADER) {
        return Err(UserscriptMetadataError::MissingHeader);
    }

    let mut builder = Builder::default();
    let mut metadata_bytes = HEADER.len().saturating_add(1);
    let mut line_count = 1_usize;
    let mut directive_count = 0_usize;
    let mut found_footer = false;
    for line in lines {
        line_count = line_count.saturating_add(1);
        if line_count > MAX_USERSCRIPT_METADATA_LINES {
            return Err(UserscriptMetadataError::TooManyLines {
                count: line_count,
                max: MAX_USERSCRIPT_METADATA_LINES,
            });
        }
        if line.len() > MAX_USERSCRIPT_METADATA_LINE_BYTES {
            return Err(UserscriptMetadataError::LineTooLong {
                line: line_count,
                length: line.len(),
                max: MAX_USERSCRIPT_METADATA_LINE_BYTES,
            });
        }
        metadata_bytes = metadata_bytes.saturating_add(line.len()).saturating_add(1);
        if metadata_bytes > MAX_USERSCRIPT_METADATA_BYTES {
            return Err(UserscriptMetadataError::MetadataTooLarge {
                length: metadata_bytes,
                max: MAX_USERSCRIPT_METADATA_BYTES,
            });
        }
        if line == FOOTER {
            found_footer = true;
            break;
        }
        let body = line
            .strip_prefix("//")
            .ok_or(UserscriptMetadataError::InvalidLine { line: line_count })?
            .trim_matches([' ', '\t']);
        if body.is_empty() {
            continue;
        }
        let declaration = body
            .strip_prefix('@')
            .ok_or(UserscriptMetadataError::InvalidLine { line: line_count })?;
        let split = declaration.find([' ', '\t']).unwrap_or(declaration.len());
        let name = &declaration[..split];
        let value = declaration[split..].trim_matches([' ', '\t']);
        if name.is_empty() || !name.bytes().all(valid_directive_name_byte) {
            return Err(UserscriptMetadataError::InvalidDirectiveName { line: line_count });
        }
        if name.len() > MAX_DIRECTIVE_NAME_BYTES {
            return Err(UserscriptMetadataError::DirectiveNameTooLong { line: line_count });
        }
        if value.chars().any(char::is_control) {
            return Err(UserscriptMetadataError::ControlCharacter { line: line_count });
        }
        directive_count = directive_count.saturating_add(1);
        if directive_count > MAX_USERSCRIPT_DIRECTIVES {
            return Err(UserscriptMetadataError::TooManyDirectives {
                count: directive_count,
                max: MAX_USERSCRIPT_DIRECTIVES,
            });
        }
        apply_directive(&mut builder, name, value, line_count)?;
    }
    if !found_footer {
        return Err(UserscriptMetadataError::MissingFooter);
    }
    let name = builder.name.ok_or(UserscriptMetadataError::MissingName)?;
    if builder.matches.is_empty() {
        return Err(UserscriptMetadataError::MissingMatches);
    }
    let matches = MatchSet::parse(
        builder.matches.iter().map(Box::as_ref),
        builder.excludes.iter().map(Box::as_ref),
        MatchOptions::default(),
    )
    .map_err(UserscriptMetadataError::InvalidMatches)?;
    Ok(ParsedUserscriptMetadata {
        name,
        namespace: builder.namespace,
        version: builder.version,
        description: builder.description,
        matches,
        run_at: builder.run_at,
        all_frames: !builder.noframes,
        grants: builder.grants,
        unsupported_directives: builder.unsupported,
    })
}

fn valid_directive_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
}

fn require_value(
    value: &str,
    line: usize,
    directive: &'static str,
) -> Result<(), UserscriptMetadataError> {
    if value.is_empty() {
        Err(UserscriptMetadataError::MissingValue { line, directive })
    } else {
        Ok(())
    }
}

fn bounded_value(
    value: &str,
    line: usize,
    directive: &'static str,
    max: usize,
) -> Result<Box<str>, UserscriptMetadataError> {
    require_value(value, line, directive)?;
    if value.len() > max {
        return Err(UserscriptMetadataError::FieldTooLong {
            line,
            directive,
            length: value.len(),
            max,
        });
    }
    Ok(value.into())
}

fn singleton(
    slot: &mut Option<Box<str>>,
    value: &str,
    line: usize,
    directive: &'static str,
    max: usize,
) -> Result<(), UserscriptMetadataError> {
    if slot.is_some() {
        return Err(UserscriptMetadataError::DuplicateDirective { line, directive });
    }
    *slot = Some(bounded_value(value, line, directive, max)?);
    Ok(())
}

fn apply_directive(
    builder: &mut Builder,
    name: &str,
    value: &str,
    line: usize,
) -> Result<(), UserscriptMetadataError> {
    match name {
        "name" => singleton(
            &mut builder.name,
            value,
            line,
            "name",
            MAX_USERSCRIPT_NAME_BYTES,
        ),
        "namespace" => singleton(
            &mut builder.namespace,
            value,
            line,
            "namespace",
            MAX_USERSCRIPT_NAMESPACE_BYTES,
        ),
        "version" => singleton(
            &mut builder.version,
            value,
            line,
            "version",
            MAX_USERSCRIPT_VERSION_BYTES,
        ),
        "description" => singleton(
            &mut builder.description,
            value,
            line,
            "description",
            MAX_USERSCRIPT_DESCRIPTION_BYTES,
        ),
        "match" => {
            builder.matches.push(bounded_value(
                value,
                line,
                "match",
                crate::injection::MAX_MATCH_PATTERN_BYTES,
            )?);
            Ok(())
        }
        "exclude-match" => {
            builder.excludes.push(bounded_value(
                value,
                line,
                "exclude-match",
                crate::injection::MAX_MATCH_PATTERN_BYTES,
            )?);
            Ok(())
        }
        "run-at" => {
            if builder.run_at.is_some() {
                return Err(UserscriptMetadataError::DuplicateDirective {
                    line,
                    directive: "run-at",
                });
            }
            require_value(value, line, "run-at")?;
            builder.run_at = Some(match value {
                "document-start" => DeclaredRunAt::Supported(RunAt::DocumentStart),
                "document-end" => DeclaredRunAt::Supported(RunAt::DocumentEnd),
                "document-idle" => DeclaredRunAt::Supported(RunAt::DocumentIdle),
                _ => DeclaredRunAt::Unsupported(value.into()),
            });
            Ok(())
        }
        "noframes" => {
            if !value.is_empty() {
                return Err(UserscriptMetadataError::UnexpectedValue {
                    line,
                    directive: "noframes",
                });
            }
            if builder.noframes {
                return Err(UserscriptMetadataError::DuplicateDirective {
                    line,
                    directive: "noframes",
                });
            }
            builder.noframes = true;
            Ok(())
        }
        "grant" => apply_grant(builder, value, line),
        _ => {
            builder.unsupported.push(UnsupportedDirective {
                name: name.into(),
                line: u16::try_from(line).unwrap_or(u16::MAX),
            });
            Ok(())
        }
    }
}

fn apply_grant(
    builder: &mut Builder,
    value: &str,
    line: usize,
) -> Result<(), UserscriptMetadataError> {
    require_value(value, line, "grant")?;
    if value.len() > MAX_DIRECTIVE_NAME_BYTES {
        return Err(UserscriptMetadataError::FieldTooLong {
            line,
            directive: "grant",
            length: value.len(),
            max: MAX_DIRECTIVE_NAME_BYTES,
        });
    }
    let grant = if value == "none" {
        DeclaredGrant::None
    } else {
        DeclaredGrant::Named(value.into())
    };
    if builder.grants.iter().any(|existing| existing == &grant) {
        return Err(UserscriptMetadataError::DuplicateGrant { line });
    }
    if matches!(grant, DeclaredGrant::None) && !builder.grants.is_empty()
        || !matches!(grant, DeclaredGrant::None)
            && builder
                .grants
                .iter()
                .any(|existing| matches!(existing, DeclaredGrant::None))
    {
        return Err(UserscriptMetadataError::ConflictingGrantNone { line });
    }
    let count = builder.grants.len().saturating_add(1);
    if count > MAX_USERSCRIPT_GRANTS {
        return Err(UserscriptMetadataError::TooManyGrants {
            count,
            max: MAX_USERSCRIPT_GRANTS,
        });
    }
    builder.grants.push(grant);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn source(metadata: &str) -> String {
        format!("// ==UserScript==\n{metadata}// ==/UserScript==\nconsole.log('ok');")
    }

    #[test]
    fn parses_supported_metadata_without_inventing_omitted_defaults() {
        let parsed = parse_userscript_metadata(&source(
            "// @name Sample\n// @namespace example\n// @version 1.0\n// @description Test\n// @match https://*.example.com/*\n// @exclude-match https://private.example.com/*\n// @noframes\n",
        ))
        .unwrap();
        assert_eq!(parsed.name.as_ref(), "Sample");
        assert_eq!(parsed.namespace.as_deref(), Some("example"));
        assert_eq!(parsed.version.as_deref(), Some("1.0"));
        assert_eq!(parsed.description.as_deref(), Some("Test"));
        assert_eq!(parsed.run_at, None);
        assert!(!parsed.all_frames);
        assert!(parsed.grants.is_empty());
        assert!(parsed.unsupported_directives.is_empty());
        assert_eq!(
            parsed.matches.includes()[0].as_str(),
            "https://*.example.com/*"
        );
    }

    #[test]
    fn preserves_explicit_grant_none_and_unsupported_declarations() {
        let parsed = parse_userscript_metadata(&source(
            "// @name Sample\n// @match https://example.com/*\n// @grant none\n// @require https://example.com/library.js\n",
        ))
        .unwrap();
        assert_eq!(parsed.grants, vec![DeclaredGrant::None]);
        assert_eq!(parsed.unsupported_directives[0].name.as_ref(), "require");
    }

    #[test]
    fn supports_one_optional_bom_but_not_leading_text() {
        let valid = format!(
            "\u{feff}{}",
            source("// @name A\n// @match https://a.example/*\n")
        );
        assert!(parse_userscript_metadata(&valid).is_ok());
        assert_eq!(
            parse_userscript_metadata(&format!(
                " \n{}",
                source("// @name A\n// @match https://a.example/*\n")
            )),
            Err(UserscriptMetadataError::MissingHeader)
        );
    }

    #[test]
    fn refuses_nul_anywhere_in_the_complete_source() {
        let mut script = source("// @name A\n// @match https://a.example/*\n");
        script.push('\0');
        assert_eq!(
            parse_userscript_metadata(&script),
            Err(UserscriptMetadataError::SourceContainsNul)
        );
    }

    #[test]
    fn refuses_duplicates_controls_missing_footer_and_invalid_matches() {
        let duplicate = source("// @name A\n// @name B\n// @match https://a.example/*\n");
        assert!(matches!(
            parse_userscript_metadata(&duplicate),
            Err(UserscriptMetadataError::DuplicateDirective {
                directive: "name",
                ..
            })
        ));
        let control = source("// @name A\u{7}\n// @match https://a.example/*\n");
        assert!(matches!(
            parse_userscript_metadata(&control),
            Err(UserscriptMetadataError::ControlCharacter { .. })
        ));
        assert_eq!(
            parse_userscript_metadata("// ==UserScript==\n// @name A\n"),
            Err(UserscriptMetadataError::MissingFooter)
        );
        let invalid = source("// @name A\n// @match javascript://*/*\n");
        assert!(matches!(
            parse_userscript_metadata(&invalid),
            Err(UserscriptMetadataError::InvalidMatches(_))
        ));
    }

    #[test]
    fn digest_is_domain_separated_and_source_exact() {
        assert_eq!(
            UserscriptSourceDigest::for_source("a"),
            UserscriptSourceDigest::for_source("a")
        );
        assert_ne!(
            UserscriptSourceDigest::for_source("a"),
            UserscriptSourceDigest::for_source("a\n")
        );
    }

    #[test]
    fn every_parser_allocation_boundary_is_fail_closed() {
        let oversized_source = "x".repeat(MAX_USER_SCRIPT_BYTES + 1);
        assert!(matches!(
            parse_userscript_metadata(&oversized_source),
            Err(UserscriptMetadataError::SourceTooLarge { .. })
        ));

        let long_line = format!(
            "// ==UserScript==\n{}\n// ==/UserScript==\n",
            "x".repeat(MAX_USERSCRIPT_METADATA_LINE_BYTES + 1)
        );
        assert!(matches!(
            parse_userscript_metadata(&long_line),
            Err(UserscriptMetadataError::LineTooLong { .. })
        ));

        let mut too_many_lines =
            "// ==UserScript==\n// @name A\n// @match https://a.example/*\n".to_owned();
        for _ in 0..(MAX_USERSCRIPT_METADATA_LINES - 3) {
            too_many_lines.push_str("//\n");
        }
        too_many_lines.push_str("// ==/UserScript==\n");
        assert!(matches!(
            parse_userscript_metadata(&too_many_lines),
            Err(UserscriptMetadataError::TooManyLines { .. })
        ));

        let mut too_many_directives =
            "// ==UserScript==\n// @name A\n// @match https://a.example/*\n".to_owned();
        for index in 0..(MAX_USERSCRIPT_DIRECTIVES - 1) {
            too_many_directives.push_str(&format!("// @unknown{index} value\n"));
        }
        too_many_directives.push_str("// ==/UserScript==\n");
        assert!(matches!(
            parse_userscript_metadata(&too_many_directives),
            Err(UserscriptMetadataError::TooManyDirectives { .. })
        ));

        let mut oversized_metadata =
            "// ==UserScript==\n// @name A\n// @match https://a.example/*\n".to_owned();
        let large_value = "x".repeat(MAX_USERSCRIPT_METADATA_LINE_BYTES - 20);
        for _ in 0..20 {
            oversized_metadata.push_str("// @unknown ");
            oversized_metadata.push_str(&large_value);
            oversized_metadata.push('\n');
        }
        oversized_metadata.push_str("// ==/UserScript==\n");
        assert!(matches!(
            parse_userscript_metadata(&oversized_metadata),
            Err(UserscriptMetadataError::MetadataTooLarge { .. })
        ));
    }

    proptest! {
        #[test]
        fn arbitrary_utf8_source_never_panics(source in any::<String>()) {
            let _ = parse_userscript_metadata(&source);
        }
    }
}
