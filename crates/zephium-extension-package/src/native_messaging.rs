//! Bounded structural admission for Chromium native-messaging metadata.
//!
//! This module owns no filesystem lookup, executable launch, code-signature
//! decision, profile grant, or WebKit port. It admits only the untrusted host
//! manifest grammar and complete length-prefixed JSON frames. A platform
//! broker must independently bind the requested host name, fixed discovery
//! root, exact file identities, publisher signature, extension authority, and
//! process lifetime before using either value.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    parse_bounded_json, BoundedJsonLimits, BoundedJsonValue, ChromiumExtensionId,
    MAX_NATIVE_MESSAGING_ALLOWED_ORIGINS, MAX_NATIVE_MESSAGING_HOST_DESCRIPTION_BYTES,
    MAX_NATIVE_MESSAGING_HOST_MANIFEST_RETAINED_BYTES, MAX_NATIVE_MESSAGING_HOST_NAME_BYTES,
    MAX_NATIVE_MESSAGING_HOST_PATH_BYTES, MAX_NATIVE_MESSAGING_MESSAGE_BYTES,
};

const CHROMIUM_ORIGIN_PREFIX: &str = "chrome-extension://";
const CHROMIUM_ORIGIN_SUFFIX: &str = "/";
const NATIVE_MESSAGING_TYPE: &str = "stdio";

/// Canonical Chromium native-messaging host identifier.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeMessagingHostName(Box<str>);

impl NativeMessagingHostName {
    /// Parses Chrome's exact lowercase host-name grammar.
    pub fn parse(value: &str) -> Result<Self, NativeMessagingHostManifestError> {
        if value.is_empty()
            || value.len() > MAX_NATIVE_MESSAGING_HOST_NAME_BYTES
            || value.starts_with('.')
            || value.ends_with('.')
            || value.contains("..")
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'.'
            })
        {
            return Err(NativeMessagingHostManifestError::InvalidName);
        }
        Ok(Self(value.into()))
    }

    /// Returns the exact canonical host name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for NativeMessagingHostName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for NativeMessagingHostName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Debug for NativeMessagingHostName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("NativeMessagingHostName")
            .field(&self.as_str())
            .finish()
    }
}

/// Structurally admitted native-messaging host manifest.
///
/// The executable path is intentionally not exposed through `Debug`. It is
/// still untrusted structural input until a platform broker opens it through
/// a fixed discovery root and verifies its live publisher identity.
#[derive(Clone, Eq, PartialEq)]
pub struct NativeMessagingHostManifest {
    name: NativeMessagingHostName,
    description: Box<str>,
    path: Box<str>,
    allowed_origins: Box<[ChromiumExtensionId]>,
    retained_bytes: usize,
}

