use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use sha2::{Digest, Sha256};

use tough::schema::{RoleType, Root, Signed};
use tough::{Transport, TransportError, TransportErrorKind, TransportStream};
use url::Url;
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, ExtensionPolicyChannel, EXTENSION_PUBLIC_POLICY_TARGET,
    MAX_EXTENSION_PUBLIC_POLICY_BYTES,
};
use zephium_update_transport::{
    ConditionalFixedOriginResponse, FixedOriginFetchError, FixedOriginTransport,
};

use super::ExtensionPolicyError;

pub(super) const MAX_METADATA_BYTES: usize = 64 * 1024;
pub(super) const MAX_ROOT_UPDATES: u64 = 16;
const MAX_REQUESTS: usize = MAX_ROOT_UPDATES as usize + 5;
// A process-local optimization only: every hit goes back through Tough and
// policy/checkpoint validation. It never supplies trust or resets expiry.
const MAX_CACHE_BYTES: usize = 2 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 32;

#[derive(Clone, Debug, Default)]
struct ResponseCache {
    bytes: usize,
    entries: BTreeMap<String, Arc<[u8]>>,
}

#[derive(Clone, Debug)]
pub(super) enum Source {
    Https(FixedOriginTransport),
    #[cfg(test)]
    Fixture(Arc<std::collections::BTreeMap<String, Vec<u8>>>),
}

#[derive(Clone, Debug)]
pub(super) struct PolicyTransport {
    pub(super) metadata: Url,
    pub(super) targets: Url,
    source: Source,
    requests: Arc<AtomicUsize>,
    cache: Arc<Mutex<ResponseCache>>,
    // Immutable previously authenticated snapshot; response staging must
    // never influence even a repeated request within this same refresh.
    conditional: Option<Arc<ResponseCache>>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Object {
    Root,
    Timestamp,
    Snapshot,
    Targets,
    Policy,
}

pub(super) fn bases(channel: ExtensionPolicyChannel) -> Result<(Url, Url), ExtensionPolicyError> {
    let path = match channel {
        ExtensionPolicyChannel::Stable => "stable",
        ExtensionPolicyChannel::Staging => "staging",
    };
    let base = format!("https://extensions.zephium.app/v1/{path}/");
    Ok((
        Url::parse(&format!("{base}metadata/"))
            .map_err(|_| ExtensionPolicyError::Authentication)?,
        Url::parse(&format!("{base}targets/")).map_err(|_| ExtensionPolicyError::Authentication)?,
    ))
}

impl PolicyTransport {
    pub(super) fn new(metadata: Url, targets: Url, source: Source) -> Self {
        Self {
            metadata,
            targets,
            source,
            requests: Arc::new(AtomicUsize::new(0)),
            cache: Arc::new(Mutex::new(ResponseCache::default())),
            conditional: None,
        }
    }
    pub(super) fn fresh(&self) -> Result<Self, ExtensionPolicyError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| ExtensionPolicyError::Authentication)?
            .clone();
        Ok(Self {
            requests: Arc::new(AtomicUsize::new(0)),
            cache: Arc::new(Mutex::new(cache.clone())),
            conditional: Some(Arc::new(cache)),
            ..self.clone()
        })
    }
    pub(super) fn retain_verified(&self, candidate: &Self) -> Result<(), ExtensionPolicyError> {
        let cache = candidate
            .cache
            .lock()
            .map_err(|_| ExtensionPolicyError::Authentication)?
            .clone();
        *self
            .cache
            .lock()
            .map_err(|_| ExtensionPolicyError::Authentication)? = cache;
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn cached_entries(&self) -> usize {
        self.cache.lock().unwrap().entries.len()
    }
    fn object(&self, url: &Url) -> Option<Object> {
        // Comparing the full canonical URL excludes credentials, query strings,
        // fragments, other channels and origins before any network request.
        if let Some(name) = url.as_str().strip_prefix(self.metadata.as_str()) {
            if name == "timestamp.json" {
                return Some(Object::Timestamp);
            }
            for (suffix, object) in [
                (".root.json", Object::Root),
                (".snapshot.json", Object::Snapshot),
                (".targets.json", Object::Targets),
            ] {
                if name.strip_suffix(suffix).is_some_and(version) {
                    return Some(object);
                }
            }
        }
        if let Some(name) = url.as_str().strip_prefix(self.targets.as_str()) {
            if name
                .strip_suffix(&format!(".{EXTENSION_PUBLIC_POLICY_TARGET}"))
                .is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
            {
                return Some(Object::Policy);
            }
        }
        None
    }
}

