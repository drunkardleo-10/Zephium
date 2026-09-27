#[cfg(target_os = "macos")]
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    let require_supported_runtime = arguments
        .iter()
        .any(|argument| argument == "--require-supported-runtime");
    let interactive_permissions = arguments
        .iter()
        .any(|argument| argument == "--interactive-permission-gate");
    let callback_cohort = arguments
        .iter()
        .any(|argument| argument == "--permission-callback-cohort-gate");
    let replacement_settlement = arguments
        .iter()
        .any(|argument| argument == "--permission-replacement-settlement-gate");
    let globs = arguments
        .iter()
        .any(|argument| argument == "--content-script-globs-gate");
    let side_panel_unavailable = arguments
        .iter()
        .any(|argument| argument == "--side-panel-unavailable-gate");
    let document_id = arguments
        .iter()
        .any(|argument| argument == "--document-id-gate");
    let oauth_redirect = arguments
        .iter()
        .any(|argument| argument == "--oauth-redirect-observation-gate");
    let oauth_redirect_immediate = arguments
        .iter()
        .any(|argument| argument == "--oauth-redirect-immediate-failure-gate");
    let oauth_redirect_clear = arguments
        .iter()
        .any(|argument| argument == "--oauth-redirect-synchronous-clear-gate");
    let oauth_redirect_nonresident = arguments
        .iter()
        .any(|argument| argument == "--oauth-redirect-same-turn-nonresident-gate");
    let oauth_broker_settle_first = arguments
        .iter()
        .any(|argument| argument == "--oauth-broker-settle-first-gate");
    let oauth_broker_network_first = arguments
        .iter()
        .any(|argument| argument == "--oauth-broker-network-first-gate");
    let worker_json_post = arguments
        .iter()
        .any(|argument| argument == "--worker-json-post-gate");
    let original_main_document_globs = arguments
        .iter()
        .position(|argument| argument == "--original-main-document-globs-extension");
    let offscreen_sandbox = arguments
        .iter()
        .any(|argument| argument == "--offscreen-sandbox-gate");
    let offscreen = arguments
        .iter()
        .any(|argument| argument == "--offscreen-gate");
    let isolated_offscreen = arguments
        .iter()
        .any(|argument| argument == "--isolated-offscreen-host-gate");
    let original_offscreen = arguments
        .iter()
        .position(|argument| argument == "--original-google-translate-offscreen-crx");
    let original_bitwarden = arguments
        .iter()
        .position(|argument| argument == "--original-bitwarden-offscreen-crx");
    let prepared_bitwarden_worker = arguments
        .iter()
        .position(|argument| argument == "--prepared-bitwarden-worker-extension");
    let original_bitwarden_erasure = arguments
        .iter()
        .position(|argument| argument == "--original-bitwarden-offscreen-erasure-crx");
    let alarm_delivery = arguments
        .iter()
        .any(|argument| argument == "--alarm-delivery-gate");
    if usize::from(interactive_permissions)
        + usize::from(callback_cohort)
        + usize::from(replacement_settlement)
        + usize::from(alarm_delivery)
        + usize::from(offscreen)
        + usize::from(isolated_offscreen)
        + usize::from(original_offscreen.is_some())
        + usize::from(original_bitwarden.is_some())
        + usize::from(prepared_bitwarden_worker.is_some())
        + usize::from(original_bitwarden_erasure.is_some())
        + usize::from(offscreen_sandbox)
        + usize::from(globs)
        + usize::from(side_panel_unavailable)
        + usize::from(document_id)
        + usize::from(oauth_redirect)
        + usize::from(oauth_redirect_immediate)
        + usize::from(oauth_redirect_clear)
        + usize::from(oauth_redirect_nonresident)
        + usize::from(oauth_broker_settle_first)
        + usize::from(oauth_broker_network_first)
        + usize::from(worker_json_post)
        + usize::from(original_main_document_globs.is_some())
        > 1
    {
        eprintln!("choose only one extended WebExtension gate");
        std::process::exit(2);
    }
    let result = if let Some(position) = original_main_document_globs {
        let Some(path) = arguments.get(position + 1) else {
            eprintln!(
                "--original-main-document-globs-extension requires a prepared extension path"
            );
            std::process::exit(2);
        };
        zephium_engine::run_macos_original_main_document_glob_probe(std::path::Path::new(path))
    } else if document_id {
        zephium_engine::run_macos_web_extension_document_id_probe()
    } else if oauth_redirect {
        zephium_engine::run_macos_oauth_redirect_observation_probe()
    } else if oauth_redirect_immediate {
        zephium_engine::run_macos_oauth_redirect_immediate_failure_probe()
    } else if oauth_redirect_clear {
        zephium_engine::run_macos_oauth_redirect_synchronous_clear_probe()
    } else if oauth_redirect_nonresident {
        zephium_engine::run_macos_oauth_redirect_same_turn_nonresident_probe()
    } else if oauth_broker_settle_first {
        zephium_engine::run_macos_oauth_redirect_broker_order_probe(true)
    } else if oauth_broker_network_first {
        zephium_engine::run_macos_oauth_redirect_broker_order_probe(false)
    } else if worker_json_post {
        zephium_engine::run_macos_worker_json_post_probe()
    } else if side_panel_unavailable {
        zephium_engine::run_macos_web_extension_side_panel_unavailable_probe()
    } else if globs {
        zephium_engine::run_macos_web_extension_content_script_globs_probe()
    } else if let Some(position) = original_bitwarden_erasure {
        let Some(path) = arguments.get(position + 1) else {
            eprintln!("--original-bitwarden-offscreen-erasure-crx requires a CRX path");
            std::process::exit(2);
        };
        zephium_engine::run_macos_original_bitwarden_offscreen_erasure_probe(std::path::Path::new(
            path,
        ))
    } else if let Some(position) = original_bitwarden {
        let Some(path) = arguments.get(position + 1) else {
            eprintln!("--original-bitwarden-offscreen-crx requires a CRX path");
            std::process::exit(2);
        };
        zephium_engine::run_macos_original_bitwarden_offscreen_probe(std::path::Path::new(path))
    } else if let Some(position) = prepared_bitwarden_worker {
        let Some(path) = arguments.get(position + 1) else {
            eprintln!("--prepared-bitwarden-worker-extension requires a prepared tree path");
            std::process::exit(2);
        };
        let Some(scheme) = arguments.get(position + 2) else {
            eprintln!("--prepared-bitwarden-worker-extension requires a scheme");
            std::process::exit(2);
        };
        let Some(user_agent_mode) = arguments.get(position + 3) else {
            eprintln!("--prepared-bitwarden-worker-extension requires a user-agent mode");
            std::process::exit(2);
        };
        zephium_engine::run_macos_prepared_bitwarden_worker_startup_probe(
            std::path::Path::new(path),
            scheme,
            user_agent_mode,
        )
    } else if let Some(position) = original_offscreen {
        let Some(path) = arguments.get(position + 1) else {
            eprintln!("--original-google-translate-offscreen-crx requires a CRX path");
            std::process::exit(2);
        };
        zephium_engine::run_macos_original_google_translate_offscreen_probe(std::path::Path::new(
            path,
        ))
    } else if isolated_offscreen {
        zephium_engine::run_macos_isolated_offscreen_host_probe()
    } else if offscreen_sandbox {
        zephium_engine::run_macos_web_extension_offscreen_sandbox_probe()
    } else if offscreen {
        zephium_engine::run_macos_web_extension_offscreen_probe()
    } else if alarm_delivery {
        zephium_engine::run_macos_web_extension_alarm_delivery_probe()
    } else if replacement_settlement {
        zephium_engine::run_macos_web_extension_permission_replacement_settlement_probe()
    } else if callback_cohort {
        zephium_engine::run_macos_web_extension_permission_callback_cohort_probe()
    } else if interactive_permissions {
        zephium_engine::run_macos_web_extension_permission_probe()
    } else {
        zephium_engine::run_macos_web_extension_probe()
    };
    match result {
        Ok(true) => {}
        Ok(false) if require_supported_runtime => {
            eprintln!(
                "macOS WKWebExtension probe did not execute on a required CI runtime (needs macOS 15.4+)"
            );
            std::process::exit(1);
        }
        Ok(false) => {
            println!(
                "native-probe: macOS WKWebExtension skipped; requires public API runtime macOS 15.4+"
            );
        }
        Err(error) => {
            eprintln!("macOS WKWebExtension probe failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS WKWebExtension probe is available only on macOS");
    std::process::exit(2);
}
