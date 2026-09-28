//! Chrome Web Store URLs and update-protocol parsing. No networking.

use std::cmp::Ordering;

use url::{form_urlencoded, Url};

use crate::ExtensionId;

const UPDATE_ENDPOINT: &str = "https://clients2.google.com/service/update2/crx";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub id: ExtensionId,
    /// `ok`, `noupdate`, or an error status such as `error-unknownApplication`.
    pub status: String,
    pub version: Option<String>,
    pub codebase: Option<String>,
    /// Hex SHA-256 of the CRX the codebase serves, when the server sends it.
    pub hash_sha256: Option<String>,
}

/// Extracts the extension ID from a Chrome Web Store listing URL.
pub fn listing_id(url: &str) -> Option<ExtensionId> {
    let url = Url::parse(url).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let mut segments = url.path_segments()?;
    match url.host_str()? {
        "chromewebstore.google.com" => {}
        "chrome.google.com" if segments.next()? == "webstore" => {}
        _ => return None,
    }
    if segments.next()? != "detail" {
        return None;
    }
    segments.take(2).find_map(ExtensionId::parse)
}

pub fn download_url(id: &ExtensionId, prodversion: &str) -> String {
    format!(
        "{UPDATE_ENDPOINT}?response=redirect&prodversion={}&acceptformat=crx3&x={}",
        encode(prodversion),
        encode(&format!("id={id}&installsource=ondemand&uc")),
    )
}

pub fn update_check_url<V: AsRef<str>>(entries: &[(ExtensionId, V)], prodversion: &str) -> String {
    let mut url = format!(
        "{UPDATE_ENDPOINT}?response=updatecheck&prodversion={}&acceptformat=crx3",
        encode(prodversion)
    );
    for (id, version) in entries {
        url.push_str("&x=");
        url.push_str(&encode(&format!("id={id}&v={}&uc", version.as_ref())));
    }
    url
}

fn encode(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Reads `<app>` results from an update-check response. Only attributes of
/// `app` and nested `updatecheck` elements are consulted.
pub fn parse_update_response(xml: &str) -> Vec<UpdateInfo> {
    struct App<'a> {
        attrs: &'a str,
        check: Option<&'a str>,
    }

    fn finish(app: App<'_>) -> Option<UpdateInfo> {
        let id = ExtensionId::parse(&attribute(app.attrs, "appid")?)?;
        let check = app.check.unwrap_or("");
        let status = attribute(check, "status")
            .or_else(|| attribute(app.attrs, "status"))
            .unwrap_or_default();
        Some(UpdateInfo {
            id,
            status,
            version: attribute(check, "version"),
            codebase: attribute(check, "codebase"),
            hash_sha256: attribute(check, "hash_sha256"),
        })
    }

    let mut updates = Vec::new();
    let mut current: Option<App<'_>> = None;
    for tag in Tags::new(xml) {
        match (tag.name, tag.closing) {
            ("app", false) => {
                updates.extend(current.take().and_then(finish));
                let app = App {
                    attrs: tag.attrs,
                    check: None,
                };
                if tag.self_closing {
                    updates.extend(finish(app));
                } else {
                    current = Some(app);
                }
            }
            ("app", true) => updates.extend(current.take().and_then(finish)),
            ("updatecheck", false) => {
                if let Some(app) = current.as_mut() {
                    app.check.get_or_insert(tag.attrs);
                }
            }
            _ => {}
        }
    }
    updates.extend(current.and_then(finish));
    updates
}

struct Tag<'a> {
    name: &'a str,
    attrs: &'a str,
    closing: bool,
    self_closing: bool,
}

struct Tags<'a> {
    xml: &'a str,
    pos: usize,
}

impl<'a> Tags<'a> {
    fn new(xml: &'a str) -> Self {
        Self { xml, pos: 0 }
    }
}

impl<'a> Iterator for Tags<'a> {
    type Item = Tag<'a>;

    fn next(&mut self) -> Option<Tag<'a>> {
        loop {
            let rest = &self.xml[self.pos..];
            let start = self.pos + rest.find('<')?;
            let rest = &self.xml[start..];
            for (open, close) in [("<!--", "-->"), ("<?", "?>"), ("<!", ">")] {
                if rest.starts_with(open) {
                    self.pos = start + rest.find(close).map_or(rest.len(), |n| n + close.len());
                    break;
                }
            }
            if self.pos > start {
                continue;
            }
            let end = start + tag_end(rest)?;
            self.pos = end + 1;
            let body = &self.xml[start + 1..end];
            let (closing, body) = match body.strip_prefix('/') {
                Some(body) => (true, body),
                None => (false, body),
            };
            let (body, self_closing) = match body.trim_end().strip_suffix('/') {
                Some(body) => (body, true),
                None => (body, false),
            };
            let name_len = body
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(body.len());
            return Some(Tag {
                name: &body[..name_len],
                attrs: &body[name_len..],
                closing,
                self_closing,
            });
        }
    }
}

