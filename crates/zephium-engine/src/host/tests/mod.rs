use super::*;

#[test]
fn native_resource_ceiling_counts_spare_and_every_cleanup_debt() {
    assert_eq!(owned_native_view_resources(32, false, 0), Some(32));
    assert_eq!(owned_native_view_resources(32, true, 0), Some(33));
    assert_eq!(owned_native_view_resources(32, true, 8), Some(41));
    assert_eq!(owned_native_view_resources(usize::MAX, true, 0), None);
}

#[test]
fn native_profile_process_group_ceiling_counts_distinct_profiles_only() {
    let profiles: Vec<_> = (1..=MAX_NATIVE_PROFILE_PROCESS_GROUPS)
        .map(|id| ProfileId::from(id as u128))
        .collect();
    let existing = profiles[0];
    let new = ProfileId::from(10_000);

    assert!(profile_process_group_capacity_allows(
        profiles.iter().copied().chain([existing, existing]),
        existing,
    ));
    assert!(!profile_process_group_capacity_allows(
        profiles.iter().copied().chain([profiles[0]]),
        new,
    ));
    assert!(profile_process_group_capacity_allows(
        profiles[..MAX_NATIVE_PROFILE_PROCESS_GROUPS - 1]
            .iter()
            .copied(),
        new,
    ));
}

#[test]
fn native_construction_reservation_is_bounded_and_released_exactly() {
    let mut reservations = NativeViewReservations::default();
    assert_eq!(
        reservations.try_reserve(MAX_NATIVE_VIEW_RESOURCES - 1),
        Ok(true)
    );
    assert_eq!(reservations.in_construction(), 1);
    // Re-entry/retry observes the first construction reservation and may
    // not allocate the forty-ninth native resource.
    assert_eq!(
        reservations.try_reserve(MAX_NATIVE_VIEW_RESOURCES - 1),
        Ok(false)
    );
    assert_eq!(reservations.in_construction(), 1);
    assert_eq!(reservations.release(), Ok(()));
    assert_eq!(reservations.in_construction(), 0);
    assert_eq!(reservations.release(), Err(()));
}

#[test]
fn profile_id_never_crosses_durable_and_ephemeral_storage_classes() {
    let durable_first = ProfileId::from(31);
    let ephemeral_first = ProfileId::from(32);
    let mut bindings = HashMap::new();

    assert!(bind_profile_persistence_class(
        &mut bindings,
        Partition::Default(durable_first)
    ));
    assert!(bind_profile_persistence_class(
        &mut bindings,
        Partition::Persistent(durable_first)
    ));
    assert!(!bind_profile_persistence_class(
        &mut bindings,
        Partition::Ephemeral(durable_first)
    ));
    assert!(bind_profile_persistence_class(
        &mut bindings,
        Partition::Default(durable_first)
    ));

    assert!(bind_profile_persistence_class(
        &mut bindings,
        Partition::Ephemeral(ephemeral_first)
    ));
    assert!(!bind_profile_persistence_class(
        &mut bindings,
        Partition::Default(ephemeral_first)
    ));
    assert!(!bind_profile_persistence_class(
        &mut bindings,
        Partition::Persistent(ephemeral_first)
    ));
    assert!(bind_profile_persistence_class(
        &mut bindings,
        Partition::Ephemeral(ephemeral_first)
    ));
}

#[test]
fn profile_scoped_values_reuse_within_profile_and_isolate_profiles() {
    let first = ProfileId::from(41);
    let second = ProfileId::from(42);
    let mut values = HashMap::new();
    let mut next = 100usize;

    let first_value = profile_scoped_value(&mut values, first, || {
        next += 1;
        Ok::<_, ()>(next)
    })
    .unwrap();
    let first_again = profile_scoped_value(&mut values, first, || {
        next += 1;
        Ok::<_, ()>(next)
    })
    .unwrap();
    let second_value = profile_scoped_value(&mut values, second, || {
        next += 1;
        Ok::<_, ()>(next)
    })
    .unwrap();

    assert_eq!(first_value, first_again);
    assert_ne!(first_value, second_value);
    assert_eq!(
        next, 102,
        "the same profile must not invoke the factory twice"
    );
    assert!(profile_value_is_isolated(
        &values,
        first,
        &first_value,
        |left, right| left == right,
    ));

    values.insert(second, first_value);
    assert!(!profile_value_is_isolated(
        &values,
        first,
        &first_value,
        |left, right| left == right,
    ));
}

#[test]
fn profile_persistence_binding_is_bounded_without_evicting_old_proof() {
    let mut bindings = HashMap::new();
    for value in 0..MAX_PROFILE_PERSISTENCE_BINDINGS {
        assert!(bind_profile_persistence_class(
            &mut bindings,
            Partition::Persistent(ProfileId::from(value as u128 + 1))
        ));
    }
    assert!(!bind_profile_persistence_class(
        &mut bindings,
        Partition::Persistent(ProfileId::from(
            MAX_PROFILE_PERSISTENCE_BINDINGS as u128 + 1
        ))
    ));
    assert!(bind_profile_persistence_class(
        &mut bindings,
        Partition::Default(ProfileId::from(1))
    ));
    assert!(!bind_profile_persistence_class(
        &mut bindings,
        Partition::Ephemeral(ProfileId::from(1))
    ));
}

