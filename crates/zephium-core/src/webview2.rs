//! Platform-independent WebView2 runtime admission policy.
//!
//! This is deliberately a reviewed security floor, not an API-compatibility
//! floor. Microsoft says the Evergreen WebView2 Runtime receives the same
//! Stable security updates listed in its Edge security release notes. The
//! policy therefore expires quickly so a once-current floor cannot silently
//! become a permanent security claim.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[cfg(target_os = "windows")]
#[path = "webview2_runtime.rs"]
mod runtime;
#[cfg(target_os = "windows")]
pub use runtime::{RuntimeCleanupTicket, RuntimeGeneration, RuntimeGenerationKind};

/// Windows Stable security release published by Microsoft on 2026-07-09.
pub const SECURITY_FLOOR: WebView2Version = WebView2Version::stable(150, 0, 4078, 65);
pub const SECURITY_FLOOR_TEXT: &str = "150.0.4078.65";
pub const SECURITY_FLOOR_PUBLISHED_ON: &str = "2026-07-09";
/// 2026-07-09T00:00:00Z. A wall clock before the reviewed release cannot
/// establish that the floor is current and must fail closed just like an
/// expired review.
pub const SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS: u64 = 1_783_555_200;
pub const SECURITY_FLOOR_SOURCE_URL: &str =
    "https://learn.microsoft.com/en-us/deployedge/microsoft-edge-relnotes-security";

/// The last UTC date on which CI may accept this review without an update.
pub const SECURITY_FLOOR_REVIEW_BY: &str = "2026-07-23";
/// 2026-07-24T00:00:00Z. The human-readable review date above is inclusive.
pub const SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS: u64 = 1_784_851_200;

/// Loader/debugger environment variables that can replace the selected
/// runtime or UDF, change channel selection, append browser flags (including
/// `--no-sandbox`), or expose targets to a debugger. Presence is rejected even
/// for an empty value so loader-version differences cannot weaken admission.
pub const SECURITY_RELEVANT_ENVIRONMENT_OVERRIDES: [&str; 8] = [
    "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
    "WEBVIEW2_USER_DATA_FOLDER",
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "WEBVIEW2_CHANNEL_SEARCH_KIND",
    "WEBVIEW2_RELEASE_CHANNELS",
    "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
    "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
    "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
];

/// Returns the first security-relevant WebView2 environment override present
/// according to `is_present`. Keeping policy enumeration pure makes the
/// startup boundary testable without mutating the process environment.
pub fn first_present_environment_override(
    mut is_present: impl FnMut(&'static str) -> bool,
) -> Option<&'static str> {
    SECURITY_RELEVANT_ENVIRONMENT_OVERRIDES
        .into_iter()
        .find(|name| is_present(name))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel {
    Beta,
    Dev,
    Canary,
}

impl Channel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "beta" => Some(Self::Beta),
            "dev" => Some(Self::Dev),
            "canary" => Some(Self::Canary),
            _ => None,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Beta => "beta",
            Self::Dev => "dev",
            Self::Canary => "canary",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebView2Version {
    components: [u32; 4],
    channel: Option<Channel>,
}

impl WebView2Version {
    pub const fn stable(major: u32, minor: u32, build: u32, patch: u32) -> Self {
        Self {
            components: [major, minor, build, patch],
            channel: None,
        }
    }

    pub const fn components(self) -> [u32; 4] {
        self.components
    }

    pub const fn channel(self) -> Option<Channel> {
        self.channel
    }

    pub fn is_at_least(self, required: Self) -> bool {
        self.components >= required.components
    }
}

impl fmt::Display for WebView2Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}.{}",
            self.components[0], self.components[1], self.components[2], self.components[3]
        )?;
        if let Some(channel) = self.channel {
            write!(formatter, " {channel}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionParseError {
    Empty,
    TooLong,
    InvalidWhitespace,
    InvalidChannel,
    WrongComponentCount,
    InvalidComponent,
    ComponentOverflow,
}

impl fmt::Display for VersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "version is empty",
            Self::TooLong => "version is longer than 64 bytes",
            Self::InvalidWhitespace => "version contains unsupported whitespace",
            Self::InvalidChannel => "version has an unknown browser channel suffix",
            Self::WrongComponentCount => "version must contain exactly four numeric components",
            Self::InvalidComponent => "version component is not an unsigned ASCII integer",
            Self::ComponentOverflow => "version component exceeds u32",
        })
    }
}

impl std::error::Error for VersionParseError {}

