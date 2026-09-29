//! Settings → Connections: the command-line tools Work can use and the MCP
//! servers the person added. Secrets go straight to the Keychain and never
//! come back; responses say only which ones are held.
use super::*;
use zephium_core::ids::ProfileId;
use zephium_core::work::WorkError;
use zephium_ipc::work::{
    WorkCliRowV1, WorkConnectionsResponseV1, WorkServerCheckV1, WorkServerDraftV1,
    WorkServerOutcomeV1,
};

fn profile_of(expected: &str) -> Result<ProfileId, WorkError> {
    ProfileId::parse(expected)
        .filter(|id| id.to_string() == expected)
        .ok_or(WorkError::Invalid)
}

fn admitted(
    caller: &WebviewWindow,
    app: &tauri::AppHandle,
    command: &str,
) -> Result<(), WorkError> {
    if !authorize(caller, CallerPolicy::Main, command) {
        return Err(WorkError::Unavailable);
    }
    if shutdown_started(app) {
        return Err(WorkError::Shutdown);
    }
    Ok(())
}

#[cfg(feature = "work-product")]
mod live {
    use super::*;
    use zephium_app::work_connections::client::keychain;
    use zephium_app::work_connections::{cli, store};
    use zephium_ipc::work::{
        WorkCliStatusV1, WorkServerAuthV1, WorkServerRowV1, WorkServerTransportV1, WorkServerV1,
    };

    pub(super) fn store(
        app: &tauri::AppHandle,
    ) -> Result<&'static store::ConnectionStore, WorkError> {
        if let Some(store) = store::shared() {
            return Ok(store);
        }
        let data = app
            .path()
            .app_data_dir()
            .map_err(|_| WorkError::Unavailable)?;
        store::install(&data);
        store::shared().ok_or(WorkError::Unavailable)
    }

    pub(super) async fn clis() -> Vec<WorkCliRowV1> {
        cli::forget();
        cli::statuses()
            .await
            .into_iter()
            .map(|status| {
                let (status_v1, account) = match (&status.path, &status.auth) {
                    (None, _) => (WorkCliStatusV1::Missing, None),
                    (Some(_), cli::CliAuth::SignedIn(account)) => {
                        (WorkCliStatusV1::SignedIn, account.clone())
                    }
                    (Some(_), cli::CliAuth::SignedOut) => (WorkCliStatusV1::SignedOut, None),
                    (Some(_), cli::CliAuth::NotNeeded(account)) => {
                        (WorkCliStatusV1::Ready, account.clone())
                    }
                    (Some(_), cli::CliAuth::Unknown) => (WorkCliStatusV1::Unknown, None),
                };
                WorkCliRowV1 {
                    id: status.cli.id().to_owned(),
                    status: status_v1,
                    version: status.version,
                    account,
                }
            })
            .collect()
    }

    /// Which secrets each server holds, from Keychain attributes only.
    pub(super) fn rows(profile: &str, servers: Vec<WorkServerV1>) -> Vec<WorkServerRowV1> {
        servers
            .into_iter()
            .map(|server| {
                let mut accounts: Vec<String> = Vec::new();
                match &server.transport {
                    WorkServerTransportV1::Stdio { env, .. } => accounts.extend(
                        env.iter()
                            .filter(|e| e.secret)
                            .map(|e| format!("env.{}", e.name)),
                    ),
                    WorkServerTransportV1::Http {
                        auth: WorkServerAuthV1::Bearer,
                        ..
                    } => accounts.push("bearer".into()),
                    _ => {}
                }
                let secrets = accounts
                    .into_iter()
                    .filter(|account| keychain::present(profile, &server.id, account))
                    .collect();
                let signed_in = matches!(
                    server.transport,
                    WorkServerTransportV1::Http {
                        auth: WorkServerAuthV1::OAuth,
                        ..
                    }
                ) && keychain::present(profile, &server.id, "oauth");
                WorkServerRowV1 {
                    server,
                    secrets,
                    signed_in,
                }
            })
            .collect()
    }

    /// The Keychain accounts a server may hold, for moving or forgetting them.
    pub(super) fn accounts(server: &WorkServerV1) -> Vec<String> {
        let mut accounts = vec!["bearer".to_owned(), "oauth".to_owned()];
        if let WorkServerTransportV1::Stdio { env, .. } = &server.transport {
            accounts.extend(env.iter().map(|e| format!("env.{}", e.name)));
        }
        accounts
    }
}

