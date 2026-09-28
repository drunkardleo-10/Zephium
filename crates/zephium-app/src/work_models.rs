//! Models for the lead agent: the catalog (built in, or Zephium Cloud's when
//! signed in), the person's keys, per-profile choices per role, and
//! [`resolve`], which hands the runtime a ready client.
//!
//! Choices are trusted host state stored through an installed settings port;
//! keys live only in the Keychain. Nothing here logs page, model or person text.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use zephium_agentic::lead::{
    self, keys, models, LeadClient, LeadCredential, LeadKeyCheck, LeadSecret, LeadSecretFuture,
    LeadStaticCredential, LeadTarget,
};
use zephium_core::ids::ProfileId;
use zephium_core::work::model::*;

use WorkModelProvider as P;

/// Keys, in the order a role falls back to a provider when nothing is chosen.
const FALLBACK_ORDER: [WorkModelProvider; 4] = [P::Anthropic, P::OpenAi, P::Google, P::DeepSeek];
/// Providers that take a key in Settings, in display order.
pub const KEYED_PROVIDERS: [WorkModelProvider; 6] = [
    P::Anthropic,
    P::OpenAi,
    P::Google,
    P::DeepSeek,
    P::OpenRouter,
    P::Compatible,
];
const ROLES: [WorkModelRole; 3] = [
    WorkModelRole::Lead,
    WorkModelRole::Page,
    WorkModelRole::Light,
];
const CLOUD_CATALOG_KEY: &str = "work.models.cloud.catalog";
const COMPATIBLE_BASE_KEY: &str = "work.models.compatible.base";
const CLOUD_REFRESH: Duration = Duration::from_secs(60 * 60);
const MAX_STORED_CATALOG_BYTES: usize = 512 * 1024;

/// App settings, installed by the composition root.
pub trait WorkModelSettings: Send + Sync {
    /// The stored value, if any.
    fn get(&self, key: &str) -> Option<String>;
    /// Stores a value; an empty value clears it.
    fn set(&self, key: String, value: String) -> bool;
}

/// A signed-in Zephium Cloud session, installed by the sign-in flow.
pub trait WorkCloudSession: Send + Sync {
    /// Whether a session is active now.
    fn signed_in(&self) -> bool;
    /// The plan's name, as the server reports it.
    fn plan(&self) -> Option<String>;
    /// The API base, such as `https://api.zephium.app`.
    fn base(&self) -> String;
    /// The short-lived bearer, refreshed by the session.
    fn credential(&self) -> Arc<dyn LeadCredential>;
}

static SETTINGS: RwLock<Option<Arc<dyn WorkModelSettings>>> = RwLock::new(None);
static CLOUD: RwLock<Option<Arc<dyn WorkCloudSession>>> = RwLock::new(None);
type Observer = Box<dyn Fn() + Send + Sync>;
static OBSERVER: OnceLock<Observer> = OnceLock::new();

/// Installs the settings store. Choices made before it exist only in memory.
pub fn install_settings(settings: Arc<dyn WorkModelSettings>) {
    if let Ok(mut slot) = SETTINGS.write() {
        *slot = Some(settings);
    }
}

/// Installs the Cloud session. Without one, Cloud reads as signed out.
pub fn install_cloud_session(session: Arc<dyn WorkCloudSession>) {
    if let Ok(mut slot) = CLOUD.write() {
        *slot = Some(session);
    }
    notify();
}

/// Called after any change a picker or Settings shows (keys, choices, catalog).
pub fn set_observer(observer: Observer) {
    let _ = OBSERVER.set(observer);
}

fn notify() {
    if let Some(observer) = OBSERVER.get() {
        observer();
    }
}

fn setting(key: &str) -> Option<String> {
    let settings = SETTINGS.read().ok()?.clone()?;
    settings.get(key).filter(|value| !value.is_empty())
}

fn store_setting(key: String, value: String) -> bool {
    let settings = SETTINGS.read().ok().and_then(|slot| slot.clone());
    settings.is_some_and(|settings| settings.set(key, value))
}

fn cloud() -> Option<Arc<dyn WorkCloudSession>> {
    CLOUD
        .read()
        .ok()?
        .clone()
        .filter(|session| session.signed_in())
}

/// What the person sees about a provider's key. Never the key itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkKeyStatus {
    /// No key stored.
    Missing,
    /// A key is stored and has not been checked yet.
    Set,
    /// The provider accepted the key.
    Valid,
    /// The provider refused the key.
    Invalid,
}

