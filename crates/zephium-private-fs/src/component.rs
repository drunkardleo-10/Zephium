use std::fmt;

use thiserror::Error;

const MAX_COMPONENT_BYTES: usize = 128;

/// One portable, traversal-free filesystem component.
///
/// Components use lowercase ASCII, digits, dot, underscore, and hyphen. This
/// deliberately smaller alphabet is safe to append exactly once and compare
/// byte-for-byte across macOS, Linux, and Windows.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrivateComponent(Box<str>);

impl PrivateComponent {
    /// Validates and owns one component.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, PrivateComponentError> {
        let value = value.into();
        validate(&value)?;
        Ok(Self(value))
    }

    /// Returns the exact portable spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PrivateComponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PrivateComponent")
            .field(&self.0)
            .finish()
    }
}

impl AsRef<str> for PrivateComponent {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// A filesystem component is not in the portable private namespace grammar.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PrivateComponentError {
    /// The component was empty or exceeded the fixed byte ceiling.
    #[error("private filesystem component length is outside the portable bound")]
    Length,
    /// The component contained traversal, separators, or nonportable bytes.
    #[error("private filesystem component contains nonportable bytes")]
    NonPortable,
    /// The component aliases a reserved Windows device name.
    #[error("private filesystem component aliases a reserved device name")]
    ReservedDevice,
}

fn validate(value: &str) -> Result<(), PrivateComponentError> {
    if value.is_empty() || value.len() > MAX_COMPONENT_BYTES {
        return Err(PrivateComponentError::Length);
    }
    if matches!(value, "." | "..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        || value.ends_with(['.', ' '])
    {
        return Err(PrivateComponentError::NonPortable);
    }
    let stem = value.split('.').next().unwrap_or_default();
    if matches!(stem, "con" | "prn" | "aux" | "nul")
        || stem.strip_prefix("com").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || stem.strip_prefix("lpt").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
    {
        return Err(PrivateComponentError::ReservedDevice);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_only_one_portable_component() {
        for value in ["lock-v1", ".stage.bin", "a_9.json"] {
            assert_eq!(PrivateComponent::new(value).unwrap().as_str(), value);
        }
        for value in [
            "", ".", "..", "a/b", "a\\b", "a:b", " white", "white ", "A.json", "café",
        ] {
            assert!(PrivateComponent::new(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn rejects_windows_devices_on_every_platform() {
        for value in ["con", "prn.txt", "aux", "nul.bin", "com1", "lpt9.log"] {
            assert_eq!(
                PrivateComponent::new(value),
                Err(PrivateComponentError::ReservedDevice),
                "accepted {value:?}"
            );
        }
    }
}