impl FromStr for WebView2Version {
    type Err = VersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(VersionParseError::Empty);
        }
        if value.len() > 64 {
            return Err(VersionParseError::TooLong);
        }
        if value.trim() != value
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() && byte != b' ')
        {
            return Err(VersionParseError::InvalidWhitespace);
        }

        let mut fields = value.split(' ');
        let numeric = fields.next().ok_or(VersionParseError::Empty)?;
        let channel = match fields.next() {
            None => None,
            Some("") => return Err(VersionParseError::InvalidWhitespace),
            Some(channel) => {
                Some(Channel::parse(channel).ok_or(VersionParseError::InvalidChannel)?)
            }
        };
        if fields.next().is_some() {
            return Err(VersionParseError::InvalidWhitespace);
        }

        let mut components = [0_u32; 4];
        let mut parsed = numeric.split('.');
        for component in &mut components {
            let raw = parsed
                .next()
                .ok_or(VersionParseError::WrongComponentCount)?;
            if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(VersionParseError::InvalidComponent);
            }
            *component = raw
                .parse()
                .map_err(|_| VersionParseError::ComponentOverflow)?;
        }
        if parsed.next().is_some() {
            return Err(VersionParseError::WrongComponentCount);
        }

        Ok(Self {
            components,
            channel,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    InvalidVersion(VersionParseError),
    PreviewChannel(Channel),
    BelowSecurityFloor {
        found: WebView2Version,
        required: WebView2Version,
    },
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVersion(error) => write!(formatter, "invalid reported version: {error}"),
            Self::PreviewChannel(channel) => write!(
                formatter,
                "the {channel} browser channel is not an Evergreen WebView2 Runtime"
            ),
            Self::BelowSecurityFloor { found, required } => write!(
                formatter,
                "reported runtime {found} is below security floor {required}"
            ),
        }
    }
}

impl std::error::Error for AdmissionError {}

/// Admit only a stable runtime at or above the currently reviewed floor.
pub fn admit_runtime(reported: &str) -> Result<WebView2Version, AdmissionError> {
    let version = reported
        .parse::<WebView2Version>()
        .map_err(AdmissionError::InvalidVersion)?;
    if let Some(channel) = version.channel() {
        return Err(AdmissionError::PreviewChannel(channel));
    }
    if !version.is_at_least(SECURITY_FLOOR) {
        return Err(AdmissionError::BelowSecurityFloor {
            found: version,
            required: SECURITY_FLOOR,
        });
    }
    Ok(version)
}

/// Resolve a WebView2 user-data directory to the filesystem object used for
/// security comparisons. A symlink is never an acceptable boundary, even if
/// it currently resolves inside the expected root: another process could
/// retarget it after admission.
pub fn canonical_user_data_directory(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebView2 user-data path must be absolute",
        ));
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebView2 user-data path contains relative traversal components",
        ));
    }
    // Checking only the leaf misses `<junction>/ordinary-child`. Walk every
    // existing component through the drive/UNC root so a retargetable ancestor
    // cannot canonicalize to the expected directory and pass equality.
    for ancestor in path.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        #[cfg(target_os = "windows")]
        let is_reparse_point = {
            use std::os::windows::fs::MetadataExt;
            // FILE_ATTRIBUTE_REPARSE_POINT. FileType::is_symlink does not
            // cover junctions, mount points, and other redirectors.
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(target_os = "windows"))]
        let is_reparse_point = false;
        if metadata.file_type().is_symlink() || is_reparse_point || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "WebView2 user-data path has a redirecting/non-directory component: {}",
                    ancestor.display()
                ),
            ));
        }
    }
    path.canonicalize()
}

/// Compare the actual directory reported by `ICoreWebView2Environment7`
/// against the directory Zephium assigned to the environment. Both operands
/// are resolved independently so spelling, relative components, and Windows
/// long-path prefixes cannot make different filesystem objects appear equal.
pub fn user_data_directory_matches(expected: &Path, actual: &Path) -> io::Result<bool> {
    Ok(canonical_user_data_directory(expected)? == canonical_user_data_directory(actual)?)
}

