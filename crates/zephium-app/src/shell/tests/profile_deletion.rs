use super::*;

use crate::shell::blocker::BlockerProfileState;

fn insert_inactive_profile_tab(
    shell: &mut Shell,
    profile: ProfileId,
    seed: u128,
) -> (SpaceId, ItemId) {
    let space = shell
        .spaces
        .iter()
        .find(|space| space.profile == profile)
        .expect("inactive profile owns its test space")
        .id;
    let item = ItemId::from(seed);
    assert!(shell.items.insert_tab(
        item,
        Placement::Space {
            space,
            section: SpaceSection::Today,
        },
    ));
    (space, item)
}

#[test]
fn profile_deletion_completes_once_only_after_native_and_store_phases() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, engine, _screen, operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, Box::new(|_| {}));
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_000);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

    shell.handle(delete_operation("delete-1", profile));

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].operation_id, "delete-1");
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    assert_eq!(
        completions[0].reason,
        OperationReason::ProfileDeletionCompleted
    );
    assert!(shell.profiles.get(profile).is_none());
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    let authorized = store.authorized_sessions.lock().unwrap();
    assert_eq!(authorized.len(), 1);
    assert!(authorized[0]
        .1
        .profiles
        .iter()
        .all(|candidate| candidate.id != profile));
    assert_eq!(
        session::canonicalize(authorized[0].1.clone()),
        authorized[0].1
    );
    assert_eq!(
        store.events.lock().unwrap().as_slice(),
        ["authorize-delete", "finalize-delete"]
    );
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        3,
        "authorization, native erasure, and finalization require independent direct fences"
    );
    assert_eq!(
        extension_state
            .retirement_continuation_calls
            .load(std::sync::atomic::Ordering::Acquire),
        3
    );
    assert_eq!(
        extension_state
            .retirement_profiles
            .lock()
            .unwrap()
            .as_slice(),
        &[profile, profile, profile]
    );
}

#[test]
fn profile_with_a_native_view_obligation_is_rejected_before_retirement() {
    let store = Arc::new(FakeStore::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, _engine, _screen, operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, Box::new(|_| {}));
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_050);
    let (_space, item) = insert_inactive_profile_tab(&mut shell, profile, 20_052);
    let effects = shell
        .items
        .navigate(item, "https://view-obligation.example/");
    assert!(matches!(effects.as_slice(), [Effect::CreateView { id, .. }] if *id == item));
    assert!(shell.items.tab(item).is_some_and(TabState::has_view));

    shell.handle(delete_operation("delete-with-live-view", profile));

    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert!(!shell.profile_deletion.states.contains_key(&profile));
    assert!(store.authorized_sessions.lock().unwrap().is_empty());
    let completions = operations.lock().unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].outcome, OperationOutcome::Rejected);
    assert_eq!(
        completions[0].reason,
        OperationReason::ProfileDeletionPolicyRejected
    );
}

#[test]
fn unavailable_retirement_retries_without_store_native_or_aggregate_side_effects() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    extension_state
        .retirement_outcomes
        .lock()
        .unwrap()
        .push_back(ExtensionProfileRetirementDisposition::Unavailable);
    let (mut shell, engine, _screen, operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, Box::new(|_| {}));
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_100);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

    shell.handle(delete_operation("delete-after-fence-retry", profile));

    assert!(shell.profiles.get(profile).is_some());
    assert!(store.authorized_sessions.lock().unwrap().is_empty());
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {profile}")));
    assert!(operations.lock().unwrap().is_empty());
    assert_eq!(
        extension_state
            .retirement_continuation_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    assert!(matches!(
        shell.profile_deletion.states[&profile].phase,
        ProfileDeletionPhase::ExtensionFenceForAuthorization
    ));

    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });

    assert!(shell.profiles.get(profile).is_none());
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert_eq!(operations.lock().unwrap().len(), 1);
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        4
    );
    assert_eq!(
        extension_state
            .retirement_continuation_calls
            .load(std::sync::atomic::Ordering::Acquire),
        3
    );
}

