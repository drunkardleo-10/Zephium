use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use zephium_core::extensions::ExtensionManifestResourceDigest;

use crate::MAX_EXTENSION_MANIFEST_BYTES;

pub(crate) const DEFAULT_EXTENSION_PAGES_CSP: &str = "script-src 'self'; object-src 'self';";
pub(crate) const DEFAULT_SANDBOX_CSP: &str = concat!(
    "sandbox allow-scripts allow-forms allow-popups allow-modals; ",
    "script-src 'self' 'unsafe-inline' 'unsafe-eval'; child-src 'self';"
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CspError {
    Shape,
    UnknownKey,
    Empty,
    InvalidDirective,
    DuplicateDirective,
    MissingExtensionScriptSource,
    MissingExtensionObjectSource,
    InsecureExtensionScriptSource,
    InsecureExtensionObjectSource,
    MissingSandboxDirective,
}

pub(crate) struct EffectiveCsp {
    pub(crate) extension_pages: EffectivePolicy,
    pub(crate) sandbox: Option<EffectivePolicy>,
}

pub(crate) struct EffectivePolicy {
    pub(crate) canonical: Box<str>,
    pub(crate) digest: ExtensionManifestResourceDigest,
}

pub(crate) fn parse_effective_csp(
    value: Option<Value>,
    has_sandbox_pages: bool,
) -> Result<EffectiveCsp, CspError> {
    let mut extension_pages = None;
    let mut sandbox = None;
    if let Some(value) = value {
        let mut object = into_object(value)?;
        extension_pages = object.remove("extension_pages");
        sandbox = object.remove("sandbox");
        if !object.is_empty() {
            return Err(CspError::UnknownKey);
        }
    }

    if sandbox.is_some() && !has_sandbox_pages {
        return Err(CspError::Shape);
    }
    let extension_pages = normalize_policy(
        extension_pages
            .as_ref()
            .map_or(Ok(DEFAULT_EXTENSION_PAGES_CSP), as_string)?,
        PolicyKind::ExtensionPages,
    )?;
    let sandbox = if has_sandbox_pages {
        Some(normalize_policy(
            sandbox
                .as_ref()
                .map_or(Ok(DEFAULT_SANDBOX_CSP), as_string)?,
            PolicyKind::Sandbox,
        )?)
    } else {
        None
    };
    Ok(EffectiveCsp {
        extension_pages: EffectivePolicy {
            digest: digest_policy(b"extension-pages", &extension_pages),
            canonical: extension_pages.into_boxed_str(),
        },
        sandbox: sandbox.map(|policy| EffectivePolicy {
            digest: digest_policy(b"sandbox", &policy),
            canonical: policy.into_boxed_str(),
        }),
    })
}

fn into_object(value: Value) -> Result<Map<String, Value>, CspError> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(CspError::Shape),
    }
}

fn as_string(value: &Value) -> Result<&str, CspError> {
    value.as_str().ok_or(CspError::Shape)
}

#[derive(Clone, Copy)]
enum PolicyKind {
    ExtensionPages,
    Sandbox,
}

fn normalize_policy(value: &str, kind: PolicyKind) -> Result<String, CspError> {
    if value.is_empty()
        || value.len() > MAX_EXTENSION_MANIFEST_BYTES
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(CspError::Empty);
    }

    let mut seen = BTreeSet::new();
    let mut directives = Vec::new();
    for source in value.split(';') {
        let source = source.trim_matches(' ');
        if source.is_empty() {
            continue;
        }
        let mut tokens = source.split_ascii_whitespace();
        let name = tokens.next().ok_or(CspError::InvalidDirective)?;
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(CspError::InvalidDirective);
        }
        if !seen.insert(name.to_owned()) {
            return Err(CspError::DuplicateDirective);
        }
        let values = tokens.collect::<Vec<_>>();
        if values.iter().any(|token| {
            token.is_empty()
                || token
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte == b';')
        }) {
            return Err(CspError::InvalidDirective);
        }
        directives.push((name, values));
    }
    if directives.is_empty() {
        return Err(CspError::Empty);
    }

    match kind {
        PolicyKind::ExtensionPages => validate_extension_pages(&directives)?,
        PolicyKind::Sandbox => validate_sandbox(&directives)?,
    }

    let mut normalized = String::with_capacity(value.len());
    for (name, values) in directives {
        normalized.push_str(name);
        for token in values {
            normalized.push(' ');
            normalized.push_str(token);
        }
        normalized.push(';');
        normalized.push(' ');
    }
    normalized.pop();
    Ok(normalized)
}

