//! Per-profile typed-decision preference. The stored choice is trusted host
//! state; the Keychain is only ever asked whether its fixed TypeSafe item
//! exists, through the one existing loader, and never for a settings read more
//! than once per process.
use super::*;
use zephium_core::ids::ProfileId;
use zephium_core::work::WorkError;
use zephium_ipc::work::{
    WorkDecisionChoiceV1, WorkDecisionPreferenceChangedV1, WorkDecisionPreferenceV1,
};

const SETTING_PREFIX: &str = "work.decisions.";

fn log(arguments: std::fmt::Arguments<'_>) {
    #[cfg(feature = "work-product")]
    super::work_provider::record_diagnostic(arguments);
    #[cfg(not(feature = "work-product"))]
    super::write_diagnostic(arguments);
}

fn setting_key(profile: ProfileId) -> String {
    format!("{SETTING_PREFIX}{profile}")
}

/// 0 unobserved, 1 absent, 2 present. Process-wide: the item is fixed, not
/// per-profile, so one observation serves every profile.
static TYPESAFE_PRESENCE: AtomicU8 = AtomicU8::new(0);

type Observer = Box<dyn Fn(WorkDecisionPreferenceChangedV1) + Send + Sync>;
static OBSERVER: OnceLock<Observer> = OnceLock::new();

pub(crate) fn set_observer(observer: Observer) {
    let _ = OBSERVER.set(observer);
}

fn notify(profile: ProfileId) {
    if let Some(observer) = OBSERVER.get() {
        observer(WorkDecisionPreferenceChangedV1 {
            profile: profile.to_string(),
        });
    }
}

/// Records what the existing loader just saw. Only a change from an already
/// observed state is an invalidation; the first observation is not.
fn record_presence(profile: ProfileId, present: bool) {
    let next = if present { 2 } else { 1 };
    let previous = TYPESAFE_PRESENCE.swap(next, Ordering::AcqRel);
    if previous != 0 && previous != next {
        notify(profile);
    }
}

#[cfg(all(feature = "work-product", target_os = "macos"))]
fn load_presence() -> bool {
    // Exactly what a run treats as usable: anything else already falls back.
    zephium_agentic::load_macos_development_typesafe_credential().is_ok()
}

#[cfg(not(all(feature = "work-product", target_os = "macos")))]
fn load_presence() -> bool {
    false
}

/// The presence the TypeSafe key had when it was last loaded. A settings read
/// observes it once per process; every later answer comes from what provider
/// construction saw.
async fn typesafe_key_present(profile: ProfileId) -> bool {
    match TYPESAFE_PRESENCE.load(Ordering::Acquire) {
        2 => return true,
        1 => return false,
        _ => {}
    }
    let Ok(present) = tokio::task::spawn_blocking(load_presence).await else {
        return false;
    };
    record_presence(profile, present);
    present
}

/// Observation from the one site that already loads the key for a real run.
#[cfg(all(feature = "work-product", target_os = "macos"))]
pub(crate) fn observe_typesafe_key(profile: ProfileId, present: bool) {
    record_presence(profile, present);
}

