//! Admission-only Unicode collision policy for extension resource paths.
//!
//! Callers keep the publisher's UTF-8 spelling for every resource lookup and
//! digest. This module only produces a conservative key for detecting package
//! paths that could alias on a supported filesystem. It does not authorize a
//! path, perform filesystem access, or replace caller-specific path grammar.

use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Why a non-ASCII resource collision key could not be produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionResourceCollisionKeyError {
    /// A non-ASCII scalar is outside the conservative letter/number subset.
    UnsupportedUnicode,
    /// Case and compatibility folding expanded beyond the caller's byte bound.
    TooLong,
}

/// Returns a bounded cross-platform collision key without changing `path`.
///
/// Callers first enforce their ordinary component grammar and maximum path
/// length. ASCII paths retain their historic ASCII-lowercase key exactly. For
/// Unicode paths, only non-ASCII letters and numbers that are not combining
/// marks are admitted. NFKC and
/// case mapping deliberately over-identify some distinct filesystem names;
/// rejecting such a package is safer than storing host-dependent resources.
/// Native exact-name checks remain the final guard against host-specific
/// aliases that Unicode tables cannot model.
pub fn extension_resource_collision_key(
    path: &str,
    maximum_bytes: usize,
) -> Result<Box<str>, ExtensionResourceCollisionKeyError> {
    if path.is_ascii() {
        return (path.len() <= maximum_bytes)
            .then(|| path.to_ascii_lowercase().into_boxed_str())
            .ok_or(ExtensionResourceCollisionKeyError::TooLong);
    }

    let mut key = String::with_capacity(path.len().min(maximum_bytes));
    for (index, component) in path.split('/').enumerate() {
        if component.chars().any(|scalar| {
            !scalar.is_ascii()
                && (!scalar.is_alphanumeric()
                    || is_combining_mark(scalar)
                    || is_invisible_hangul_filler(scalar))
        }) {
            return Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode);
        }
        if index != 0 {
            push_bounded(&mut key, '/', maximum_bytes)?;
        }
        let component_start = key.len();
        // Work per component so normalization cannot create a new path
        // separator or change the file/directory ancestor relationship.
        for scalar in component
            .nfkc()
            .flat_map(char::to_uppercase)
            .flat_map(char::to_lowercase)
            .nfkc()
        {
            if matches!(scalar, '/' | '\\') {
                return Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode);
            }
            push_bounded(&mut key, scalar, maximum_bytes)?;
        }
        if !folded_component_safe(&key[component_start..]) {
            return Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode);
        }
    }
    Ok(key.into_boxed_str())
}

fn is_invisible_hangul_filler(scalar: char) -> bool {
    // These default-ignorable Hangul fillers are alphanumeric in Unicode.
    matches!(scalar, '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}')
}

fn folded_component_safe(component: &str) -> bool {
    if component.is_empty()
        || matches!(component, "." | "..")
        || component.starts_with(' ')
        || component.ends_with([' ', '.'])
        || component.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(
                    byte,
                    b'\\' | b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'%' | b'#'
                )
        })
    {
        return false;
    }
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem);
    let stem = stem.to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !(stem.len() == 4
        && (stem.as_bytes()[..3] == *b"COM" || stem.as_bytes()[..3] == *b"LPT")
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn push_bounded(
    output: &mut String,
    scalar: char,
    maximum_bytes: usize,
) -> Result<(), ExtensionResourceCollisionKeyError> {
    if output
        .len()
        .checked_add(scalar.len_utf8())
        .is_none_or(|length| length > maximum_bytes)
    {
        return Err(ExtensionResourceCollisionKeyError::TooLong);
    }
    output.push(scalar);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ascii_keys_and_folds_unicode_without_rewriting_paths() {
        let key = |path| extension_resource_collision_key(path, 512).unwrap();
        assert_eq!(key("Scripts/Main.JS").as_ref(), "scripts/main.js");
        assert_eq!(
            key("src/js/сlickableCard.common.chunk.js"),
            key("SRC/JS/СLICKABLECARD.COMMON.CHUNK.JS")
        );
        assert_eq!(key("café.js"), key("CAFÉ.JS"));
        assert_eq!(key("資源/説明.txt").as_ref(), "資源/説明.txt");
        assert_eq!(key("Kelvin.js"), key("kelvin.js"));
        assert_ne!(key("сlickable.js"), key("clickable.js"));
    }

    #[test]
    fn bounds_expansion_and_rejects_non_alphanumeric_unicode() {
        assert_eq!(
            extension_resource_collision_key("name😀.js", 512),
            Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode)
        );
        for mark in ['\u{0345}', '\u{05b0}'] {
            assert_eq!(
                extension_resource_collision_key(&format!("name{mark}.js"), 512),
                Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode)
            );
        }
        for filler in ['\u{115f}', '\u{1160}', '\u{3164}', '\u{ffa0}'] {
            assert_eq!(
                extension_resource_collision_key(&format!("name{filler}.js"), 512),
                Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode)
            );
        }
        assert_eq!(
            extension_resource_collision_key("ＣＯＮ.txt", 512),
            Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode)
        );
        assert_eq!(
            extension_resource_collision_key("COM¹.txt", 512),
            Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode)
        );
        assert_eq!(
            extension_resource_collision_key("\u{037a}name.js", 512),
            Err(ExtensionResourceCollisionKeyError::UnsupportedUnicode)
        );
        assert_eq!(
            extension_resource_collision_key("é".repeat(128).as_str(), 127),
            Err(ExtensionResourceCollisionKeyError::TooLong)
        );
        assert_eq!(
            extension_resource_collision_key("é", 1),
            Err(ExtensionResourceCollisionKeyError::TooLong)
        );
        assert_eq!(
            extension_resource_collision_key("ß".repeat(128).as_str(), 128),
            Err(ExtensionResourceCollisionKeyError::TooLong)
        );
    }
}