#[test]
fn unavailable_retirement_keeps_profile_quarantined_from_native_and_stale_event_ingress() {
    let store = Arc::new(FakeStore::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    extension_state
        .retirement_outcomes
        .lock()
        .unwrap()
        .push_back(ExtensionProfileRetirementDisposition::Unavailable);
    let (mut shell, engine, _screen, operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, Box::new(|_| {}));
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_150);
    let (space, item) = insert_inactive_profile_tab(&mut shell, profile, 20_152);

    shell.handle(delete_operation("delete-quarantine", profile));

    assert!(shell.profile_deletion_quarantines(profile));
    assert!(!shell.item_in_scope(item, profile, space));
    assert_eq!(
        shell.blocker.profiles[&profile].state,
        BlockerProfileState::Retired
    );
    let effects = shell.items.navigate(item, "https://quarantine.example/");
    let native = shell.apply(effects);
    assert!(native.rejected);
    assert!(!shell.items.tab(item).is_some_and(TabState::has_view));
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call.starts_with(&format!("create {item} "))));
    let quarantined_title = shell.items.tab(item).unwrap().title.clone();

    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id: item,
        title: "stale native title".into(),
    }));
    assert_eq!(shell.items.tab(item).unwrap().title, quarantined_title);
    assert!(store.authorized_sessions.lock().unwrap().is_empty());
    assert!(operations.lock().unwrap().is_empty());
}

#[test]
fn failed_closed_retirement_terminalizes_once_and_preserves_profile_data() {
    let store = Arc::new(FakeStore::default());
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    extension_state
        .retirement_outcomes
        .lock()
        .unwrap()
        .push_back(ExtensionProfileRetirementDisposition::FailedClosed);
    let failures = Arc::new(Mutex::new(Vec::new()));
    let (mut shell, engine, _screen, operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
            let failures = Arc::clone(&failures);
            Box::new(move |failure| failures.lock().unwrap().push(failure))
        });
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_200);

    shell.handle(delete_operation("delete-failed-closed", profile));
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation: 0,
    });

    assert_eq!(
        failures.lock().unwrap().as_slice(),
        &[ShellTerminalFailure::ExtensionProfileRetirementFailedClosed]
    );
    assert!(shell.profiles.get(profile).is_some());
    assert!(store.authorized_sessions.lock().unwrap().is_empty());
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {profile}")));
    assert!(operations.lock().unwrap().is_empty());
    assert!(matches!(
        shell.profile_deletion.states[&profile].phase,
        ProfileDeletionPhase::FailedClosed
    ));
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert_eq!(
        extension_state
            .retirement_continuation_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );

    assert!(shell.extension_startup_ready);
    assert!(!shell.extension_service_ready_for_bootstrap());
    let retirement_calls = extension_state
        .retirement_calls
        .load(std::sync::atomic::Ordering::Acquire);
    let second_profile = add_inactive_named_profile(&mut shell, 20_220);
    shell.handle(delete_operation("delete-after-terminal", second_profile));
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        retirement_calls
    );
    let terminal_rejections = operations.lock().unwrap();
    assert_eq!(terminal_rejections.len(), 1);
    assert_eq!(terminal_rejections[0].outcome, OperationOutcome::Rejected);
    drop(terminal_rejections);
    shell.bootstrapped = false;
    shell.handle(Command::Bootstrap);
    assert!(!shell.bootstrapped);
}

#[test]
fn retirement_panics_and_missing_lifecycle_fail_closed_with_the_owner_restored() {
    // A panic before the continuation cannot create Store or native effects,
    // and catch_unwind must restore the unique lifecycle owner.
    {
        let store = Arc::new(FakeStore::default());
        let (extension_service, extension_state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        extension_state
            .panic_on_retirement
            .store(true, std::sync::atomic::Ordering::Release);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let (mut shell, engine, _screen, _operations) =
            setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            });
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_230);

        shell.handle(delete_operation("panic-before-callback", profile));

        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[ShellTerminalFailure::ExtensionProfileRetirementBoundaryPanicked]
        );
        assert!(shell.extension_service.is_some());
        assert!(store.authorized_sessions.lock().unwrap().is_empty());
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
    }

    // A panic after the callback may leave a durable authorization. Preserve
    // that crash-recoverable truth, do not advance to native erasure, and
    // restore the lifecycle owner before terminal handoff.
    {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
        let (extension_service, extension_state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        extension_state
            .panic_after_retirement_continuation
            .store(true, std::sync::atomic::Ordering::Release);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let (mut shell, engine, _screen, _operations) =
            setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            });
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_240);

        shell.handle(delete_operation("panic-after-callback", profile));

        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[ShellTerminalFailure::ExtensionProfileRetirementBoundaryPanicked]
        );
        assert!(shell.extension_service.is_some());
        assert_eq!(store.pending_deletions.lock().unwrap().len(), 1);
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
    }

    // Losing the lifecycle owner is independently terminal and cannot be
    // treated as an empty extension profile.
    {
        let store = Arc::new(FakeStore::default());
        let failures = Arc::new(Mutex::new(Vec::new()));
        let (extension_service, _state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        let (mut shell, engine, _screen, _operations) =
            setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            });
        shell.handle(Command::Bootstrap);
        let _lost_owner = shell.extension_service.take().unwrap();
        let profile = add_inactive_named_profile(&mut shell, 20_245);

        shell.handle(delete_operation("missing-lifecycle", profile));

        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[ShellTerminalFailure::ExtensionProfileRetirementLifecycleMissing]
        );
        assert!(store.authorized_sessions.lock().unwrap().is_empty());
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
    }
}

