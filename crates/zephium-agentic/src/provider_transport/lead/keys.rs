//! One login-Keychain generic password per provider. Secrets are read only to
//! authenticate a call; presence is answered from attributes alone.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zephium_core::work::model::{WorkModelError, WorkModelProvider};

use super::{LeadCredential, LeadSecret, LeadSecretFuture};

/// Shared with the development OpenAI credential and every older loader.
const ACCOUNT: &str = "development";

/// The Keychain service holding `provider`'s key, if it takes one.
pub fn service(provider: WorkModelProvider) -> Option<&'static str> {
    Some(match provider {
        WorkModelProvider::OpenAi => "app.zephium.agent-provider.openai",
        WorkModelProvider::Anthropic => "app.zephium.agent-provider.anthropic",
        WorkModelProvider::Google => "app.zephium.agent-provider.google",
        WorkModelProvider::DeepSeek => "app.zephium.agent-provider.deepseek",
        WorkModelProvider::OpenRouter => "app.zephium.agent-provider.openrouter",
        WorkModelProvider::Compatible => "app.zephium.agent-provider.compatible",
        WorkModelProvider::Cloud => return None,
    })
}

/// Content-free Keychain failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeadKeyError {
    /// No item for this provider.
    Missing,
    /// The Keychain refused or is unavailable.
    Inaccessible,
    /// The value is not a usable key.
    Invalid,
}

fn cache() -> &'static Mutex<HashMap<WorkModelProvider, Arc<LeadSecret>>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<WorkModelProvider, Arc<LeadSecret>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn forget(provider: WorkModelProvider) {
    if let Ok(mut cache) = cache().lock() {
        cache.remove(&provider);
    }
}

fn cached(provider: WorkModelProvider) -> Option<Arc<LeadSecret>> {
    cache().lock().ok()?.get(&provider).cloned()
}

/// Loads `provider`'s key. Blocking; call off the async executor.
pub fn load(provider: WorkModelProvider) -> Result<Arc<LeadSecret>, LeadKeyError> {
    if let Some(secret) = cached(provider) {
        return Ok(secret);
    }
    let service = service(provider).ok_or(LeadKeyError::Missing)?;
    let secret = Arc::new(platform::read(service)?);
    if let Ok(mut cache) = cache().lock() {
        cache.insert(provider, secret.clone());
    }
    Ok(secret)
}

/// Stores `secret` as `provider`'s key, replacing any earlier one. Blocking.
pub fn store(provider: WorkModelProvider, secret: LeadSecret) -> Result<(), LeadKeyError> {
    let service = service(provider).ok_or(LeadKeyError::Invalid)?;
    forget(provider);
    platform::write(service, &secret)?;
    if let Ok(mut cache) = cache().lock() {
        cache.insert(provider, Arc::new(secret));
    }
    Ok(())
}

/// Removes `provider`'s key. Removing a missing key succeeds. Blocking.
pub fn clear(provider: WorkModelProvider) -> Result<(), LeadKeyError> {
    let service = service(provider).ok_or(LeadKeyError::Invalid)?;
    forget(provider);
    platform::delete(service)
}

/// Whether an item exists, from its attributes only (never its secret). Blocking.
pub fn present(provider: WorkModelProvider) -> Result<bool, LeadKeyError> {
    if cached(provider).is_some() {
        return Ok(true);
    }
    let service = service(provider).ok_or(LeadKeyError::Missing)?;
    platform::present(service)
}

/// Reads the provider's Keychain key once per process, again after a refusal.
pub struct KeychainCredential(WorkModelProvider);

impl KeychainCredential {
    /// The key source for `provider`.
    pub fn new(provider: WorkModelProvider) -> Self {
        Self(provider)
    }
}