async fn listing(
    app: &tauri::AppHandle,
    profile: ProfileId,
    with_clis: bool,
) -> Result<(Vec<WorkCliRowV1>, Vec<zephium_ipc::work::WorkServerRowV1>), WorkError> {
    #[cfg(feature = "work-product")]
    {
        let store = live::store(app)?;
        let id = profile.to_string();
        let servers = store.servers(&id).map_err(|_| WorkError::Unavailable)?;
        let clis = if with_clis {
            live::clis().await
        } else {
            Vec::new()
        };
        let rows = tokio::task::spawn_blocking(move || live::rows(&id, servers))
            .await
            .map_err(|_| WorkError::Unavailable)?;
        Ok((clis, rows))
    }
    #[cfg(not(feature = "work-product"))]
    {
        let _ = (app, profile, with_clis);
        Err(WorkError::Unavailable)
    }
}

fn response(
    expected: String,
    result: Result<(Vec<WorkCliRowV1>, Vec<zephium_ipc::work::WorkServerRowV1>), WorkError>,
) -> WorkConnectionsResponseV1 {
    match result {
        Ok((clis, servers)) => WorkConnectionsResponseV1 {
            version: 1,
            profile: expected,
            clis,
            servers,
            error: None,
        },
        Err(error) => WorkConnectionsResponseV1 {
            version: 1,
            profile: expected,
            clis: Vec::new(),
            servers: Vec::new(),
            error: Some(error.into()),
        },
    }
}

/// The tools found on this Mac and the servers this profile added.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_connections(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
) -> WorkConnectionsResponseV1 {
    let result = async {
        admitted(&caller, &app, "work_connections")?;
        let profile = profile_of(&expected_profile)?;
        listing(&app, profile, true).await
    }
    .await;
    response(expected_profile, result)
}

/// Adds or replaces a server; new secrets go to the Keychain.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_save_connection(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    draft: WorkServerDraftV1,
) -> WorkConnectionsResponseV1 {
    let result = async {
        admitted(&caller, &app, "work_save_connection")?;
        let profile = profile_of(&expected_profile)?;
        #[cfg(feature = "work-product")]
        {
            use zephium_app::work_connections::client::keychain;
            let store = live::store(&app)?;
            let id = profile.to_string();
            if !draft.server.validate()
                || draft.secrets.len() > 40
                || draft.secrets.iter().any(|s| {
                    s.value.is_empty()
                        || s.value.len() > 8192
                        || !live::accounts(&draft.server).contains(&s.account)
                })
            {
                return Err(WorkError::Invalid);
            }
            let previous = draft.previous.clone();
            let server = draft.server.clone();
            let secrets = draft.secrets;
            tokio::task::spawn_blocking(move || {
                store
                    .put(&id, server.clone(), previous.as_deref())
                    .map_err(|_| WorkError::Invalid)?;
                if let Some(old) = previous.filter(|old| *old != server.id) {
                    for account in live::accounts(&server) {
                        if let Ok(value) = keychain::read(&id, &old, &account) {
                            let _ = keychain::write(&id, &server.id, &account, &value);
                        }
                        let _ = keychain::delete(&id, &old, &account);
                    }
                    let _ = store.put_tools(&id, &old, None);
                }
                for secret in &secrets {
                    keychain::write(&id, &server.id, &secret.account, &secret.value)
                        .map_err(|_| WorkError::Unavailable)?;
                }
                Ok::<_, WorkError>(())
            })
            .await
            .map_err(|_| WorkError::Unavailable)??;
        }
        #[cfg(not(feature = "work-product"))]
        let _ = draft;
        listing(&app, profile, false).await
    }
    .await;
    response(expected_profile, result)
}