impl NativeMessagingHostManifest {
    /// Parses one duplicate-key-safe, hard-bounded Chrome host manifest.
    pub fn parse(bytes: &[u8]) -> Result<Self, NativeMessagingHostManifestError> {
        let bounded =
            parse_bounded_json(bytes, BoundedJsonLimits::native_messaging_host_manifest())
                .map_err(|_| NativeMessagingHostManifestError::InvalidJson)?;
        let raw: RawNativeMessagingHostManifest = serde_json::from_value(bounded.into_value())
            .map_err(|_| NativeMessagingHostManifestError::InvalidShape)?;
        let name = NativeMessagingHostName::parse(&raw.name)?;
        if raw.kind != NATIVE_MESSAGING_TYPE {
            return Err(NativeMessagingHostManifestError::UnsupportedType);
        }
        if !valid_text(
            &raw.description,
            MAX_NATIVE_MESSAGING_HOST_DESCRIPTION_BYTES,
            true,
        ) {
            return Err(NativeMessagingHostManifestError::InvalidDescription);
        }
        if !valid_text(&raw.path, MAX_NATIVE_MESSAGING_HOST_PATH_BYTES, false) {
            return Err(NativeMessagingHostManifestError::InvalidPath);
        }
        if raw.allowed_origins.is_empty()
            || raw.allowed_origins.len() > MAX_NATIVE_MESSAGING_ALLOWED_ORIGINS
        {
            return Err(NativeMessagingHostManifestError::InvalidAllowedOrigins);
        }
        let mut allowed_origins = Vec::new();
        allowed_origins
            .try_reserve_exact(raw.allowed_origins.len())
            .map_err(|_| NativeMessagingHostManifestError::RetainedBytesExceeded)?;
        for origin in raw.allowed_origins {
            let extension_id = parse_chromium_origin(&origin)?;
            allowed_origins.push(extension_id);
        }
        allowed_origins.sort_unstable();
        if allowed_origins.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(NativeMessagingHostManifestError::DuplicateAllowedOrigin);
        }
        let retained_bytes = size_of::<Self>()
            .checked_add(name.as_str().len())
            .and_then(|total| total.checked_add(raw.description.len()))
            .and_then(|total| total.checked_add(raw.path.len()))
            .and_then(|total| {
                allowed_origins.iter().try_fold(total, |total, origin| {
                    total
                        .checked_add(size_of::<ChromiumExtensionId>())
                        .and_then(|total| total.checked_add(origin.as_str().len()))
                })
            })
            .ok_or(NativeMessagingHostManifestError::RetainedBytesExceeded)?;
        if retained_bytes > MAX_NATIVE_MESSAGING_HOST_MANIFEST_RETAINED_BYTES {
            return Err(NativeMessagingHostManifestError::RetainedBytesExceeded);
        }
        Ok(Self {
            name,
            description: raw.description.into_boxed_str(),
            path: raw.path.into_boxed_str(),
            allowed_origins: allowed_origins.into_boxed_slice(),
            retained_bytes,
        })
    }

    /// Returns the exact requested host identifier.
    pub const fn name(&self) -> &NativeMessagingHostName {
        &self.name
    }

    /// Returns the inert human-readable host description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the untrusted executable path for platform verification.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the canonical allowed Chromium extension identifiers.
    pub fn allowed_origins(&self) -> &[ChromiumExtensionId] {
        &self.allowed_origins
    }

    /// Checks the host's exact authenticated upstream extension allowlist.
    pub fn allows_extension(&self, extension: &ChromiumExtensionId) -> bool {
        self.allowed_origins.binary_search(extension).is_ok()
    }

    /// Returns the explicit logical retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

impl fmt::Debug for NativeMessagingHostManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeMessagingHostManifest")
            .field("name", &self.name)
            .field("allowed_origin_count", &self.allowed_origins.len())
            .field("retained_bytes", &self.retained_bytes)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNativeMessagingHostManifest {
    name: String,
    description: String,
    path: String,
    #[serde(rename = "type")]
    kind: String,
    allowed_origins: Vec<String>,
}

/// Exact validated native-message frame payload length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMessagingFrameLength(u32);

impl NativeMessagingFrameLength {
    /// Decodes the native-endian four-byte prefix before any body allocation.
    pub fn decode(prefix: [u8; 4]) -> Result<Self, NativeMessagingMessageError> {
        let length = u32::from_ne_bytes(prefix);
        if length == 0 || length as usize > MAX_NATIVE_MESSAGING_MESSAGE_BYTES {
            return Err(NativeMessagingMessageError::InvalidLength);
        }
        Ok(Self(length))
    }

    /// Returns the exact payload byte length.
    pub const fn get(self) -> usize {
        self.0 as usize
    }

    const fn prefix(self) -> [u8; 4] {
        self.0.to_ne_bytes()
    }
}