#[test]
fn failed_profile_erasure_stays_tombstoned_and_is_retryable() {
    use std::sync::mpsc;

    let profile = ProfileId::from(44);
    let mut tombstones = HashSet::new();
    let mut attempts = HashMap::new();

    let (first_tx, first_rx) = mpsc::channel();
    let first = crate::erasure::Completion::start(
        Box::new(move |outcome| {
            first_tx.send(outcome).unwrap();
        }),
        Arc::new(AtomicBool::new(true)),
    );
    assert!(admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        profile,
        &first
    ));
    assert!(tombstones.contains(&profile));

    let (duplicate_tx, duplicate_rx) = mpsc::channel();
    let duplicate = crate::erasure::Completion::start(
        Box::new(move |outcome| {
            duplicate_tx.send(outcome).unwrap();
        }),
        Arc::new(AtomicBool::new(true)),
    );
    assert!(!admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        profile,
        &duplicate
    ));
    assert_eq!(
        duplicate_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap(),
        zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
    );

    first.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
    assert_eq!(
        first_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap(),
        zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
    );
    assert!(tombstones.contains(&profile));

    let retry =
        crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
    assert!(admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        profile,
        &retry
    ));
    assert!(tombstones.contains(&profile));
    retry.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
}

#[test]
fn caller_timeout_before_host_dispatch_still_tombstones_and_blocks_retry() {
    use std::sync::mpsc;

    let profile = ProfileId::from(45);
    let mut tombstones = HashSet::new();
    let mut attempts = HashMap::new();
    let (tx, rx) = mpsc::channel();
    let delayed = crate::erasure::Completion::start(
        Box::new(move |outcome| {
            tx.send(outcome).unwrap();
        }),
        Arc::new(AtomicBool::new(true)),
    );

    delayed.report_unsettled(zephium_core::ports::engine::ProfileDataErasureOutcome::TimedOut);
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .unwrap(),
        zephium_core::ports::engine::ProfileDataErasureOutcome::TimedOut
    );
    assert!(admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        profile,
        &delayed
    ));
    assert!(tombstones.contains(&profile));

    let retry =
        crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
    assert!(!admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        profile,
        &retry
    ));

    delayed.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
    let settled_retry =
        crate::erasure::Completion::start(Box::new(|_| {}), Arc::new(AtomicBool::new(true)));
    assert!(admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        profile,
        &settled_retry
    ));
    settled_retry.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Verified);
}

#[test]
fn host_erasure_proof_maps_reject_new_profiles_at_profile_bound() {
    use std::sync::mpsc;

    let mut tombstones = HashSet::new();
    let mut attempts = HashMap::new();
    for value in 0..zephium_core::session::MAX_SESSION_PROFILES {
        let profile = ProfileId::from(value as u128 + 1);
        tombstones.insert(profile);
        attempts.insert(profile, Arc::new(AtomicBool::new(false)));
    }
    let (tx, rx) = mpsc::channel();
    let completion = crate::erasure::Completion::start(
        Box::new(move |outcome| tx.send(outcome).unwrap()),
        Arc::new(AtomicBool::new(true)),
    );

    assert!(!admit_profile_erasure(
        &mut tombstones,
        &mut attempts,
        ProfileId::from(100_000),
        &completion,
    ));
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .unwrap(),
        zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
    );
    assert_eq!(
        tombstones.len(),
        zephium_core::session::MAX_SESSION_PROFILES
    );
    assert_eq!(attempts.len(), zephium_core::session::MAX_SESSION_PROFILES);
}

#[test]
fn renderer_crash_classification_never_exposes_dead_spare() {
    let spare = ItemId::from(1);
    let live = ItemId::from(2);
    let retired = ItemId::from(3);
    assert_eq!(
        renderer_crash_target(Some(spare), false, spare),
        RendererCrashTarget::Spare
    );
    assert_eq!(
        renderer_crash_target(Some(spare), true, live),
        RendererCrashTarget::Live
    );
    assert_eq!(
        renderer_crash_target(Some(spare), false, retired),
        RendererCrashTarget::Retired
    );
}

#[test]
fn attempted_webview2_construction_prevents_empty_maps_from_proving_absence() {
    assert!(browser_group_absence_is_proven(false, false, false));
    assert!(!browser_group_absence_is_proven(false, false, true));
    assert!(!browser_group_absence_is_proven(true, false, false));
    assert!(!browser_group_absence_is_proven(false, true, false));
}

#[test]
fn recovery_requires_a_present_exact_signalled_browser_process() {
    assert!(exact_browser_process_exit_proves_recovery(
        Some((41, true)),
        41,
        41,
        true,
    ));
    assert!(!exact_browser_process_exit_proves_recovery(
        None, 41, 41, true,
    ));
    assert!(!exact_browser_process_exit_proves_recovery(
        Some((41, false)),
        41,
        41,
        true,
    ));
    assert!(!exact_browser_process_exit_proves_recovery(
        Some((42, true)),
        41,
        41,
        true,
    ));
    assert!(!exact_browser_process_exit_proves_recovery(
        Some((41, true)),
        41,
        42,
        true,
    ));
    assert!(!exact_browser_process_exit_proves_recovery(
        Some((41, true)),
        41,
        41,
        false,
    ));
}

#[test]
fn successful_windows_profile_erasure_restores_the_shutdown_map_invariant() {
    assert_eq!(
        transferred_erasure_exit_settlement(41, 41, true, true, false),
        TransferredErasureExitSettlement::Proven
    );
    assert!(
        !windows_profile_provenance_presence_is_consistent(false, false, true, false),
        "an observer retained after transferring the other obligations must block shutdown"
    );
    assert!(
        windows_profile_provenance_presence_is_consistent(false, false, false, false),
        "the exact exit callback releases the final observer-only entry"
    );

    assert_eq!(
        transferred_erasure_exit_settlement(41, 41, true, false, true),
        TransferredErasureExitSettlement::Invalid
    );
    assert_eq!(
        transferred_erasure_exit_settlement(41, 41, false, true, false),
        TransferredErasureExitSettlement::Stale
    );
    assert_eq!(
        transferred_erasure_exit_settlement(41, 41, true, false, false),
        TransferredErasureExitSettlement::Pending
    );
}