fn version(text: &str) -> bool {
    !text.starts_with('0')
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text
            .parse::<u64>()
            .is_ok_and(|value| value > 0 && value <= i64::MAX as u64)
}

#[async_trait::async_trait]
impl Transport for PolicyTransport {
    async fn fetch(&self, url: Url) -> Result<TransportStream, TransportError> {
        let error = |kind| TransportError::new(kind, "extension-policy");
        let object = self
            .object(&url)
            .ok_or_else(|| error(TransportErrorKind::UnsupportedUrlScheme))?;
        if self.requests.fetch_add(1, Ordering::Relaxed) >= MAX_REQUESTS {
            return Err(error(TransportErrorKind::Other));
        }
        let max = if matches!(object, Object::Policy) {
            MAX_EXTENSION_PUBLIC_POLICY_BYTES
        } else {
            MAX_METADATA_BYTES
        };
        let cached = self
            .conditional
            .as_ref()
            .and_then(|cache| cache.entries.get(url.as_str()))
            .cloned();
        let digest = cached
            .as_ref()
            .map(|bytes| <[u8; 32]>::from(Sha256::digest(bytes)));
        let response = match &self.source {
            Source::Https(source) => source
                .fetch_bounded_conditional(url.clone(), max, digest)
                .await
                .map_err(|failure| {
                    error(if failure == FixedOriginFetchError::NotFound {
                        TransportErrorKind::FileNotFound
                    } else {
                        TransportErrorKind::Other
                    })
                })?,
            #[cfg(test)]
            Source::Fixture(files) => {
                let bytes = files
                    .get(url.as_str())
                    .ok_or_else(|| error(TransportErrorKind::FileNotFound))?;
                if digest == Some(Sha256::digest(bytes).into()) {
                    ConditionalFixedOriginResponse::NotModified
                } else {
                    ConditionalFixedOriginResponse::Modified(bytes.clone().into_boxed_slice())
                }
            }
        };
        let bytes: Arc<[u8]> = match response {
            ConditionalFixedOriginResponse::NotModified => {
                cached.ok_or_else(|| error(TransportErrorKind::Other))?
            }
            ConditionalFixedOriginResponse::Modified(bytes) => Arc::from(bytes),
        };
        preflight(&bytes, object).map_err(|_| error(TransportErrorKind::Other))?;
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| error(TransportErrorKind::Other))?;
            if let Some(old) = cache.entries.remove(url.as_str()) {
                cache.bytes -= old.len();
            }
            if cache.bytes + bytes.len() > MAX_CACHE_BYTES
                || cache.entries.len() >= MAX_CACHE_ENTRIES
            {
                cache.entries.clear();
                cache.bytes = 0;
            }
            cache.bytes += bytes.len();
            cache.entries.insert(url.to_string(), Arc::clone(&bytes));
        }
        Ok(Box::pin(futures_util::stream::once(async move {
            Ok(bytes::Bytes::copy_from_slice(&bytes))
        })))
    }
}

