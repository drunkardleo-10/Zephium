//! Platform-independent macOS/WebKit runtime admission policy.
//!
//! Apple ships current WebKit security fixes as a Safari update on Sonoma and
//! Sequoia, but as a macOS update on Tahoe. Admission therefore checks both
//! product versions and requires the installed Safari build to match the
//! WebKit framework build that actually owns `WKWebView`. The review deadline
//! prevents these point-release floors from becoming a permanent claim.

use std::fmt;
use std::str::FromStr;

pub const SONOMA_SECURITY_FLOOR: ProductVersion = ProductVersion::new(14, 8, 7);
pub const SEQUOIA_SECURITY_FLOOR: ProductVersion = ProductVersion::new(15, 7, 7);
pub const TAHOE_SECURITY_FLOOR: ProductVersion = ProductVersion::new(26, 5, 2);
pub const SAFARI_SECURITY_FLOOR: ProductVersion = ProductVersion::new(26, 5, 2);

pub const SONOMA_SECURITY_FLOOR_TEXT: &str = "14.8.7";
pub const SEQUOIA_SECURITY_FLOOR_TEXT: &str = "15.7.7";
pub const TAHOE_SECURITY_FLOOR_TEXT: &str = "26.5.2";
pub const SAFARI_SECURITY_FLOOR_TEXT: &str = "26.5.2";
pub const SECURITY_FLOOR_PUBLISHED_ON: &str = "2026-06-29";
/// 2026-06-29T00:00:00Z. A wall clock before the reviewed Apple security
/// release cannot establish that this floor was published and must fail closed.
pub const SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS: u64 = 1_782_691_200;
pub const SECURITY_FLOOR_SOURCE_URL: &str = "https://support.apple.com/en-us/100100";
pub const SAFARI_SECURITY_SOURCE_URL: &str = "https://support.apple.com/en-us/127685";
pub const TAHOE_SECURITY_SOURCE_URL: &str = "https://support.apple.com/en-us/127595";

/// The last UTC date on which CI may accept this review without an update.
pub const SECURITY_FLOOR_REVIEW_BY: &str = "2026-07-27";
/// 2026-07-28T00:00:00Z. The human-readable review date above is inclusive.
pub const SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS: u64 = 1_785_196_800;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProductVersion([u32; 3]);

impl ProductVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self([major, minor, patch])
    }

    pub const fn components(self) -> [u32; 3] {
        self.0
    }

    pub const fn major(self) -> u32 {
        self.0[0]
    }
}

impl fmt::Display for ProductVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionParseError {
    Empty,
    TooLong,
    WrongComponentCount,
    InvalidComponent,
    NonCanonicalComponent,
    ComponentOverflow,
}

impl fmt::Display for VersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "version is empty",
            Self::TooLong => "version is longer than 32 bytes",
            Self::WrongComponentCount => "version has an unsupported numeric component count",
            Self::InvalidComponent => "version component is not an unsigned ASCII integer",
            Self::NonCanonicalComponent => "version component has a leading zero",
            Self::ComponentOverflow => "version component exceeds u32",
        })
    }
}

impl std::error::Error for VersionParseError {}

impl FromStr for ProductVersion {
    type Err = VersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(VersionParseError::Empty);
        }
        if value.len() > 32 {
            return Err(VersionParseError::TooLong);
        }

        let mut components = [0_u32; 3];
        let mut parsed = value.split('.');
        for component in &mut components {
            let raw = parsed
                .next()
                .ok_or(VersionParseError::WrongComponentCount)?;
            if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(VersionParseError::InvalidComponent);
            }
            if raw.len() > 1 && raw.starts_with('0') {
                return Err(VersionParseError::NonCanonicalComponent);
            }
            *component = raw
                .parse()
                .map_err(|_| VersionParseError::ComponentOverflow)?;
        }
        if parsed.next().is_some() {
            return Err(VersionParseError::WrongComponentCount);
        }
        Ok(Self(components))
    }
}

fn parse_safari_version(value: &str) -> Result<ProductVersion, VersionParseError> {
    if value.is_empty() {
        return Err(VersionParseError::Empty);
    }
    if value.len() > 32 {
        return Err(VersionParseError::TooLong);
    }

    let mut parsed = value.split('.');
    let major = parse_version_component(
        parsed
            .next()
            .ok_or(VersionParseError::WrongComponentCount)?,
    )?;
    let minor = parse_version_component(
        parsed
            .next()
            .ok_or(VersionParseError::WrongComponentCount)?,
    )?;
    let patch = match parsed.next() {
        Some(raw) => parse_version_component(raw)?,
        None => 0,
    };
    if parsed.next().is_some() {
        return Err(VersionParseError::WrongComponentCount);
    }
    Ok(ProductVersion::new(major, minor, patch))
}