#[test]
fn native_navigation_observations_are_bounded_deduplicated_and_filtered() {
    let id = ItemId::from(7);
    let mut previous = NavigationSnapshot::default();

    let initial = navigation_observation_events(
        id,
        &mut previous,
        Some("https://example.com/"),
        Some((false, false)),
    );
    assert_eq!(initial.len(), 2);
    assert!(matches!(
        &initial[0],
        EngineEvent::UrlChanged { id: observed, url }
            if *observed == id && url == "https://example.com/"
    ));
    assert!(matches!(
        initial[1],
        EngineEvent::NavState {
            id: observed,
            can_go_back: false,
            can_go_forward: false,
        } if observed == id
    ));

    assert!(navigation_observation_events(
        id,
        &mut previous,
        Some("https://example.com/"),
        Some((false, false)),
    )
    .is_empty());

    let same_document = navigation_observation_events(
        id,
        &mut previous,
        Some("https://example.com/#state"),
        Some((true, false)),
    );
    assert_eq!(same_document.len(), 2);

    let forbidden = navigation_observation_events(
        id,
        &mut previous,
        Some("file:///etc/passwd"),
        Some((true, true)),
    );
    assert_eq!(forbidden.len(), 1);
    assert!(matches!(
        forbidden[0],
        EngineEvent::NavState {
            can_go_back: true,
            can_go_forward: true,
            ..
        }
    ));
    assert_eq!(previous.url.as_deref(), Some("https://example.com/#state"));
}

#[test]
fn native_url_policy_distinguishes_unavailable_from_forbidden_sources() {
    assert_eq!(classify_observed_url(None), ObservedUrl::Unavailable);
    assert_eq!(
        classify_observed_url(Some(String::new())),
        ObservedUrl::Unavailable
    );
    assert_eq!(
        classify_observed_url(Some("https://example.com/#state".to_owned())),
        ObservedUrl::Allowed("https://example.com/#state".to_owned())
    );
    assert_eq!(
        classify_observed_url(Some("file:///etc/passwd".to_owned())),
        ObservedUrl::Forbidden
    );
}

fn commit_test_navigation(
    tracker: &NavigationEpochTracker,
    native_id: u64,
    target: &str,
) -> NavigationEpoch {
    let epoch = tracker.begin(target).expect("allowed test URL");
    for phase in [
        wry::NavigationEventPhase::Started,
        wry::NavigationEventPhase::Committed,
    ] {
        assert!(tracker
            .observe_navigation(&wry::NavigationEvent {
                id: wry::NavigationId::from_raw(native_id),
                phase,
                url: target.to_owned(),
            })
            .is_some());
    }
    epoch
}

#[test]
fn unavailable_native_url_uses_only_the_exact_committed_snapshot() {
    let tracker = NavigationEpochTracker::new();
    let epoch = commit_test_navigation(&tracker, 71, "https://committed.example/path");
    assert_eq!(
        resolve_committed_observed_url(&tracker, epoch, None),
        ObservedUrl::Allowed("https://committed.example/path".to_owned())
    );

    let pending = tracker.begin("https://pending.example/").unwrap();
    assert_eq!(
        resolve_committed_observed_url(&tracker, pending, None),
        ObservedUrl::Unavailable
    );
    assert_eq!(
        resolve_committed_observed_url(&tracker, epoch, None),
        ObservedUrl::Unavailable
    );
}

#[test]
fn provisional_failure_can_restore_real_hidden_commit_but_not_spare_bootstrap() {
    let tracker = NavigationEpochTracker::new();
    let bootstrap = tracker.begin("about:blank").unwrap();
    let real = tracker.begin("https://real.example/").unwrap();

    assert!(!restored_navigation_can_present(
        false,
        Some(bootstrap),
        bootstrap
    ));
    assert!(restored_navigation_can_present(
        false,
        Some(bootstrap),
        real
    ));
    assert!(restored_navigation_can_present(
        true,
        Some(bootstrap),
        bootstrap
    ));
}

#[test]
fn failed_navigation_cannot_reveal_its_stale_epoch() {
    let tracker = NavigationEpochTracker::new();
    let visible = commit_test_navigation(&tracker, 81, "https://visible.example/");
    let failed = tracker.begin("https://failed.example/").unwrap();
    assert!(tracker
        .observe_navigation(&wry::NavigationEvent {
            id: wry::NavigationId::from_raw(82),
            phase: wry::NavigationEventPhase::Started,
            url: "https://failed.example/".to_owned(),
        })
        .is_some());
    assert!(tracker
        .observe_navigation(&wry::NavigationEvent {
            id: wry::NavigationId::from_raw(82),
            phase: wry::NavigationEventPhase::Failed,
            url: "https://failed.example/".to_owned(),
        })
        .is_some());

    assert_eq!(
        resolve_committed_observed_url(&tracker, failed, None),
        ObservedUrl::Unavailable
    );
    assert_eq!(
        resolve_committed_observed_url(&tracker, visible, None),
        ObservedUrl::Allowed("https://visible.example/".to_owned())
    );
}

#[test]
fn forbidden_native_url_never_falls_back_to_trusted_chrome() {
    let tracker = NavigationEpochTracker::new();
    let epoch = commit_test_navigation(&tracker, 91, "https://trusted.example/");
    assert_eq!(
        resolve_committed_observed_url(&tracker, epoch, Some("file:///etc/passwd".to_owned())),
        ObservedUrl::Forbidden
    );
    assert_eq!(
        tracker.committed_snapshot(),
        Some((epoch, "https://trusted.example/".to_owned()))
    );
}

#[test]
fn discard_report_accepts_only_the_exact_safe_primitive_mask() {
    assert!(renderer_report_allows_discard("1"));
    for protected in [
        "0", "2", "3", "255", "256", "511", "null", "\"1\"", "{}", " 1",
    ] {
        assert!(
            !renderer_report_allows_discard(protected),
            "alternate renderer value must veto discard: {protected:?}"
        );
    }
    assert!(DISCARD_SAFETY_BOOTSTRAP_JS.contains("localUncertain ? 256 : 0"));
    assert!(DISCARD_SAFETY_QUERY_JS.contains("return 256"));
    assert!(!DISCARD_SAFETY_QUERY_JS.contains("return {"));
}