#[test]
fn retirement_disposition_callback_mismatches_fail_closed_at_the_app_boundary() {
    // Continued without invoking the continuation must not be mistaken for
    // authorization merely because the copied disposition says Continued.
    {
        let store = Arc::new(FakeStore::default());
        let (extension_service, extension_state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        *extension_state.retirement_invoke_override.lock().unwrap() = Some(false);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let (mut shell, engine, _screen, operations) =
            setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            });
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_250);

        shell.handle(delete_operation("continued-without-callback", profile));

        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[ShellTerminalFailure::ExtensionProfileRetirementContractViolated]
        );
        assert!(shell.profiles.get(profile).is_some());
        assert!(store.authorized_sessions.lock().unwrap().is_empty());
        assert!(store.pending_deletions.lock().unwrap().is_empty());
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
        assert!(operations.lock().unwrap().is_empty());
    }

    // Conversely, an implementation that invokes the continuation and then
    // reports Unavailable may already have created a durable journal row. The
    // app must terminalize with that crash-resumable truth intact and must not
    // advance into native erasure.
    {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
        let (extension_service, extension_state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        extension_state
            .retirement_outcomes
            .lock()
            .unwrap()
            .push_back(ExtensionProfileRetirementDisposition::Unavailable);
        *extension_state.retirement_invoke_override.lock().unwrap() = Some(true);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let (mut shell, engine, _screen, operations) =
            setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            });
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_260);

        shell.handle(delete_operation("callback-before-unavailable", profile));

        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[ShellTerminalFailure::ExtensionProfileRetirementContractViolated]
        );
        assert!(shell.profiles.get(profile).is_none());
        assert_eq!(store.pending_deletions.lock().unwrap().len(), 1);
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
        assert!(operations.lock().unwrap().is_empty());
        assert!(matches!(
            shell.profile_deletion.states[&profile].phase,
            ProfileDeletionPhase::FailedClosed
        ));
    }
}

#[test]
fn impossible_store_authorization_outcomes_after_fence_are_terminal_invariants() {
    for (offset, outcome) in [
        ProfileDeletionAuthorizeOutcome::NotRegistered,
        ProfileDeletionAuthorizeOutcome::SessionConflict,
        ProfileDeletionAuthorizeOutcome::InvalidSession,
        ProfileDeletionAuthorizeOutcome::ExtensionNativeOwnershipPending,
    ]
    .into_iter()
    .enumerate()
    {
        let store = Arc::new(FakeStore::default());
        store.authorize_outcomes.lock().unwrap().push_back(outcome);
        let (extension_service, extension_state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        let failures = Arc::new(Mutex::new(Vec::new()));
        let (mut shell, engine, _screen, operations) =
            setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
                let failures = Arc::clone(&failures);
                Box::new(move |failure| failures.lock().unwrap().push(failure))
            });
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 20_300 + offset as u128 * 10);

        shell.handle(delete_operation("delete-store-conflict", profile));

        assert_eq!(
            failures.lock().unwrap().as_slice(),
            &[ShellTerminalFailure::ExtensionProfileDeletionInvariant],
            "{outcome:?}"
        );
        assert!(shell.profiles.get(profile).is_some(), "{outcome:?}");
        assert!(
            store.pending_deletions.lock().unwrap().is_empty(),
            "{outcome:?}"
        );
        assert!(!engine
            .calls()
            .iter()
            .any(|call| call == &format!("erase-profile {profile}")));
        assert!(operations.lock().unwrap().is_empty(), "{outcome:?}");
        assert_eq!(
            extension_state
                .retirement_continuation_calls
                .load(std::sync::atomic::Ordering::Acquire),
            1,
            "{outcome:?}"
        );
    }
}

