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
const FALLBACK_ORDER: [WorkModelProvider; 6] = KEYED_PROVIDERS;
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

/// Credential acceptance and service availability are independent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkKeyFault {
    WrongKey,
    Billing,
    RateLimited,
    ProviderDown,
    Offline,
    Keychain,
    Request,
}

fn failure(error: WorkModelError) -> Option<WorkKeyFault> {
    Some(match error {
        WorkModelError::Unauthorized => WorkKeyFault::WrongKey,
        WorkModelError::OverBudget => WorkKeyFault::Billing,
        WorkModelError::RateLimited { .. } => WorkKeyFault::RateLimited,
        WorkModelError::Overloaded => WorkKeyFault::ProviderDown,
        WorkModelError::Network => WorkKeyFault::Offline,
        WorkModelError::BadRequest | WorkModelError::Protocol => WorkKeyFault::Request,
        _ => return None,
    })
}

fn record_observation(
    provider: P,
    generation: u64,
    status: Option<WorkKeyStatus>,
    fault: Option<WorkKeyFault>,
) {
    let changed = {
        let mut state = state();
        if state.generations.get(&provider).copied().unwrap_or(0) != generation {
            return;
        }
        let mut changed = match fault {
            Some(fault) => state.faults.insert(provider, fault) != Some(fault),
            None => state.faults.remove(&provider).is_some(),
        };
        if let Some(status) = status {
            changed |= state.verdicts.insert(provider, status) != Some(status);
            store_setting(key_setting(provider), status.verdict().to_owned());
        }
        changed
    };
    if changed {
        notify();
    }
}

fn record_failure(provider: P, generation: u64, fault: Option<WorkKeyFault>) {
    record_observation(
        provider,
        generation,
        (fault == Some(WorkKeyFault::WrongKey)).then_some(WorkKeyStatus::Invalid),
        fault,
    );
}

pub fn provider_failure(provider: P) -> Option<WorkKeyFault> {
    state().faults.get(&provider).copied()
}

fn generation(provider: P) -> u64 {
    state().generations.get(&provider).copied().unwrap_or(0)
}

fn vault() -> Arc<dyn keys::KeyVault> {
    #[cfg(not(test))]
    {
        Arc::new(keys::SystemVault)
    }
    #[cfg(test)]
    {
        static MEMORY: OnceLock<Arc<tests::MemoryVault>> = OnceLock::new();
        MEMORY.get_or_init(Default::default).clone()
    }
}

fn mutations() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}