#[test]
fn unchanged_layout_does_not_reseed_retained_stage_readiness() {
    assert!(should_seed_stage_readiness(true, true, true));
    assert!(!should_seed_stage_readiness(false, true, true));
    assert!(!should_seed_stage_readiness(true, false, true));
    assert!(!should_seed_stage_readiness(true, true, false));

    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    assert!(
        source
            .matches("should_seed_stage_readiness(\n                inserted,")
            .count()
            >= 2
    );
}

#[test]
fn raw_page_print_guard_is_installed_for_subframes_before_user_scripts() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let all_frames_call = [
        "builder.with_initialization_script_for_main_only(",
        "crate::PAGE_PRINT_DENY_SCRIPT, false)",
    ]
    .concat();
    assert_eq!(source.matches(&all_frames_call).count(), 1);
    let guard = source.find(&all_frames_call).unwrap();
    let user_scripts = source.find("for script in scripts").unwrap();
    assert!(guard < user_scripts);
}

#[test]
fn raw_popups_use_wrys_synchronous_deny_without_metadata_path() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let raw_policy = source
        .split("let mut builder = builder")
        .nth(1)
        .expect("raw view policy builder")
        .split("// This host-owned guard")
        .next()
        .expect("pre-script raw view policy");
    assert!(!raw_policy.contains("with_new_window_req_handler"));
    assert!(raw_policy.contains("Intentionally do not install a new-window callback"));
}

#[test]
fn both_successful_view_insertion_paths_reconcile_retained_layouts() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let create_view = source
        .split("pub(crate) fn create_view(")
        .nth(1)
        .expect("view creation implementation")
        .split("// Rebuilt after adoption")
        .next()
        .expect("bounded view creation implementation");

    // One call follows warm-spare adoption and one follows a fresh native
    // build. Keeping both prevents latest-layout coalescing from stranding
    // either construction path behind an earlier queued layout task.
    assert_eq!(
        create_view
            .matches("self.finish_new_view_insertion")
            .count(),
        2
    );
    assert!(source.contains("filter(|stage| stage.contains_item(id))"));
    assert!(source.contains("Stage::exclude_unstaged(view)"));
}

#[test]
fn windows_superseded_native_placement_requeues_without_spending_failure_budget() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/windows/stage.rs"
    ));
    let application = source
        .split("fn apply_native_placement(")
        .nth(1)
        .expect("Windows placement application")
        .split("fn conceal_superseded_placement")
        .next()
        .expect("bounded Windows placement application");
    for (start, end, primitive) in [
        (
            "if placement.delta.container_rect",
            "if placement.delta.rounded",
            "SetWindowPos(",
        ),
        (
            "if placement.delta.rounded",
            "if placement.delta.controller_size",
            "SetWindowRgn(",
        ),
        (
            "if placement.delta.controller_size",
            "if placement.delta.notify_parent_position",
            "SetBounds(",
        ),
        (
            "if placement.delta.notify_parent_position",
            "// COM can re-enter the message pump",
            "NotifyParentWindowPositionChanged()",
        ),
    ] {
        let boundary = application
            .split(start)
            .nth(1)
            .expect("native geometry boundary")
            .split(end)
            .next()
            .expect("bounded native geometry boundary");
        assert!(
            boundary.find(primitive).unwrap() < boundary.find("if !placement_is_current").unwrap()
        );
        assert!(boundary.contains("conceal_superseded_placement"));
    }
    let conceal = source
        .split("fn conceal_superseded_placement")
        .nth(1)
        .expect("superseded geometry concealment")
        .split("fn finish_native_placement")
        .next()
        .expect("bounded superseded geometry concealment");
    assert!(conceal.contains("SW_HIDE"));
    assert!(conceal.contains("SetIsVisible(false)"));
    assert!(conceal.contains("finish_native_placement(state, placement, applied, false)"));

    let settlement = source
        .split("fn finish_native_placement(")
        .nth(1)
        .expect("Windows placement settlement")
        .split("fn draw_indicator(")
        .next()
        .expect("bounded Windows placement settlement");

    assert!(settlement.contains("state.revision != placement.revision"));
    assert!(settlement.contains("if superseded || failed"));
    let superseded = settlement
        .split("if superseded {")
        .nth(1)
        .expect("superseded placement branch")
        .split("} else if failed {")
        .next()
        .expect("bounded superseded placement branch");
    assert!(superseded.contains("retry = true"));
    assert!(!superseded.contains("consecutive_failures"));
}

#[test]
fn windows_reentrant_hide_preserves_newer_cache_and_forces_an_exact_redrive() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/windows/stage.rs"
    ));
    let hide = source
        .split("fn hide_views(")
        .nth(1)
        .expect("Windows immediate hide")
        .split("fn hide_is_current")
        .next()
        .expect("bounded Windows immediate hide");
    assert!(hide.contains("revision: state.revision"));
    for primitive in ["ShowWindow(hide.container, SW_HIDE)", "SetIsVisible(false)"] {
        let boundary = hide.find(primitive).expect("native hide boundary");
        assert!(hide[boundary..].contains("hide_is_current("));
        assert!(hide[boundary..].contains("redrive_uncertain_visibility("));
    }

    let settlement = source
        .split("fn finish_native_placement(")
        .nth(1)
        .expect("Windows placement settlement")
        .split("fn draw_indicator(")
        .next()
        .expect("bounded Windows placement settlement");
    let superseded = settlement
        .split("if superseded {")
        .nth(1)
        .expect("superseded cache branch")
        .split("} else if failed {")
        .next()
        .expect("bounded superseded cache branch");
    assert!(!superseded.contains("view.applied.set"));
    assert!(settlement.contains("state.visibility_uncertain.insert(placement.id)"));
}