async fn read_choice(profile: ProfileId) -> WorkDecisionChoiceV1 {
    let key = setting_key(profile);
    tokio::task::spawn_blocking(move || {
        use zephium_core::ports::store::Store;
        APP_STORE
            .get()
            .and_then(|store| store.app_setting(&key))
            .as_deref()
            .and_then(WorkDecisionChoiceV1::parse)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default()
}

async fn write_choice(profile: ProfileId, choice: WorkDecisionChoiceV1) -> Result<(), WorkError> {
    let key = setting_key(profile);
    let stored = tokio::task::spawn_blocking(move || {
        use zephium_core::ports::store::Store;
        APP_STORE
            .get()
            .is_some_and(|store| store.set_app_setting(key, choice.as_str().to_owned()))
    })
    .await
    .map_err(|_| WorkError::Unavailable)?;
    if stored {
        Ok(())
    } else {
        Err(WorkError::Capacity)
    }
}

/// The preference the next read will run under; a running read keeps the
/// configuration it was constructed with.
#[cfg(all(feature = "work-product", target_os = "macos"))]
pub(crate) async fn selected_choice(profile: ProfileId) -> WorkDecisionChoiceV1 {
    read_choice(profile).await
}

#[cfg(all(feature = "work-product", target_os = "macos"))]
pub(crate) fn composition_preference(
    choice: WorkDecisionChoiceV1,
) -> zephium_work_composition::durable_runtime::WorkDecisionPreference {
    use zephium_work_composition::durable_runtime::WorkDecisionPreference;
    match choice {
        WorkDecisionChoiceV1::Recommended => WorkDecisionPreference::Recommended,
        WorkDecisionChoiceV1::Standard => WorkDecisionPreference::Emulation,
        WorkDecisionChoiceV1::Off => WorkDecisionPreference::Disabled,
    }
}

/// The actor selects the Work profile; a caller-supplied id may only confirm it.
async fn confirm_profile(app: &tauri::AppHandle, expected: &str) -> Result<ProfileId, WorkError> {
    if shutdown_started(app) {
        return Err(WorkError::Shutdown);
    }
    let profile = ProfileId::parse(expected)
        .filter(|id| id.to_string() == expected)
        .ok_or(WorkError::Invalid)?;
    #[cfg(all(feature = "work-product", target_os = "macos"))]
    {
        let request = app.state::<zephium_app::Handle>().work_profile_binding();
        let readiness = tokio::time::timeout(std::time::Duration::from_secs(4), async move {
            loop {
                if let Some(readiness) = request.try_recv() {
                    break readiness;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| WorkError::Unavailable)?;
        let bound = match readiness {
            zephium_app::AgentWorkProfileReadiness::Ready(binding)
            | zephium_app::AgentWorkProfileReadiness::PolicyPending(binding) => binding.profile(),
            _ => return Err(WorkError::ProfileUnavailable),
        };
        if bound != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        Ok(profile)
    }
    #[cfg(not(all(feature = "work-product", target_os = "macos")))]
    {
        let _ = profile;
        Err(WorkError::Unavailable)
    }
}

async fn preference(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    selection: Option<WorkDecisionChoiceV1>,
    label: &'static str,
) -> WorkDecisionPreferenceV1 {
    let result = async {
        if !authorize(&caller, CallerPolicy::Main, label) {
            return Err(WorkError::Unavailable);
        }
        let profile = confirm_profile(&app, &expected_profile).await?;
        if let Some(choice) = selection {
            write_choice(profile, choice).await?;
            notify(profile);
        }
        let choice = read_choice(profile).await;
        let present = typesafe_key_present(profile).await;
        log(format_args!(
            "work: phase=decision_preference choice={} effective={} typesafe_key={present}",
            choice.as_str(),
            choice.effective(present).as_str(),
        ));
        Ok((choice, present))
    }
    .await;
    match result {
        Ok((choice, present)) => {
            WorkDecisionPreferenceV1::settled(expected_profile, choice, present)
        }
        Err(error) => WorkDecisionPreferenceV1::failed(expected_profile, error.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_decision_preference(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
) -> WorkDecisionPreferenceV1 {
    preference(
        caller,
        app,
        expected_profile,
        None,
        "work_decision_preference",
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_set_decision_preference(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    choice: WorkDecisionChoiceV1,
) -> WorkDecisionPreferenceV1 {
    preference(
        caller,
        app,
        expected_profile,
        Some(choice),
        "work_set_decision_preference",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stored_key_is_profile_scoped_and_carries_the_wire_spelling() {
        let profile = ProfileId::from(7_u128);
        assert_eq!(setting_key(profile), format!("work.decisions.{profile}"));
        assert_eq!(WorkDecisionChoiceV1::Standard.as_str(), "standard");
    }
}
