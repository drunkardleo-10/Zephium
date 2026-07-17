use super::*;

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
    let (mut shell, engine, _screen, operations) = setup_with_operation_log(store.clone());
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
    let (mut shell, engine, screen, operations) = setup_with_operation_log(store.clone());
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
    let (mut restarted, engine, _screen) = setup_with(store.clone());
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
    let (mut shell, engine, _screen) = setup_with(store.clone());

    shell.handle(Command::Bootstrap);

    assert!(!engine
        .calls()
        .iter()
        .any(|call| call == &format!("erase-profile {removed}")));
    assert!(store.pending_deletions.lock().unwrap().is_empty());
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
