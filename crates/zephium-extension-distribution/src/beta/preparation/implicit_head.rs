//! Small, versioned normalization for HTML pages with an omitted head tag.
//! Existing explicit heads retain byte-for-byte compiler inputs. This never
//! scans script bodies or rewrites publisher code, URLs, attributes or CSP.
use zephium_extension_package::macos_compatibility as html;

pub(super) const DESCRIPTOR: &[u8] = b"zephium:local.implicit-head.v1\0utf8;bom-comments-doctype-html-prefix;insert-head-before-meta-title-link-base-style-script-body-div-p;explicit-head-input-unchanged;other-bytes-exact\0";

pub(super) fn normalize(bytes: Vec<u8>) -> Result<(Vec<u8>, bool), String> {
    let source = std::str::from_utf8(&bytes).map_err(|_| "extension HTML must be UTF-8")?;
    if html::explicit_head_end(source).is_ok() {
        return Ok((bytes, false));
    }
    let mut cursor = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        3
    } else {
        0
    };
    loop {
        cursor = skip_comments(source, cursor)?;
        if html::starts_ascii_case_insensitive(&bytes, cursor, b"<!doctype")
            && bytes
                .get(cursor + 9)
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
        {
            cursor = html::tag_end(&bytes, cursor)?;
        } else {
            break;
        }
    }
    if html::starts_start_tag(&bytes, cursor, b"html") {
        cursor = html::tag_end(&bytes, cursor)?;
    }
    cursor = skip_comments(source, cursor)?;
    if ![
        "meta", "title", "link", "base", "style", "script", "body", "div", "p",
    ]
    .iter()
    .any(|name| html::starts_start_tag(&bytes, cursor, name.as_bytes()))
    {
        return Err("extension HTML needs a supported explicit or implicit head".into());
    }
    // HTML closes this head implicitly before body content, exactly as it
    // closes the parser-created head in the original document.
    let mut output = Vec::with_capacity(bytes.len() + 6);
    output.extend_from_slice(&bytes[..cursor]);
    output.extend_from_slice(b"<head>");
    output.extend_from_slice(&bytes[cursor..]);
    Ok((output, true))
}
fn skip_comments(source: &str, mut cursor: usize) -> Result<usize, String> {
    loop {
        cursor = html::skip_ascii_whitespace(source.as_bytes(), cursor);
        if !source[cursor..].starts_with("<!--") {
            return Ok(cursor);
        }
        cursor = source[cursor + 4..]
            .find("-->")
            .map(|end| cursor + 4 + end + 3)
            .ok_or("unterminated extension HTML comment")?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_explicit_pages_and_inserts_before_any_publisher_script() {
        let explicit =
            b"<!doctype html><html><head lang='en'><script src='x.js'></script></head>".to_vec();
        assert_eq!(normalize(explicit.clone()).unwrap(), (explicit, false));
        for body in [
            "<meta charset='utf-8'><script src='x.js'></script><div>ok</div>",
            "<p>Fixture</p>",
            "<script src='x.js'></script>",
            "<body class='page'>Hi</body>",
        ] {
            let input = format!("\u{feff}<!--<head>--><!DOCTYPE html><html lang='en'>\n{body}");
            let (normalized, changed) = normalize(input.as_bytes().to_vec()).unwrap();
            assert!(changed);
            assert_eq!(
                String::from_utf8(normalized).unwrap(),
                input.replace(body, &format!("<head>{body}"))
            );
        }
    }
    #[test]
    fn ambiguous_xml_comments_and_malformed_heads_stay_rejected() {
        for text in [
            "<?xml version='1.0'?><html/>",
            "<!-- missing end",
            "<head/>",
            "<html bad='unterminated><body>",
            "<head data='>'/>",
            "<headish><script></script>",
        ] {
            assert!(normalize(text.as_bytes().to_vec()).is_err(), "{text}");
        }
    }
}