/// Encodes one bounded JSON value as a complete Chrome native-message frame.
pub fn encode_native_messaging_frame(
    value: &Value,
) -> Result<Vec<u8>, NativeMessagingMessageError> {
    let payload =
        serde_json::to_vec(value).map_err(|_| NativeMessagingMessageError::InvalidJson)?;
    let length = frame_length(payload.len())?;
    let capacity = 4_usize
        .checked_add(payload.len())
        .ok_or(NativeMessagingMessageError::Allocation)?;
    let mut frame = Vec::new();
    frame
        .try_reserve_exact(capacity)
        .map_err(|_| NativeMessagingMessageError::Allocation)?;
    frame.extend_from_slice(&length.prefix());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decodes one complete frame and rejects short, long, duplicate-key, or
/// malformed JSON payloads before returning a value.
pub fn decode_native_messaging_frame(
    frame: &[u8],
) -> Result<BoundedJsonValue, NativeMessagingMessageError> {
    let prefix: [u8; 4] = frame
        .get(..4)
        .ok_or(NativeMessagingMessageError::Incomplete)?
        .try_into()
        .map_err(|_| NativeMessagingMessageError::Incomplete)?;
    let length = NativeMessagingFrameLength::decode(prefix)?;
    let expected = 4_usize
        .checked_add(length.get())
        .ok_or(NativeMessagingMessageError::InvalidLength)?;
    if frame.len() < expected {
        return Err(NativeMessagingMessageError::Incomplete);
    }
    if frame.len() != expected {
        return Err(NativeMessagingMessageError::TrailingBytes);
    }
    parse_bounded_json(&frame[4..], BoundedJsonLimits::native_messaging_message())
        .map_err(|_| NativeMessagingMessageError::InvalidJson)
}

fn frame_length(length: usize) -> Result<NativeMessagingFrameLength, NativeMessagingMessageError> {
    if length == 0 || length > MAX_NATIVE_MESSAGING_MESSAGE_BYTES {
        return Err(NativeMessagingMessageError::InvalidLength);
    }
    let length = u32::try_from(length).map_err(|_| NativeMessagingMessageError::InvalidLength)?;
    Ok(NativeMessagingFrameLength(length))
}

fn parse_chromium_origin(
    origin: &str,
) -> Result<ChromiumExtensionId, NativeMessagingHostManifestError> {
    let extension_id = origin
        .strip_prefix(CHROMIUM_ORIGIN_PREFIX)
        .and_then(|origin| origin.strip_suffix(CHROMIUM_ORIGIN_SUFFIX))
        .filter(|extension_id| !extension_id.contains('/'))
        .ok_or(NativeMessagingHostManifestError::InvalidAllowedOrigin)?;
    ChromiumExtensionId::parse(extension_id)
        .map_err(|_| NativeMessagingHostManifestError::InvalidAllowedOrigin)
}

fn valid_text(value: &str, max_bytes: usize, allow_unicode: bool) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && (allow_unicode || value.is_ascii())
        && !value.chars().any(char::is_control)
}

/// Native host manifest structural-admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMessagingHostManifestError {
    /// JSON exceeded a bound, contained duplicate keys, or was malformed.
    InvalidJson,
    /// Required fields were missing, duplicated, mistyped, or unknown.
    InvalidShape,
    /// The native host identifier violated Chromium's grammar.
    InvalidName,
    /// The description was empty, contained controls, or exceeded its bound.
    InvalidDescription,
    /// The executable path was empty, non-ASCII, contained controls, or exceeded its bound.
    InvalidPath,
    /// Only Chromium's `stdio` transport is admitted.
    UnsupportedType,
    /// The allowed-origin cohort was empty or exceeded its bound.
    InvalidAllowedOrigins,
    /// One allowed origin was not one exact Chromium extension origin.
    InvalidAllowedOrigin,
    /// Allowed origins must be unique.
    DuplicateAllowedOrigin,
    /// Retained-memory accounting overflowed or exceeded its ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for NativeMessagingHostManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidJson => "native messaging host manifest JSON is invalid",
            Self::InvalidShape => "native messaging host manifest shape is invalid",
            Self::InvalidName => "native messaging host name is invalid",
            Self::InvalidDescription => "native messaging host description is invalid",
            Self::InvalidPath => "native messaging host path is invalid",
            Self::UnsupportedType => "native messaging host transport is unsupported",
            Self::InvalidAllowedOrigins => "native messaging host origin cohort is invalid",
            Self::InvalidAllowedOrigin => "native messaging host origin is invalid",
            Self::DuplicateAllowedOrigin => "native messaging host origin is duplicated",
            Self::RetainedBytesExceeded => {
                "native messaging host retained-memory bound was exceeded"
            }
        })
    }
}

impl Error for NativeMessagingHostManifestError {}

/// Native-message frame admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMessagingMessageError {
    /// The native-endian length was zero or exceeded the hard ceiling.
    InvalidLength,
    /// The complete declared frame has not arrived.
    Incomplete,
    /// A complete frame contained bytes after its declared payload.
    TrailingBytes,
    /// The payload was malformed, duplicate-key, or exceeded JSON bounds.
    InvalidJson,
    /// Bounded output allocation failed.
    Allocation,
}

impl fmt::Display for NativeMessagingMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength => "native messaging frame length is invalid",
            Self::Incomplete => "native messaging frame is incomplete",
            Self::TrailingBytes => "native messaging frame has trailing bytes",
            Self::InvalidJson => "native messaging frame JSON is invalid",
            Self::Allocation => "native messaging frame allocation failed",
        })
    }
}

