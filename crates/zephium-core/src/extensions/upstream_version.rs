//! Bounded upstream version ordering, distinct from Zephium catalog revisions.

use std::fmt;

/// Numeric Chromium extension version, zero-padded to four components.
///
/// `1.2` and `1.2.0.0` identify the same version for rollback protection.
/// Display-only `version_name` values must never be used for this comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ExtensionUpstreamVersion([u16; 4]);

impl ExtensionUpstreamVersion {
    /// Parses one to four decimal components, each in 0..=65535, without
    /// leading zeroes or an all-zero version. Parsing performs no allocation.
    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > 23 {
            return None;
        }
        let mut components = [0; 4];
        for (index, part) in value.split('.').enumerate() {
            if index >= components.len()
                || part.is_empty()
                || part.len() > 5
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return None;
            }
            components[index] = part.parse().ok()?;
        }
        Self::from_components(components)
    }

    /// Reconstructs structural durable data; this authenticates no package.
    pub const fn from_components(components: [u16; 4]) -> Option<Self> {
        if components[0] == 0 && components[1] == 0 && components[2] == 0 && components[3] == 0 {
            None
        } else {
            Some(Self(components))
        }
    }

    /// Returns the canonical comparison key for durable rollback checks.
    pub const fn components(self) -> [u16; 4] {
        self.0
    }
}

impl fmt::Display for ExtensionUpstreamVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let last = self.0.iter().rposition(|part| *part != 0).unwrap_or(0);
        for (index, part) in self.0[..=last].iter().enumerate() {
            if index != 0 {
                formatter.write_str(".")?;
            }
            write!(formatter, "{part}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numeric_versions_and_normalizes_trailing_zeroes() {
        let version = |value| ExtensionUpstreamVersion::parse(value).unwrap();
        assert!(version("1.10") > version("1.9.65535"));
        assert!(version("2") > version("1.65535.65535.65535"));
        assert_eq!(version("1.2"), version("1.2.0.0"));
        assert_eq!(version("1.2.0.0").to_string(), "1.2");
        assert_eq!(version("0.0.0.1").components(), [0, 0, 0, 1]);
        assert!(version("65535.65535.65535.65535") > version("65535.65535.65535"));
    }

    #[test]
    fn rejects_ambiguous_or_unbounded_versions() {
        for value in [
            "",
            "0",
            "0.0.0.0",
            "01",
            "1.00",
            "1.",
            ".1",
            "1..2",
            "1.2.3.4.5",
            "65536",
            "1.-1",
            "+1",
            "1e2",
            "1-beta",
            " 1",
            "1\n",
            "١",
            "000000001",
        ] {
            assert!(
                ExtensionUpstreamVersion::parse(value).is_none(),
                "{value:?}"
            );
        }
        assert!(ExtensionUpstreamVersion::from_components([0; 4]).is_none());
    }
}
