//! Platform-independent WebKitGTK runtime admission and review policy.
//!
//! The installed Linux library is part of the browser's security boundary.
//! Keep the enforced minimum tied to a published security advisory, admit
//! only the stable release line reviewed by maintainers, and separately
//! record the newest stable release included in that review.

use std::fmt;

pub const SECURITY_FLOOR: [u32; 3] = [2, 52, 4];
pub const SECURITY_FLOOR_TEXT: &str = "2.52.4";
pub const SECURITY_FLOOR_PUBLISHED_ON: &str = "2026-06-02";
pub const SECURITY_FLOOR_SOURCE_URL: &str = "https://webkitgtk.org/security/WSA-2026-0003.html";

pub const REVIEWED_STABLE_RELEASE_LINE: [u32; 2] = [2, 52];
pub const REVIEWED_STABLE_RELEASE_LINE_TEXT: &str = "2.52";

pub const LATEST_REVIEWED: [u32; 3] = [2, 52, 5];
pub const LATEST_REVIEWED_TEXT: &str = "2.52.5";
pub const LATEST_REVIEWED_PUBLISHED_ON: &str = "2026-07-09";
pub const LATEST_REVIEWED_SOURCE_URL: &str =
    "https://webkitgtk.org/2026/07/09/webkitgtk2.52.5-released.html";

/// The last UTC date on which CI may accept this review without an update.
pub const SECURITY_FLOOR_REVIEW_BY: &str = "2026-07-27";
/// 2026-07-28T00:00:00Z. The human-readable review date above is inclusive.
pub const SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS: u64 = 1_785_196_800;

/// Environment switches that can disable/replace renderer confinement,
/// expose a remote inspector, pause a child for a debugger, or turn off
/// JavaScriptCore allocator/JIT mitigations. The current stable WebKitGTK no
/// longer lets `WEBKIT_FORCE_SANDBOX=0` disable the sandbox, but rejecting the
/// legacy switch keeps admission independent of version-specific parsing and
/// prevents it becoming dangerous again after a runtime change.
pub const SECURITY_RELEVANT_ENVIRONMENT_OVERRIDES: [&str; 10] = [
    "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
    "WEBKIT_FORCE_SANDBOX",
    "WEBKIT_INSPECTOR_SERVER",
    "WEBKIT_INSPECTOR_HTTP_SERVER",
    "WEBKIT2_PAUSE_WEB_PROCESS_ON_LAUNCH",
    "WEBKIT_SAMPLE_MEMORY",
    "WEBKIT_DISABLE_MEMORY_PRESSURE_MONITOR",
    "GIGACAGE_ENABLED",
    "JavaScriptCoreUseJIT",
    "Malloc",
];

pub fn environment_override_is_security_relevant(name: &str) -> bool {
    name.starts_with("JSC_") || SECURITY_RELEVANT_ENVIRONMENT_OVERRIDES.contains(&name)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    BelowSecurityFloor { found: [u32; 3], required: [u32; 3] },
    UnreviewedReleaseLine { found: [u32; 3], reviewed: [u32; 2] },
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BelowSecurityFloor { found, required } => write!(
                formatter,
                "WebKitGTK {}.{}.{} is below security floor {}.{}.{}",
                found[0], found[1], found[2], required[0], required[1], required[2],
            ),
            Self::UnreviewedReleaseLine { found, reviewed } => write!(
                formatter,
                "WebKitGTK {}.{}.{} is outside the reviewed stable {}.{} release line",
                found[0], found[1], found[2], reviewed[0], reviewed[1],
            ),
        }
    }
}

impl std::error::Error for AdmissionError {}

pub fn admit_runtime(major: u32, minor: u32, micro: u32) -> Result<(), AdmissionError> {
    let found = [major, minor, micro];
    if found < SECURITY_FLOOR {
        return Err(AdmissionError::BelowSecurityFloor {
            found,
            required: SECURITY_FLOOR,
        });
    }
    if [major, minor] != REVIEWED_STABLE_RELEASE_LINE {
        return Err(AdmissionError::UnreviewedReleaseLine {
            found,
            reviewed: REVIEWED_STABLE_RELEASE_LINE,
        });
    }
    Ok(())
}

pub const fn security_floor_review_is_current(unix_seconds: u64) -> bool {
    unix_seconds < SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_is_limited_to_the_reviewed_stable_release_line() {
        assert_eq!(
            admit_runtime(2, 52, 3),
            Err(AdmissionError::BelowSecurityFloor {
                found: [2, 52, 3],
                required: SECURITY_FLOOR,
            })
        );
        assert_eq!(admit_runtime(2, 52, 4), Ok(()));
        assert_eq!(admit_runtime(2, 52, 5), Ok(()));
        assert_eq!(admit_runtime(2, 52, u32::MAX), Ok(()));

        for found in [[2, 53, 0], [2, 54, 0], [3, 0, 0]] {
            assert_eq!(
                admit_runtime(found[0], found[1], found[2]),
                Err(AdmissionError::UnreviewedReleaseLine {
                    found,
                    reviewed: REVIEWED_STABLE_RELEASE_LINE,
                })
            );
        }
    }

    #[test]
    fn maintenance_deadline_is_an_exclusive_utc_boundary() {
        assert!(security_floor_review_is_current(
            SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS - 1
        ));
        assert!(!security_floor_review_is_current(
            SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS
        ));
        assert!(!security_floor_review_is_current(u64::MAX));
    }

    #[test]
    fn security_relevant_environment_overrides_are_fail_closed() {
        for name in SECURITY_RELEVANT_ENVIRONMENT_OVERRIDES {
            assert!(environment_override_is_security_relevant(name), "{name}");
        }
        assert!(environment_override_is_security_relevant("JSC_useJITCage"));
        assert!(environment_override_is_security_relevant("JSC_dumpOptions"));
        assert!(!environment_override_is_security_relevant("GTK_THEME"));
        assert!(!environment_override_is_security_relevant("WEBKIT_DEBUG"));
    }
}
