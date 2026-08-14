use super::*;
use zephium_core::ports::extensions::ExtensionRuntimeGrantPromptSettlement;

fn prompt(profile: ProfileId, request_id: u64) -> zephium_core::ports::engine::EngineEvent {
    let runtime = zephium_core::extensions::ExtensionRuntimeInstance::new(
        profile,
        zephium_core::ids::ExtensionInstallId::from(41),
        zephium_core::extensions::ExtensionRuntimeGeneration::new(7).unwrap(),
    );
    let request = zephium_core::ports::extensions::ExtensionRuntimeGrantRequest::new(
        vec![zephium_core::extensions::ApiPermissionName::parse_exact("clipboardWrite").unwrap()],
        Vec::new(),
    )
    .unwrap();
    let key = zephium_core::extensions::ExtensionNativeOwnershipKey::new(
        runtime.profile(),
        runtime.install_id(),
        zephium_core::extensions::ExtensionGrantBrowsingContext::Regular,
    );
    zephium_core::ports::engine::EngineEvent::ExtensionRuntimeGrantRequested {
        prompt: Box::new(
            zephium_core::ports::extensions::ExtensionRuntimeGrantPrompt::new(
                zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId::new(request_id)
                    .unwrap(),
                runtime,
                key,
                "Fixture extension",
                request,
            )
            .unwrap(),
        ),
    }
}

#[test]
fn prebootstrap_prompt_is_denied_and_ready_shell_retains_consent_work() {
    let (mut shell, engine, _screen) = setup();

    shell.handle(Command::Engine(prompt(ProfileId::from(1), 1)));
    shell.handle(Command::Bootstrap);
    let profile = shell.windows.focused().unwrap().profile;
    shell.handle(Command::Engine(prompt(profile, 2)));

    let settlements = engine.extension_runtime_grant_settlements();
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0].1.get(), 1);
    assert_eq!(
        settlements[0].2,
        ExtensionRuntimeGrantPromptSettlement::Denied
    );
    assert_eq!(
        shell
            .extension_runtime_grants
            .active()
            .map(|(prompt, processing)| (prompt.id().get(), processing)),
        Some((2, false))
    );
}

#[test]
fn explicit_denial_settles_exact_native_prompt_without_calling_service() {
    let (lifecycle, lifecycle_state) = extension_lifecycle_with_outcome(
        zephium_core::ports::extensions::ExtensionServiceShutdownOutcome::Clean,
    );
    let (mut shell, engine, screen, operations) = setup_with_operation_log_and_lifecycle(
        Arc::new(FakeStore::default()),
        lifecycle,
        Box::new(|_| {}),
    );
    shell.handle(Command::Bootstrap);
    let tab = active_id(&screen);
    navigate_and_commit(&mut shell, tab, "permission.example");
    assert_eq!(engine.last_layout(), vec![tab.to_string()]);
    let profile = shell.windows.focused().unwrap().profile;
    shell.handle(Command::Engine(prompt(profile, 9)));
    assert!(
        engine.last_layout().is_empty(),
        "browser consent must remove page views from the native stage"
    );
    let (runtime, request) = shell
        .extension_runtime_grants
        .active()
        .map(|(prompt, _)| (prompt.runtime(), prompt.id()))
        .unwrap();
    shell.handle(Command::Operation {
        operation_id: "deny-runtime-grant".into(),
        command: Box::new(Command::RespondToExtensionRuntimeGrantPrompt {
            runtime,
            request,
            allow: false,
        }),
    });

    assert!(shell.extension_runtime_grants.active().is_none());
    assert_eq!(
        engine.last_layout(),
        vec![tab.to_string()],
        "settlement must restore the prior native content layout"
    );
    assert!(lifecycle_state
        .runtime_grant_calls
        .lock()
        .unwrap()
        .is_empty());
    assert_eq!(
        engine
            .extension_runtime_grant_settlements()
            .last()
            .unwrap()
            .2,
        ExtensionRuntimeGrantPromptSettlement::Denied
    );
    let completion = operations.lock().unwrap().last().cloned().unwrap();
    assert_eq!(completion.operation_id, "deny-runtime-grant");
    assert_eq!(completion.outcome, OperationOutcome::Applied);
}