#[test]
fn collapsed_split_leaves_hide_without_invalid_native_bounds_and_reappear_on_resize() {
    // The shared pure helper has collapse/grow unit coverage. These
    // platform-boundary assertions ensure every stage uses that decision
    // before its native geometry/reveal primitive.
    let windows = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/windows/stage.rs"
    ));
    let windows_layout = windows
        .split("let pane_rects = tree")
        .nth(1)
        .expect("Windows pane geometry")
        .split("let mut parent_screen")
        .next()
        .expect("bounded Windows pane geometry");
    assert!(windows_layout.contains(".filter_map"));
    assert!(windows_layout.contains("rounded_native_size"));
    assert!(!windows_layout.contains(".max(0)"));

    let linux = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/linux/stage.rs"
    ));
    let linux_placement = linux
        .split("let placements = views")
        .nth(1)
        .expect("Linux pane placement")
        .split("// Fail closed")
        .next()
        .expect("bounded Linux pane placement");
    assert!(linux_placement.contains("rounded_native_size"));
    assert!(linux_placement.contains("pane.is_some()"));
    let linux_geometry = linux
        .split("// Keep provisional current-layout widgets mapped")
        .nth(1)
        .expect("Linux native geometry")
        .split("fn revision_is_current")
        .next()
        .expect("bounded Linux native geometry");
    assert!(!linux_geometry.contains("max(1.0)"));
    assert!(linux_geometry.contains("fixed.move_(&view.view, parked_x, 0)"));
    assert!(linux_geometry.contains("view.view.set_size_request"));

    let mac = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/macos/stage.rs"
    ));
    let resize = mac
        .split("fn resize_subviews")
        .nth(1)
        .expect("macOS resize callback")
        .split("fn mouse_down")
        .next()
        .expect("bounded macOS resize callback");
    assert!(resize.contains("self.bump_layout_epoch()"));
    assert!(resize.contains("self.sync_visibility()"));
    let visibility = mac
        .split("fn sync_visibility(&self)")
        .nth(1)
        .expect("macOS visibility pass")
        .split("fn defer_stage_retry")
        .next()
        .expect("bounded macOS visibility pass");
    assert!(visibility.contains("paintable.contains(id)"));
}

#[test]
fn terminal_native_stage_failures_have_exact_retirement_and_mandatory_fatal_handoff() {
    let host = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let mac_failure = host
        .split("fn on_macos_stage_failure(")
        .nth(1)
        .expect("macOS terminal stage handler")
        .split("fn ensure_stage(")
        .next()
        .expect("bounded macOS terminal stage handler");
    assert!(mac_failure.contains("Retained::as_ptr(stage) as usize != failed_identity"));
    assert!(
        mac_failure.find("attached_items()").unwrap() < mac_failure.find("stages.remove").unwrap()
    );
    assert!(mac_failure.contains("self.native_terminal_failure"));

    let mac_stage = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/macos/stage.rs"
    ));
    assert!(mac_stage.contains("pub fn attached_items(&self) -> Option<Vec<ItemId>>"));
    let container = mac_stage
        .split("fn sync_container_visibility(&self)")
        .nth(1)
        .expect("macOS container reveal")
        .split("fn bump_layout_epoch")
        .next()
        .expect("bounded macOS container reveal");
    let reveal = container.find("self.setHidden(false)").unwrap();
    assert!(container[..reveal].contains("stage_retry_terminal"));
    assert!(container[reveal..].contains("stage_retry_terminal"));

    for reason in [
        "terminal Windows stage failure was not admitted by the engine host",
        "terminal macOS stage failure was not admitted by the engine host",
    ] {
        let failure = host.find(reason).expect("terminal admission handoff");
        assert!(host[..failure].rfind("if !admitted").is_some());
        assert!(host[..failure].rfind("native_terminal_failure").is_some());
    }
}

#[test]
fn macos_divider_capture_survives_geometry_only_relayout_but_not_topology_change() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/macos/stage.rs"
    ));
    let set_tree = source
        .split("pub fn set_tree(&self, tree: Option<Pane>)")
        .nth(1)
        .expect("macOS stage tree setter")
        .split("pub fn set_on_ratio")
        .next()
        .expect("bounded macOS stage tree setter");

    assert!(set_tree.contains("!current.same_topology(next)"));
    let geometry_only = set_tree
        .split("if drag_active && !topology_changed")
        .nth(1)
        .expect("geometry-only drag guard")
        .split("if topology_changed")
        .next()
        .expect("bounded geometry-only drag guard");
    assert!(geometry_only.contains("return true;"));
    let topology_change = set_tree
        .split("if topology_changed")
        .nth(1)
        .expect("topology-change capture revocation");
    assert!(topology_change.contains("drag.take()"));
    assert!(
        topology_change.find("drag.take()").unwrap()
            < topology_change.find("*current = next").unwrap()
    );

    let begin_update = source
        .split("pub fn begin_content_update(&self, visible: bool)")
        .nth(1)
        .expect("macOS stage content-update reservation")
        .split("pub fn content_update_is_current")
        .next()
        .expect("bounded content-update reservation");
    let hidden = begin_update
        .split("if !visible")
        .nth(1)
        .expect("hidden-stage capture revocation");
    assert!(hidden.contains("drag.take()"));
}

