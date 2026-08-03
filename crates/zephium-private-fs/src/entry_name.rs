//! Case-preserving portable names for authenticated payload entries.

use std::fmt;

use thiserror::Error;

/// Maximum UTF-8 bytes in one portable private payload entry name.
pub const MAX_PRIVATE_ENTRY_NAME_BYTES: usize = 128;

/// One exact, case-preserving portable payload entry name.
///
/// This type is deliberately distinct from [`crate::PrivateComponent`].
/// `PrivateComponent` is the normalized, lowercase namespace-control grammar;
/// `PrivateEntryName` preserves authenticated package spelling while admitting
/// only the single-component subset portable across macOS, Linux, and Windows.
/// It performs no Unicode, case, whitespace, or punctuation normalization.
///
/// Constructing a name does not authorize filesystem access or mutation.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrivateEntryName(Box<str>);

impl PrivateEntryName {
    /// Validates and owns one exact portable entry name.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, PrivateEntryNameError> {
        let value = value.into();
        validate(&value)?;
        Ok(Self(value))
    }

    /// Returns the exact case-preserving spelling supplied at construction.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PrivateEntryName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PrivateEntryName")
            .field(&self.0)
            .finish()
    }
}

impl AsRef<str> for PrivateEntryName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Stable rejection reason for a portable private payload entry name.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PrivateEntryNameError {
    /// The entry name is empty.
    #[error("private payload entry name is empty")]
    Empty,
    /// The entry name exceeds the fixed byte ceiling.
    #[error("private payload entry name exceeds the portable byte bound")]
    TooLong,
    /// The entry name contains non-ASCII text.
    #[error("private payload entry name is not ASCII")]
    NonAscii,
    /// The entry name is `.` or `..`.
    #[error("private payload entry name is a traversal component")]
    DotComponent,
    /// The entry name contains a separator, control, URL delimiter/escape, or
    /// non-portable filesystem character.
    #[error("private payload entry name contains a forbidden character")]
    ForbiddenCharacter,
    /// The entry name ends in a dot or space and aliases on Windows.
    #[error("private payload entry name has an ambiguous ending")]
    AmbiguousEnding,
    /// The entry name aliases a reserved Windows device name.
    #[error("private payload entry name aliases a reserved device name")]
    ReservedDeviceName,
}

fn validate(value: &str) -> Result<(), PrivateEntryNameError> {
    if value.is_empty() {
        return Err(PrivateEntryNameError::Empty);
    }
    if value.len() > MAX_PRIVATE_ENTRY_NAME_BYTES {
        return Err(PrivateEntryNameError::TooLong);
    }
    if !value.is_ascii() {
        return Err(PrivateEntryNameError::NonAscii);
    }
    if matches!(value, "." | "..") {
        return Err(PrivateEntryNameError::DotComponent);
    }
    let bytes = value.as_bytes();
    if bytes.iter().any(|byte| {
        byte.is_ascii_control()
            || matches!(
                *byte,
                b'/' | b'\\' | b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'%' | b'#'
            )
    }) {
        return Err(PrivateEntryNameError::ForbiddenCharacter);
    }
    if bytes
        .last()
        .is_some_and(|byte| matches!(*byte, b' ' | b'.'))
    {
        return Err(PrivateEntryNameError::AmbiguousEnding);
    }
    if is_reserved_windows_device(value) {
        return Err(PrivateEntryNameError::ReservedDeviceName);
    }
    Ok(())
}

fn is_reserved_windows_device(value: &str) -> bool {
    let stem = value.split_once('.').map_or(value, |(stem, _)| stem);
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_exact_authenticated_spelling() {
        let value = "Mixed CASE + spaces (v1)@$^_`{}~.JS";
        let name = PrivateEntryName::new(value).unwrap();
        assert_eq!(name.as_str(), value);
        assert_eq!(name.as_ref(), value);
    }

    #[test]
    fn enforces_empty_dot_and_exact_byte_boundaries() {
        assert_eq!(PrivateEntryName::new(""), Err(PrivateEntryNameError::Empty));
        for value in [".", ".."] {
            assert_eq!(
                PrivateEntryName::new(value),
                Err(PrivateEntryNameError::DotComponent)
            );
        }

        let maximum = "A".repeat(MAX_PRIVATE_ENTRY_NAME_BYTES);
        assert_eq!(
            PrivateEntryName::new(maximum.clone()).unwrap().as_str(),
            maximum
        );
        assert_eq!(
            PrivateEntryName::new("A".repeat(MAX_PRIVATE_ENTRY_NAME_BYTES + 1)),
            Err(PrivateEntryNameError::TooLong)
        );
        assert_eq!(
            PrivateEntryName::new("café.js"),
            Err(PrivateEntryNameError::NonAscii)
        );
    }

    #[test]
    fn exhaustively_enforces_the_ascii_character_alphabet() {
        for byte in 0_u8..=127 {
            let value = String::from_utf8(vec![b'a', byte, b'b']).unwrap();
            let should_admit = !byte.is_ascii_control()
                && !matches!(
                    byte,
                    b'/' | b'\\' | b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'%' | b'#'
                );
            assert_eq!(
                PrivateEntryName::new(value.clone()).is_ok(),
                should_admit,
                "unexpected result for ASCII byte {byte:#04x} in {value:?}"
            );
        }
    }

    #[test]
    fn rejects_ambiguous_endings_but_allows_internal_dot_and_space() {
        for value in ["name.", "name "] {
            assert_eq!(
                PrivateEntryName::new(value),
                Err(PrivateEntryNameError::AmbiguousEnding)
            );
        }
        for value in ["name .txt", "name. txt", ".hidden", "two words.js"] {
            assert_eq!(PrivateEntryName::new(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn rejects_windows_devices_case_insensitively_with_extensions() {
        for stem in ["CON", "prn", "AuX", "nul", "ConIn$", "cOnOuT$"] {
            for value in [stem.to_owned(), format!("{stem}.js")] {
                assert_eq!(
                    PrivateEntryName::new(value),
                    Err(PrivateEntryNameError::ReservedDeviceName)
                );
            }
        }
        for prefix in ["COM", "lPt"] {
            for number in '1'..='9' {
                for value in [format!("{prefix}{number}"), format!("{prefix}{number}.bin")] {
                    assert_eq!(
                        PrivateEntryName::new(value),
                        Err(PrivateEntryNameError::ReservedDeviceName)
                    );
                }
            }
        }
    }

    #[test]
    fn does_not_reject_device_name_lookalikes() {
        for value in [
            "COM0",
            "COM10",
            "LPT0",
            "LPT10",
            "CONSOLE",
            "CON-IN",
            "CONIN",
            "CONOUT",
            "AUXILIARY",
            "NULLED",
            "x.CON",
        ] {
            assert_eq!(PrivateEntryName::new(value).unwrap().as_str(), value);
        }
    }
}
