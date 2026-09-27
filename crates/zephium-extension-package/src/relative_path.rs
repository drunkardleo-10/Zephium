use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::ops::Bound::{Included, Unbounded};
use zephium_core::extension_resource_path::{
    extension_resource_collision_key, ExtensionResourceCollisionKeyError,
};

use crate::{
    MAX_EXTENSION_PATH_COMPONENT_BYTES, MAX_EXTENSION_RELATIVE_PATH_BYTES,
    MAX_EXTENSION_RELATIVE_PATH_DEPTH,
};

/// Canonical cross-platform package-relative resource path.
///
/// The grammar is intentionally narrower than any one host filesystem. Paths
/// use `/` separators, reject URL delimiters/escapes and Windows device names
/// and characters, and preserve exact UTF-8 publisher spelling. Non-ASCII
/// characters are limited to Unicode letters and numbers excluding combining
/// marks. Package collections must additionally reject duplicate
/// [`PortableRelativePath::collision_key`] values before materialization;
/// native exact-name checks guard against further host-specific aliases.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PortableRelativePath(Box<str>);

impl PortableRelativePath {
    /// Parses one exact package-relative path.
    pub fn parse(value: &str) -> Result<Self, PortableRelativePathError> {
        if value.is_empty() {
            return Err(PortableRelativePathError::Empty);
        }
        if value.len() > MAX_EXTENSION_RELATIVE_PATH_BYTES {
            return Err(PortableRelativePathError::TooLong {
                bytes: value.len(),
                max: MAX_EXTENSION_RELATIVE_PATH_BYTES,
            });
        }
        if value.starts_with('/') || value.ends_with('/') {
            return Err(PortableRelativePathError::AbsoluteOrEmptyComponent);
        }

        let mut depth = 0_usize;
        for component in value.split('/') {
            depth = depth
                .checked_add(1)
                .ok_or(PortableRelativePathError::TooDeep {
                    depth: usize::MAX,
                    max: MAX_EXTENSION_RELATIVE_PATH_DEPTH,
                })?;
            validate_component(component)?;
        }
        if depth > MAX_EXTENSION_RELATIVE_PATH_DEPTH {
            return Err(PortableRelativePathError::TooDeep {
                depth,
                max: MAX_EXTENSION_RELATIVE_PATH_DEPTH,
            });
        }
        if !value.is_ascii() {
            extension_resource_collision_key(value, MAX_EXTENSION_RELATIVE_PATH_BYTES).map_err(
                |error| match error {
                    ExtensionResourceCollisionKeyError::UnsupportedUnicode => {
                        PortableRelativePathError::UnsupportedUnicode
                    }
                    ExtensionResourceCollisionKeyError::TooLong => {
                        PortableRelativePathError::CollisionKeyTooLong
                    }
                },
            )?;
        }
        Ok(Self(value.into()))
    }

    /// Returns the exact canonical path.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the number of path components without allocating.
    pub fn depth(&self) -> usize {
        self.0.bytes().filter(|byte| *byte == b'/').count() + 1
    }

    /// Returns the final resource-name component.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// Returns the bounded cross-platform collision key.
    ///
    /// Package collections must reject repeated keys before materialization;
    /// otherwise a signed package could name different resources on different
    /// supported filesystems.
    pub fn collision_key(&self) -> Box<str> {
        extension_resource_collision_key(&self.0, MAX_EXTENSION_RELATIVE_PATH_BYTES)
            .expect("parsed extension path has a bounded collision key")
    }
}

/// Returns whether `candidate` aliases an existing file or would make one
/// portable path both a file and a directory.
///
/// Callers pass [`PortableRelativePath::collision_key`]
/// values. The bounded ancestor walk and ordered descendant lookup avoid a
/// quadratic comparison of complete inventories.
pub(crate) fn portable_path_shape_conflicts(paths: &BTreeSet<Box<str>>, candidate: &str) -> bool {
    if paths.contains(candidate) {
        return true;
    }

    let mut ancestor = candidate;
    while let Some(separator) = ancestor.rfind('/') {
        ancestor = &ancestor[..separator];
        if paths.contains(ancestor) {
            return true;
        }
    }

    // Begin at the exact descendant prefix rather than at `candidate`'s first
    // lexical successor. A sibling such as `a-` sorts between `a` and `a/b`
    // and must not hide the descendant from this check.
    let mut descendant_prefix = String::with_capacity(candidate.len() + 1);
    descendant_prefix.push_str(candidate);
    descendant_prefix.push('/');
    paths
        .range::<str, _>((Included(descendant_prefix.as_str()), Unbounded))
        .next()
        .is_some_and(|path| path.starts_with(descendant_prefix.as_str()))
}