#[test]
fn linux_stage_exhaustion_retains_one_coalesced_idle_redrive() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/linux/stage.rs"
    ));
    let sync = source
        .split("fn sync(")
        .nth(1)
        .expect("Linux stage sync")
        .split("fn schedule_sync")
        .next()
        .expect("bounded Linux stage sync");
    assert!(sync.matches("schedule_sync(state, sync_scheduled)").count() >= 2);
    assert!(sync.contains("conceal_views_until_retry(state)"));

    let retry = source
        .split("fn schedule_sync")
        .nth(1)
        .expect("Linux stage retry driver")
        .split("fn make_indicator")
        .next()
        .expect("bounded Linux stage retry driver");
    assert!(retry.contains("sync_scheduled.replace(true)"));
    assert!(retry.contains("glib::idle_add_local_once"));
    assert!(retry.contains("Rc::downgrade(state)"));
    assert!(retry.contains("sync_scheduled.set(false)"));
    assert!(retry.contains("sync(&state, &sync_scheduled)"));
    assert!(retry.contains("revoke_and_unmap_for_retry(state, revision, &view.view)"));

    let fail_closed = source
        .split("fn revoke_and_unmap_for_retry")
        .nth(1)
        .expect("Linux fail-closed retry barrier")
        .split("fn view_may_reveal")
        .next()
        .expect("bounded Linux fail-closed retry barrier");
    assert!(fail_closed.contains("view.set_sensitive(false)"));
    assert!(fail_closed.contains("view.set_opacity(0.0)"));
    assert_eq!(
        fail_closed.matches("view.set_child_visible(false)").count(),
        2
    );
}

#[test]
fn raw_native_views_never_request_focus_during_construction() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let raw_policy = source
        .split("let mut builder = builder")
        .nth(1)
        .expect("raw view policy builder")
        .split("// This host-owned guard")
        .next()
        .expect("pre-script raw view policy");
    assert_eq!(raw_policy.matches(".with_focused(false)").count(), 1);
}

#[test]
fn native_completion_waits_for_shell_ordered_presentation_acknowledgement() {
    let host = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let permits = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/permits.rs"));
    let completion = permits
        .split("fn queue_navigation_completion(")
        .nth(1)
        .expect("navigation completion queue")
        .split("fn queue_navigation_failure(")
        .next()
        .expect("bounded navigation completion queue");
    assert!(completion.contains("emit_navigation_observation"));
    assert!(completion.contains("emit_navigation_ready"));
    assert!(!completion.contains("present_navigation_epoch"));

    let ready = host
        .split("fn emit_navigation_ready(")
        .nth(1)
        .expect("navigation-ready emitter")
        .split("fn present_navigation_epoch(")
        .next()
        .expect("bounded navigation-ready emitter");
    assert!(ready.contains("EngineEvent::PresentationReady"));
}

#[test]
fn every_identity_bearing_commit_rearms_presentation_but_history_observation_does_not() {
    let host = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let permits = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/permits.rs"));
    let native_handler = host
        .split("builder = builder.with_navigation_event_handler")
        .nth(1)
        .expect("raw navigation identity handler")
        .split("#[cfg(target_os = \"windows\")]")
        .next()
        .expect("bounded raw navigation identity handler");
    let committed = native_handler
        .split("NavigationTransition::Committed(epoch)")
        .nth(1)
        .expect("identity-bearing commit branch")
        .split("NavigationTransition::Finished(epoch)")
        .next()
        .expect("bounded identity-bearing commit branch");
    assert!(committed.contains("queue_navigation_commit"));

    let commit_queue = permits
        .split("fn queue_navigation_commit(")
        .nth(1)
        .expect("commit presentation queue")
        .split("fn queue_navigation_completion(")
        .next()
        .expect("bounded commit presentation queue");
    assert!(commit_queue.contains("rearm_navigation_presentation"));
    assert!(commit_queue.contains("emit_navigation_observation"));

    let source_observer = host
        .split("let observer = match crate::platform::imp::install_navigation_observer")
        .nth(1)
        .expect("same-document source observer")
        .split("if !event_permit.allows_navigation(url)")
        .next()
        .expect("bounded same-document source observer");
    assert!(source_observer.contains("emit_navigation_observation"));
    assert!(!source_observer.contains("rearm_navigation_presentation"));

    // Reload, history traversal, explicit navigation and page-driven
    // navigation all converge on the same native Committed transition.
    // The Wry guard hides before callback admission on all desktop ports.
    let raw_policy = host
        .split("let mut builder = builder")
        .nth(1)
        .expect("raw view policy builder")
        .split("// This host-owned guard")
        .next()
        .expect("pre-script raw view policy");
    assert!(raw_policy.contains("with_navigation_presentation_guard(move ||"));
    assert!(raw_policy.contains("guard_presentation_permit.store(false, Ordering::Release)"));
    let webview2 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/wry/src/webview2/mod.rs"
    ));
    assert!(webview2.contains("navigation_presentation_guard"));
    assert!(webview2.contains("ShowWindow(hwnd, SW_HIDE)"));
    assert!(webview2.contains("committed_controller.SetIsVisible(false)"));
    let guarded_webview2 = webview2
        .split("if let Some(guard) = navigation_presentation_guard.as_ref()")
        .nth(1)
        .expect("WebView2 commit guard");
    assert!(
        guarded_webview2.find("guard();").unwrap()
            < guarded_webview2.find("ShowWindow(hwnd, SW_HIDE)").unwrap()
    );

    let webkitgtk = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/wry/src/webkitgtk/mod.rs"
    ));
    let guarded_gtk = webkitgtk
        .split("if native_committed {")
        .nth(1)
        .and_then(|source| source.split("// Legacy page load handler").next())
        .expect("bounded WebKitGTK commit guard");
    let gtk_guard = guarded_gtk.find("guard();").unwrap();
    let gtk_input = guarded_gtk.find("webview.set_sensitive(false)").unwrap();
    let gtk_paint = guarded_gtk.find("webview.set_opacity(0.0)").unwrap();
    assert!(gtk_guard < gtk_input);
    assert!(gtk_input < gtk_paint);

    let wkwebview = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/wry/src/wkwebview/navigation.rs"
    ));
    let guarded_wk = wkwebview
        .split("if let Some(guard) = &this.ivars().navigation_presentation_guard")
        .nth(1)
        .expect("WKWebView commit guard");
    assert!(
        guarded_wk.find("guard();").unwrap() < guarded_wk.find("webview.setHidden(true)").unwrap()
    );
}