fn validate_extension_pages(directives: &[(&str, Vec<&str>)]) -> Result<(), CspError> {
    let script = directives
        .iter()
        .find(|(name, _)| *name == "script-src")
        .or_else(|| directives.iter().find(|(name, _)| *name == "default-src"))
        .ok_or(CspError::MissingExtensionScriptSource)?;
    let object = directives
        .iter()
        .find(|(name, _)| *name == "object-src")
        .or_else(|| directives.iter().find(|(name, _)| *name == "default-src"))
        .ok_or(CspError::MissingExtensionObjectSource)?;

    if !safe_extension_sources(&script.1, true) {
        return Err(CspError::InsecureExtensionScriptSource);
    }
    if !safe_extension_sources(&object.1, false) {
        return Err(CspError::InsecureExtensionObjectSource);
    }
    // `worker-src` falls back through `child-src` before `script-src`. A
    // remote `child-src` would therefore reopen remote worker code even with
    // an otherwise-safe `script-src`.
    for name in [
        "worker-src",
        "child-src",
        "script-src-elem",
        "script-src-attr",
    ] {
        if let Some((_, values)) = directives.iter().find(|(candidate, _)| *candidate == name) {
            if !safe_extension_sources(values, name != "script-src-attr") {
                return Err(CspError::InsecureExtensionScriptSource);
            }
        }
    }
    Ok(())
}

fn validate_sandbox(directives: &[(&str, Vec<&str>)]) -> Result<(), CspError> {
    let (_, flags) = directives
        .iter()
        .find(|(name, _)| *name == "sandbox")
        .ok_or(CspError::MissingSandboxDirective)?;
    const SAFE_FLAGS: &[&str] = &[
        "allow-scripts",
        "allow-forms",
        "allow-popups",
        "allow-modals",
    ];
    let mut seen = BTreeSet::new();
    if flags
        .iter()
        .any(|flag| !SAFE_FLAGS.contains(flag) || !seen.insert(*flag))
    {
        return Err(CspError::InvalidDirective);
    }
    if !directives.iter().any(|(name, _)| *name == "script-src") {
        return Err(CspError::MissingExtensionScriptSource);
    }
    for name in [
        "script-src",
        "worker-src",
        "child-src",
        "script-src-elem",
        "script-src-attr",
    ] {
        if let Some((_, values)) = directives.iter().find(|(candidate, _)| *candidate == name) {
            if values.contains(&"'none'") && values.len() != 1 {
                return Err(CspError::InsecureExtensionScriptSource);
            }
            let allowed = values.iter().all(|source| {
                matches!(
                    *source,
                    "'self'" | "'none'" | "'unsafe-inline'" | "'unsafe-eval'"
                )
            });
            if !allowed {
                return Err(CspError::InsecureExtensionScriptSource);
            }
        }
    }
    Ok(())
}

fn safe_extension_sources(values: &[&str], script: bool) -> bool {
    if values.is_empty() {
        return false;
    }
    if values == ["'none'"] {
        return true;
    }
    values
        .iter()
        .all(|source| *source == "'self'" || (script && *source == "'wasm-unsafe-eval'"))
        && values.contains(&"'self'")
}

