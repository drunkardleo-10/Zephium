//! Icons for the extensions the browser recommends. Each is fetched once from
//! the Chrome Web Store's image host and kept on disk, so the frame bundle
//! carries none and the privileged page never loads a remote image itself.

use std::path::Path;
use std::time::Duration;

use base64::Engine as _;

const HOST: &str = "https://lh3.googleusercontent.com/";
const LARGEST: usize = 64 * 1024;

/// The path part of a Web Store image address, which is all the frame may
/// name; the host is fixed here.
pub(super) fn valid_token(token: &str) -> bool {
    (16..=200).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(super) async fn icon(root: &Path, token: &str) -> Option<String> {
    if !valid_token(token) {
        return None;
    }
    let path = root.join("catalog-icons").join(format!("{token}.img"));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            let bytes = fetch(token).await?;
            let parent = path.parent()?;
            let temporary = path.with_extension("tmp");
            if std::fs::create_dir_all(parent).is_ok() && std::fs::write(&temporary, &bytes).is_ok()
            {
                let _ = std::fs::rename(&temporary, &path);
            }
            bytes
        }
    };
    let kind = image_type(&bytes)?;
    Some(format!(
        "data:{kind};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

async fn fetch(token: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;
    let response = client
        .get(format!("{HOST}{token}=s64"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() <= LARGEST && image_type(&bytes).is_some()).then(|| bytes.to_vec())
}

/// Only raster formats the frame can show; anything else is dropped.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_image_paths_on_the_store_host_are_fetched() {
        assert!(valid_token(
            "T66wTLk-gpBBGsMm0SDJJ3VaI8YM0Utr8NaGCSANmXOfb84K-9GmyXORLKoslfxtasKtQ4spDCdq_zlp_t3QQ6SI0A"
        ));
        assert!(!valid_token("../../etc/passwd"));
        assert!(!valid_token("evil.example/xxxxxxxxxxxxxxxx"));
        assert!(!valid_token("short"));
    }

    #[test]
    fn only_raster_images_are_kept() {
        assert_eq!(image_type(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(image_type(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
    }
}