#[test]
fn every_native_stage_revalidates_the_generation_permit_around_reveal() {
    let mac = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/macos/stage.rs"
    ));
    let mac_sync = mac
        .split("fn sync_visibility(&self)")
        .nth(1)
        .expect("macOS visibility sync")
        .split("fn bump_layout_epoch")
        .next()
        .expect("bounded macOS visibility sync");
    let mac_reveal = mac_sync
        .find("view.view.setHidden(false)")
        .expect("macOS reveal primitive");
    assert!(mac_sync[..mac_reveal]
        .rfind("presentation_permit.load(Ordering::Acquire)")
        .is_some());
    assert!(mac_sync[mac_reveal..]
        .find("presentation_permit.load(Ordering::Acquire)")
        .is_some());

    let linux = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/linux/stage.rs"
    ));
    let linux_sync = linux
        .split("fn sync(")
        .nth(1)
        .expect("Linux stage sync")
        .split("fn schedule_sync")
        .next()
        .expect("bounded Linux stage sync");
    let linux_reveal = linux_sync
        .find("view.view.set_opacity(1.0)")
        .expect("Linux reveal primitive");
    assert!(linux_sync[..linux_reveal]
        .rfind("view_may_reveal(state, revision, *id, view)")
        .is_some());
    assert!(linux_sync[linux_reveal..]
        .find("view_may_reveal(state, revision, *id, view)")
        .is_some());
    let linux_input = linux_sync
        .rfind("view.view.set_sensitive(true)")
        .expect("Linux input reveal primitive");
    assert!(linux_reveal < linux_input);
    assert!(linux_sync[..linux_input]
        .rfind("view_may_reveal(state, revision, *id, view)")
        .is_some());
    assert!(linux_sync[linux_input..]
        .find("view_may_reveal(state, revision, *id, view)")
        .is_some());

    let windows = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/windows/stage.rs"
    ));
    let windows_reveal = windows
        .split("fn apply_native_placement(")
        .nth(1)
        .expect("Windows native placement")
        .split("fn finish_native_placement")
        .next()
        .expect("bounded Windows native placement");
    let controller_reveal = windows_reveal
        .find("placement.controller.SetIsVisible(true)")
        .expect("WebView2 controller reveal");
    assert!(windows_reveal[..controller_reveal]
        .rfind("placement_may_reveal(state, placement)")
        .is_some());
    assert!(windows_reveal[controller_reveal..]
        .find("placement_may_reveal(state, placement)")
        .is_some());
    let window_reveal = windows_reveal
        .find("ShowWindow(placement.container, SW_SHOWNA)")
        .expect("WebView2 child window reveal");
    assert!(windows_reveal[..window_reveal]
        .rfind("placement_may_reveal(state, placement)")
        .is_some());
    assert!(windows_reveal[window_reveal..]
        .find("placement_may_reveal(state, placement)")
        .is_some());

    let mac_container = mac
        .split("fn sync_container_visibility(&self)")
        .nth(1)
        .expect("macOS container visibility sync")
        .split("fn bump_layout_epoch")
        .next()
        .expect("bounded macOS container visibility sync");
    let container_reveal = mac_container
        .find("self.setHidden(false)")
        .expect("macOS stage-container reveal");
    assert!(mac_container[..container_reveal]
        .rfind("content_update_epoch.get() == epoch")
        .is_some());
    assert!(mac_container[container_reveal..]
        .find("content_update_epoch.get() != epoch")
        .is_some());

    let mac_host_layout = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"))
        .split("#[cfg(target_os = \"macos\")]\n    pub(crate) fn set_content(")
        .nth(1)
        .expect("macOS host layout")
        .split("#[cfg(target_os = \"macos\")]\n    pub(crate) fn set_drop_indicator")
        .next()
        .expect("bounded macOS host layout");
    assert!(
        mac_host_layout.find("begin_content_update").unwrap()
            < mac_host_layout.find("stage_set_frame").unwrap()
    );
    assert!(mac_host_layout.contains("content_update_is_current"));
    assert!(mac_host_layout.contains("finish_content_update"));
    assert!(!mac_host_layout.contains("stage.setHidden(false)"));

    let host = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    assert!(
        host.matches("view.presentation_permit.load(Ordering::Acquire)")
            .count()
            >= 5
    );
    assert!(host.matches("view.presentable").count() >= 5);
}

#[test]
fn title_callbacks_are_quarantined_until_exact_finished_document_attribution() {
    let host = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let permits = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/permits.rs"));
    let title_callback = host
        .split(".with_document_title_changed_handler")
        .nth(1)
        .expect("raw title callback")
        .split("// Intentionally do not install a new-window callback")
        .next()
        .expect("bounded raw title callback");
    assert!(title_callback.contains("with_title_observation"));
    assert!(!title_callback.contains("title_permit.emit"));

    let completion = permits
        .split("fn queue_navigation_completion(")
        .nth(1)
        .expect("navigation completion queue")
        .split("fn queue_navigation_failure(")
        .next()
        .expect("bounded navigation completion queue");
    let title = completion
        .find("complete_title_attribution")
        .expect("finished native title query");
    let ready = completion
        .find("emit_navigation_ready")
        .expect("presentation-ready event");
    assert!(title < ready);

    let observed = host
        .split("fn emit_title_observation(")
        .nth(1)
        .expect("title observation gate")
        .split("fn emit_navigation_observation(")
        .next()
        .expect("bounded title observation gate");
    assert!(observed.contains("view.presentable"));
    assert!(observed.contains("view.title_ready != Some(epoch)"));
}

#[test]
fn zoom_settlement_keeps_the_last_proven_native_scale_on_failure() {
    assert_eq!(settled_zoom_scale(1.0, 1.25, true), 1.25);
    assert_eq!(settled_zoom_scale(1.25, 1.5, false), 1.25);
}