impl AsRef<str> for PortableRelativePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for PortableRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Debug for PortableRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PortableRelativePath")
            .field(&self.as_str())
            .finish()
    }
}

fn validate_component(component: &str) -> Result<(), PortableRelativePathError> {
    if component.is_empty() || matches!(component, "." | "..") {
        return Err(PortableRelativePathError::AbsoluteOrEmptyComponent);
    }
    if component.len() > MAX_EXTENSION_PATH_COMPONENT_BYTES {
        return Err(PortableRelativePathError::ComponentTooLong {
            bytes: component.len(),
            max: MAX_EXTENSION_PATH_COMPONENT_BYTES,
        });
    }
    let bytes = component.as_bytes();
    if bytes.iter().any(|byte| {
        byte.is_ascii_control()
            || matches!(
                *byte,
                b'\\' | b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'%' | b'#'
            )
    }) {
        return Err(PortableRelativePathError::ForbiddenCharacter);
    }
    if bytes
        .last()
        .is_some_and(|byte| matches!(*byte, b' ' | b'.'))
    {
        return Err(PortableRelativePathError::AmbiguousComponentEnding);
    }
    let device_stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .to_ascii_uppercase();
    let reserved_numbered = device_stem.len() == 4
        && (device_stem.as_bytes()[..3] == *b"COM" || device_stem.as_bytes()[..3] == *b"LPT")
        && matches!(device_stem.as_bytes()[3], b'1'..=b'9');
    if matches!(
        device_stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || reserved_numbered
    {
        return Err(PortableRelativePathError::ReservedDeviceName);
    }
    Ok(())
}

/// Stable portable-path rejection reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortableRelativePathError {
    /// The path is empty.
    Empty,
    /// The complete path exceeds its byte ceiling.
    TooLong {
        /// Observed bytes.
        bytes: usize,
        /// Maximum bytes.
        max: usize,
    },
    /// A component exceeds its byte ceiling.
    ComponentTooLong {
        /// Observed bytes.
        bytes: usize,
        /// Maximum bytes.
        max: usize,
    },
    /// The path contains a non-ASCII scalar outside the admitted subset or
    /// one whose folded spelling is unsafe as a path component.
    UnsupportedUnicode,
    /// The collision key would exceed the existing path byte ceiling.
    CollisionKeyTooLong,
    /// The path is absolute, has an empty component, or contains `.`/`..`.
    AbsoluteOrEmptyComponent,
    /// The path has too many components.
    TooDeep {
        /// Observed depth.
        depth: usize,
        /// Maximum depth.
        max: usize,
    },
    /// A component contains a control, URL delimiter/escape, or non-portable
    /// filesystem character.
    ForbiddenCharacter,
    /// A component ends in a dot or space and aliases on Windows.
    AmbiguousComponentEnding,
    /// A component names a reserved Windows device.
    ReservedDeviceName,
}

impl fmt::Display for PortableRelativePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("extension resource path is empty"),
            Self::TooLong { bytes, max } => {
                write!(
                    formatter,
                    "extension resource path uses {bytes} bytes; maximum is {max}"
                )
            }
            Self::ComponentTooLong { bytes, max } => write!(
                formatter,
                "extension resource path component uses {bytes} bytes; maximum is {max}"
            ),
            Self::UnsupportedUnicode => {
                formatter.write_str("extension resource path contains unsupported Unicode")
            }
            Self::CollisionKeyTooLong => {
                formatter.write_str("extension resource path collision key exceeds its byte bound")
            }
            Self::AbsoluteOrEmptyComponent => formatter
                .write_str("extension resource path is absolute or has an empty/dot component"),
            Self::TooDeep { depth, max } => write!(
                formatter,
                "extension resource path has depth {depth}; maximum is {max}"
            ),
            Self::ForbiddenCharacter => formatter.write_str(
                "extension resource path contains a forbidden filesystem or URL character",
            ),
            Self::AmbiguousComponentEnding => formatter.write_str(
                "extension resource path component has an ambiguous trailing dot or space",
            ),
            Self::ReservedDeviceName => {
                formatter.write_str("extension resource path names a reserved device")
            }
        }
    }
}