impl WorkKeyStatus {
    fn verdict(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Missing | Self::Set => "",
        }
    }
}

#[derive(Default)]
struct State {
    presence: HashMap<WorkModelProvider, bool>,
    verdicts: HashMap<WorkModelProvider, WorkKeyStatus>,
    listed: HashMap<WorkModelProvider, Vec<WorkModelEntry>>,
    cloud: Vec<models::CloudModel>,
    cloud_loaded: bool,
    cloud_fetched: Option<Instant>,
    /// Choices kept in memory when no settings store is installed.
    choices: HashMap<String, WorkModelEntry>,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn key_setting(provider: WorkModelProvider) -> String {
    format!("work.models.key.{}", models::provider_slug(provider))
}

fn choice_key(profile: ProfileId, role: WorkModelRole) -> String {
    let role = match role {
        WorkModelRole::Lead => "lead",
        WorkModelRole::Page => "page",
        WorkModelRole::Light => "light",
        WorkModelRole::Decision => "decision",
    };
    format!("work.models.{profile}.{role}")
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

/// Reads which keys exist, from Keychain attributes only, once per process.
async fn ensure_presence() {
    let missing: Vec<_> = {
        let state = state();
        KEYED_PROVIDERS
            .into_iter()
            .filter(|provider| !state.presence.contains_key(provider))
            .collect()
    };
    if missing.is_empty() {
        return;
    }
    let found = blocking(move || {
        missing
            .into_iter()
            .map(|provider| (provider, keys::present(provider).unwrap_or(false)))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let mut state = state();
    for (provider, present) in found {
        state.presence.insert(provider, present);
        let verdict = match setting(&key_setting(provider)).as_deref() {
            Some("valid") => WorkKeyStatus::Valid,
            Some("invalid") => WorkKeyStatus::Invalid,
            _ => WorkKeyStatus::Set,
        };
        state.verdicts.entry(provider).or_insert(verdict);
    }
}

fn key_status(state: &State, provider: WorkModelProvider) -> WorkKeyStatus {
    if !state.presence.get(&provider).copied().unwrap_or(false) {
        return WorkKeyStatus::Missing;
    }
    state
        .verdicts
        .get(&provider)
        .copied()
        .unwrap_or(WorkKeyStatus::Set)
}

fn record_verdict(provider: WorkModelProvider, status: WorkKeyStatus) {
    let changed = {
        let mut state = state();
        let previous = state.verdicts.insert(provider, status);
        previous != Some(status)
    };
    if changed {
        store_setting(key_setting(provider), status.verdict().to_owned());
        notify();
    }
}

fn compatible_base() -> Option<String> {
    setting(COMPATIBLE_BASE_KEY)
}

/// Whether calls to `provider` can be made now.
fn usable(state: &State, provider: WorkModelProvider) -> bool {
    match provider {
        P::Cloud => cloud().is_some(),
        P::Compatible => compatible_base().is_some(),
        provider => {
            state.presence.get(&provider).copied().unwrap_or(false)
                && key_status(state, provider) != WorkKeyStatus::Invalid
        }
    }
}

fn load_cloud_cache(state: &mut State) {
    if state.cloud_loaded {
        return;
    }
    state.cloud_loaded = true;
    if let Some(stored) = setting(CLOUD_CATALOG_KEY) {
        if let Ok(body) = serde_json::from_str(&stored) {
            state.cloud = models::parse_cloud(&body);
        }
    }
}

/// Refreshes the Cloud catalog when signed in and the copy is older than an
/// hour; offline, the cached copy (or the built-in list) stays.
pub async fn refresh_cloud_catalog() {
    let Some(session) = cloud() else { return };
    let stale = {
        let mut state = state();
        load_cloud_cache(&mut state);
        state
            .cloud_fetched
            .is_none_or(|fetched| fetched.elapsed() > CLOUD_REFRESH)
    };
    if !stale {
        return;
    }
    let Ok(secret) = session.credential().secret().await else {
        return;
    };
    let Ok(fetched) = lead::fetch_cloud_catalog(&session.base(), &secret).await else {
        return;
    };
    let raw: Vec<serde_json::Value> = fetched
        .iter()
        .map(|model| {
            serde_json::json!({
                "id": model.entry.id.trim_start_matches("zephium/"),
                "provider": models::provider_slug(model.upstream),
                "upstream_model": model.entry.model.model,
                "display_name": model.entry.display_name,
                "roles": model.entry.roles,
                "recommended": model.entry.recommended,
                "context_window": model.entry.context_window,
                "max_output": model.entry.max_output,
                "supports": model.entry.supports,
                "plan_min": model.plan_min,
            })
        })
        .collect();
    if let Ok(text) = serde_json::to_string(&raw) {
        if text.len() <= MAX_STORED_CATALOG_BYTES {
            store_setting(CLOUD_CATALOG_KEY.to_owned(), text);
        }
    }
    {
        let mut state = state();
        state.cloud = fetched;
        state.cloud_fetched = Some(Instant::now());
    }
    notify();
}

/// Every entry the person can pick right now: Cloud's first when signed in,
/// then the built-in list, then models listed from providers ("More models").
fn entries(state: &State) -> Vec<WorkModelEntry> {
    let mut all = Vec::new();
    if cloud().is_some() {
        all.extend(state.cloud.iter().map(|model| model.entry.clone()));
    }
    all.extend(models::builtin());
    for provider in KEYED_PROVIDERS {
        for entry in state.listed.get(&provider).into_iter().flatten() {
            if !all.iter().any(|known| known.id == entry.id) {
                all.push(entry.clone());
            }
        }
    }
    all
}

fn stored_choice(state: &State, profile: ProfileId, role: WorkModelRole) -> Option<WorkModelEntry> {
    let key = choice_key(profile, role);
    let stored: WorkModelEntry = match setting(&key) {
        Some(text) => serde_json::from_str(&text).ok()?,
        None => state.choices.get(&key).cloned()?,
    };
    // A catalog entry may have changed since it was chosen (limits, prices).
    Some(
        entries(state)
            .into_iter()
            .find(|entry| entry.id == stored.id)
            .unwrap_or(stored),
    )
}

fn family_entry(
    state: &State,
    provider: WorkModelProvider,
    role: WorkModelRole,
) -> Option<WorkModelEntry> {
    if provider == P::Cloud {
        return state
            .cloud
            .iter()
            .filter(|model| model.entry.roles.contains(&role))
            .max_by_key(|model| model.entry.recommended)
            .map(|model| model.entry.clone());
    }
    let model = models::family_default(provider, role)?;
    models::builtin()
        .into_iter()
        .find(|entry| entry.model.provider == provider && entry.model.model == model)
}

/// The model a role runs with: the person's choice when its provider is
/// usable, else the lead's provider family, else the first keyed provider.
fn effective(state: &State, profile: ProfileId, role: WorkModelRole) -> Option<WorkModelEntry> {
    if let Some(choice) = stored_choice(state, profile, role) {
        if usable(state, choice.model.provider) {
            return Some(choice);
        }
    }
    if role != WorkModelRole::Lead {
        if let Some(lead) = effective(state, profile, WorkModelRole::Lead) {
            if let Some(entry) = family_entry(state, lead.model.provider, role) {
                return Some(entry);
            }
            if lead.roles.contains(&role) && (role != WorkModelRole::Page || lead.supports.vision) {
                return Some(lead);
            }
        }
    }
    if cloud().is_some() {
        if let Some(entry) = family_entry(state, P::Cloud, role) {
            return Some(entry);
        }
    }
    FALLBACK_ORDER
        .into_iter()
        .filter(|provider| usable(state, *provider))
        .find_map(|provider| family_entry(state, provider, role))
}

/// Tracks the key's standing from real calls: a refusal marks it invalid,
/// success marks it valid, so Settings and the picker stay truthful.
struct Observed {
    inner: LeadClient,
    provider: WorkModelProvider,
}

impl WorkModelClient for Observed {
    fn call<'a>(
        &'a self,
        request: WorkModelRequest,
        events: &'a (dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> WorkModelFuture<'a> {
        Box::pin(async move {
            let result = self.inner.call(request, events).await;
            if KEYED_PROVIDERS.contains(&self.provider) && self.provider != P::Compatible {
                match &result {
                    Ok(_) => record_verdict(self.provider, WorkKeyStatus::Valid),
                    Err(WorkModelError::Unauthorized) => {
                        record_verdict(self.provider, WorkKeyStatus::Invalid)
                    }
                    Err(_) => {}
                }
            }
            result
        })
    }
}

/// A local endpoint may need no key; it then gets a placeholder bearer.
struct CompatibleCredential;

impl LeadCredential for CompatibleCredential {
    fn secret(&self) -> LeadSecretFuture<'_> {
        Box::pin(async move {
            let stored = blocking(|| keys::load(P::Compatible)).await;
            match stored {
                Some(Ok(secret)) => Ok(secret),
                _ => LeadSecret::new("zephium".into()).map(Arc::new),
            }
        })
    }

    fn rejected(&self) {}
}

fn client(
    state: &State,
    entry: &WorkModelEntry,
) -> Result<Arc<dyn WorkModelClient>, WorkModelError> {
    let provider = entry.model.provider;
    let (target, credential): (LeadTarget, Arc<dyn LeadCredential>) = match provider {
        P::Cloud => {
            let session = cloud().ok_or(WorkModelError::MissingKey)?;
            let upstream = state
                .cloud
                .iter()
                .find(|model| model.entry.id == entry.id)
                .map(|model| model.upstream)
                .ok_or(WorkModelError::BadRequest)?;
            (
                LeadTarget::cloud(&session.base(), upstream)?,
                session.credential(),
            )
        }
        P::Compatible => (
            LeadTarget::compatible(&compatible_base().ok_or(WorkModelError::MissingKey)?)?,
            Arc::new(CompatibleCredential),
        ),
        provider => (
            LeadTarget::direct(provider)?,
            Arc::new(keys::KeychainCredential::new(provider)),
        ),
    };
    if target.wire() != entry.model.wire {
        return Err(WorkModelError::BadRequest);
    }
    Ok(Arc::new(Observed {
        inner: LeadClient::new(target, credential, entry.price.clone()),
        provider,
    }))
}

/// The model and a ready client for `role` in `profile`.
pub async fn resolve(
    profile: ProfileId,
    role: WorkModelRole,
) -> Result<(WorkModelRef, Arc<dyn WorkModelClient>), WorkModelError> {
    let (entry, client) = resolve_entry(profile, role).await?;
    Ok((entry.model, client))
}

/// Like [`resolve`], with the catalog entry (limits, supports, price).
pub async fn resolve_entry(
    profile: ProfileId,
    role: WorkModelRole,
) -> Result<(WorkModelEntry, Arc<dyn WorkModelClient>), WorkModelError> {
    ensure_presence().await;
    let state = state();
    let entry = effective(&state, profile, role).ok_or(WorkModelError::MissingKey)?;
    let client = client(&state, &entry)?;
    Ok((entry, client))
}

/// One provider as Settings and the picker show it.
#[derive(Clone, Debug)]
pub struct WorkProviderView {
    pub provider: WorkModelProvider,
    pub key: WorkKeyStatus,
    /// The configured base URL, for the OpenAI-compatible endpoint only.
    pub base: Option<String>,
}

/// Everything the picker and Settings → AI render.
#[derive(Clone, Debug)]
pub struct WorkModelsView {
    pub entries: Vec<WorkModelEntry>,
    /// What the person chose per role (lead, page, light), if anything.
    pub chosen: [Option<String>; 3],
    /// What each role runs with now.
    pub effective: [Option<String>; 3],
    pub providers: Vec<WorkProviderView>,
    pub cloud_signed_in: bool,
    pub cloud_plan: Option<String>,
}

/// The current view for `profile`.
pub async fn view(profile: ProfileId) -> WorkModelsView {
    ensure_presence().await;
    let state = state();
    let chosen = ROLES.map(|role| stored_choice(&state, profile, role).map(|entry| entry.id));
    let effective = ROLES.map(|role| effective(&state, profile, role).map(|entry| entry.id));
    let mut entries = entries(&state);
    for role in ROLES {
        if let Some(choice) = stored_choice(&state, profile, role) {
            if !entries.iter().any(|entry| entry.id == choice.id) {
                entries.push(choice);
            }
        }
    }
    let providers = KEYED_PROVIDERS
        .into_iter()
        .map(|provider| WorkProviderView {
            provider,
            key: key_status(&state, provider),
            base: (provider == P::Compatible).then(compatible_base).flatten(),
        })
        .collect();
    let session = cloud();
    WorkModelsView {
        entries,
        chosen,
        effective,
        providers,
        cloud_signed_in: session.is_some(),
        cloud_plan: session.and_then(|session| session.plan()),
    }
}

/// Stores the person's choice for `role`; `None` returns it to the default.
pub fn choose(
    profile: ProfileId,
    role: WorkModelRole,
    id: Option<&str>,
) -> Result<(), WorkModelError> {
    if role == WorkModelRole::Decision {
        return Err(WorkModelError::BadRequest);
    }
    let key = choice_key(profile, role);
    let value = match id {
        None => String::new(),
        Some(id) => {
            let state = state();
            let entry = entries(&state)
                .into_iter()
                .find(|entry| entry.id == id)
                .ok_or(WorkModelError::BadRequest)?;
            if !entry.roles.contains(&role) {
                return Err(WorkModelError::BadRequest);
            }
            serde_json::to_string(&entry).map_err(|_| WorkModelError::BadRequest)?
        }
    };
    {
        let mut state = state();
        if value.is_empty() {
            state.choices.remove(&key);
        } else if let Ok(entry) = serde_json::from_str(&value) {
            state.choices.insert(key.clone(), entry);
        }
    }
    store_setting(key, value);
    notify();
    Ok(())
}

fn check_target(provider: WorkModelProvider) -> Result<LeadTarget, WorkModelError> {
    match provider {
        P::Compatible => {
            LeadTarget::compatible(&compatible_base().ok_or(WorkModelError::BadRequest)?)
        }
        P::Cloud => Err(WorkModelError::BadRequest),
        provider => LeadTarget::direct(provider),
    }
}

/// Checks `secret` with the provider, then stores it. A refused key is not
/// stored; an unreachable provider stores it unchecked.
pub async fn set_key(
    provider: WorkModelProvider,
    secret: String,
) -> Result<WorkKeyStatus, WorkModelError> {
    if !KEYED_PROVIDERS.contains(&provider) {
        return Err(WorkModelError::BadRequest);
    }
    let secret = LeadSecret::new(secret)?;
    let verdict = match check_target(provider) {
        Ok(target) => lead::check_key(&target, &secret).await,
        Err(_) => LeadKeyCheck::Unreachable,
    };
    if verdict == LeadKeyCheck::Invalid {
        return Ok(WorkKeyStatus::Invalid);
    }
    blocking(move || keys::store(provider, secret))
        .await
        .ok_or(WorkModelError::Unauthorized)?
        .map_err(|_| WorkModelError::Unauthorized)?;
    let status = if verdict == LeadKeyCheck::Valid {
        WorkKeyStatus::Valid
    } else {
        WorkKeyStatus::Set
    };
    {
        let mut state = state();
        state.presence.insert(provider, true);
        state.listed.remove(&provider);
    }
    record_verdict(provider, status);
    notify();
    Ok(status)
}

/// Checks the stored key again.
pub async fn test_key(provider: WorkModelProvider) -> Result<WorkKeyStatus, WorkModelError> {
    let secret = match blocking(move || keys::load(provider)).await {
        Some(Ok(secret)) => secret,
        Some(Err(keys::LeadKeyError::Missing)) => return Ok(WorkKeyStatus::Missing),
        _ => return Err(WorkModelError::Unauthorized),
    };
    let status = match lead::check_key(&check_target(provider)?, &secret).await {
        LeadKeyCheck::Valid => WorkKeyStatus::Valid,
        LeadKeyCheck::Invalid => WorkKeyStatus::Invalid,
        LeadKeyCheck::Unreachable => return Err(WorkModelError::Network),
    };
    record_verdict(provider, status);
    Ok(status)
}

/// Removes the key; roles that ran on it fall back to another provider.
pub async fn clear_key(provider: WorkModelProvider) -> Result<(), WorkModelError> {
    if !KEYED_PROVIDERS.contains(&provider) {
        return Err(WorkModelError::BadRequest);
    }
    blocking(move || keys::clear(provider))
        .await
        .ok_or(WorkModelError::Unauthorized)?
        .map_err(|_| WorkModelError::Unauthorized)?;
    {
        let mut state = state();
        state.presence.insert(provider, false);
        state.verdicts.remove(&provider);
        state.listed.remove(&provider);
    }
    store_setting(key_setting(provider), String::new());
    notify();
    Ok(())
}

/// Sets or clears the OpenAI-compatible endpoint's base URL.
pub fn set_compatible_base(base: Option<&str>) -> Result<(), WorkModelError> {
    let value = match base.map(str::trim).filter(|base| !base.is_empty()) {
        Some(base) => {
            LeadTarget::compatible(base)?;
            base.trim_end_matches('/').to_owned()
        }
        None => String::new(),
    };
    state().listed.remove(&P::Compatible);
    if !store_setting(COMPATIBLE_BASE_KEY.to_owned(), value) {
        return Err(WorkModelError::Unauthorized);
    }
    notify();
    Ok(())
}

/// The provider's own model list for "More models", kept for this session.
pub async fn more_models(
    provider: WorkModelProvider,
) -> Result<Vec<WorkModelEntry>, WorkModelError> {
    if let Some(listed) = state().listed.get(&provider) {
        return Ok(listed.clone());
    }
    let target = check_target(provider)?;
    let secret: Arc<LeadSecret> = if provider == P::Compatible {
        CompatibleCredential.secret().await?
    } else {
        match blocking(move || keys::load(provider)).await {
            Some(Ok(secret)) => secret,
            Some(Err(keys::LeadKeyError::Missing)) => return Err(WorkModelError::MissingKey),
            _ => return Err(WorkModelError::Unauthorized),
        }
    };
    let listed = lead::list_models(&target, &secret).await;
    if listed == Err(WorkModelError::Unauthorized) && provider != P::Compatible {
        record_verdict(provider, WorkKeyStatus::Invalid);
    }
    let listed = listed?;
    state().listed.insert(provider, listed.clone());
    notify();
    Ok(listed)
}

#[doc(hidden)]
pub fn cloud_session_for_tests(session: Option<Arc<dyn WorkCloudSession>>) {
    if let Ok(mut slot) = CLOUD.write() {
        *slot = session;
    }
}

#[doc(hidden)]
pub fn static_credential(secret: LeadSecret) -> Arc<dyn LeadCredential> {
    Arc::new(LeadStaticCredential::new(secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Memory(Mutex<HashMap<String, String>>);

    impl WorkModelSettings for Memory {
        fn get(&self, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn set(&self, key: String, value: String) -> bool {
            self.0.lock().unwrap().insert(key, value);
            true
        }
    }

    fn with_keys(present: &[WorkModelProvider]) {
        let mut state = state();
        *state = State::default();
        for provider in KEYED_PROVIDERS {
            state.presence.insert(provider, present.contains(&provider));
        }
    }

    #[test]
    fn roles_follow_the_choice_then_the_lead_family_then_any_key() {
        install_settings(Arc::new(Memory(Mutex::new(HashMap::new()))));
        let profile = ProfileId::from(41_u128);
        with_keys(&[P::OpenAi]);
        let ids = |role| effective(&state(), profile, role).map(|entry| entry.id);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("openai/gpt-6-sol")
        );
        assert_eq!(
            ids(WorkModelRole::Page).as_deref(),
            Some("openai/gpt-6-luna")
        );

        with_keys(&[P::OpenAi, P::Anthropic]);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("anthropic/claude-opus-5-5")
        );
        choose(profile, WorkModelRole::Lead, Some("openai/gpt-6-sol")).unwrap();
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("openai/gpt-6-sol")
        );
        assert_eq!(
            ids(WorkModelRole::Light).as_deref(),
            Some("openai/gpt-6-luna")
        );
        choose(
            profile,
            WorkModelRole::Page,
            Some("anthropic/claude-sonnet-5-5"),
        )
        .unwrap();
        assert_eq!(
            ids(WorkModelRole::Page).as_deref(),
            Some("anthropic/claude-sonnet-5-5")
        );

        // A choice whose key went away falls back instead of failing the run.
        with_keys(&[P::Anthropic]);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("anthropic/claude-opus-5-5")
        );
        state()
            .verdicts
            .insert(P::Anthropic, WorkKeyStatus::Invalid);
        assert_eq!(ids(WorkModelRole::Lead), None);

        assert!(choose(
            profile,
            WorkModelRole::Light,
            Some("anthropic/claude-opus-5-5")
        )
        .is_err());
        assert!(choose(profile, WorkModelRole::Lead, Some("nobody/nothing")).is_err());
        choose(profile, WorkModelRole::Lead, None).unwrap();
        with_keys(&[P::DeepSeek]);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("deepseek/deepseek-v4-pro")
        );
        // DeepSeek reads no images: the page role finds no vision model.
        assert_eq!(ids(WorkModelRole::Page), None);
    }
}