fn parse_version_component(raw: &str) -> Result<u32, VersionParseError> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VersionParseError::InvalidComponent);
    }
    if raw.len() > 1 && raw.starts_with('0') {
        return Err(VersionParseError::NonCanonicalComponent);
    }
    raw.parse()
        .map_err(|_| VersionParseError::ComponentOverflow)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildVersionError {
    Empty,
    TooLong,
    TooManyComponents,
    InvalidComponent,
    NonCanonicalComponent,
}

impl fmt::Display for BuildVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "build version is empty",
            Self::TooLong => "build version is longer than 64 bytes",
            Self::TooManyComponents => "build version has more than eight components",
            Self::InvalidComponent => "build version component is not an unsigned ASCII integer",
            Self::NonCanonicalComponent => "build version component has a leading zero",
        })
    }
}

impl std::error::Error for BuildVersionError {}

fn validate_build_version(value: &str) -> Result<(), BuildVersionError> {
    if value.is_empty() {
        return Err(BuildVersionError::Empty);
    }
    if value.len() > 64 {
        return Err(BuildVersionError::TooLong);
    }
    let mut count = 0_usize;
    for raw in value.split('.') {
        count += 1;
        if count > 8 {
            return Err(BuildVersionError::TooManyComponents);
        }
        if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BuildVersionError::InvalidComponent);
        }
        if raw.len() > 1 && raw.starts_with('0') {
            return Err(BuildVersionError::NonCanonicalComponent);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    InvalidOperatingSystemVersion(VersionParseError),
    InvalidSafariVersion(VersionParseError),
    InvalidSafariBuild(BuildVersionError),
    InvalidWebKitBuild(BuildVersionError),
    UnsupportedOperatingSystemMajor(u32),
    BelowOperatingSystemFloor {
        found: ProductVersion,
        required: ProductVersion,
    },
    UnsupportedSafariMajor(u32),
    BelowSafariFloor {
        found: ProductVersion,
        required: ProductVersion,
    },
    SafariWebKitBuildMismatch,
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOperatingSystemVersion(error) => {
                write!(formatter, "invalid operating-system version: {error}")
            }
            Self::InvalidSafariVersion(error) => {
                write!(formatter, "invalid Safari version: {error}")
            }
            Self::InvalidSafariBuild(error) => write!(formatter, "invalid Safari build: {error}"),
            Self::InvalidWebKitBuild(error) => write!(formatter, "invalid WebKit build: {error}"),
            Self::UnsupportedOperatingSystemMajor(major) => {
                write!(
                    formatter,
                    "macOS major {major} has not been security-reviewed"
                )
            }
            Self::BelowOperatingSystemFloor { found, required } => {
                write!(
                    formatter,
                    "macOS {found} is below security floor {required}"
                )
            }
            Self::UnsupportedSafariMajor(major) => {
                write!(
                    formatter,
                    "Safari major {major} has not been security-reviewed"
                )
            }
            Self::BelowSafariFloor { found, required } => {
                write!(
                    formatter,
                    "Safari {found} is below security floor {required}"
                )
            }
            Self::SafariWebKitBuildMismatch => formatter.write_str(
                "Safari and the loaded WebKit framework report different build versions",
            ),
        }
    }
}

impl std::error::Error for AdmissionError {}

/// Admit only an Apple-supported, reviewed macOS/WebKit combination.
///
/// `safari_build` must come from the protected system Safari bundle and
/// `webkit_build` from the bundle owning the loaded `WKWebView` class. Their
/// equality is what connects Safari's marketing version to the shared WebKit
/// framework used by the embedder.
pub fn admit_runtime(
    operating_system: &str,
    safari: &str,
    safari_build: &str,
    webkit_build: &str,
) -> Result<(), AdmissionError> {
    let operating_system = operating_system
        .parse::<ProductVersion>()
        .map_err(AdmissionError::InvalidOperatingSystemVersion)?;
    let safari = parse_safari_version(safari).map_err(AdmissionError::InvalidSafariVersion)?;
    validate_build_version(safari_build).map_err(AdmissionError::InvalidSafariBuild)?;
    validate_build_version(webkit_build).map_err(AdmissionError::InvalidWebKitBuild)?;
    if safari_build != webkit_build {
        return Err(AdmissionError::SafariWebKitBuildMismatch);
    }

    let required_os = match operating_system.major() {
        14 => SONOMA_SECURITY_FLOOR,
        15 => SEQUOIA_SECURITY_FLOOR,
        26 => TAHOE_SECURITY_FLOOR,
        major => return Err(AdmissionError::UnsupportedOperatingSystemMajor(major)),
    };
    if operating_system < required_os {
        return Err(AdmissionError::BelowOperatingSystemFloor {
            found: operating_system,
            required: required_os,
        });
    }

    // The reviewed Safari/WebKit release line is 26. Tahoe receives the same
    // WebKit fixes in its OS update, so only Sonoma and Sequoia independently
    // require the 26.5.2 Safari point release.
    if safari.major() != SAFARI_SECURITY_FLOOR.major() {
        return Err(AdmissionError::UnsupportedSafariMajor(safari.major()));
    }
    if matches!(operating_system.major(), 14 | 15) && safari < SAFARI_SECURITY_FLOOR {
        return Err(AdmissionError::BelowSafariFloor {
            found: safari,
            required: SAFARI_SECURITY_FLOOR,
        });
    }
    Ok(())
}