pub const fn security_floor_review_is_current(unix_seconds: u64) -> bool {
    unix_seconds >= SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS
        && unix_seconds < SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_documented_channel_and_stable_runtime() {
        let stable: WebView2Version = "150.0.4078.65".parse().unwrap();
        assert_eq!(stable.components(), [150, 0, 4078, 65]);
        assert_eq!(stable.channel(), None);
        assert_eq!(stable.to_string(), "150.0.4078.65");

        for (suffix, expected) in [
            ("beta", Channel::Beta),
            ("dev", Channel::Dev),
            ("canary", Channel::Canary),
        ] {
            let version: WebView2Version = format!("150.0.4078.65 {suffix}").parse().unwrap();
            assert_eq!(version.channel(), Some(expected));
            assert_eq!(version.to_string(), format!("150.0.4078.65 {suffix}"));
        }
    }

    #[test]
    fn parser_rejects_malformed_and_ambiguous_versions() {
        let cases = [
            "",
            "150",
            "150.0.4078",
            "150.0.4078.65.1",
            ".0.4078.65",
            "150..4078.65",
            "150.0.4078.",
            "+150.0.4078.65",
            "-150.0.4078.65",
            "１５０.0.4078.65",
            "150.0.4078.65 ",
            " 150.0.4078.65",
            "150.0.4078.65  beta",
            "150.0.4078.65\tbeta",
            "150.0.4078.65 stable",
            "150.0.4078.65 Beta",
            "150.0.4078.65 beta extra",
            "4294967296.0.0.0",
        ];
        for value in cases {
            assert!(
                value.parse::<WebView2Version>().is_err(),
                "unexpectedly accepted {value:?}"
            );
        }
        assert_eq!(
            "1".repeat(65).parse::<WebView2Version>(),
            Err(VersionParseError::TooLong)
        );
    }

    #[test]
    fn numeric_order_compares_all_four_components() {
        let required = WebView2Version::stable(150, 0, 4078, 65);
        for older in [
            WebView2Version::stable(149, u32::MAX, u32::MAX, u32::MAX),
            WebView2Version::stable(150, 0, 4077, u32::MAX),
            WebView2Version::stable(150, 0, 4078, 64),
        ] {
            assert!(!older.is_at_least(required), "{older} must be older");
        }
        for accepted in [
            required,
            WebView2Version::stable(150, 0, 4078, 66),
            WebView2Version::stable(150, 0, 4079, 0),
            WebView2Version::stable(151, 0, 0, 0),
        ] {
            assert!(
                accepted.is_at_least(required),
                "{accepted} must satisfy floor"
            );
        }
    }

    #[test]
    fn admission_rejects_old_invalid_and_preview_runtimes() {
        assert_eq!(admit_runtime(SECURITY_FLOOR_TEXT), Ok(SECURITY_FLOOR));
        assert!(admit_runtime("150.0.4078.66").is_ok());
        assert!(matches!(
            admit_runtime("150.0.4078.64"),
            Err(AdmissionError::BelowSecurityFloor { .. })
        ));
        assert_eq!(
            admit_runtime("150.0.4078.65 beta"),
            Err(AdmissionError::PreviewChannel(Channel::Beta))
        );
        assert!(matches!(
            admit_runtime("not-a-version"),
            Err(AdmissionError::InvalidVersion(_))
        ));
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

    #[test]
    fn every_security_relevant_environment_override_is_rejected_by_presence() {
        for expected in SECURITY_RELEVANT_ENVIRONMENT_OVERRIDES {
            assert_eq!(
                first_present_environment_override(|name| name == expected),
                Some(expected)
            );
        }
        assert_eq!(first_present_environment_override(|_| false), None);
    }

    #[test]
    fn user_data_directory_match_uses_canonical_directory_identity() {
        let temp = tempfile::tempdir().unwrap();
        let temp_root = temp.path().canonicalize().unwrap();
        let expected = temp_root.join("expected");
        let other = temp_root.join("other");
        std::fs::create_dir(&expected).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::create_dir(expected.join("child")).unwrap();

        assert!(user_data_directory_matches(&expected, &expected).unwrap());
        // `Path::components` intentionally normalizes a harmless trailing `.`.
        assert!(user_data_directory_matches(&expected, &expected.join(".")).unwrap());
        assert!(!user_data_directory_matches(&expected, &other).unwrap());
        assert_eq!(
            user_data_directory_matches(&expected, &expected.join("child").join(".."))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[cfg(unix)]
    #[test]
    fn user_data_directory_match_rejects_symlink_aliases() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let temp_root = temp.path().canonicalize().unwrap();
        let expected = temp_root.join("expected");
        let alias = temp_root.join("alias");
        std::fs::create_dir(&expected).unwrap();
        symlink(&expected, &alias).unwrap();

        assert_eq!(
            user_data_directory_matches(&expected, &alias)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn user_data_directory_match_rejects_redirecting_ancestor() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let temp_root = temp.path().canonicalize().unwrap();
        let expected_parent = temp_root.join("expected-parent");
        let expected = expected_parent.join("profile");
        std::fs::create_dir_all(&expected).unwrap();
        let alias_parent = temp_root.join("alias-parent");
        symlink(&expected_parent, &alias_parent).unwrap();
        let alias = alias_parent.join("profile");

        assert_eq!(
            user_data_directory_matches(&expected, &alias)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