#[test]
fn untracked_profile_deletion_command_is_inert() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, _screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 20_500);

    shell.handle(Command::DeleteProfile(profile));

    assert!(shell.profiles.get(profile).is_some());
    assert!(store.authorized_sessions.lock().unwrap().is_empty());
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {profile}")));
}

#[test]
fn profile_deletion_retry_emits_no_intermediate_or_duplicate_completion() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store);
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 21_000);
    engine.push_erasure_outcomes([
        ProfileDataErasureOutcome::TimedOut,
        ProfileDataErasureOutcome::Verified,
    ]);

    shell.handle(delete_operation("delete-retry", profile));
    assert!(operations.lock().unwrap().is_empty());
    assert!(shell.profiles.get(profile).is_none());
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].operation_id, "delete-retry");
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
}

#[test]
fn duplicate_profile_deletion_is_rejected_without_stealing_original_id() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store);
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 22_000);
    engine
        .hold_erasures
        .store(true, std::sync::atomic::Ordering::Release);

    shell.handle(delete_operation("delete-original", profile));
    shell.handle(delete_operation("delete-duplicate", profile));
    {
        let completions = operations.lock().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].operation_id, "delete-duplicate");
        assert_eq!(completions[0].outcome, OperationOutcome::Rejected);
        assert_eq!(
            completions[0].reason,
            OperationReason::ProfileDeletionInProgress
        );
    }

    engine.complete_held_erasure(ProfileDataErasureOutcome::Verified);
    shell.handle(Command::ProfileDeletionReady(profile));
    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 2);
    assert_eq!(
        completions
            .iter()
            .filter(|completion| completion.operation_id == "delete-original")
            .count(),
        1
    );
}

#[test]
fn uncertain_profile_deletion_rpcs_reconcile_from_the_journal() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::OutcomeUnknown);
    store
        .authorize_unknown_commits
        .store(true, std::sync::atomic::Ordering::Release);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::OutcomeUnknown);
    store
        .finalize_unknown_completes
        .store(true, std::sync::atomic::Ordering::Release);
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let profile = add_inactive_named_profile(&mut shell, 23_000);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

    shell.handle(delete_operation("delete-uncertain", profile));

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].operation_id, "delete-uncertain");
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    assert!(store.pending_deletions.lock().unwrap().is_empty());
}

#[test]
fn uncertain_profile_deletion_reauthorization_rebuilds_the_survivor_snapshot() {
    let store = Arc::new(FakeStore::default());
    store.authorize_outcomes.lock().unwrap().extend([
        ProfileDeletionAuthorizeOutcome::OutcomeUnknown,
        ProfileDeletionAuthorizeOutcome::Authorized,
    ]);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, engine, screen, operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, Box::new(|_| {}));
    shell.handle(Command::Bootstrap);
    let survivor = active_id(&screen);
    let profile = add_inactive_named_profile(&mut shell, 23_500);
    // Fail the ordered journal read after the first durability-ambiguous
    // authorization. This leaves a real actor-order window in which newer
    // survivor mutations can be accepted before a safe retry.
    store
        .pending_load_failures
        .store(1, std::sync::atomic::Ordering::Release);

    shell.handle(delete_operation("delete-rebuild", profile));
    assert!(operations.lock().unwrap().is_empty());
    navigate_and_commit(&mut shell, survivor, "new-survivor.example");
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });
    // A stale timer/callback cannot complete the already-removed state a
    // second time.
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });
    shell.handle(Command::ProfileDeletionReady(profile));

    let authorized = store.authorized_sessions.lock().unwrap();
    assert_eq!(authorized.len(), 2);
    let retried = &authorized[1].1;
    assert!(retried
        .profiles
        .iter()
        .all(|candidate| candidate.id != profile));
    assert!(retried.items.iter().any(|item| {
        item.id == survivor
            && matches!(
                &item.kind,
                PersistedKind::Tab { url, .. } if url == "https://new-survivor.example/"
            )
    }));
    let completions = operations.lock().unwrap();
    assert_eq!(
        completions
            .iter()
            .filter(|completion| completion.operation_id == "delete-rebuild")
            .count(),
        1
    );
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        4,
        "the second Store authorization must reacquire the retirement continuation"
    );
}

