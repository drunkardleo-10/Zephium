//! Versioned main-document relay for glob-filtered content scripts.
//! The original JavaScript bytes remain untouched and execute only after a
//! native sender document ID has been checked by the existing worker.

use super::*;
use cssparser::{Parser, ParserInput, Token};
use zephium_core::injection::{
    MatchPattern, MatchPatternComponents, MatchPatternHost, MatchPatternPort, MatchPatternScheme,
};

pub(super) const WORKER_PATH: &str = "__zephium__/webkit-glob-worker-v1.js";
const BOOTSTRAP_PREFIX: &str = "__zephium__/webkit-glob-bootstrap-";
const BOOTSTRAP_SUFFIX: &str = "-v1.js";
pub(super) const WORKER_TEMPLATE: &str =
    include_str!("../../assets/macos/webkit-glob-worker-v1.js");
pub(super) const BOOTSTRAP_TEMPLATE: &str =
    include_str!("../../assets/macos/webkit-glob-bootstrap-v1.js");

pub(super) struct GlobRelayPlan {
    pub(super) resources: Vec<(String, Vec<u8>)>,
    pub(super) group_count: usize,
    pub(super) withheld_font_css: bool,
}

pub(super) fn plan(
    root: &mut Map<String, Value>,
    tree: &CanonicalExtensionTreeIndex,
    source_root: &mut dyn FnMut(&crate::ExtensionTreeFile) -> Result<Vec<u8>, String>,
) -> Result<GlobRelayPlan, String> {
    let scripts = root
        .get_mut("content_scripts")
        .and_then(Value::as_array_mut)
        .ok_or("glob relay requires content scripts")?;
    let mut resources = Vec::new();
    let mut routes = Vec::new();
    let mut withheld_font_css = false;
    for (index, entry) in scripts.iter_mut().enumerate() {
        let script = entry
            .as_object_mut()
            .ok_or_else(|| format!("content_scripts[{index}] is not an object"))?;
        if !script.contains_key("include_globs") && !script.contains_key("exclude_globs") {
            continue;
        }
        if script.get("run_at").and_then(Value::as_str) != Some("document_idle")
            || !matches!(
                script.get("world").and_then(Value::as_str),
                None | Some("ISOLATED")
            )
            || script.get("match_origin_as_fallback") == Some(&Value::Bool(true))
        {
            return Err(format!(
                "content_scripts[{index}] is outside main-document idle relay scope"
            ));
        }
        let files = script
            .get("js")
            .and_then(Value::as_array)
            .filter(|files| !files.is_empty())
            .ok_or_else(|| format!("content_scripts[{index}] has no JavaScript files"))?;
        let mut original_files = Vec::with_capacity(files.len());
        for (file_index, file) in files.iter().enumerate() {
            let path = file
                .as_str()
                .ok_or_else(|| format!("content_scripts[{index}].js[{file_index}] is invalid"))?;
            require_indexed_resource(tree, path, "glob relay JavaScript")?;
            original_files.push(path.to_owned());
        }
        if let Some(css) = script.get("css") {
            let styles = css
                .as_array()
                .ok_or_else(|| format!("content_scripts[{index}].css is invalid"))?;
            for file in styles {
                let path = file
                    .as_str()
                    .ok_or_else(|| format!("content_scripts[{index}].css path is invalid"))?;
                let portable =
                    PortableRelativePath::parse(path).map_err(|error| error.to_string())?;
                let indexed = tree
                    .file(&portable)
                    .ok_or_else(|| format!("glob relay CSS is absent: {path}"))?;
                let bytes = source_root(indexed)?;
                if bytes.len() as u64 != indexed.length() || !font_face_only(&bytes) {
                    return Err(format!(
                        "glob relay refuses behavioral or invalid CSS: {path}"
                    ));
                }
                withheld_font_css = true;
            }
        }
        let includes = globs(script.get("include_globs"), index, "include_globs")?;
        let excludes = globs(script.get("exclude_globs"), index, "exclude_globs")?;
        let primary = patterns(script.get("matches"), index, "matches", true)?;
        let primary_excludes = patterns(
            script.get("exclude_matches"),
            index,
            "exclude_matches",
            false,
        )?;
        let bootstrap_path = format!("{BOOTSTRAP_PREFIX}{index}{BOOTSTRAP_SUFFIX}");
        let bootstrap = BOOTSTRAP_TEMPLATE.replace("__ZEPHIUM_ROUTE__", &index.to_string());
        resources.push((bootstrap_path.clone(), bootstrap.into_bytes()));
        routes.push(serde_json::json!({"index": index, "primary": primary,
            "primaryExcludes": primary_excludes, "include": includes, "exclude": excludes,
            "files": original_files}));
        script.remove("include_globs");
        script.remove("exclude_globs");
        script.remove("css");
        script.insert("all_frames".into(), Value::Bool(false));
        script.insert("match_about_blank".into(), Value::Bool(false));
        script.insert("js".into(), serde_json::json!([bootstrap_path]));
    }
    if routes.is_empty() {
        return Err("glob relay target has no glob-filtered script groups".into());
    }
    let config = serde_json::to_string(&routes).map_err(|error| error.to_string())?;
    resources.push((
        WORKER_PATH.into(),
        WORKER_TEMPLATE
            .replace("__ZEPHIUM_GLOB_ROUTES__", &config)
            .into_bytes(),
    ));
    Ok(GlobRelayPlan {
        resources,
        group_count: routes.len(),
        withheld_font_css,
    })
}