#[test]
fn user_native_action_results_are_never_silently_discarded() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let actions = source
        .split("fn invoke_navigation_action(")
        .nth(1)
        .expect("native navigation-action adapter")
        .split("pub(crate) fn zoom(")
        .next()
        .expect("bounded native navigation-action adapter");
    assert!(actions.contains("EngineEvent::NativeActionFailed"));
    assert!(actions.contains("emit_navigation_observation"));
    assert!(!actions.contains("let _ = view.reload()"));
    assert!(!actions.contains("let _ = view.go_back()"));
    assert!(!actions.contains("let _ = view.go_forward()"));

    let zoom = source
        .split("pub(crate) fn zoom(")
        .nth(1)
        .expect("native zoom adapter")
        .split("pub(crate) fn extract_html(")
        .next()
        .expect("bounded native zoom adapter");
    assert!(zoom.contains("EngineEvent::ZoomSettled"));
    assert!(zoom.contains("settled_zoom_scale"));
    assert!(!zoom.contains("let _ = view.zoom"));
}

#[test]
fn raw_native_media_surfaces_are_denied_per_view() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let raw_policy = source
        .split("let mut builder = builder")
        .nth(1)
        .expect("raw view policy builder")
        .split("// This host-owned guard")
        .next()
        .expect("pre-script raw view policy");
    assert!(raw_policy.contains("with_fullscreen_enabled(false)"));
    assert!(raw_policy.contains("with_picture_in_picture_enabled(false)"));
}

#[test]
fn warm_spare_cannot_outlive_its_profiles_last_real_view() {
    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/mod.rs"));
    let ensure_spare = source
        .split("pub(crate) fn ensure_spare(&mut self, partition: Partition)")
        .nth(1)
        .expect("native warm-spare implementation")
        .split("pub(crate) fn ensure_spare(&mut self, partition: Partition)")
        .next()
        .expect("end of native warm-spare implementation");
    assert!(ensure_spare.contains("!self.has_live_profile_view(profile)"));

    let idle_close = source
        .split("fn close_idle_spare(&mut self, profile: ProfileId)")
        .nth(1)
        .expect("idle spare retirement")
        .split("pub(crate) fn close(&mut self, id: ItemId)")
        .next()
        .expect("end of idle spare retirement");
    assert!(idle_close.contains("spare.view.close_explicit()"));
    assert!(idle_close.contains("self.web_contexts.remove(&profile)"));

    let close = source
        .split("pub(crate) fn close(&mut self, id: ItemId)")
        .nth(1)
        .expect("view close implementation")
        .split("pub(crate) fn erase_profile_data")
        .next()
        .expect("end of view close implementation");
    assert!(close.contains("self.close_idle_spare(profile)"));
}

#[test]
fn windows_raw_autofill_surfaces_are_mandatory_verified_postconditions() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/platform/windows/mod.rs"
    ));
    let configure = source
        .split("pub fn configure(")
        .nth(1)
        .expect("raw WebView2 configure function")
        .split("// Wry's navigation callback")
        .next()
        .expect("pre-navigation raw WebView2 policy");
    for required in [
        "settings.cast::<ICoreWebView2Settings4>()?",
        "SetIsPasswordAutosaveEnabled(false)?",
        "SetIsGeneralAutofillEnabled(false)?",
        "IsPasswordAutosaveEnabled(&mut password_autosave_enabled)?",
        "IsGeneralAutofillEnabled(&mut general_autofill_enabled)?",
        "password_autosave_enabled.as_bool() || general_autofill_enabled.as_bool()",
        "E_ACCESSDENIED",
    ] {
        assert!(
            configure.contains(required),
            "raw WebView2 autofill postcondition lost invariant: {required}"
        );
    }
}

#[test]
fn discard_probe_requires_exact_generation_and_current_navigation_epoch() {
    let first_token = Arc::new(AtomicBool::new(true));
    let first_permit = EventPermit::bound(&first_token);
    let first_navigation = NavigationEpochTracker::new();
    let first_epoch = first_navigation.begin("https://first.example/").unwrap();
    assert_eq!(
        first_navigation.observe_navigation(&wry::NavigationEvent {
            id: wry::NavigationId::from_raw(1),
            phase: wry::NavigationEventPhase::Started,
            url: "https://first.example/".into(),
        }),
        Some(NavigationTransition::Started(first_epoch))
    );
    assert_eq!(
        first_navigation.observe_navigation(&wry::NavigationEvent {
            id: wry::NavigationId::from_raw(1),
            phase: wry::NavigationEventPhase::Committed,
            url: "https://first.example/".into(),
        }),
        Some(NavigationTransition::Committed(first_epoch))
    );
    assert!(discard_probe_identity_matches(
        &first_permit,
        &first_navigation,
        &first_permit,
        &first_navigation,
        first_epoch,
    ));

    let second_epoch = first_navigation.begin("https://second.example/").unwrap();
    assert_ne!(first_epoch, second_epoch);
    assert!(!discard_probe_identity_matches(
        &first_permit,
        &first_navigation,
        &first_permit,
        &first_navigation,
        first_epoch,
    ));

    let replacement_token = Arc::new(AtomicBool::new(true));
    let replacement_permit = EventPermit::bound(&replacement_token);
    let replacement_navigation = NavigationEpochTracker::new();
    let replacement_epoch = replacement_navigation
        .begin("https://second.example/")
        .unwrap();
    assert!(!discard_probe_identity_matches(
        &replacement_permit,
        &replacement_navigation,
        &first_permit,
        &first_navigation,
        second_epoch,
    ));
    assert!(!discard_probe_identity_matches(
        &first_permit,
        &first_navigation,
        &replacement_permit,
        &replacement_navigation,
        replacement_epoch,
    ));
}