impl LeadCredential for KeychainCredential {
    fn secret(&self) -> LeadSecretFuture<'_> {
        let provider = self.0;
        Box::pin(async move {
            if let Some(secret) = cached(provider) {
                return Ok(secret);
            }
            match tokio::task::spawn_blocking(move || load(provider)).await {
                Ok(Ok(secret)) => Ok(secret),
                Ok(Err(LeadKeyError::Missing | LeadKeyError::Invalid)) => {
                    Err(WorkModelError::MissingKey)
                }
                Ok(Err(LeadKeyError::Inaccessible)) | Err(_) => Err(WorkModelError::Unauthorized),
            }
        })
    }

    fn rejected(&self) {
        forget(self.0);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use security_framework::item::{ItemClass, ItemSearchOptions};
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::os::macos::passwords::find_generic_password;
    use security_framework_sys::base::errSecItemNotFound;
    use zeroize::Zeroizing;

    use super::{LeadKeyError, LeadSecret, ACCOUNT};

    fn login() -> Result<SecKeychain, LeadKeyError> {
        let path = dirs::home_dir()
            .ok_or(LeadKeyError::Inaccessible)?
            .join("Library/Keychains/login.keychain-db");
        SecKeychain::open(path).map_err(|_| LeadKeyError::Inaccessible)
    }

    fn search(service: &str, keychain: &SecKeychain) -> ItemSearchOptions {
        let mut options = ItemSearchOptions::new();
        options
            .class(ItemClass::generic_password())
            .keychains(std::slice::from_ref(keychain))
            .service(service)
            .account(ACCOUNT);
        options
    }

    pub(super) fn read(service: &str) -> Result<LeadSecret, LeadKeyError> {
        let keychain = login()?;
        let (password, _item) = find_generic_password(Some(&[keychain]), service, ACCOUNT)
            .map_err(|error| {
                if error.code() == errSecItemNotFound {
                    LeadKeyError::Missing
                } else {
                    LeadKeyError::Inaccessible
                }
            })?;
        let bytes = Zeroizing::new(password.as_ref().to_vec());
        let text = std::str::from_utf8(&bytes).map_err(|_| LeadKeyError::Invalid)?;
        LeadSecret::new(text.to_owned()).map_err(|_| LeadKeyError::Invalid)
    }

    pub(super) fn write(service: &str, secret: &LeadSecret) -> Result<(), LeadKeyError> {
        let keychain = login()?;
        // Replace by delete + add: updating in place would first read the
        // old secret, which prompts when another binary created the item.
        let _ = search(service, &keychain).delete();
        keychain
            .add_generic_password(service, ACCOUNT, secret.expose().as_bytes())
            .or_else(|_| {
                keychain.set_generic_password(service, ACCOUNT, secret.expose().as_bytes())
            })
            .map_err(|_| LeadKeyError::Inaccessible)
    }

    pub(super) fn delete(service: &str) -> Result<(), LeadKeyError> {
        let keychain = login()?;
        match search(service, &keychain).delete() {
            Ok(()) => Ok(()),
            Err(error) if error.code() == errSecItemNotFound => Ok(()),
            Err(_) => Err(LeadKeyError::Inaccessible),
        }
    }

    pub(super) fn present(service: &str) -> Result<bool, LeadKeyError> {
        let keychain = login()?;
        let mut options = search(service, &keychain);
        options.load_attributes(true);
        match options.search() {
            Ok(items) => Ok(!items.is_empty()),
            Err(error) if error.code() == errSecItemNotFound => Ok(false),
            Err(_) => Err(LeadKeyError::Inaccessible),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{LeadKeyError, LeadSecret};

    pub(super) fn read(_: &str) -> Result<LeadSecret, LeadKeyError> {
        Err(LeadKeyError::Missing)
    }
    pub(super) fn write(_: &str, _: &LeadSecret) -> Result<(), LeadKeyError> {
        Err(LeadKeyError::Inaccessible)
    }
    pub(super) fn delete(_: &str) -> Result<(), LeadKeyError> {
        Ok(())
    }
    pub(super) fn present(_: &str) -> Result<bool, LeadKeyError> {
        Ok(false)
    }
}