#[derive(Default)]
struct State {
    faults: HashMap<P, WorkKeyFault>,
    generations: HashMap<P, u64>,
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
            .map(|provider| (provider, vault().present(provider).unwrap_or(false)))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let mut state = state();
    for (provider, present) in found {
        state.presence.entry(provider).or_insert(present);
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

fn compatible_base() -> Option<String> {
    setting(COMPATIBLE_BASE_KEY)
}

/// Whether calls to `provider` can be made now.
fn usable(state: &State, provider: WorkModelProvider) -> bool {
    match provider {
        P::Cloud => false,
        P::Compatible => {
            compatible_base().is_some() && key_status(state, provider) != WorkKeyStatus::Invalid
        }
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

/// Curated BYOK entries and custom endpoint models. Cloud stays dormant.
fn entries(state: &State) -> Vec<WorkModelEntry> {
    let mut all = Vec::new();
    all.extend(models::builtin().into_iter().filter(|entry| {
        state
            .listed
            .get(&entry.model.provider)
            .is_none_or(|listed| listed.iter().any(|item| item.id == entry.id))
    }));
    for provider in [P::Compatible] {
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
    // Keep an explicit choice even if it disappeared; never silently replace it.
    Some(
        entries(state)
            .into_iter()
            .chain(state.listed.values().flatten().cloned())
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
        return None;
    }
    let offered = |entry: &&WorkModelEntry| {
        entry.model.provider == provider
            && state
                .listed
                .get(&provider)
                .is_none_or(|listed| listed.iter().any(|item| item.id == entry.id))
    };
    let catalog = models::builtin();
    let preferred = models::family_default(provider, role);
    catalog
        .iter()
        .filter(offered)
        .find(|entry| Some(entry.model.model.as_str()) == preferred)
        .or_else(|| {
            catalog
                .iter()
                .filter(offered)
                .find(|entry| entry.roles.contains(&role))
        })
        .or_else(|| {
            catalog
                .iter()
                .filter(offered)
                .find(|entry| entry.roles.contains(&WorkModelRole::Lead))
        })
        .cloned()
        .or_else(|| {
            state
                .listed
                .get(&provider)?
                .iter()
                .find(|entry| {
                    entry.roles.contains(&role) || entry.roles.contains(&WorkModelRole::Lead)
                })
                .cloned()
        })
}

/// The model a role runs with: the person's choice when its provider is
/// usable, else the lead's provider family, else the first keyed provider.
fn effective(state: &State, profile: ProfileId, role: WorkModelRole) -> Option<WorkModelEntry> {
    if let Some(choice) = stored_choice(state, profile, role) {
        let exists = state
            .listed
            .get(&choice.model.provider)
            .is_none_or(|entries| entries.iter().any(|entry| entry.id == choice.id));
        return (usable(state, choice.model.provider) && exists).then_some(choice);
    }
    if role != WorkModelRole::Lead {
        if let Some(lead) = effective(state, profile, WorkModelRole::Lead) {
            return family_entry(state, lead.model.provider, role).or(Some(lead));
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
    generation: u64,
}

impl WorkModelClient for Observed {
    fn call<'a>(
        &'a self,
        request: WorkModelRequest,
        events: &'a (dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> WorkModelFuture<'a> {
        Box::pin(async move {
            let result = self.inner.call(request, events).await;
            if KEYED_PROVIDERS.contains(&self.provider)
                && generation(self.provider) == self.generation
            {
                match &result {
                    Ok(_) => {
                        record_observation(
                            self.provider,
                            self.generation,
                            Some(WorkKeyStatus::Valid),
                            None,
                        );
                    }
                    Err(error) => {
                        if let Some(fault) = failure(*error) {
                            record_failure(self.provider, self.generation, Some(fault));
                        }
                    }
                }
            }
            result
        })
    }
}

struct VaultCredential {
    provider: P,
    generation: u64,
}
impl LeadCredential for VaultCredential {
    fn secret(&self) -> LeadSecretFuture<'_> {
        Box::pin(async move {
            let provider = self.provider;
            match blocking(move || vault().load(provider)).await {
                Some(Ok(secret)) => Ok(secret),
                Some(Err(keys::LeadKeyError::Missing)) => Err(WorkModelError::MissingKey),
                _ => {
                    record_failure(provider, self.generation, Some(WorkKeyFault::Keychain));
                    // A vault refusal is not a provider authentication failure.
                    Err(WorkModelError::MissingKey)
                }
            }
        })
    }
    fn rejected(&self) {}
}

/// A local endpoint may need no key; it then gets a placeholder bearer.
struct CompatibleCredential {
    generation: u64,
}

impl LeadCredential for CompatibleCredential {
    fn secret(&self) -> LeadSecretFuture<'_> {
        Box::pin(async move {
            let stored = blocking(|| vault().load(P::Compatible)).await;
            match stored {
                Some(Ok(secret)) => Ok(secret),
                Some(Err(keys::LeadKeyError::Missing)) => {
                    LeadSecret::new("zephium".into()).map(Arc::new)
                }
                _ => {
                    record_failure(P::Compatible, self.generation, Some(WorkKeyFault::Keychain));
                    Err(WorkModelError::MissingKey)
                }
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
            Arc::new(CompatibleCredential {
                generation: state.generations.get(&provider).copied().unwrap_or(0),
            }),
        ),
        provider => (
            LeadTarget::direct(provider)?,
            Arc::new(VaultCredential {
                provider,
                generation: state.generations.get(&provider).copied().unwrap_or(0),
            }),
        ),
    };
    if target.wire() != entry.model.wire {
        return Err(WorkModelError::BadRequest);
    }
    Ok(Arc::new(Observed {
        inner: LeadClient::new(target, credential, entry.price.clone()),
        provider,
        generation: state.generations.get(&provider).copied().unwrap_or(0),
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
    pub fault: Option<WorkKeyFault>,
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
            fault: state.faults.get(&provider).copied(),
            base: (provider == P::Compatible).then(compatible_base).flatten(),
        })
        .collect();
    WorkModelsView {
        entries,
        chosen,
        effective,
        providers,
        cloud_signed_in: false,
        cloud_plan: None,
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
                .chain(state.listed.values().flatten().cloned())
                .find(|entry| entry.id == id)
                .ok_or(WorkModelError::BadRequest)?;
            if !entry.roles.contains(&role) && !entry.roles.contains(&WorkModelRole::Lead) {
                return Err(WorkModelError::BadRequest);
            }
            serde_json::to_string(&entry).map_err(|_| WorkModelError::BadRequest)?
        }
    };
    if SETTINGS.read().ok().is_some_and(|slot| slot.is_some())
        && !store_setting(key.clone(), value.clone())
    {
        return Err(WorkModelError::Unauthorized);
    }
    {
        let mut state = state();
        if value.is_empty() {
            state.choices.remove(&key);
        } else if let Ok(entry) = serde_json::from_str(&value) {
            state.choices.insert(key.clone(), entry);
        }
    }
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

fn apply_check(provider: P, check: LeadKeyCheck) -> WorkKeyStatus {
    let (status, fault) = match check {
        LeadKeyCheck::Valid(entries) => {
            if provider != P::OpenRouter {
                state().listed.insert(provider, entries);
            }
            (WorkKeyStatus::Valid, None)
        }
        LeadKeyCheck::Invalid => (WorkKeyStatus::Invalid, Some(WorkKeyFault::WrongKey)),
        LeadKeyCheck::Billing => (key_status(&state(), provider), Some(WorkKeyFault::Billing)),
        LeadKeyCheck::RateLimited => (
            key_status(&state(), provider),
            Some(WorkKeyFault::RateLimited),
        ),
        LeadKeyCheck::ProviderDown => (
            key_status(&state(), provider),
            Some(WorkKeyFault::ProviderDown),
        ),
        LeadKeyCheck::Unreachable => (key_status(&state(), provider), Some(WorkKeyFault::Offline)),
        LeadKeyCheck::Failed => (key_status(&state(), provider), Some(WorkKeyFault::Request)),
    };
    record_observation(provider, generation(provider), Some(status), fault);
    status
}

/// Check without paid inference. Store even a refused key so its row stays truthful.
pub async fn set_key(provider: P, secret: String) -> Result<WorkKeyStatus, WorkModelError> {
    let _guard = mutations().lock().await;
    if !KEYED_PROVIDERS.contains(&provider) {
        return Err(WorkModelError::BadRequest);
    }
    let target = check_target(provider)?;
    let secret = LeadSecret::new(secret)?;
    let verdict = lead::check_key(&target, &secret).await;
    blocking(move || vault().store(provider, secret))
        .await
        .ok_or(WorkModelError::Unauthorized)?
        .map_err(|_| WorkModelError::Unauthorized)?;
    {
        let mut state = state();
        *state.generations.entry(provider).or_default() += 1;
        state.presence.insert(provider, true);
        state.verdicts.insert(provider, WorkKeyStatus::Set);
        state.listed.remove(&provider);
    }
    let status = apply_check(provider, verdict);
    notify();
    Ok(status)
}

pub async fn test_key(provider: P) -> Result<WorkKeyStatus, WorkModelError> {
    let _guard = mutations().lock().await;
    let secret = match blocking(move || vault().load(provider)).await {
        Some(Ok(secret)) => secret,
        Some(Err(keys::LeadKeyError::Missing)) => return Ok(WorkKeyStatus::Missing),
        _ => {
            record_failure(provider, generation(provider), Some(WorkKeyFault::Keychain));
            return Err(WorkModelError::Unauthorized);
        }
    };
    Ok(apply_check(
        provider,
        lead::check_key(&check_target(provider)?, &secret).await,
    ))
}

/// Persist defaults only into empty roles after the first accepted key in this profile.
pub fn fill_defaults(profile: ProfileId, provider: P) {
    if key_status(&state(), provider) != WorkKeyStatus::Valid {
        return;
    }
    for role in ROLES {
        let entry = {
            let state = state();
            if stored_choice(&state, profile, role).is_some() {
                continue;
            }
            family_entry(&state, provider, role)
        };
        if let Some(entry) = entry {
            let _ = choose(profile, role, Some(&entry.id));
        }
    }
}

/// Lead availability alone gates Work; page and light can fall back to the lead.
pub async fn ready(profile: ProfileId) -> bool {
    ensure_presence().await;
    effective(&state(), profile, WorkModelRole::Lead).is_some()
}

/// Removes the key, preserving explicit role choices for the person to replace.
pub async fn clear_key(provider: WorkModelProvider) -> Result<(), WorkModelError> {
    let _guard = mutations().lock().await;
    if !KEYED_PROVIDERS.contains(&provider) {
        return Err(WorkModelError::BadRequest);
    }
    blocking(move || vault().clear(provider))
        .await
        .ok_or(WorkModelError::Unauthorized)?
        .map_err(|_| WorkModelError::Unauthorized)?;
    {
        let mut state = state();
        *state.generations.entry(provider).or_default() += 1;
        state.faults.remove(&provider);
        state.presence.insert(provider, false);
        state.verdicts.remove(&provider);
        state.listed.remove(&provider);
    }
    store_setting(key_setting(provider), String::new());
    notify();
    Ok(())
}

/// Sets or clears the OpenAI-compatible endpoint's base URL.
pub async fn set_compatible_base(base: Option<&str>) -> Result<(), WorkModelError> {
    let _guard = mutations().lock().await;
    let value = match base.map(str::trim).filter(|base| !base.is_empty()) {
        Some(base) => {
            LeadTarget::compatible(base)?;
            base.trim_end_matches('/').to_owned()
        }
        None => String::new(),
    };
    if compatible_base().as_deref().unwrap_or_default() != value {
        blocking(|| vault().clear(P::Compatible))
            .await
            .ok_or(WorkModelError::Unauthorized)?
            .map_err(|_| WorkModelError::Unauthorized)?;
        let mut state = state();
        *state.generations.entry(P::Compatible).or_default() += 1;
        state.presence.insert(P::Compatible, false);
        state.verdicts.remove(&P::Compatible);
        state.faults.remove(&P::Compatible);
        state.listed.remove(&P::Compatible);
    }
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
    let _guard = mutations().lock().await;
    let current_generation = generation(provider);
    if let Some(listed) = state().listed.get(&provider) {
        return Ok(listed.clone());
    }
    let target = check_target(provider)?;
    let secret: Arc<LeadSecret> = if provider == P::Compatible {
        CompatibleCredential {
            generation: current_generation,
        }
        .secret()
        .await?
    } else {
        match blocking(move || vault().load(provider)).await {
            Some(Ok(secret)) => secret,
            Some(Err(keys::LeadKeyError::Missing)) => return Err(WorkModelError::MissingKey),
            _ => return Err(WorkModelError::Unauthorized),
        }
    };
    let listed = lead::list_models(&target, &secret).await;
    if let Err(error) = &listed {
        if let Some(fault) = failure(*error) {
            record_failure(provider, current_generation, Some(fault));
        }
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

    #[derive(Default)]
    pub(super) struct MemoryVault(Mutex<HashMap<P, Arc<LeadSecret>>>);
    impl keys::KeyVault for MemoryVault {
        fn load(&self, provider: P) -> Result<Arc<LeadSecret>, keys::LeadKeyError> {
            self.0
                .lock()
                .unwrap()
                .get(&provider)
                .cloned()
                .ok_or(keys::LeadKeyError::Missing)
        }
        fn store(&self, provider: P, secret: LeadSecret) -> Result<(), keys::LeadKeyError> {
            self.0.lock().unwrap().insert(provider, Arc::new(secret));
            Ok(())
        }
        fn clear(&self, provider: P) -> Result<(), keys::LeadKeyError> {
            self.0.lock().unwrap().remove(&provider);
            Ok(())
        }
        fn present(&self, provider: P) -> Result<bool, keys::LeadKeyError> {
            Ok(self.0.lock().unwrap().contains_key(&provider))
        }
    }

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

    #[tokio::test]
    async fn defaults_choices_failures_and_stale_results_stay_truthful() {
        install_settings(Arc::new(Memory(Mutex::new(HashMap::new()))));
        let profile = ProfileId::from(41_u128);
        with_keys(&[P::OpenAi]);
        apply_check(
            P::OpenAi,
            LeadKeyCheck::Valid(
                models::builtin()
                    .into_iter()
                    .filter(|e| e.model.provider == P::OpenAi)
                    .collect(),
            ),
        );
        fill_defaults(profile, P::OpenAi);
        let ids = |role| effective(&state(), profile, role).map(|entry| entry.id);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("openai/gpt-6-astra")
        );
        assert_eq!(
            ids(WorkModelRole::Page).as_deref(),
            Some("openai/gpt-6-luna")
        );
        state().presence.insert(P::Anthropic, true);
        apply_check(
            P::Anthropic,
            LeadKeyCheck::Valid(
                models::builtin()
                    .into_iter()
                    .filter(|e| e.model.provider == P::Anthropic)
                    .collect(),
            ),
        );
        fill_defaults(profile, P::Anthropic);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("openai/gpt-6-astra")
        );
        choose(
            profile,
            WorkModelRole::Lead,
            Some("anthropic/claude-sonnet-5-5"),
        )
        .unwrap();
        fill_defaults(profile, P::OpenAi);
        assert_eq!(
            ids(WorkModelRole::Lead).as_deref(),
            Some("anthropic/claude-sonnet-5-5")
        );
        assert!(ready(profile).await);
        for (check, fault) in [
            (LeadKeyCheck::Billing, WorkKeyFault::Billing),
            (LeadKeyCheck::RateLimited, WorkKeyFault::RateLimited),
            (LeadKeyCheck::ProviderDown, WorkKeyFault::ProviderDown),
            (LeadKeyCheck::Unreachable, WorkKeyFault::Offline),
        ] {
            assert_eq!(apply_check(P::Anthropic, check), WorkKeyStatus::Valid);
            assert_eq!(provider_failure(P::Anthropic), Some(fault));
        }
        apply_check(P::Anthropic, LeadKeyCheck::Invalid);
        assert!(!ready(profile).await);
        assert_eq!(
            stored_choice(&state(), profile, WorkModelRole::Lead)
                .unwrap()
                .id,
            "anthropic/claude-sonnet-5-5"
        );
        // An in-flight response using the previous credential cannot invalidate its replacement.
        state().generations.insert(P::Anthropic, 1);
        record_observation(P::Anthropic, 1, Some(WorkKeyStatus::Valid), None);
        record_failure(P::Anthropic, 0, Some(WorkKeyFault::WrongKey));
        assert!(ready(profile).await);
        assert_eq!(provider_failure(P::Anthropic), None);
        assert!(choose(profile, WorkModelRole::Decision, Some("openai/gpt-6-luna")).is_err());
        let other = ProfileId::from(42_u128);
        with_keys(&[P::DeepSeek]);
        assert_eq!(
            effective(&state(), other, WorkModelRole::Page)
                .unwrap()
                .model
                .model,
            "deepseek-v4-pro"
        );
        assert!(ready(other).await);
        with_keys(&[P::OpenRouter]);
        assert!(ready(other).await);
        with_keys(&[]);
        assert!(!ready(other).await);
        // All test secret operations use the injected memory vault.
        vault()
            .store(P::OpenAi, LeadSecret::new("test-key".into()).unwrap())
            .unwrap();
        assert!(vault().present(P::OpenAi).unwrap());
        vault().clear(P::OpenAi).unwrap();
        assert!(!vault().present(P::OpenAi).unwrap());
    }
}
