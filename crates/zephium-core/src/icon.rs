//! Fixed-shape favicon values crossing from an untrusted page renderer into
//! privileged browser chrome.
//!
//! The site renderer owns all network fetching and image decoding. Rust and
//! the chrome renderer accept only a 32x32 RGBA raster, so malformed PNG,
//! ICO, SVG, font and animation parsers are never exposed in the privileged
//! process for a page-controlled favicon. The one exception is the Work
//! origin probe, which fetches an icon anonymously (no page, no session) and
//! decodes it with a bounded, memory-safe decoder into this same raster.

use base64::Engine as _;

pub const ICON_SIDE: usize = 32;
pub const RGBA32_BYTES: usize = ICON_SIDE * ICON_SIDE * 4;
pub const RGBA32_MIME: &str = "application/x-zephium-rgba32";
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

pub fn encode_rgba32(bytes: &[u8]) -> Option<String> {
    validated_rgba32(bytes)?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Identifies one raster's contents so chrome can cache by origin and repaint
/// only when the pixels actually change. A cache tag, not a security boundary.
pub fn revision(bytes: &[u8]) -> Option<String> {
    validated_rgba32(bytes)?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("{hash:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_round_trip_is_exact_and_canonical() {
        let bytes: Vec<u8> = (0..RGBA32_BYTES).map(|n| (n % 251) as u8).collect();
        let encoded = encode_rgba32(&bytes).unwrap();
        assert_eq!(encoded.len(), RGBA32_BASE64_BYTES);
        assert_eq!(decode_rgba32(&encoded), Some(bytes));
    }

    #[test]
    fn revision_tracks_pixels_and_rejects_malformed_rasters() {
        let bytes = vec![7; RGBA32_BYTES];
        let mut changed = bytes.clone();
        changed[RGBA32_BYTES - 1] = 8;
        assert_eq!(revision(&bytes), revision(&vec![7; RGBA32_BYTES]));
        assert_ne!(revision(&bytes), revision(&changed));
        assert_eq!(revision(&vec![7; RGBA32_BYTES - 1]), None);
    }

    #[test]
    fn rejects_encoded_containers_and_wrong_sized_rasters() {
        assert_eq!(validated_rgba32(b"<svg><script/></svg>"), None);
        assert_eq!(validated_rgba32(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(validated_rgba32(&vec![0; RGBA32_BYTES - 1]), None);
        assert_eq!(validated_rgba32(&vec![0; RGBA32_BYTES + 1]), None);
        assert_eq!(decode_rgba32("AAAA"), None);

        let bytes = vec![7; RGBA32_BYTES];
        let mut encoded = encode_rgba32(&bytes).unwrap();
        encoded.pop();
        assert_eq!(decode_rgba32(&encoded), None);
    }
}