pub(super) fn preflight(bytes: &[u8], object: Object) -> Result<(), ExtensionPolicyError> {
    let fail = ExtensionPolicyError::Authentication;
    let max = if matches!(object, Object::Policy) {
        MAX_EXTENSION_PUBLIC_POLICY_BYTES
    } else {
        MAX_METADATA_BYTES
    };
    if bytes.is_empty() || bytes.len() > max {
        return Err(fail);
    }
    let bounded = parse_bounded_json(bytes, BoundedJsonLimits::public_extension_policy())
        .map_err(|_| fail)?;
    let value = bounded.into_value();
    if matches!(object, Object::Policy) {
        return Ok(());
    }
    let signed = value.get("signed").ok_or(fail)?;
    // Keep role counters below the signed storage ceiling and leave room for
    // Tough's bounded bootstrap-version arithmetic before it sees the bytes.
    if !signed
        .get("version")
        .and_then(|version| version.as_u64())
        .is_some_and(|version| version > 0 && version <= i64::MAX as u64)
    {
        return Err(fail);
    }
    let expected = match object {
        Object::Root => "root",
        Object::Timestamp => "timestamp",
        Object::Snapshot => "snapshot",
        Object::Targets => "targets",
        Object::Policy => return Err(fail),
    };
    if signed.get("_type").and_then(|value| value.as_str()) != Some(expected) {
        return Err(fail);
    }
    let signatures = value
        .get("signatures")
        .and_then(|value| value.as_array())
        .ok_or(fail)?;
    let mut unique = BTreeSet::new();
    if signatures.is_empty()
        || signatures.len() > 32
        || signatures.iter().any(|signature| {
            signature
                .get("keyid")
                .and_then(|key| key.as_str())
                .is_none_or(|key| !unique.insert(key))
        })
    {
        return Err(fail);
    }
    match object {
        Object::Root => {
            let root: Signed<Root> = serde_json::from_value(value).map_err(|_| fail)?;
            if !root.signed.consistent_snapshot || root.signed.roles.len() != 4 {
                return Err(fail);
            }
            let mut keys = BTreeSet::new();
            let mut public_keys = BTreeSet::new();
            for role in [
                RoleType::Root,
                RoleType::Timestamp,
                RoleType::Snapshot,
                RoleType::Targets,
            ] {
                let binding = root.signed.roles.get(&role).ok_or(fail)?;
                if (role == RoleType::Root
                    && (binding.threshold.get() != 2 || binding.keyids.len() != 3))
                    || binding.keyids.is_empty()
                    || binding.keyids.len() > 8
                    || binding.threshold.get() > binding.keyids.len() as u64
                {
                    return Err(fail);
                }
                for id in &binding.keyids {
                    let key = root.signed.keys.get(id).ok_or(fail)?;
                    // Compare decoded key material: JSON extras, PEM whitespace
                    // and the legacy ECDSA tag must not disguise a reused key.
                    let material = match key {
                        tough::schema::key::Key::Ed25519 { keyval, .. } => {
                            (0, keyval.public.as_ref().to_vec())
                        }
                        tough::schema::key::Key::Rsa { keyval, .. } => {
                            (1, keyval.public.as_ref().to_vec())
                        }
                        tough::schema::key::Key::Ecdsa { keyval, .. }
                        | tough::schema::key::Key::EcdsaOld { keyval, .. } => {
                            (2, keyval.public.as_ref().to_vec())
                        }
                    };
                    if !keys.insert(id.as_ref().to_vec()) || !public_keys.insert(material) {
                        return Err(fail);
                    }
                }
            }
        }
        Object::Targets => {
            if signed
                .get("delegations")
                .is_some_and(|value| !value.is_null())
            {
                return Err(fail);
            }
            let targets = signed
                .get("targets")
                .and_then(|value| value.as_object())
                .ok_or(fail)?;
            if targets.len() != 1 || !targets.contains_key(EXTENSION_PUBLIC_POLICY_TARGET) {
                return Err(fail);
            }
        }
        Object::Snapshot | Object::Timestamp => {
            let expected = if matches!(object, Object::Snapshot) {
                "targets.json"
            } else {
                "snapshot.json"
            };
            let meta = signed
                .get("meta")
                .and_then(|value| value.as_object())
                .ok_or(fail)?;
            if meta.len() != 1 || !meta.contains_key(expected) {
                return Err(fail);
            }
        }
        Object::Policy => {}
    }
    Ok(())
}