#[test]
fn delayed_authorization_proof_reschedules_newer_survivor_durability() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::OutcomeUnknown);
    store
        .authorize_unknown_commits
        .store(true, std::sync::atomic::Ordering::Release);
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (mut shell, engine, screen, operations) = setup_with_operation_log(store.clone());
    shell.handle(Command::Bootstrap);
    let survivor = active_id(&screen);
    let profile = add_inactive_named_profile(&mut shell, 23_750);
    store
        .pending_load_failures
        .store(1, std::sync::atomic::Ordering::Release);

    shell.handle(delete_operation("delete-post-barrier", profile));
    navigate_and_commit(&mut shell, survivor, "post-barrier.example");
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
    let generation = shell
        .profile_deletion
        .states
        .get(&profile)
        .unwrap()
        .retry_generation;
    shell.handle(Command::ProfileDeletionRetry {
        profile,
        generation,
    });

    let persisted = store.saved.lock().unwrap().clone().unwrap();
    assert!(persisted
        .profiles
        .iter()
        .all(|candidate| candidate.id != profile));
    assert!(persisted.items.iter().any(|item| {
        item.id == survivor
            && matches!(
                &item.kind,
                PersistedKind::Tab { url, .. } if url == "https://post-barrier.example/"
            )
    }));
    let completions = operations.lock().unwrap();
    assert_eq!(
        completions
            .iter()
            .filter(|completion| completion.operation_id == "delete-post-barrier")
            .count(),
        1
    );
    assert_eq!(completions[0].outcome, OperationOutcome::Applied);
}

#[test]
fn restart_resumes_journaled_native_erasure_before_creating_views() {
    let store = Arc::new(FakeStore::default());
    store
        .authorize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionAuthorizeOutcome::Authorized);
    let (mut first, first_engine, screen, first_operations) =
        setup_with_operation_log(store.clone());
    first.handle(Command::Bootstrap);
    let active = active_id(&screen);
    navigate_and_commit(&mut first, active, "survivor.example");
    let profile = add_inactive_named_profile(&mut first, 24_000);
    first_engine.push_erasure_outcomes([ProfileDataErasureOutcome::Failed]);
    first.handle(delete_operation("delete-before-crash", profile));
    assert!(first_operations.lock().unwrap().is_empty());
    assert_eq!(store.pending_deletions.lock().unwrap().len(), 1);
    drop(first);

    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut restarted, engine, _screen) =
        setup_with_extension_lifecycle(store.clone(), extension_service);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);
    restarted.handle(Command::Bootstrap);

    let calls = engine.calls();
    let erase = calls
        .iter()
        .position(|call| call == &format!("erase-profile {profile}"))
        .unwrap();
    let first_create = calls
        .iter()
        .position(|call| call.starts_with("create "))
        .unwrap();
    assert!(erase < first_create);
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert!(restarted.profile_deletion.states.is_empty());
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2,
        "recovered native erasure and finalization must each reacquire the fence"
    );
    assert_eq!(
        extension_state
            .retirement_profiles
            .lock()
            .unwrap()
            .as_slice(),
        &[profile, profile]
    );
}

#[test]
fn restart_with_native_proof_skips_engine_and_finishes_local_purge() {
    let store = Arc::new(FakeStore::default());
    let default_profile = ProfileId::from(25_000);
    let default_space = SpaceId::from(25_001);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: default_profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: default_space,
            profile: default_profile,
            name: "Space".into(),
        }],
        active_space: Some(default_space),
        ..SessionState::default()
    });
    let removed = ProfileId::from(25_002);
    store
        .pending_deletions
        .lock()
        .unwrap()
        .push(PendingProfileDeletion {
            profile: removed,
            native_erasure_verified: true,
        });
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, engine, _screen) =
        setup_with_extension_lifecycle(store.clone(), extension_service);

    shell.handle(Command::Bootstrap);

    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {removed}")));
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        1,
        "recovered FinalizeReady may skip Engine but never the extension fence"
    );
    assert_eq!(
        extension_state
            .retirement_profiles
            .lock()
            .unwrap()
            .as_slice(),
        &[removed]
    );
}

