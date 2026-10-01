//! Models and keys for the lead agent: the picker's catalog and choices, and
//! Settings → AI. Keys cross this boundary only inbound; the frame sees
//! missing / set / valid / invalid, never a key.
use super::*;
use zephium_core::ids::ProfileId;
use zephium_core::work::model::{WorkModelEntry, WorkModelProvider, WorkModelRole};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkKeyStateV1 {
    Missing,
    Set,
    Valid,
    Invalid,
}

/// Why the last action did not do what was asked. Closed; no provider text.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkModelsFaultV1 {
    /// The provider refused the key.
    KeyRefused,
    /// The provider could not be reached.
    Unreachable,
    Billing,
    RateLimited,
    ProviderDown,
    Request,
    /// The Keychain refused.
    Keychain,
    /// The request was malformed (an unknown model, a bad address).
    Invalid,
    /// Models are unavailable in this build or profile.
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct WorkProviderStatusV1 {
    pub(crate) provider: WorkModelProvider,
    pub(crate) key: WorkKeyStateV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fault: Option<WorkModelsFaultV1>,
    /// The OpenAI-compatible endpoint's base URL.
    pub(crate) base: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
pub(crate) struct WorkModelRolesV1 {
    pub(crate) lead: Option<String>,
    pub(crate) page: Option<String>,
    pub(crate) light: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
pub(crate) struct WorkCloudStatusV1 {
    pub(crate) signed_in: bool,
    pub(crate) plan: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct WorkModelsV1 {
    pub(crate) version: u32,
    pub(crate) profile: String,
    pub(crate) entries: Vec<WorkModelEntry>,
    /// What the person chose per role.
    pub(crate) chosen: WorkModelRolesV1,
    /// What each role runs with now; null when no provider is usable.
    pub(crate) effective: WorkModelRolesV1,
    pub(crate) providers: Vec<WorkProviderStatusV1>,
    pub(crate) cloud: WorkCloudStatusV1,
    pub(crate) fault: Option<WorkModelsFaultV1>,
}

impl WorkModelsV1 {
    fn failed(profile: String, fault: WorkModelsFaultV1) -> Self {
        Self {
            version: 1,
            profile,
            entries: Vec::new(),
            chosen: WorkModelRolesV1::default(),
            effective: WorkModelRolesV1::default(),
            providers: Vec::new(),
            cloud: WorkCloudStatusV1::default(),
            fault: Some(fault),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct WorkMoreModelsV1 {
    pub(crate) provider: WorkModelProvider,
    pub(crate) entries: Vec<WorkModelEntry>,
    pub(crate) fault: Option<WorkModelsFaultV1>,
}

/// Something in the picker or Settings → AI changed; read it again.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "zephium:work-models-changed")]
pub(crate) struct WorkModelsChanged {
    pub(crate) version: u32,
}

#[cfg(feature = "work-product")]
mod product {
    use super::*;
    use zephium_app::work_models as models;
    use zephium_core::work::model::WorkModelError;

    struct AppSettings;

    impl models::WorkModelSettings for AppSettings {
        fn get(&self, key: &str) -> Option<String> {
            use zephium_core::ports::store::Store;
            APP_STORE.get().and_then(|store| store.app_setting(key))
        }
        fn set(&self, key: String, value: String) -> bool {
            use zephium_core::ports::store::Store;
            APP_STORE
                .get()
                .is_some_and(|store| store.set_app_setting(key, value))
        }
    }

    pub(super) fn install(app: &tauri::AppHandle) {
        models::install_settings(std::sync::Arc::new(AppSettings));
        let app = app.clone();
        models::set_observer(Box::new(move || {
            emit_to_privileged(
                &app,
                MAIN_LABEL,
                "zephium:work-models-changed",
                &WorkModelsChanged { version: 1 },
            );
        }));
    }

    fn key(status: models::WorkKeyStatus) -> WorkKeyStateV1 {
        match status {
            models::WorkKeyStatus::Missing => WorkKeyStateV1::Missing,
            models::WorkKeyStatus::Set => WorkKeyStateV1::Set,
            models::WorkKeyStatus::Valid => WorkKeyStateV1::Valid,
            models::WorkKeyStatus::Invalid => WorkKeyStateV1::Invalid,
        }
    }

    fn provider_fault(fault: models::WorkKeyFault) -> WorkModelsFaultV1 {
        match fault {
            models::WorkKeyFault::WrongKey => WorkModelsFaultV1::KeyRefused,
            models::WorkKeyFault::Billing => WorkModelsFaultV1::Billing,
            models::WorkKeyFault::RateLimited => WorkModelsFaultV1::RateLimited,
            models::WorkKeyFault::ProviderDown => WorkModelsFaultV1::ProviderDown,
            models::WorkKeyFault::Offline => WorkModelsFaultV1::Unreachable,
            models::WorkKeyFault::Keychain => WorkModelsFaultV1::Keychain,
            models::WorkKeyFault::Request => WorkModelsFaultV1::Request,
        }
    }

    fn checked(
        profile: ProfileId,
        provider: WorkModelProvider,
        status: models::WorkKeyStatus,
    ) -> Option<WorkModelsFaultV1> {
        models::fill_defaults(profile, provider);
        if status == models::WorkKeyStatus::Invalid {
            return Some(WorkModelsFaultV1::KeyRefused);
        }
        models::provider_failure(provider).map(provider_fault)
    }

    pub(super) fn fault(error: WorkModelError) -> WorkModelsFaultV1 {
        match error {
            WorkModelError::Network => WorkModelsFaultV1::Unreachable,
            WorkModelError::Overloaded => WorkModelsFaultV1::ProviderDown,
            WorkModelError::RateLimited { .. } => WorkModelsFaultV1::RateLimited,
            WorkModelError::OverBudget => WorkModelsFaultV1::Billing,
            WorkModelError::Protocol => WorkModelsFaultV1::Request,
            WorkModelError::Unauthorized => WorkModelsFaultV1::Keychain,
            WorkModelError::MissingKey => WorkModelsFaultV1::KeyRefused,
            _ => WorkModelsFaultV1::Invalid,
        }
    }

    pub(super) async fn view(profile: ProfileId, fault: Option<WorkModelsFaultV1>) -> WorkModelsV1 {
        let view = models::view(profile).await;
        let [lead, page, light] = view.chosen;
        let chosen = WorkModelRolesV1 { lead, page, light };
        let [lead, page, light] = view.effective;
        let effective = WorkModelRolesV1 { lead, page, light };
        WorkModelsV1 {
            version: 1,
            profile: profile.to_string(),
            entries: view.entries,
            chosen,
            effective,
            providers: view
                .providers
                .into_iter()
                .map(|provider| WorkProviderStatusV1 {
                    provider: provider.provider,
                    key: key(provider.key),
                    fault: provider.fault.map(provider_fault),
                    base: provider.base,
                })
                .collect(),
            cloud: WorkCloudStatusV1 {
                signed_in: view.cloud_signed_in,
                plan: view.cloud_plan,
            },
            fault,
        }
    }

    pub(super) async fn act(profile: ProfileId, action: Action) -> Option<WorkModelsFaultV1> {
        let result = match action {
            Action::Read => Ok(()),
            Action::Choose(role, id) => models::choose(profile, role, id.as_deref()),
            Action::SetKey(provider, secret) => match models::set_key(provider, secret).await {
                Ok(status) => return checked(profile, provider, status),
                Err(WorkModelError::BadRequest) => return Some(WorkModelsFaultV1::Invalid),
                Err(error) => Err(error),
            },
            Action::TestKey(provider) => match models::test_key(provider).await {
                Ok(status) => return checked(profile, provider, status),
                Err(error) => Err(error),
            },
            Action::ClearKey(provider) => models::clear_key(provider).await,
            Action::Endpoint(base) => models::set_compatible_base(base.as_deref()).await,
        };
        result.err().map(fault)
    }

    pub(super) async fn more(
        provider: WorkModelProvider,
    ) -> Result<Vec<WorkModelEntry>, WorkModelsFaultV1> {
        models::more_models(provider)
            .await
            .map_err(|error| match error {
                WorkModelError::Unauthorized => WorkModelsFaultV1::KeyRefused,
                WorkModelError::MissingKey => WorkModelsFaultV1::KeyRefused,
                error => fault(error),
            })
    }
}

pub(crate) enum Action {
    Read,
    Choose(WorkModelRole, Option<String>),
    SetKey(WorkModelProvider, String),
    TestKey(WorkModelProvider),
    ClearKey(WorkModelProvider),
    Endpoint(Option<String>),
}

pub(crate) fn install(app: &tauri::AppHandle) {
    #[cfg(feature = "work-product")]
    product::install(app);
    #[cfg(not(feature = "work-product"))]
    let _ = app;
}

fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Read => "read",
        Action::Choose(..) => "choose",
        Action::SetKey(..) => "set_key",
        Action::TestKey(..) => "test_key",
        Action::ClearKey(..) => "clear_key",
        Action::Endpoint(..) => "endpoint",
    }
}

fn log(arguments: std::fmt::Arguments<'_>) {
    #[cfg(feature = "work-product")]
    super::work_provider::record_diagnostic(arguments);
    #[cfg(not(feature = "work-product"))]
    super::write_diagnostic(arguments);
}

/// Models are app-wide except choices, which belong to the Work profile the
/// caller names; the id must parse exactly.
fn profile_of(caller: &WebviewWindow, expected: &str, label: &str) -> Option<ProfileId> {
    if !authorize(caller, CallerPolicy::Main, label) {
        return None;
    }
    ProfileId::parse(expected).filter(|id| id.to_string() == expected)
}

async fn respond(
    caller: WebviewWindow,
    expected_profile: String,
    action: Action,
    label: &'static str,
) -> WorkModelsV1 {
    let Some(profile) = profile_of(&caller, &expected_profile, label) else {
        return WorkModelsV1::failed(expected_profile, WorkModelsFaultV1::Unavailable);
    };
    let name = action_name(&action);
    #[cfg(feature = "work-product")]
    {
        let fault = product::act(profile, action).await;
        log(format_args!(
            "work: phase=models action={name} fault={fault:?}"
        ));
        product::view(profile, fault).await
    }
    #[cfg(not(feature = "work-product"))]
    {
        let _ = (profile, action);
        log(format_args!(
            "work: phase=models action={name} fault=unavailable"
        ));
        WorkModelsV1::failed(expected_profile, WorkModelsFaultV1::Unavailable)
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_models(caller: WebviewWindow, expected_profile: String) -> WorkModelsV1 {
    respond(caller, expected_profile, Action::Read, "work_models").await
}

/// A narrow readiness contract for the Work composer. Only a missing lead blocks.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub(crate) struct WorkModelsReadyV1 {
    pub(crate) version: u16,
    pub(crate) profile: String,
    pub(crate) ready: bool,
    pub(crate) missing_roles: Vec<WorkModelRole>,
    pub(crate) fault: Option<WorkModelsFaultV1>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_models_ready(
    caller: WebviewWindow,
    expected_profile: String,
) -> WorkModelsReadyV1 {
    let profile = profile_of(&caller, &expected_profile, "work_models_ready");
    #[cfg(feature = "work-product")]
    let ready = match profile {
        Some(profile) => zephium_app::work_models::ready(profile).await,
        None => false,
    };
    #[cfg(not(feature = "work-product"))]
    let ready = false;
    WorkModelsReadyV1 {
        version: 1,
        profile: expected_profile,
        ready,
        missing_roles: if ready {
            vec![]
        } else {
            vec![WorkModelRole::Lead]
        },
        fault: (profile.is_none() || !cfg!(feature = "work-product"))
            .then_some(WorkModelsFaultV1::Unavailable),
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_choose_model(
    caller: WebviewWindow,
    expected_profile: String,
    role: WorkModelRole,
    id: Option<String>,
) -> WorkModelsV1 {
    respond(
        caller,
        expected_profile,
        Action::Choose(role, id),
        "work_choose_model",
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_set_provider_key(
    caller: WebviewWindow,
    expected_profile: String,
    provider: WorkModelProvider,
    key: String,
) -> WorkModelsV1 {
    respond(
        caller,
        expected_profile,
        Action::SetKey(provider, key),
        "work_set_provider_key",
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_test_provider_key(
    caller: WebviewWindow,
    expected_profile: String,
    provider: WorkModelProvider,
) -> WorkModelsV1 {
    respond(
        caller,
        expected_profile,
        Action::TestKey(provider),
        "work_test_provider_key",
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_clear_provider_key(
    caller: WebviewWindow,
    expected_profile: String,
    provider: WorkModelProvider,
) -> WorkModelsV1 {
    respond(
        caller,
        expected_profile,
        Action::ClearKey(provider),
        "work_clear_provider_key",
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_set_model_endpoint(
    caller: WebviewWindow,
    expected_profile: String,
    base: Option<String>,
) -> WorkModelsV1 {
    respond(
        caller,
        expected_profile,
        Action::Endpoint(base),
        "work_set_model_endpoint",
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_more_models(
    caller: WebviewWindow,
    expected_profile: String,
    provider: WorkModelProvider,
) -> WorkMoreModelsV1 {
    let refused = |fault| WorkMoreModelsV1 {
        provider,
        entries: Vec::new(),
        fault: Some(fault),
    };
    if profile_of(&caller, &expected_profile, "work_more_models").is_none() {
        return refused(WorkModelsFaultV1::Unavailable);
    }
    #[cfg(feature = "work-product")]
    {
        match product::more(provider).await {
            Ok(entries) => {
                log(format_args!(
                    "work: phase=models action=more entries={}",
                    entries.len()
                ));
                WorkMoreModelsV1 {
                    provider,
                    entries,
                    fault: None,
                }
            }
            Err(fault) => {
                log(format_args!(
                    "work: phase=models action=more fault={fault:?}"
                ));
                refused(fault)
            }
        }
    }
    #[cfg(not(feature = "work-product"))]
    refused(WorkModelsFaultV1::Unavailable)
}

#[cfg(test)]
mod release_tests {
    use super::*;

    #[test]
    fn readiness_and_provider_faults_have_stable_wire_shapes() {
        let ready = WorkModelsReadyV1 {
            version: 1,
            profile: "profile".into(),
            ready: false,
            missing_roles: vec![WorkModelRole::Lead],
            fault: None,
        };
        assert_eq!(
            serde_json::to_value(ready).unwrap(),
            serde_json::json!({
                "version": 1, "profile": "profile", "ready": false,
                "missing_roles": ["lead"], "fault": null
            })
        );
        let mut provider = WorkProviderStatusV1 {
            provider: WorkModelProvider::OpenAi,
            key: WorkKeyStateV1::Valid,
            fault: None,
            base: None,
        };
        assert!(serde_json::to_value(&provider)
            .unwrap()
            .get("fault")
            .is_none());
        provider.fault = Some(WorkModelsFaultV1::Billing);
        let value = serde_json::to_value(provider).unwrap();
        assert_eq!(value["key"], "valid");
        assert_eq!(value["fault"], "billing");
    }
}