fn digest_policy(kind: &[u8], canonical: &str) -> ExtensionManifestResourceDigest {
    let mut digest = Sha256::new();
    digest.update(b"zephium:extension-effective-csp:v1\0");
    digest.update((kind.len() as u64).to_be_bytes());
    digest.update(kind);
    digest.update((canonical.len() as u64).to_be_bytes());
    digest.update(canonical.as_bytes());
    ExtensionManifestResourceDigest::from_bytes(digest.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_mv3_defaults_are_stable() {
        let default = parse_effective_csp(None, true).unwrap();
        let explicit = parse_effective_csp(
            Some(serde_json::json!({
                "extension_pages": DEFAULT_EXTENSION_PAGES_CSP,
                "sandbox": DEFAULT_SANDBOX_CSP,
            })),
            true,
        )
        .unwrap();
        assert_eq!(
            default.extension_pages.digest,
            explicit.extension_pages.digest
        );
        assert_eq!(
            default.sandbox.as_ref().map(|policy| policy.digest),
            explicit.sandbox.as_ref().map(|policy| policy.digest)
        );
        assert_eq!(
            default.extension_pages.canonical.as_ref(),
            DEFAULT_EXTENSION_PAGES_CSP
        );
        assert_eq!(
            default.sandbox.unwrap().canonical.as_ref(),
            DEFAULT_SANDBOX_CSP
        );
    }

    #[test]
    fn extension_pages_never_admit_remote_or_unsafe_eval_script_sources() {
        for source in [
            "https://cdn.example",
            "'unsafe-eval'",
            "'unsafe-inline'",
            "*",
        ] {
            let policy = format!("script-src 'self' {source}; object-src 'self';");
            assert_eq!(
                normalize_policy(&policy, PolicyKind::ExtensionPages),
                Err(CspError::InsecureExtensionScriptSource)
            );
        }
    }

    #[test]
    fn restrictive_default_source_safely_supplies_missing_object_source() {
        let policy = normalize_policy(
            "default-src 'none'; script-src 'self' 'wasm-unsafe-eval';",
            PolicyKind::ExtensionPages,
        )
        .unwrap();
        assert_eq!(
            policy,
            "default-src 'none'; script-src 'self' 'wasm-unsafe-eval';"
        );

        assert_eq!(
            normalize_policy(
                "default-src https://remote.example; script-src 'self';",
                PolicyKind::ExtensionPages,
            ),
            Err(CspError::InsecureExtensionObjectSource)
        );
    }

    #[test]
    fn duplicate_and_unscoped_sandbox_policies_fail_closed() {
        assert_eq!(
            normalize_policy(
                "script-src 'self'; script-src 'self'; object-src 'self';",
                PolicyKind::ExtensionPages,
            ),
            Err(CspError::DuplicateDirective)
        );
        assert_eq!(
            parse_effective_csp(
                Some(serde_json::json!({"sandbox": DEFAULT_SANDBOX_CSP})),
                false,
            )
            .err(),
            Some(CspError::Shape)
        );
    }

    #[test]
    fn worker_and_sandbox_escape_capabilities_fail_closed() {
        for source in ["https://cdn.example", "'unsafe-eval'", "blob:"] {
            let policy =
                format!("script-src 'self'; object-src 'self'; worker-src 'self' {source};");
            assert_eq!(
                normalize_policy(&policy, PolicyKind::ExtensionPages),
                Err(CspError::InsecureExtensionScriptSource)
            );
        }
        for flag in [
            "allow-same-origin",
            "allow-top-navigation",
            "allow-downloads",
        ] {
            let policy = format!("sandbox allow-scripts {flag}; script-src 'self';");
            assert_eq!(
                normalize_policy(&policy, PolicyKind::Sandbox),
                Err(CspError::InvalidDirective)
            );
        }
        for directive in ["script-src", "worker-src", "script-src-elem"] {
            let policy = if directive == "script-src" {
                "sandbox allow-scripts; script-src 'none' 'unsafe-eval';".to_owned()
            } else {
                format!(
                    "sandbox allow-scripts; script-src 'self'; {directive} 'none' 'unsafe-eval';"
                )
            };
            assert_eq!(
                normalize_policy(&policy, PolicyKind::Sandbox),
                Err(CspError::InsecureExtensionScriptSource)
            );
        }

        for kind in [PolicyKind::ExtensionPages, PolicyKind::Sandbox] {
            let policy = match kind {
                PolicyKind::ExtensionPages => concat!(
                    "script-src 'self'; object-src 'self'; ",
                    "child-src https://remote-worker.example;"
                ),
                PolicyKind::Sandbox => concat!(
                    "sandbox allow-scripts; script-src 'self'; ",
                    "child-src https://remote-worker.example;"
                ),
            };
            assert_eq!(
                normalize_policy(policy, kind),
                Err(CspError::InsecureExtensionScriptSource)
            );
        }
    }
}