#[test]
fn recovered_native_ready_waits_for_retirement_without_engine_or_finalize_side_effects() {
    let store = Arc::new(FakeStore::default());
    let default_profile = ProfileId::from(25_100);
    let default_space = SpaceId::from(25_101);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: default_profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: default_space,
            profile: default_profile,
            name: "Space".into(),
        }],
        active_space: Some(default_space),
        ..SessionState::default()
    });
    let removed = ProfileId::from(25_102);
    store
        .pending_deletions
        .lock()
        .unwrap()
        .push(PendingProfileDeletion {
            profile: removed,
            native_erasure_verified: false,
        });
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    extension_state
        .retirement_outcomes
        .lock()
        .unwrap()
        .push_back(ExtensionProfileRetirementDisposition::Unavailable);
    let (mut shell, engine, _screen) =
        setup_with_extension_lifecycle(store.clone(), extension_service);
    engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

    shell.handle(Command::Bootstrap);

    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {removed}")));
    assert!(!store.events.lock().unwrap().contains(&"finalize-delete"));
    assert_eq!(store.pending_deletions.lock().unwrap().len(), 1);
    assert_eq!(
        extension_state
            .retirement_continuation_calls
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    let generation = shell.profile_deletion.states[&removed].retry_generation;

    shell.handle(Command::ProfileDeletionRetry {
        profile: removed,
        generation,
    });

    assert!(engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {removed}")));
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert!(shell.profile_deletion.states.is_empty());
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        3
    );
    assert_eq!(
        extension_state
            .retirement_continuation_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2
    );
}

#[test]
fn recovered_finalize_ready_waits_for_retirement_before_store_finalization() {
    let store = Arc::new(FakeStore::default());
    let default_profile = ProfileId::from(25_200);
    let default_space = SpaceId::from(25_201);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: default_profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: default_space,
            profile: default_profile,
            name: "Space".into(),
        }],
        active_space: Some(default_space),
        ..SessionState::default()
    });
    let removed = ProfileId::from(25_202);
    store
        .pending_deletions
        .lock()
        .unwrap()
        .push(PendingProfileDeletion {
            profile: removed,
            native_erasure_verified: true,
        });
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::Completed);
    let (extension_service, extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    extension_state
        .retirement_outcomes
        .lock()
        .unwrap()
        .push_back(ExtensionProfileRetirementDisposition::Unavailable);
    let (mut shell, engine, _screen) =
        setup_with_extension_lifecycle(store.clone(), extension_service);

    shell.handle(Command::Bootstrap);

    assert!(!store.events.lock().unwrap().contains(&"finalize-delete"));
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {removed}")));
    assert!(matches!(
        shell.profile_deletion.states[&removed].phase,
        ProfileDeletionPhase::FinalizeReady
    ));
    let generation = shell.profile_deletion.states[&removed].retry_generation;

    shell.handle(Command::ProfileDeletionRetry {
        profile: removed,
        generation,
    });

    assert!(store.events.lock().unwrap().contains(&"finalize-delete"));
    assert!(store.pending_deletions.lock().unwrap().is_empty());
    assert_eq!(
        extension_state
            .retirement_calls
            .load(std::sync::atomic::Ordering::Acquire),
        2
    );
}

#[test]
fn recovered_finalize_not_authorized_is_a_terminal_post_fence_invariant() {
    let store = Arc::new(FakeStore::default());
    let default_profile = ProfileId::from(25_300);
    let default_space = SpaceId::from(25_301);
    *store.saved.lock().unwrap() = Some(SessionState {
        profiles: vec![PersistedProfile {
            id: default_profile,
            name: "Personal".into(),
            kind: ProfileKind::Default,
        }],
        spaces: vec![PersistedSpace {
            id: default_space,
            profile: default_profile,
            name: "Space".into(),
        }],
        active_space: Some(default_space),
        ..SessionState::default()
    });
    let removed = ProfileId::from(25_302);
    store
        .pending_deletions
        .lock()
        .unwrap()
        .push(PendingProfileDeletion {
            profile: removed,
            native_erasure_verified: true,
        });
    store
        .finalize_outcomes
        .lock()
        .unwrap()
        .push_back(ProfileDeletionFinalizeOutcome::NotAuthorized);
    let failures = Arc::new(Mutex::new(Vec::new()));
    let (extension_service, _extension_state) =
        extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
    let (mut shell, engine, _screen, _operations) =
        setup_with_operation_log_and_lifecycle(store.clone(), extension_service, {
            let failures = Arc::clone(&failures);
            Box::new(move |failure| failures.lock().unwrap().push(failure))
        });

    shell.handle(Command::Bootstrap);

    assert_eq!(
        failures.lock().unwrap().as_slice(),
        &[ShellTerminalFailure::ExtensionProfileDeletionInvariant]
    );
    assert!(matches!(
        shell.profile_deletion.states[&removed].phase,
        ProfileDeletionPhase::FailedClosed
    ));
    assert!(store
        .pending_deletions
        .lock()
        .unwrap()
        .iter()
        .any(|deletion| deletion.profile == removed));
    assert!(!engine
        .calls()
        .iter()
        .any(|call| call.starts_with("create ")));
}