fn globs(value: Option<&Value>, index: usize, field: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or_else(|| format!("content_scripts[{index}].{field} is invalid"))?;
    let mut result = Vec::with_capacity(array.len());
    for item in array {
        let pattern = item
            .as_str()
            .filter(|pattern| {
                !pattern.is_empty() && pattern.len() <= MAX_EXTENSION_RESOURCE_PATTERN_BYTES
            })
            .ok_or_else(|| format!("content_scripts[{index}].{field} pattern is invalid"))?;
        result.push(pattern.to_owned());
    }
    Ok(result)
}

fn patterns(
    value: Option<&Value>,
    index: usize,
    field: &str,
    required: bool,
) -> Result<Vec<Value>, String> {
    let Some(value) = value else {
        return if required {
            Err(format!("content_scripts[{index}] omitted {field}"))
        } else {
            Ok(Vec::new())
        };
    };
    let array = value
        .as_array()
        .filter(|items| !required || !items.is_empty())
        .ok_or_else(|| format!("content_scripts[{index}].{field} is invalid"))?;
    let mut result = Vec::with_capacity(array.len());
    for item in array {
        let source = item
            .as_str()
            .ok_or_else(|| format!("content_scripts[{index}].{field} pattern is invalid"))?;
        let pattern = MatchPattern::parse(source).map_err(|error| {
            format!("content_scripts[{index}].{field} pattern is invalid: {error}")
        })?;
        result.push(match pattern.components() {
            MatchPatternComponents::AllUrls => serde_json::json!({"all":true}),
            MatchPatternComponents::Standard { scheme, host, port, path } => {
                let scheme = match scheme {
                    MatchPatternScheme::Http => "http",
                    MatchPatternScheme::Https => "https",
                    MatchPatternScheme::HttpAndHttps => "web",
                    MatchPatternScheme::File => "file",
                };
                let (host_kind, host_value) = match host {
                    Some(MatchPatternHost::Any) => ("any", String::new()),
                    Some(MatchPatternHost::ExactDomain(name)) => ("exact", name.to_owned()),
                    Some(MatchPatternHost::ExactIpv4(address)) => ("exact", address.to_string()),
                    Some(MatchPatternHost::ExactIpv6(address)) => ("exact", format!("[{address}]")),
                    Some(MatchPatternHost::DomainAndSubdomains(name)) => ("subdomains", name.to_owned()),
                    None => ("none", String::new()),
                };
                serde_json::json!({"scheme":scheme,"hostKind":host_kind,"host":host_value,
                    "port":match port { MatchPatternPort::Any => None, MatchPatternPort::Exact(value) => Some(value) },
                    "path":path.as_str()})
            }
        });
    }
    Ok(result)
}

fn consume_block<'i, 't>(parser: &mut Parser<'i, 't>) -> Result<(), cssparser::ParseError<'i, ()>> {
    while !parser.is_exhausted() {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token,
            Err(_) => return Err(parser.new_custom_error(())),
        };
        match token {
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock => {
                parser.parse_nested_block(consume_block)?;
            }
            Token::AtKeyword(_) | Token::CurlyBracketBlock => {
                return Err(parser.new_custom_error(()))
            }
            _ => {}
        }
    }
    Ok(())
}

fn font_face_only(bytes: &[u8]) -> bool {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut count = 0usize;
    while !parser.is_exhausted() {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token,
            Err(_) => return false,
        };
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) => continue,
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("font-face") => {}
            _ => return false,
        }
        loop {
            match parser.next_including_whitespace_and_comments() {
                Ok(Token::WhiteSpace(_) | Token::Comment(_)) => continue,
                Ok(Token::CurlyBracketBlock) => break,
                _ => return false,
            }
        }
        if parser.parse_nested_block(consume_block).is_err() {
            return false;
        }
        count += 1;
    }
    count > 0
}

#[cfg(test)]
mod tests {
    use super::font_face_only;
    #[test]
    fn only_font_faces_are_cosmetic() {
        assert!(font_face_only(
            b"/* font */ @font-face { font-family: x; src: url(data:font/woff2;base64,AA); }"
        ));
        assert!(!font_face_only(
            b"@font-face { font-family: x; } body { display:none }"
        ));
        assert!(!font_face_only(b"@media screen { body { display:none } }"));
    }
}