/// Removes a server and forgets its secrets.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_remove_connection(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    id: String,
) -> WorkConnectionsResponseV1 {
    let result = async {
        admitted(&caller, &app, "work_remove_connection")?;
        let profile = profile_of(&expected_profile)?;
        #[cfg(feature = "work-product")]
        {
            use zephium_app::work_connections::client::keychain;
            let store = live::store(&app)?;
            let owner = profile.to_string();
            tokio::task::spawn_blocking(move || {
                let servers = store.servers(&owner).map_err(|_| WorkError::Unavailable)?;
                let server = servers
                    .into_iter()
                    .find(|s| s.id == id)
                    .ok_or(WorkError::NotFound)?;
                store
                    .remove(&owner, &id)
                    .map_err(|_| WorkError::Unavailable)?;
                for account in live::accounts(&server) {
                    let _ = keychain::delete(&owner, &id, &account);
                }
                Ok::<_, WorkError>(())
            })
            .await
            .map_err(|_| WorkError::Unavailable)??;
        }
        #[cfg(not(feature = "work-product"))]
        let _ = id;
        listing(&app, profile, false).await
    }
    .await;
    response(expected_profile, result)
}

fn check_failed(expected: String, id: String, error: WorkError) -> WorkServerCheckV1 {
    WorkServerCheckV1 {
        version: 1,
        profile: expected,
        id,
        outcome: WorkServerOutcomeV1::Failed,
        server_name: None,
        tools: Vec::new(),
        error: Some(error.into()),
    }
}

/// Connects to a server once and lists its tools.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_check_connection(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    id: String,
) -> WorkServerCheckV1 {
    let key = id.clone();
    let result = async {
        admitted(&caller, &app, "work_check_connection")?;
        let profile = profile_of(&expected_profile)?;
        #[cfg(feature = "work-product")]
        {
            let store = live::store(&app)?;
            let owner = profile.to_string();
            let server = store
                .servers(&owner)
                .map_err(|_| WorkError::Unavailable)?
                .into_iter()
                .find(|s| s.id == id)
                .ok_or(WorkError::NotFound)?;
            Ok(zephium_app::work_connections::mcp::check(&owner, &server).await)
        }
        #[cfg(not(feature = "work-product"))]
        {
            let _ = profile;
            Err(WorkError::Unavailable)
        }
    }
    .await;
    result.unwrap_or_else(|error| check_failed(expected_profile, key, error))
}

/// Signs in to an HTTP server in a new tab, then checks it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_sign_in_connection(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    id: String,
) -> WorkServerCheckV1 {
    let key = id.clone();
    let result = async {
        admitted(&caller, &app, "work_sign_in_connection")?;
        let profile = profile_of(&expected_profile)?;
        #[cfg(feature = "work-product")]
        {
            use zephium_ipc::work::WorkServerTransportV1;
            let store = live::store(&app)?;
            let owner = profile.to_string();
            let server = store
                .servers(&owner)
                .map_err(|_| WorkError::Unavailable)?
                .into_iter()
                .find(|s| s.id == id)
                .ok_or(WorkError::NotFound)?;
            let WorkServerTransportV1::Http { url, .. } = &server.transport else {
                return Err(WorkError::Invalid);
            };
            let tokens =
                zephium_app::work_connections::client::keychain::oauth_store(&owner, &server.id);
            let opener = app.clone();
            let signed =
                zephium_app::work_connections::client::oauth::sign_in(url, tokens, move |page| {
                    if zephium_core::navigation::is_allowed_str(&page) {
                        let shell = opener.state::<Handle>();
                        let _ = dispatch_operation(
                            &opener,
                            &shell,
                            Command::OpenUrl {
                                input: page,
                                new_tab: true,
                            },
                        );
                    }
                })
                .await;
            let mut check = zephium_app::work_connections::mcp::check(&owner, &server).await;
            if signed.is_err() && check.outcome == WorkServerOutcomeV1::Ready {
                check.outcome = WorkServerOutcomeV1::SignIn;
            }
            Ok(check)
        }
        #[cfg(not(feature = "work-product"))]
        {
            let _ = profile;
            Err(WorkError::Unavailable)
        }
    }
    .await;
    result.unwrap_or_else(|error| check_failed(expected_profile, key, error))
}