pub const fn security_floor_review_is_current(unix_seconds: u64) -> bool {
    unix_seconds >= SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS
        && unix_seconds < SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD: &str = "21624.3.4.5.6";

    #[test]
    fn strict_product_version_parser_rejects_ambiguous_input() {
        let version: ProductVersion = "26.5.2".parse().unwrap();
        assert_eq!(version.components(), [26, 5, 2]);
        assert_eq!(version.to_string(), "26.5.2");

        for value in [
            "",
            "26",
            "26.5",
            "26.5.2.0",
            ".5.2",
            "26..2",
            "26.5.",
            "026.5.2",
            "26.05.2",
            "26.5.02",
            "+26.5.2",
            "26.5.-2",
            "26.5.2 ",
            " 26.5.2",
            "２６.5.2",
            "4294967296.0.0",
        ] {
            assert!(
                value.parse::<ProductVersion>().is_err(),
                "unexpectedly accepted {value:?}"
            );
        }
        assert_eq!(
            "1".repeat(33).parse::<ProductVersion>(),
            Err(VersionParseError::TooLong)
        );
    }

    #[test]
    fn safari_parser_canonically_supports_apples_omitted_zero_patch() {
        assert_eq!(
            parse_safari_version("26.5").unwrap().components(),
            [26, 5, 0]
        );
        assert_eq!(
            parse_safari_version("26.5.2").unwrap().components(),
            [26, 5, 2]
        );
        for value in ["26", "26.5.2.0", "26.05", "26.5.", "26.5 ", "26.beta"] {
            assert!(parse_safari_version(value).is_err());
        }
    }

    #[test]
    fn admits_only_reviewed_fully_patched_release_lines() {
        for (os, safari) in [
            ("14.8.7", "26.5.2"),
            ("14.8.8", "26.5.3"),
            ("15.7.7", "26.5.2"),
            ("15.8.0", "26.6.0"),
            ("26.5.2", "26.5"),
            ("26.5.3", "26.5.2"),
        ] {
            assert_eq!(admit_runtime(os, safari, BUILD, BUILD), Ok(()));
        }
    }

    #[test]
    fn rejects_old_and_unknown_os_or_safari_lines() {
        for (os, safari, expected) in [
            ("14.8.6", "26.5.2", "os"),
            ("15.7.6", "26.5.2", "os"),
            ("26.5.1", "26.5.2", "os"),
            ("14.8.7", "26.5.1", "safari"),
            ("15.7.7", "26.5.1", "safari"),
            ("13.9.9", "26.5.2", "major"),
            ("16.0.0", "26.5.2", "major"),
            ("27.0.0", "26.5.2", "major"),
            ("26.5.2", "27.0.0", "safari-major"),
        ] {
            let error = admit_runtime(os, safari, BUILD, BUILD).unwrap_err();
            match expected {
                "os" => assert!(matches!(
                    error,
                    AdmissionError::BelowOperatingSystemFloor { .. }
                )),
                "safari" => assert!(matches!(error, AdmissionError::BelowSafariFloor { .. })),
                "major" => assert!(matches!(
                    error,
                    AdmissionError::UnsupportedOperatingSystemMajor(_)
                )),
                "safari-major" => {
                    assert!(matches!(error, AdmissionError::UnsupportedSafariMajor(_)))
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn rejects_unmatched_or_malformed_bundle_builds() {
        assert_eq!(
            admit_runtime("26.5.2", "26.5.2", "21624.1", "21624.2"),
            Err(AdmissionError::SafariWebKitBuildMismatch)
        );
        for malformed in [
            "",
            ".21624",
            "21624.",
            "21624..1",
            "021624.1",
            "21624.a",
            "21624-1",
            "1.2.3.4.5.6.7.8.9",
        ] {
            assert!(admit_runtime("26.5.2", "26.5.2", malformed, BUILD).is_err());
            assert!(admit_runtime("26.5.2", "26.5.2", BUILD, malformed).is_err());
        }
    }

    #[test]
    fn maintenance_deadline_is_an_exclusive_utc_boundary() {
        assert!(!security_floor_review_is_current(0));
        assert!(!security_floor_review_is_current(
            SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS - 1
        ));
        assert!(security_floor_review_is_current(
            SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS
        ));
        assert!(security_floor_review_is_current(
            SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS - 1
        ));
        assert!(!security_floor_review_is_current(
            SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS
        ));
        assert!(!security_floor_review_is_current(u64::MAX));
    }
}