#[test]
fn retryable_store_authorization_refusals_retain_quarantine_and_reacquire_fence() {
    for (offset, first) in [
        ProfileDeletionAuthorizeOutcome::NotAdmitted,
        ProfileDeletionAuthorizeOutcome::Failed,
    ]
    .into_iter()
    .enumerate()
    {
        let store = Arc::new(FakeStore::default());
        store
            .authorize_outcomes
            .lock()
            .unwrap()
            .extend([first, ProfileDeletionAuthorizeOutcome::Authorized]);
        store
            .finalize_outcomes
            .lock()
            .unwrap()
            .push_back(ProfileDeletionFinalizeOutcome::Completed);
        let (extension_service, extension_state) =
            extension_lifecycle_with_outcome(ExtensionServiceShutdownOutcome::Clean);
        let (mut shell, engine, _screen, operations) = setup_with_operation_log_and_lifecycle(
            store.clone(),
            extension_service,
            Box::new(|_| {}),
        );
        shell.handle(Command::Bootstrap);
        let profile = add_inactive_named_profile(&mut shell, 25_400 + offset as u128 * 10);
        engine.push_erasure_outcomes([ProfileDataErasureOutcome::Verified]);

        shell.handle(delete_operation("retry-store-authorization", profile));

        assert!(shell.profile_deletion_quarantines(profile));
        assert!(shell.profiles.get(profile).is_some());
        assert!(store.pending_deletions.lock().unwrap().is_empty());
        assert!(operations.lock().unwrap().is_empty());
        let generation = shell.profile_deletion.states[&profile].retry_generation;

        shell.handle(Command::ProfileDeletionRetry {
            profile,
            generation,
        });

        assert!(!shell.profile_deletion.states.contains_key(&profile));
        assert!(shell.profiles.get(profile).is_none());
        assert!(store.pending_deletions.lock().unwrap().is_empty());
        assert_eq!(operations.lock().unwrap().len(), 1);
        assert_eq!(
            extension_state
                .retirement_calls
                .load(std::sync::atomic::Ordering::Acquire),
            4,
            "authorization retry plus native erasure and finalization each reacquire the fence"
        );
    }
}

#[test]
fn profile_deletion_policy_rejects_default_active_private_and_last_profile() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, _screen, operations) = setup_with_operation_log(store);
    shell.handle(Command::Bootstrap);
    let default = shell.windows.focused().unwrap().profile;

    shell.handle(delete_operation("delete-default", default));
    let private = ProfileId::from(26_000);
    assert!(shell.profiles.insert(Profile {
        id: private,
        name: "Private".into(),
        kind: ProfileKind::Incognito,
    }));
    shell.handle(delete_operation("delete-private", private));

    let completions = operations.lock().unwrap().clone();
    assert_eq!(completions.len(), 2);
    assert!(completions.iter().all(|completion| {
        completion.outcome == OperationOutcome::Rejected
            && completion.reason == OperationReason::ProfileDeletionPolicyRejected
    }));

    let (mut last_only, _engine, _screen) = setup();
    last_only.bootstrapped = true;
    let only = ProfileId::from(26_100);
    assert!(last_only.profiles.insert(Profile {
        id: only,
        name: "Only".into(),
        kind: ProfileKind::Named,
    }));
    assert!(last_only
        .filtered_session_for_profile_deletion(only)
        .is_none());
}
