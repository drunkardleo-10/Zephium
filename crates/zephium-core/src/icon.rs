//! Fixed-shape favicon values crossing from an untrusted page renderer into
//! privileged browser chrome.
//!
//! The site renderer owns all network fetching and image decoding. Rust and
//! the chrome renderer accept only a 32x32 RGBA raster, so malformed PNG,
//! ICO, SVG, font and animation parsers are never exposed in the privileged
//! process for a page-controlled favicon.

use base64::Engine as _;

pub const ICON_SIDE: usize = 32;
pub const RGBA32_BYTES: usize = ICON_SIDE * ICON_SIDE * 4;
pub const RGBA32_MIME: &str = "application/x-zephium-rgba32";
pub const RGBA32_PREFIX: &str = "rgba32:";
// Four-byte RGBA input always has this exact canonical padded length.
pub const RGBA32_BASE64_BYTES: usize = RGBA32_BYTES.div_ceil(3) * 4;

pub fn validated_rgba32(bytes: &[u8]) -> Option<&[u8]> {
    (bytes.len() == RGBA32_BYTES).then_some(bytes)
}

pub fn decode_rgba32(encoded: &str) -> Option<Vec<u8>> {
    if encoded.len() != RGBA32_BASE64_BYTES {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    validated_rgba32(&bytes)?;
    // Reject alternate/non-canonical encodings before this value becomes a
    // cache key or crosses another process boundary.
    (base64::engine::general_purpose::STANDARD.encode(&bytes) == encoded).then_some(bytes)
}

pub fn chrome_value(bytes: &[u8]) -> Option<String> {
    validated_rgba32(bytes)?;
    Some(format!(
        "{RGBA32_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_round_trip_is_exact_and_canonical() {
        let bytes: Vec<u8> = (0..RGBA32_BYTES).map(|n| (n % 251) as u8).collect();
        let value = chrome_value(&bytes).unwrap();
        let encoded = value.strip_prefix(RGBA32_PREFIX).unwrap();
        assert_eq!(encoded.len(), RGBA32_BASE64_BYTES);
        assert_eq!(decode_rgba32(encoded), Some(bytes));
    }

    #[test]
    fn rejects_encoded_containers_and_wrong_sized_rasters() {
        assert_eq!(validated_rgba32(b"<svg><script/></svg>"), None);
        assert_eq!(validated_rgba32(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(validated_rgba32(&vec![0; RGBA32_BYTES - 1]), None);
        assert_eq!(validated_rgba32(&vec![0; RGBA32_BYTES + 1]), None);
        assert_eq!(decode_rgba32("AAAA"), None);

        let bytes = vec![7; RGBA32_BYTES];
        let mut encoded = chrome_value(&bytes)
            .unwrap()
            .strip_prefix(RGBA32_PREFIX)
            .unwrap()
            .to_owned();
        encoded.pop();
        assert_eq!(decode_rgba32(&encoded), None);
    }
}