impl Error for PortableRelativePathError {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn accepts_realistic_portable_extension_paths() {
        for path in [
            "manifest.json",
            "_locales/en/messages.json",
            "js/background-a1B2_3.js",
            "images/icon@2x.png",
            "wasm/argon2.wasm",
            "src/js/сlickableCard.common.chunk.js",
            "icons/café.png",
            "資源/説明.txt",
        ] {
            let parsed = PortableRelativePath::parse(path).expect(path);
            assert_eq!(parsed.as_str(), path);
        }
    }

    #[test]
    fn rejects_cross_platform_aliases_and_traversal() {
        for path in [
            "",
            "/manifest.json",
            "manifest.json/",
            "a//b",
            "a/./b",
            "a/../b",
            r"a\b",
            "a:b",
            "a?.js",
            "a#fragment.js",
            "%2e%2e/secret.js",
            "images/%2Fsecret.png",
            "images/%73ecret.png",
            "trailing. ",
            "con",
            "NUL.txt",
            "lpt9.js",
            "emoji-😀.js",
        ] {
            assert!(PortableRelativePath::parse(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn rejects_every_source_form_that_a_url_layer_can_decode_or_strip() {
        let normalized = url::Url::parse("zephium-extension://abcdefghijkl/a/%2e%2e/secret.js")
            .expect("hostile extension URL parses");
        assert_eq!(normalized.path(), "/secret.js");

        for path in [
            "a/%2e%2e/secret.js",
            "a/%2E./secret.js",
            "a/%2fsecret.js",
            "a/%5csecret.js",
            "a/file.js#ignored",
        ] {
            assert_eq!(
                PortableRelativePath::parse(path),
                Err(PortableRelativePathError::ForbiddenCharacter),
                "accepted URL-ambiguous path {path:?}",
            );
        }
    }

    #[test]
    fn collision_keys_are_ascii_case_insensitive() {
        let first = PortableRelativePath::parse("Scripts/Main.JS").unwrap();
        let second = PortableRelativePath::parse("scripts/main.js").unwrap();
        assert_ne!(first, second);
        assert_eq!(first.collision_key(), second.collision_key());
        let punctuation = PortableRelativePath::parse("Foo + @2x/Bar (v1)~.JS").unwrap();
        assert_eq!(
            punctuation.collision_key().as_ref(),
            "foo + @2x/bar (v1)~.js"
        );
    }

    #[test]
    fn unicode_collision_keys_keep_distinct_publishers_spelling_but_reject_aliases() {
        let original = PortableRelativePath::parse("src/js/сlickableCard.common.chunk.js").unwrap();
        let cased = PortableRelativePath::parse("SRC/JS/СLICKABLECARD.COMMON.CHUNK.JS").unwrap();
        assert_ne!(original, cased);
        assert_eq!(original.collision_key(), cased.collision_key());
        assert_eq!(original.as_str(), "src/js/сlickableCard.common.chunk.js");
        assert_ne!(
            original.collision_key().as_ref(),
            "src/js/clickablecard.common.chunk.js"
        );
        assert_eq!(
            PortableRelativePath::parse("cafe\u{301}.js"),
            Err(PortableRelativePathError::UnsupportedUnicode)
        );
        assert_eq!(
            PortableRelativePath::parse("ＣＯＮ.txt"),
            Err(PortableRelativePathError::UnsupportedUnicode)
        );
        for path in ["COM¹.txt", "name\u{0345}.js"] {
            assert_eq!(
                PortableRelativePath::parse(path),
                Err(PortableRelativePathError::UnsupportedUnicode)
            );
        }
    }

    proptest! {
        #[test]
        fn parsing_arbitrary_text_never_changes_an_accepted_path(value in any::<String>()) {
            if let Ok(path) = PortableRelativePath::parse(&value) {
                prop_assert_eq!(path.as_str(), value.as_str());
                prop_assert!(path.as_str().len() <= MAX_EXTENSION_RELATIVE_PATH_BYTES);
                prop_assert!(path.depth() <= MAX_EXTENSION_RELATIVE_PATH_DEPTH);
                prop_assert!(path.collision_key().len() <= MAX_EXTENSION_RELATIVE_PATH_BYTES);
            }
        }
    }
}