/// Offset of the `>` closing the tag that opens `text`, skipping quoted
/// attribute values.
pub(crate) fn tag_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut quote = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn attribute(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let name_len = rest
            .find(|c: char| c == '=' || c.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let name = &rest[..name_len];
        rest = rest[name_len..].trim_start();
        let Some(after_eq) = rest.strip_prefix('=') else {
            continue;
        };
        rest = after_eq.trim_start();
        let value = match rest.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let end = rest[1..].find(quote)? + 1;
                let value = &rest[1..end];
                rest = &rest[end + 1..];
                value
            }
            _ => {
                let end = rest
                    .find(|c: char| c.is_ascii_whitespace())
                    .unwrap_or(rest.len());
                let value = &rest[..end];
                rest = &rest[end..];
                value
            }
        };
        if name == key {
            return Some(unescape(value));
        }
    }
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').and_then(|semi| {
            let entity = &rest[1..semi];
            let c = match entity {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let code = match entity.strip_prefix("#x").or(entity.strip_prefix("#X")) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => entity.strip_prefix('#')?.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, semi + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether `bytes` are the package an update response described by its
/// `hash_sha256`.
pub fn matches_sha256(bytes: &[u8], hex: &str) -> bool {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let expected = hex.trim().to_ascii_lowercase();
    expected.len() == 64
        && digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            == expected
}

/// Orders Chrome's dotted versions numerically; missing parts count as zero.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn parts(version: &str) -> [u64; 4] {
        let mut out = [0; 4];
        for (slot, part) in out.iter_mut().zip(version.trim().split('.')) {
            *slot = part.parse().unwrap_or(0);
        }
        out
    }
    parts(a).cmp(&parts(b))
}

#[cfg(test)]
mod tests {
    #[test]
    fn package_hashes_are_compared_case_insensitively() {
        let hash = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(matches_sha256(b"hello", hash));
        assert!(matches_sha256(b"hello", &hash.to_uppercase()));
        assert!(!matches_sha256(b"hello!", hash));
        assert!(!matches_sha256(b"hello", ""));
    }

    use super::*;

    const BITWARDEN: &str = "nngceckbapebfimnlniiiahkandclblb";

    fn id(s: &str) -> ExtensionId {
        ExtensionId::parse(s).unwrap()
    }

    #[test]
    fn recognizes_listing_urls() {
        for url in [
            "https://chromewebstore.google.com/detail/bitwarden-password-manager/nngceckbapebfimnlniiiahkandclblb",
            "https://chromewebstore.google.com/detail/nngceckbapebfimnlniiiahkandclblb?hl=en",
            "https://chromewebstore.google.com/detail/bitwarden/nngceckbapebfimnlniiiahkandclblb/reviews#x",
            "https://chrome.google.com/webstore/detail/bitwarden/nngceckbapebfimnlniiiahkandclblb",
            "https://chrome.google.com/webstore/detail/nngceckbapebfimnlniiiahkandclblb/",
        ] {
            assert_eq!(listing_id(url), Some(id(BITWARDEN)), "{url}");
        }
        for url in [
            "http://chromewebstore.google.com/detail/x/nngceckbapebfimnlniiiahkandclblb",
            "https://evil.test/detail/x/nngceckbapebfimnlniiiahkandclblb",
            "https://chrome.google.com/detail/nngceckbapebfimnlniiiahkandclblb",
            "https://chromewebstore.google.com/category/nngceckbapebfimnlniiiahkandclblb",
            "https://chromewebstore.google.com/detail/a/b/nngceckbapebfimnlniiiahkandclblb",
        ] {
            assert_eq!(listing_id(url), None, "{url}");
        }
    }

    #[test]
    fn builds_update_urls() {
        assert_eq!(
            download_url(&id(BITWARDEN), "130.0.0.0"),
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=130.0.0.0\
             &acceptformat=crx3&x=id%3Dnngceckbapebfimnlniiiahkandclblb%26installsource%3Dondemand%26uc"
        );
        let other = "abcdefghijklmnopabcdefghijklmnop";
        assert_eq!(
            update_check_url(&[(id(BITWARDEN), "2024.1.0"), (id(other), "1.0")], "130.0"),
            "https://clients2.google.com/service/update2/crx?response=updatecheck&prodversion=130.0\
             &acceptformat=crx3&x=id%3Dnngceckbapebfimnlniiiahkandclblb%26v%3D2024.1.0%26uc\
             &x=id%3Dabcdefghijklmnopabcdefghijklmnop%26v%3D1.0%26uc"
        );
    }

    #[test]
    fn parses_update_responses() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="prod">
  <daystart elapsed_days="6843"/>
  <!-- <app appid="abcdefghijklmnopabcdefghijklmnop"><updatecheck status="ok"/></app> -->
  <app appid="nngceckbapebfimnlniiiahkandclblb" cohort="1::" status="ok">
    <updatecheck codebase="https://clients2.googleusercontent.com/crx/blobs/a?x=1&amp;y=2"
      hash_sha256="ab12" protocol="2.0" size="100" status="ok" version="2024.9.1"/>
  </app>
  <app appid='abcdefghijklmnopabcdefghijklmnop' status="ok"><updatecheck status='noupdate'/></app>
  <app appid="not-an-id" status="ok"><updatecheck status="ok" version="1"/></app>
  <app appid="ponmlkjihgfedcbaponmlkjihgfedcba" status="error-unknownApplication"/>
</gupdate>"#;
        let updates = parse_update_response(xml);
        assert_eq!(updates.len(), 3);
        assert_eq!(
            updates[0],
            UpdateInfo {
                id: id(BITWARDEN),
                status: "ok".into(),
                version: Some("2024.9.1".into()),
                codebase: Some("https://clients2.googleusercontent.com/crx/blobs/a?x=1&y=2".into()),
                hash_sha256: Some("ab12".into()),
            }
        );
        assert_eq!(updates[1].status, "noupdate");
        assert_eq!(updates[1].version, None);
        assert_eq!(updates[2].status, "error-unknownApplication");
    }

    #[test]
    fn compares_dotted_versions() {
        assert_eq!(compare_versions("1.0", "1.0.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.10", "1.9"), Ordering::Greater);
        assert_eq!(compare_versions("2024.1.0", "2024.10"), Ordering::Less);
        assert_eq!(compare_versions("1.2.3.4", "1.2.3.5"), Ordering::Less);
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(unescape("a&amp;b&#x41;&#66;&bogus;&"), "a&bAB&bogus;&");
    }
}