impl Error for NativeMessagingMessageError {}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_ID: &str = "aeblfdkhhhdcdjpifhhbdiojplfjncoa";
    const SECOND_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn manifest(name: &str, origins: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "name": name,
            "description": "Publisher browser support",
            "path": "/Applications/Publisher.app/Contents/MacOS/BrowserSupport",
            "type": "stdio",
            "allowed_origins": origins,
        }))
        .unwrap()
    }

    #[test]
    fn host_manifest_is_bounded_canonical_and_redacted() {
        let bytes = manifest(
            "com.1password.1password",
            &[
                &format!("chrome-extension://{SECOND_ID}/"),
                &format!("chrome-extension://{FIRST_ID}/"),
            ],
        );
        let admitted = NativeMessagingHostManifest::parse(&bytes).unwrap();
        assert_eq!(admitted.name().as_str(), "com.1password.1password");
        assert_eq!(admitted.allowed_origins()[0].as_str(), FIRST_ID);
        assert!(admitted.allows_extension(&ChromiumExtensionId::parse(FIRST_ID).unwrap()));
        assert!(admitted.retained_bytes() <= MAX_NATIVE_MESSAGING_HOST_MANIFEST_RETAINED_BYTES);
        let debug = format!("{admitted:?}");
        assert!(debug.contains("com.1password.1password"));
        assert!(!debug.contains("/Applications/"));
        assert!(!debug.contains("Publisher browser support"));
    }

    #[test]
    fn host_manifest_rejects_ambiguous_or_widened_authority() {
        for name in ["", ".host", "host.", "host..name", "Host", "host-name"] {
            assert_eq!(
                NativeMessagingHostManifest::parse(&manifest(
                    name,
                    &[&format!("chrome-extension://{FIRST_ID}/")],
                )),
                Err(NativeMessagingHostManifestError::InvalidName)
            );
        }
        for origin in [
            "*",
            "chrome-extension://*/",
            "chrome-extension://aeblfdkhhhdcdjpifhhbdiojplfjncoa",
            "https://aeblfdkhhhdcdjpifhhbdiojplfjncoa/",
        ] {
            assert_eq!(
                NativeMessagingHostManifest::parse(&manifest("com.example.host", &[origin],)),
                Err(NativeMessagingHostManifestError::InvalidAllowedOrigin)
            );
        }
        let duplicate = format!("chrome-extension://{FIRST_ID}/");
        assert_eq!(
            NativeMessagingHostManifest::parse(&manifest(
                "com.example.host",
                &[&duplicate, &duplicate],
            )),
            Err(NativeMessagingHostManifestError::DuplicateAllowedOrigin)
        );
        let duplicate_key = format!(
            r#"{{"name":"com.example.host","name":"com.example.peer","description":"x","path":"/host","type":"stdio","allowed_origins":["chrome-extension://{FIRST_ID}/"]}}"#
        );
        assert_eq!(
            NativeMessagingHostManifest::parse(duplicate_key.as_bytes()),
            Err(NativeMessagingHostManifestError::InvalidJson)
        );
    }

    #[test]
    fn native_message_frames_are_exact_bounded_and_duplicate_safe() {
        let value = serde_json::json!({"kind":"hello","sequence":1});
        let frame = encode_native_messaging_frame(&value).unwrap();
        assert_eq!(
            decode_native_messaging_frame(&frame).unwrap().into_value(),
            value
        );
        let mut short = frame.clone();
        short.pop();
        assert_eq!(
            decode_native_messaging_frame(&short),
            Err(NativeMessagingMessageError::Incomplete)
        );
        let mut long = frame.clone();
        long.push(0);
        assert_eq!(
            decode_native_messaging_frame(&long),
            Err(NativeMessagingMessageError::TrailingBytes)
        );
        let duplicate = br#"{"key":1,"key":2}"#;
        let mut duplicate_frame = Vec::from((duplicate.len() as u32).to_ne_bytes());
        duplicate_frame.extend_from_slice(duplicate);
        assert_eq!(
            decode_native_messaging_frame(&duplicate_frame),
            Err(NativeMessagingMessageError::InvalidJson)
        );
        assert_eq!(
            NativeMessagingFrameLength::decode(0_u32.to_ne_bytes()),
            Err(NativeMessagingMessageError::InvalidLength)
        );
        assert_eq!(
            NativeMessagingFrameLength::decode(
                u32::try_from(MAX_NATIVE_MESSAGING_MESSAGE_BYTES + 1)
                    .unwrap()
                    .to_ne_bytes(),
            ),
            Err(NativeMessagingMessageError::InvalidLength)
        );
    }
}