#[test]
fn spawned_actor_grants_exact_prompt_and_waits_for_durable_service_settlement() {
    let engine = Arc::new(FakeEngine::default());
    let (lifecycle, lifecycle_state) = extension_lifecycle_with_outcome(
        zephium_core::ports::extensions::ExtensionServiceShutdownOutcome::Clean,
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = spawn(
        engine.clone(),
        Arc::new(FakeStore::default()),
        Arc::new(ImmediateAllowAllCompiler),
        lifecycle,
        Box::new(|_| {}),
        Arc::new(FakeChrome),
        Box::new(move |projection| {
            let _ = tx.send(projection);
        }),
    )
    .expect("spawn test shell");
    assert!(handle.dispatch(Command::SetWindowSize(Size::new(1200.0, 800.0))));
    assert!(handle.dispatch(Command::Bootstrap));
    let profile = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::Items(items) => items.profile.map(|profile| profile.id),
            _ => None,
        })
        .and_then(|profile| ProfileId::parse(&profile))
        .expect("bootstrap must publish the focused profile");
    let _ = rx.try_iter().count();

    assert!(handle.dispatch(Command::Engine(prompt(profile, 27))));
    let projected = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(2)).ok())
        .find_map(|projection| match projection {
            Projection::ExtensionRuntimeGrantPrompt(view) => view.prompt,
            _ => None,
        })
        .expect("native prompt must reach privileged chrome");
    assert_eq!(projected.profile_id, profile.to_string());
    assert_eq!(projected.request_id, "000000000000001b");
    assert!(!projected.processing);

    let runtime = ExtensionRuntimeInstance::new(
        profile,
        zephium_core::ids::ExtensionInstallId::from(41),
        zephium_core::extensions::ExtensionRuntimeGeneration::new(7).unwrap(),
    );
    assert!(handle.dispatch_operation(
        "allow-runtime-grant".into(),
        Command::RespondToExtensionRuntimeGrantPrompt {
            runtime,
            request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId::new(27)
                .unwrap(),
            allow: true,
        },
    ));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while lifecycle_state
        .runtime_grant_callbacks
        .lock()
        .unwrap()
        .is_empty()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let calls = lifecycle_state.runtime_grant_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0.profile(), profile);
    assert_eq!(calls[0].0.install_id(), runtime.install_id());
    assert_eq!(calls[0].1, runtime.generation());
    assert_eq!(calls[0].2.api()[0].as_str(), "clipboardWrite");
    drop(calls);

    lifecycle_state
        .runtime_grant_callbacks
        .lock()
        .unwrap()
        .pop()
        .expect("accepted service request retains its callback")(
        zephium_core::ports::extensions::ExtensionManagementSettlement::new(
            zephium_core::ports::extensions::ExtensionRuntimeGrantOutcome::Granted {
                revision: zephium_core::extensions::ExtensionGrantRevision::new(2).unwrap(),
                runtime: zephium_core::ports::extensions::ExtensionRuntimeGrantRuntimeState::Active(
                    runtime.generation(),
                ),
            },
            None,
        ),
    );

    let mut saw_processing = false;
    let mut saw_closed = false;
    let mut completion = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline
        && (!saw_processing || !saw_closed || completion.is_none())
    {
        let Ok(projection) = rx.recv_timeout(std::time::Duration::from_millis(50)) else {
            continue;
        };
        match projection {
            Projection::ExtensionRuntimeGrantPrompt(view) => match view.prompt {
                Some(prompt) => saw_processing |= prompt.processing,
                None => saw_closed = true,
            },
            Projection::OperationProcessed(disposition)
                if disposition.operation_id == "allow-runtime-grant" =>
            {
                completion = Some(disposition);
            }
            _ => {}
        }
    }
    assert!(saw_processing, "allow must become visibly non-repeatable");
    assert!(saw_closed, "durable settlement must close the exact prompt");
    let completion = completion.expect("grant operation must settle truthfully");
    assert_eq!(completion.outcome, OperationOutcome::Applied);
    assert_eq!(completion.reason, OperationReason::MutationApplied);
    assert_eq!(
        engine
            .extension_runtime_grant_settlements()
            .last()
            .expect("native completion")
            .2,
        ExtensionRuntimeGrantPromptSettlement::Granted
    );

    assert_eq!(
        handle
            .shutdown()
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap(),
        ShutdownOutcome::Clean
    );
}
