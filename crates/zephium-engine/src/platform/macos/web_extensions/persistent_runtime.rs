//! Persistent MV3 background and extension-data admission gates.
//!
//! This child module is reachable only from the feature-gated macOS probe. It
//! serializes its fixed WebKit namespaces across processes, exercises storage
//! through fresh controller lifetimes, and leaves no product-facing runtime
//! construction or registration path.

use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::{
    drain_run_loop_once, new_context, persistent_probe_profiles, set_phase, wait_for_result,
    write_fixture_file, PROBE_TIMEOUT, PROBE_TOKEN,
};
use crate::platform::macos::{PersistentControllerRegistry, ProbeControllerPreparation};
use objc2::rc::{Retained, Weak};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSDictionary, NSError, NSRunLoop, NSSet, NSString,
};
use objc2_web_kit::{
    WKWebExtension, WKWebExtensionContext, WKWebExtensionContextPermissionStatus,
    WKWebExtensionController, WKWebExtensionDataRecord, WKWebExtensionDataRecordError,
    WKWebExtensionDataType, WKWebExtensionMatchPattern, WKWebExtensionPermission,
    WKWebsiteDataStore,
};
use serde_json::json;

pub(super) const EXTENSION_PRINCIPAL: &str = "abcdefghijklmnopabcdefghijklmnop";
const STORAGE_KEY: &str = "zephiumNativeProbe";
const STORAGE_SENTINEL_KEY: &str = "zephiumNativeProbeCompletion";
const LOCK_FILE_NAME: &str = ".zephium-wk-web-extension-probe.lock";
const MACOS_O_NOFOLLOW: i32 = 0x0000_0100;
const LOCK_TIMEOUT: Duration = Duration::from_secs(45);
const RELEASE_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_LOCK_ATTEMPTS: usize = 4_500;
const MAX_DATA_POLLS: usize = 128;
const MAX_RECORD_ERRORS: usize = 4;
const MAX_FAILURES: usize = 8;
const MAX_DIAGNOSTIC_CHARS: usize = 256;
const BACKGROUND_SETTLE_TURNS: usize = 16;
const WRITER_SENTINEL_BYTES: usize = 16 * 1_024;
const VERIFIER_ONE_SENTINEL_BYTES: usize = 32 * 1_024;
const VERIFIER_TWO_SENTINEL_BYTES: usize = 64 * 1_024;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn geteuid() -> u32;
}

pub(super) struct RuntimeFixturePaths {
    pub(super) writer: PathBuf,
    pub(super) verifier_one: PathBuf,
    pub(super) verifier_two: PathBuf,
    pub(super) empty: PathBuf,
}

pub(super) struct PersistentProbeEvidence {
    pub(super) all_type_removal_callbacks: usize,
    pub(super) controllers_released: usize,
    pub(super) contexts_released: usize,
    pub(super) stores_released: usize,
}

pub(super) struct NamespaceLock {
    _file: File,
}

struct PersistentProfilePair {
    registry: PersistentControllerRegistry,
    controller_a: Retained<WKWebExtensionController>,
    controller_b: Retained<WKWebExtensionController>,
    store_a: Retained<WKWebsiteDataStore>,
    store_b: Retained<WKWebsiteDataStore>,
}

pub(super) struct NativeDataTypes {
    all: Retained<NSSet<WKWebExtensionDataType>>,
    persistent: Retained<NSSet<WKWebExtensionDataType>>,
    local: Retained<NSSet<WKWebExtensionDataType>>,
    local_type: &'static WKWebExtensionDataType,
    error_domain: &'static NSString,
}

#[derive(Clone, Copy)]
enum RuntimePhase {
    Writer,
    VerifierOne,
    VerifierTwo,
    Empty,
}

impl RuntimePhase {
    fn minimum_completed_local_bytes(self) -> usize {
        match self {
            Self::Writer => WRITER_SENTINEL_BYTES,
            Self::VerifierOne => VERIFIER_ONE_SENTINEL_BYTES,
            Self::VerifierTwo => VERIFIER_TWO_SENTINEL_BYTES,
            Self::Empty => 0,
        }
    }
}

#[derive(Default)]
struct CycleOutcome {
    local_size_a: usize,
    local_size_b: usize,
    removal_callbacks: usize,
    controllers_released: usize,
    contexts_released: usize,
    stores_released: usize,
}

struct ReleaseCounts {
    controllers: usize,
    contexts: usize,
    stores: usize,
}

#[derive(Default)]
struct ReleaseInventory {
    controllers: Vec<Weak<WKWebExtensionController>>,
    contexts: Vec<Weak<WKWebExtensionContext>>,
    stores: Vec<Weak<WKWebsiteDataStore>>,
}

pub(super) fn validate_persistent_runtime_gates(
    writer: &WKWebExtension,
    verifier_one: &WKWebExtension,
    verifier_two: &WKWebExtension,
    empty: &WKWebExtension,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<PersistentProbeEvidence, String> {
    validate_principal()?;
    let _namespace_lock = NamespaceLock::acquire()?;
    let data_types = NativeDataTypes::discover(mtm)?;
    let storage_permission = webkit_string_symbol(
        b"WKWebExtensionPermissionStorage\0",
        "extension storage permission",
    )?;

    let gate_result = exercise_persistent_runtime_sequence(
        writer,
        verifier_one,
        verifier_two,
        empty,
        &data_types,
        storage_permission,
        run_loop,
        mtm,
    );
    if gate_result.is_err() {
        set_phase("persistent-failure-finalizer");
        let cleanup = finalize_failed_namespaces(&data_types, run_loop, mtm);
        combine_gate_and_cleanup(gate_result, cleanup)
    } else {
        gate_result
    }
}

#[allow(clippy::too_many_arguments)]
fn exercise_persistent_runtime_sequence(
    writer: &WKWebExtension,
    verifier_one: &WKWebExtension,
    verifier_two: &WKWebExtension,
    empty: &WKWebExtension,
    data_types: &NativeDataTypes,
    storage_permission: &WKWebExtensionPermission,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<PersistentProbeEvidence, String> {
    set_phase("persistent-writer");
    let writer_outcome = run_cycle(
        writer,
        writer,
        RuntimePhase::Writer,
        RuntimePhase::Writer,
        CycleKind::Writer,
        data_types,
        storage_permission,
        run_loop,
        mtm,
    )?;
    if writer_outcome.local_size_a == 0 || writer_outcome.local_size_b == 0 {
        return Err("writer cycle returned zero-byte local-storage evidence".into());
    }

    set_phase("persistent-verifier-one");
    let verifier_one_outcome = run_cycle(
        verifier_one,
        verifier_one,
        RuntimePhase::VerifierOne,
        RuntimePhase::VerifierOne,
        CycleKind::RetireA,
        data_types,
        storage_permission,
        run_loop,
        mtm,
    )?;
    if verifier_one_outcome.local_size_a <= writer_outcome.local_size_a
        || verifier_one_outcome.local_size_b <= writer_outcome.local_size_b
    {
        return Err(format!(
            "verifier-one completion was not observable as monotonic local bytes: writer=({}, {}), verifier=({}, {})",
            writer_outcome.local_size_a,
            writer_outcome.local_size_b,
            verifier_one_outcome.local_size_a,
            verifier_one_outcome.local_size_b,
        ));
    }

    set_phase("persistent-peer-survival");
    let verifier_two_outcome = run_cycle(
        empty,
        verifier_two,
        RuntimePhase::Empty,
        RuntimePhase::VerifierTwo,
        CycleKind::RetireB,
        data_types,
        storage_permission,
        run_loop,
        mtm,
    )?;
    if verifier_two_outcome.local_size_a != 0
        || verifier_two_outcome.local_size_b <= verifier_one_outcome.local_size_b
    {
        return Err(format!(
            "peer-survival cycle returned invalid local bytes: A={}, B={} (prior B={})",
            verifier_two_outcome.local_size_a,
            verifier_two_outcome.local_size_b,
            verifier_one_outcome.local_size_b,
        ));
    }

    set_phase("persistent-final-reopen");
    let empty_outcome = run_cycle(
        empty,
        empty,
        RuntimePhase::Empty,
        RuntimePhase::Empty,
        CycleKind::FinalReadback,
        data_types,
        storage_permission,
        run_loop,
        mtm,
    )?;
    if empty_outcome.local_size_a != 0 || empty_outcome.local_size_b != 0 {
        return Err("final reopen observed persistent extension bytes".into());
    }

    let all_type_removal_callbacks = writer_outcome
        .removal_callbacks
        .checked_add(verifier_one_outcome.removal_callbacks)
        .and_then(|count| count.checked_add(verifier_two_outcome.removal_callbacks))
        .and_then(|count| count.checked_add(empty_outcome.removal_callbacks))
        .ok_or_else(|| "removal-callback count overflow".to_owned())?;
    if all_type_removal_callbacks < 2 {
        return Err(format!(
            "expected at least two all-type retirement callbacks, got {all_type_removal_callbacks}"
        ));
    }

    let controllers_released = [
        &writer_outcome,
        &verifier_one_outcome,
        &verifier_two_outcome,
        &empty_outcome,
    ]
    .into_iter()
    .try_fold(0usize, |total, outcome| {
        total.checked_add(outcome.controllers_released)
    })
    .ok_or_else(|| "released-controller count overflow".to_owned())?;
    let contexts_released = [
        &writer_outcome,
        &verifier_one_outcome,
        &verifier_two_outcome,
        &empty_outcome,
    ]
    .into_iter()
    .try_fold(0usize, |total, outcome| {
        total.checked_add(outcome.contexts_released)
    })
    .ok_or_else(|| "released-context count overflow".to_owned())?;
    let stores_released = [
        &writer_outcome,
        &verifier_one_outcome,
        &verifier_two_outcome,
        &empty_outcome,
    ]
    .into_iter()
    .try_fold(0usize, |total, outcome| {
        total.checked_add(outcome.stores_released)
    })
    .ok_or_else(|| "released-store count overflow".to_owned())?;

    Ok(PersistentProbeEvidence {
        all_type_removal_callbacks,
        controllers_released,
        contexts_released,
        stores_released,
    })
}

#[derive(Clone, Copy)]
enum CycleKind {
    Writer,
    RetireA,
    RetireB,
    FinalReadback,
}

#[allow(clippy::too_many_arguments)]
fn run_cycle(
    extension_a: &WKWebExtension,
    extension_b: &WKWebExtension,
    phase_a: RuntimePhase,
    phase_b: RuntimePhase,
    kind: CycleKind,
    data_types: &NativeDataTypes,
    storage_permission: &WKWebExtensionPermission,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<CycleOutcome, String> {
    let (gate_result, release_inventory) = objc2::rc::autoreleasepool(|_| {
        let mut release_inventory = ReleaseInventory::default();
        let gate_result = (|| {
            let profiles = new_persistent_profiles(mtm)?;
            let controller_weak_a = Weak::from_retained(&profiles.controller_a);
            let controller_weak_b = Weak::from_retained(&profiles.controller_b);
            assert_distinct_controller_identity(
                &profiles.controller_a,
                &profiles.controller_b,
                &controller_weak_a,
                &controller_weak_b,
            )?;
            release_inventory
                .controllers
                .extend([controller_weak_a, controller_weak_b]);
            release_inventory.stores.extend([
                Weak::from_retained(&profiles.store_a),
                Weak::from_retained(&profiles.store_b),
            ]);

            let context_a = new_context(extension_a, EXTENSION_PRINCIPAL)?;
            let context_b = new_context(extension_b, EXTENSION_PRINCIPAL)?;
            let context_weak_a = Weak::from_retained(&context_a);
            let context_weak_b = Weak::from_retained(&context_b);
            assert_distinct_context_identity(
                &context_a,
                &context_b,
                &context_weak_a,
                &context_weak_b,
            )?;
            release_inventory
                .contexts
                .extend([context_weak_a, context_weak_b]);

            let result = exercise_cycle(
                &profiles.controller_a,
                &profiles.controller_b,
                &context_a,
                &context_b,
                phase_a,
                phase_b,
                kind,
                data_types,
                storage_permission,
                run_loop,
            );
            let result = if result.is_err() {
                let cleanup = recover_failed_cycle(
                    &profiles.controller_a,
                    &profiles.controller_b,
                    &context_a,
                    &context_b,
                    data_types,
                    storage_permission,
                    run_loop,
                );
                combine_gate_and_cleanup(result, cleanup)
            } else {
                result
            };

            drop(context_a);
            drop(context_b);
            combine_gate_and_cleanup(result, release_persistent_profiles(profiles))
        })();
        (gate_result, release_inventory)
    });

    let release_result = wait_for_release(&release_inventory, run_loop);
    match (gate_result, release_result) {
        (Ok(mut outcome), Ok(released)) => {
            outcome.controllers_released = released.controllers;
            outcome.contexts_released = released.contexts;
            outcome.stores_released = released.stores;
            Ok(outcome)
        }
        (Err(gate), Ok(_)) => Err(bounded_text(&gate)),
        (Ok(_), Err(release)) => Err(bounded_text(&format!("native release failed: {release}"))),
        (Err(gate), Err(release)) => Err(bounded_text(&format!(
            "{}; native release also failed: {}",
            bounded_text(&gate),
            bounded_text(&release)
        ))),
    }
}

fn assert_distinct_controller_identity(
    controller_a: &Retained<WKWebExtensionController>,
    controller_b: &Retained<WKWebExtensionController>,
    weak_a: &Weak<WKWebExtensionController>,
    weak_b: &Weak<WKWebExtensionController>,
) -> Result<(), String> {
    if Retained::as_ptr(controller_a) == Retained::as_ptr(controller_b) {
        return Err("persistent profiles resolved to the same native controller".into());
    }
    let loaded_a = weak_a
        .load()
        .ok_or_else(|| "controller A weak identity was dead before admission".to_owned())?;
    let loaded_b = weak_b
        .load()
        .ok_or_else(|| "controller B weak identity was dead before admission".to_owned())?;
    if Retained::as_ptr(&loaded_a) == Retained::as_ptr(&loaded_b) {
        return Err("persistent controller weak identities alias".into());
    }
    Ok(())
}

fn assert_distinct_context_identity(
    context_a: &Retained<WKWebExtensionContext>,
    context_b: &Retained<WKWebExtensionContext>,
    weak_a: &Weak<WKWebExtensionContext>,
    weak_b: &Weak<WKWebExtensionContext>,
) -> Result<(), String> {
    if Retained::as_ptr(context_a) == Retained::as_ptr(context_b) {
        return Err("persistent profiles resolved to the same native context".into());
    }
    let loaded_a = weak_a
        .load()
        .ok_or_else(|| "context A weak identity was dead before admission".to_owned())?;
    let loaded_b = weak_b
        .load()
        .ok_or_else(|| "context B weak identity was dead before admission".to_owned())?;
    if Retained::as_ptr(&loaded_a) == Retained::as_ptr(&loaded_b) {
        return Err("persistent context weak identities alias".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn exercise_cycle(
    controller_a: &WKWebExtensionController,
    controller_b: &WKWebExtensionController,
    context_a: &WKWebExtensionContext,
    context_b: &WKWebExtensionContext,
    phase_a: RuntimePhase,
    phase_b: RuntimePhase,
    kind: CycleKind,
    data_types: &NativeDataTypes,
    storage_permission: &WKWebExtensionPermission,
    run_loop: &NSRunLoop,
) -> Result<CycleOutcome, String> {
    let mut outcome = CycleOutcome::default();
    let [principals_a, principals_b] = profile_record_principal_scopes();
    if matches!(kind, CycleKind::Writer) {
        outcome.removal_callbacks += erase_all_extension_data(
            controller_a,
            data_types,
            run_loop,
            "controller A preflight",
            &principals_a,
        )?;
        outcome.removal_callbacks += erase_all_extension_data(
            controller_b,
            data_types,
            run_loop,
            "controller B preflight",
            &principals_b,
        )?;
    }

    prepare_exact_storage_permission(context_a, storage_permission, "storage context A")?;
    prepare_exact_storage_permission(context_b, storage_permission, "storage context B")?;
    load_context_bounded(controller_a, context_a, "persistent storage A")?;
    load_context_bounded(controller_b, context_b, "persistent storage B")?;
    load_background_content(context_a, run_loop, "persistent storage A")?;
    load_background_content(context_b, run_loop, "persistent storage B")?;

    // WebKit's background-load callback establishes admission, while each
    // fixture's top-level module performs the exact write/read state machine.
    // Give those bounded event-loop turns to settle, then unload before asking
    // WebKit to calculate persistent data records. The turns are scheduling
    // headroom, not completion proof: admission still requires the final
    // post-self-read sentinel's phase-specific native byte floor. A native
    // record error remains terminal.
    for _ in 0..BACKGROUND_SETTLE_TURNS {
        drain_run_loop_once(run_loop);
    }
    validate_context_errors_bounded(context_a, "persistent storage A")?;
    validate_context_errors_bounded(context_b, "persistent storage B")?;
    unload_context_bounded(controller_a, context_a, "persistent storage A")?;
    unload_context_bounded(controller_b, context_b, "persistent storage B")?;
    clear_permission_state(context_a, storage_permission, "storage context A")?;
    clear_permission_state(context_b, storage_permission, "storage context B")?;

    outcome.local_size_a = observe_phase(
        controller_a,
        phase_a,
        data_types,
        run_loop,
        "persistent storage A",
        &principals_a,
    )?;
    outcome.local_size_b = observe_phase(
        controller_b,
        phase_b,
        data_types,
        run_loop,
        "persistent storage B",
        &principals_b,
    )?;

    match kind {
        CycleKind::Writer | CycleKind::FinalReadback => {}
        CycleKind::RetireA => {
            outcome.removal_callbacks += erase_all_extension_data(
                controller_a,
                data_types,
                run_loop,
                "controller A retirement",
                &principals_a,
            )?;
            let surviving_b = fetch_exact_local_size(
                controller_b,
                data_types,
                run_loop,
                "controller B after A retirement",
                &principals_b,
            )?;
            if surviving_b != outcome.local_size_b {
                return Err(format!(
                    "controller A retirement mutated B local bytes: before={}, after={surviving_b}",
                    outcome.local_size_b
                ));
            }
        }
        CycleKind::RetireB => {
            outcome.removal_callbacks += erase_all_extension_data(
                controller_b,
                data_types,
                run_loop,
                "controller B retirement",
                &principals_b,
            )?;
            wait_for_zero_persistent_data(
                controller_a,
                data_types,
                run_loop,
                "controller A after B retirement",
                &principals_a,
            )?;
        }
    }
    Ok(outcome)
}

fn observe_phase(
    controller: &WKWebExtensionController,
    phase: RuntimePhase,
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<usize, String> {
    match phase {
        RuntimePhase::Empty => {
            wait_for_zero_persistent_data(
                controller,
                data_types,
                run_loop,
                description,
                allowed_principals,
            )?;
            Ok(0)
        }
        RuntimePhase::Writer | RuntimePhase::VerifierOne | RuntimePhase::VerifierTwo => {
            let size = wait_for_local_storage(
                controller,
                data_types,
                phase.minimum_completed_local_bytes(),
                run_loop,
                description,
                allowed_principals,
            )?;
            Ok(size)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn recover_failed_cycle(
    controller_a: &WKWebExtensionController,
    controller_b: &WKWebExtensionController,
    context_a: &WKWebExtensionContext,
    context_b: &WKWebExtensionContext,
    data_types: &NativeDataTypes,
    storage_permission: &WKWebExtensionPermission,
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    let [principals_a, principals_b] = profile_record_principal_scopes();
    let mut failures = Vec::new();
    for (controller, context, name) in [
        (controller_a, context_a, "storage A failure cleanup"),
        (controller_b, context_b, "storage B failure cleanup"),
    ] {
        if unsafe { context.isLoaded() } {
            collect_failure(
                &mut failures,
                unload_context_bounded(controller, context, name),
            );
        }
        collect_failure(
            &mut failures,
            clear_permission_state(context, storage_permission, name),
        );
    }
    for (controller, name, allowed_principals) in [
        (
            controller_a,
            "controller A failure cleanup",
            principals_a.as_slice(),
        ),
        (
            controller_b,
            "controller B failure cleanup",
            principals_b.as_slice(),
        ),
    ] {
        collect_failure(
            &mut failures,
            erase_all_extension_data(controller, data_types, run_loop, name, allowed_principals)
                .map(|_| ()),
        );
    }
    failures_to_result(failures)
}

fn finalize_failed_namespaces(
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    mtm: MainThreadMarker,
) -> Result<(), String> {
    let (cleanup_result, controller_weaks) = objc2::rc::autoreleasepool(|_| {
        let mut controller_weaks = Vec::new();
        let cleanup_result = (|| {
            let profiles = new_persistent_profiles(mtm)?;
            let weak_a = Weak::from_retained(&profiles.controller_a);
            let weak_b = Weak::from_retained(&profiles.controller_b);
            assert_distinct_controller_identity(
                &profiles.controller_a,
                &profiles.controller_b,
                &weak_a,
                &weak_b,
            )?;
            controller_weaks.extend([weak_a, weak_b]);

            let mut failures = Vec::new();
            let [principals_a, principals_b] = profile_record_principal_scopes();
            collect_failure(
                &mut failures,
                erase_all_extension_data(
                    &profiles.controller_a,
                    data_types,
                    run_loop,
                    "controller A outer failure finalizer",
                    &principals_a,
                )
                .map(|_| ()),
            );
            collect_failure(
                &mut failures,
                erase_all_extension_data(
                    &profiles.controller_b,
                    data_types,
                    run_loop,
                    "controller B outer failure finalizer",
                    &principals_b,
                )
                .map(|_| ()),
            );
            combine_gate_and_cleanup(
                failures_to_result(failures),
                release_persistent_profiles(profiles),
            )
        })();
        (cleanup_result, controller_weaks)
    });
    let release_result = wait_for_controller_release(&controller_weaks, run_loop);
    combine_gate_and_cleanup(cleanup_result, release_result)
}

fn wait_for_controller_release(
    controllers: &[Weak<WKWebExtensionController>],
    run_loop: &NSRunLoop,
) -> Result<(), String> {
    if controllers.len() != 2 {
        return Err(format!(
            "outer finalizer controller inventory mismatch: {}",
            controllers.len()
        ));
    }
    let deadline = Instant::now() + RELEASE_TIMEOUT;
    loop {
        let released = controllers
            .iter()
            .filter(|controller| controller.load().is_none())
            .count();
        if released == controllers.len() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "outer finalizer native release did not converge: {released}/{}",
                controllers.len()
            ));
        }
        drain_run_loop_once(run_loop);
    }
}

fn combine_gate_and_cleanup<T>(
    gate: Result<T, String>,
    cleanup: Result<(), String>,
) -> Result<T, String> {
    match (gate, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(gate), Ok(())) => Err(bounded_text(&gate)),
        (Ok(_), Err(cleanup)) => Err(bounded_text(&format!("native cleanup failed: {cleanup}"))),
        (Err(gate), Err(cleanup)) => Err(bounded_text(&format!(
            "{}; native cleanup also failed: {}",
            bounded_text(&gate),
            bounded_text(&cleanup)
        ))),
    }
}

fn collect_failure(failures: &mut Vec<String>, result: Result<(), String>) {
    if let Err(error) = result {
        if failures.len() < MAX_FAILURES {
            failures.push(bounded_text(&error));
        }
    }
}

fn failures_to_result(failures: Vec<String>) -> Result<(), String> {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

impl NamespaceLock {
    pub(super) fn acquire() -> Result<Self, String> {
        let effective_uid = unsafe { geteuid() };
        let temp_directory = std::env::temp_dir().canonicalize().map_err(|error| {
            format!("cannot canonicalize per-user temporary directory: {error}")
        })?;
        let directory_metadata = std::fs::symlink_metadata(&temp_directory)
            .map_err(|error| format!("cannot inspect per-user temporary directory: {error}"))?;
        validate_private_temp_directory(&temp_directory, &directory_metadata, effective_uid)?;

        let path = temp_directory.join(LOCK_FILE_NAME);
        let before = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                validate_lock_metadata(&path, &metadata, effective_uid)?;
                Some(metadata)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "cannot inspect persistent-probe lock {}: {error}",
                    path.display()
                ));
            }
        };
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(MACOS_O_NOFOLLOW)
            .open(&path)
            .map_err(|error| {
                format!(
                    "cannot securely open persistent-probe lock {}: {error}",
                    path.display()
                )
            })?;
        let open_metadata = file
            .metadata()
            .map_err(|error| format!("cannot inspect opened persistent-probe lock: {error}"))?;
        validate_lock_metadata(&path, &open_metadata, effective_uid)?;
        let path_metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot re-inspect persistent-probe lock: {error}"))?;
        validate_lock_metadata(&path, &path_metadata, effective_uid)?;
        validate_same_file(&open_metadata, &path_metadata, "opened/path lock identity")?;
        if let Some(before) = before {
            validate_same_file(&before, &open_metadata, "pre-open/opened lock identity")?;
        }

        let deadline = Instant::now() + LOCK_TIMEOUT;
        for attempt in 1..=MAX_LOCK_ATTEMPTS {
            match file.try_lock() {
                Ok(()) => {
                    let locked_path_metadata =
                        std::fs::symlink_metadata(&path).map_err(|error| {
                            format!(
                                "cannot inspect persistent-probe lock after acquisition: {error}"
                            )
                        })?;
                    validate_lock_metadata(&path, &locked_path_metadata, effective_uid)?;
                    let locked_file_metadata = file.metadata().map_err(|error| {
                        format!("cannot inspect opened lock after acquisition: {error}")
                    })?;
                    validate_same_file(
                        &locked_file_metadata,
                        &locked_path_metadata,
                        "locked file/path identity",
                    )?;
                    return Ok(Self { _file: file });
                }
                Err(TryLockError::WouldBlock)
                    if attempt < MAX_LOCK_ATTEMPTS && Instant::now() < deadline =>
                {
                    std::thread::sleep(LOCK_POLL_INTERVAL);
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(format!(
                        "timed out acquiring the persistent-probe namespace lock after {attempt} attempts"
                    ));
                }
                Err(TryLockError::Error(error)) => {
                    return Err(format!(
                        "cannot acquire persistent-probe namespace lock: {error}"
                    ));
                }
            }
        }
        Err("persistent-probe lock attempt bound was exhausted".into())
    }
}

fn validate_private_temp_directory(
    path: &Path,
    metadata: &std::fs::Metadata,
    effective_uid: u32,
) -> Result<(), String> {
    let mode = metadata.mode() & 0o777;
    if !metadata.is_dir()
        || metadata.uid() != effective_uid
        || mode & 0o700 != 0o700
        || mode & 0o077 != 0
    {
        return Err(format!(
            "temporary directory {} is not an owner-private directory (uid={}, expected_uid={effective_uid}, mode={mode:o})",
            path.display(),
            metadata.uid(),
        ));
    }
    Ok(())
}

fn validate_lock_metadata(
    path: &Path,
    metadata: &std::fs::Metadata,
    effective_uid: u32,
) -> Result<(), String> {
    let mode = metadata.mode() & 0o777;
    if !metadata.is_file()
        || metadata.uid() != effective_uid
        || metadata.nlink() != 1
        || mode != 0o600
    {
        return Err(format!(
            "persistent-probe lock {} failed ownership/type checks (uid={}, expected_uid={effective_uid}, nlink={}, mode={mode:o})",
            path.display(),
            metadata.uid(),
            metadata.nlink(),
        ));
    }
    Ok(())
}

fn validate_same_file(
    left: &std::fs::Metadata,
    right: &std::fs::Metadata,
    description: &str,
) -> Result<(), String> {
    if left.dev() != right.dev() || left.ino() != right.ino() {
        return Err(format!(
            "{description} changed during secure lock acquisition"
        ));
    }
    Ok(())
}

impl NativeDataTypes {
    pub(super) fn discover(mtm: MainThreadMarker) -> Result<Self, String> {
        let local_type = webkit_string_symbol(
            b"WKWebExtensionDataTypeLocal\0",
            "local extension data type",
        )?;
        let session_type = webkit_string_symbol(
            b"WKWebExtensionDataTypeSession\0",
            "session extension data type",
        )?;
        let synchronized_type = webkit_string_symbol(
            b"WKWebExtensionDataTypeSynchronized\0",
            "synchronized extension data type",
        )?;
        let error_domain = webkit_string_symbol(
            b"WKWebExtensionDataRecordErrorDomain\0",
            "extension data-record error domain",
        )?;
        let all = unsafe { WKWebExtensionController::allExtensionDataTypes(mtm) };
        if all.count() != 3
            || !all.containsObject(local_type)
            || !all.containsObject(session_type)
            || !all.containsObject(synchronized_type)
        {
            return Err(format!(
                "unknown WebKit extension-data inventory: expected exact local/session/synchronized, count={}",
                all.count()
            ));
        }
        Ok(Self {
            all,
            persistent: NSSet::from_slice(&[local_type, synchronized_type]),
            local: NSSet::from_slice(&[local_type]),
            local_type,
            error_domain,
        })
    }
}

/// Resolve macOS 15.4 WebKit data exports only after the parent runtime gate.
///
/// Direct references to these generated Rust statics produce strong Mach-O
/// imports. The probe itself targets the browser's older macOS deployment
/// floor, so such imports would let dyld terminate the process before
/// `supported_runtime` can return the documented unsupported result.
fn webkit_string_symbol(
    nul_terminated_name: &'static [u8],
    description: &str,
) -> Result<&'static NSString, String> {
    if nul_terminated_name.last() != Some(&0)
        || nul_terminated_name[..nul_terminated_name.len().saturating_sub(1)].contains(&0)
    {
        return Err(format!("invalid dynamic symbol name for {description}"));
    }
    // Darwin defines RTLD_DEFAULT as `(void *)-2`. `dlsym` returns the
    // address of the exported Objective-C object pointer for data symbols.
    let symbol = unsafe {
        dlsym(
            (-2_isize) as *mut c_void,
            nul_terminated_name.as_ptr().cast(),
        )
    };
    let Some(slot) = NonNull::new(symbol.cast::<*const NSString>()) else {
        return Err(format!("WebKit did not publish its {description}"));
    };
    let object = unsafe { slot.as_ptr().read() };
    unsafe { object.as_ref() }.ok_or_else(|| format!("WebKit published a null {description}"))
}

fn validate_principal() -> Result<(), String> {
    if EXTENSION_PRINCIPAL.len() != 32
        || !EXTENSION_PRINCIPAL
            .bytes()
            .all(|byte| (b'a'..=b'p').contains(&byte))
    {
        return Err("persistent probe principal is not a Chromium-compatible extension ID".into());
    }
    Ok(())
}

fn new_persistent_profiles(_mtm: MainThreadMarker) -> Result<PersistentProfilePair, String> {
    let [profile_a, profile_b] = persistent_probe_profiles();
    let mut registry = PersistentControllerRegistry::new();
    for profile in [profile_a, profile_b] {
        match registry
            .prepare_for_native_probe(profile)
            .map_err(|error| format!("cannot prepare product profile controller: {error}"))?
        {
            ProbeControllerPreparation::Prepared => {}
            ProbeControllerPreparation::RuntimeUnavailable => {
                return Err("supported probe runtime refused profile controller preparation".into())
            }
        }
    }

    let prepared_a = registry
        .configuration_for_durable_profile(profile_a)
        .map_err(|error| format!("cannot configure product profile A: {error}"))?
        .ok_or_else(|| "prepared product profile A returned no view configuration".to_owned())?;
    let prepared_b = registry
        .configuration_for_durable_profile(profile_b)
        .map_err(|error| format!("cannot configure product profile B: {error}"))?
        .ok_or_else(|| "prepared product profile B returned no view configuration".to_owned())?;
    let (configuration_a, proof_a) = prepared_a.into_parts();
    let (configuration_b, proof_b) = prepared_b.into_parts();
    let store_a = unsafe { configuration_a.websiteDataStore() };
    let store_b = unsafe { configuration_b.websiteDataStore() };
    let controller_a = unsafe { configuration_a.webExtensionController() }
        .ok_or_else(|| "product profile A omitted its extension controller".to_owned())?;
    let controller_b = unsafe { configuration_b.webExtensionController() }
        .ok_or_else(|| "product profile B omitted its extension controller".to_owned())?;
    if Retained::as_ptr(&store_a) == Retained::as_ptr(&store_b) {
        return Err("product profiles resolved to the same website data store".into());
    }
    if Retained::as_ptr(&controller_a) == Retained::as_ptr(&controller_b) {
        return Err("product profiles resolved to the same extension controller".into());
    }
    drop(proof_a);
    drop(proof_b);
    drop(configuration_a);
    drop(configuration_b);
    Ok(PersistentProfilePair {
        registry,
        controller_a,
        controller_b,
        store_a,
        store_b,
    })
}

fn release_persistent_profiles(mut profiles: PersistentProfilePair) -> Result<(), String> {
    drop(profiles.controller_a);
    drop(profiles.controller_b);
    drop(profiles.store_a);
    drop(profiles.store_b);
    profiles.registry.seal();
    if profiles.registry.release_all_after_views() {
        Ok(())
    } else {
        Err("product profile controller registry did not release cleanly".into())
    }
}

fn prepare_exact_storage_permission(
    context: &WKWebExtensionContext,
    storage_permission: &WKWebExtensionPermission,
    description: &str,
) -> Result<(), String> {
    let inherited_state = permission_state_is_nonempty(context);
    reset_permission_state(context);
    assert_empty_permission_state(context, storage_permission, description)?;
    if inherited_state {
        return Err(format!(
            "{description} inherited permission state; it was cleared, but this run is refused"
        ));
    }

    unsafe {
        context.setPermissionStatus_forPermission(
            WKWebExtensionContextPermissionStatus::GrantedExplicitly,
            storage_permission,
        );
    }
    let granted = unsafe { context.grantedPermissions() };
    let denied = unsafe { context.deniedPermissions() };
    let granted_patterns = unsafe { context.grantedPermissionMatchPatterns() };
    let denied_patterns = unsafe { context.deniedPermissionMatchPatterns() };
    let status = unsafe { context.permissionStatusForPermission(storage_permission) };
    if granted.count() != 1
        || granted.objectForKey(storage_permission).is_none()
        || denied.count() != 0
        || granted_patterns.count() != 0
        || denied_patterns.count() != 0
        || status != WKWebExtensionContextPermissionStatus::GrantedExplicitly
        || !unsafe { context.hasPermission(storage_permission) }
        || unsafe { context.hasRequestedOptionalAccessToAllHosts() }
        || unsafe { context.hasAccessToPrivateData() }
    {
        return Err(format!(
            "{description} did not settle to the exact single storage grant: granted={}, denied={}, granted_patterns={}, denied_patterns={}, status={status:?}",
            granted.count(),
            denied.count(),
            granted_patterns.count(),
            denied_patterns.count(),
        ));
    }
    Ok(())
}

pub(super) fn prepare_private_storage_page_context(
    context: &WKWebExtensionContext,
) -> Result<(), String> {
    let storage_permission = webkit_string_symbol(
        b"WKWebExtensionPermissionStorage\0",
        "extension storage permission",
    )?;
    prepare_exact_storage_permission(context, storage_permission, "private storage page")?;
    unsafe { context.setHasAccessToPrivateData(true) };
    if !unsafe { context.hasAccessToPrivateData() } {
        return Err("private storage page refused explicit private-data access".into());
    }
    Ok(())
}

pub(super) fn clear_private_storage_page_context(
    context: &WKWebExtensionContext,
) -> Result<(), String> {
    let storage_permission = webkit_string_symbol(
        b"WKWebExtensionPermissionStorage\0",
        "extension storage permission",
    )?;
    clear_permission_state(context, storage_permission, "private storage page")
}

fn clear_permission_state(
    context: &WKWebExtensionContext,
    storage_permission: &WKWebExtensionPermission,
    description: &str,
) -> Result<(), String> {
    reset_permission_state(context);
    assert_empty_permission_state(context, storage_permission, description)
}

fn permission_state_is_nonempty(context: &WKWebExtensionContext) -> bool {
    unsafe {
        context.grantedPermissions().count() != 0
            || context.deniedPermissions().count() != 0
            || context.grantedPermissionMatchPatterns().count() != 0
            || context.deniedPermissionMatchPatterns().count() != 0
            || context.hasRequestedOptionalAccessToAllHosts()
            || context.hasAccessToPrivateData()
    }
}

fn reset_permission_state(context: &WKWebExtensionContext) {
    let empty_permissions = NSDictionary::<WKWebExtensionPermission, NSDate>::new();
    let empty_patterns = NSDictionary::<WKWebExtensionMatchPattern, NSDate>::new();
    unsafe {
        context.setGrantedPermissions(&empty_permissions);
        context.setDeniedPermissions(&empty_permissions);
        context.setGrantedPermissionMatchPatterns(&empty_patterns);
        context.setDeniedPermissionMatchPatterns(&empty_patterns);
        context.setHasRequestedOptionalAccessToAllHosts(false);
        context.setHasAccessToPrivateData(false);
    }
}

fn assert_empty_permission_state(
    context: &WKWebExtensionContext,
    storage_permission: &WKWebExtensionPermission,
    description: &str,
) -> Result<(), String> {
    let granted = unsafe { context.grantedPermissions() };
    let denied = unsafe { context.deniedPermissions() };
    let granted_patterns = unsafe { context.grantedPermissionMatchPatterns() };
    let denied_patterns = unsafe { context.deniedPermissionMatchPatterns() };
    let status = unsafe { context.permissionStatusForPermission(storage_permission) };
    if granted.count() != 0
        || denied.count() != 0
        || granted_patterns.count() != 0
        || denied_patterns.count() != 0
        || status == WKWebExtensionContextPermissionStatus::GrantedExplicitly
        || status == WKWebExtensionContextPermissionStatus::GrantedImplicitly
        || unsafe { context.hasPermission(storage_permission) }
        || unsafe { context.hasRequestedOptionalAccessToAllHosts() }
        || unsafe { context.hasAccessToPrivateData() }
    {
        return Err(format!(
            "{description} retained permission state: granted={}, denied={}, granted_patterns={}, denied_patterns={}, status={status:?}, optional_all_hosts={}, private={}",
            granted.count(),
            denied.count(),
            granted_patterns.count(),
            denied_patterns.count(),
            unsafe { context.hasRequestedOptionalAccessToAllHosts() },
            unsafe { context.hasAccessToPrivateData() },
        ));
    }
    Ok(())
}

fn load_context_bounded(
    controller: &WKWebExtensionController,
    context: &WKWebExtensionContext,
    description: &str,
) -> Result<(), String> {
    super::load_context(controller, context, description).map_err(|error| bounded_text(&error))
}

fn unload_context_bounded(
    controller: &WKWebExtensionController,
    context: &WKWebExtensionContext,
    description: &str,
) -> Result<(), String> {
    super::unload_context(controller, context, description).map_err(|error| bounded_text(&error))
}

fn validate_context_errors_bounded(
    context: &WKWebExtensionContext,
    description: &str,
) -> Result<(), String> {
    let errors = unsafe { context.errors() };
    if errors.count() > MAX_RECORD_ERRORS {
        return Err(format!(
            "{description} returned {} context errors, exceeding bound {MAX_RECORD_ERRORS}",
            errors.count()
        ));
    }
    if errors.count() != 0 {
        return Err(bounded_text(&format!(
            "{description} context has {} error(s): {}",
            errors.count(),
            describe_record_errors(&errors)
        )));
    }
    Ok(())
}

fn load_background_content(
    context: &WKWebExtensionContext,
    run_loop: &NSRunLoop,
    description: &str,
) -> Result<(), String> {
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let description = description.to_owned();
    let callback = block2::RcBlock::new(move |error: *mut NSError| {
        let value = if let Some(error) = unsafe { Retained::retain(error) } {
            Err(bounded_text(&format!(
                "load {description} background content failed: {}",
                bounded_native_error(&error)
            )))
        } else {
            Ok(())
        };
        *callback_result.borrow_mut() = Some(value);
    });
    unsafe { context.loadBackgroundContentWithCompletionHandler(&callback) };
    wait_for_result(&result, run_loop, "background-content initialization")
}

fn fetch_extension_data_records(
    controller: &WKWebExtensionController,
    data_types: &NSSet<WKWebExtensionDataType>,
    max_records: usize,
    run_loop: &NSRunLoop,
    operation: &str,
) -> Result<Retained<NSArray<WKWebExtensionDataRecord>>, String> {
    if max_records == 0 {
        return Err(format!("{operation} has an invalid zero record bound"));
    }
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let callback =
        block2::RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
            let records = unsafe { Retained::retain(records.as_ptr()) }
                .ok_or_else(|| "WebKit released extension records before callback".to_owned());
            *callback_result.borrow_mut() = Some(records);
        });
    unsafe {
        controller.fetchDataRecordsOfTypes_completionHandler(data_types, &callback);
    }
    let records = wait_for_result(result.as_ref(), run_loop, operation)?;
    if records.count() > max_records {
        let identifiers = (0..records.count().min(4))
            .map(|index| {
                bounded_text(
                    &unsafe { records.objectAtIndex(index).uniqueIdentifier() }.to_string(),
                )
            })
            .collect::<Vec<_>>();
        return Err(format!(
            "{operation} returned {} records, exceeding its exact bound {max_records}; identifiers={identifiers:?}",
            records.count(),
        ));
    }
    Ok(records)
}

fn remove_extension_data_records(
    controller: &WKWebExtensionController,
    data_types: &NSSet<WKWebExtensionDataType>,
    records: &NSArray<WKWebExtensionDataRecord>,
    run_loop: &NSRunLoop,
    operation: &str,
) -> Result<(), String> {
    let result = Rc::new(RefCell::new(None));
    let callback_result = result.clone();
    let callback = block2::RcBlock::new(move || {
        *callback_result.borrow_mut() = Some(Ok(()));
    });
    unsafe {
        controller
            .removeDataOfTypes_fromDataRecords_completionHandler(data_types, records, &callback);
    }
    wait_for_result(result.as_ref(), run_loop, operation)
}

fn erase_all_extension_data(
    controller: &WKWebExtensionController,
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<usize, String> {
    erase_extension_data_for_principals(
        controller,
        data_types,
        run_loop,
        description,
        allowed_principals,
    )
}

pub(super) fn erase_extension_data_for_principals(
    controller: &WKWebExtensionController,
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<usize, String> {
    validate_allowed_principals(allowed_principals)?;
    let records = fetch_extension_data_records(
        controller,
        &data_types.all,
        allowed_principals.len(),
        run_loop,
        &format!("fetch {description} all-type records"),
    )?;
    let callback_count = if records.count() == 0 {
        0
    } else {
        validate_record_identity(&records, description, allowed_principals)?;
        validate_removal_record_errors(&records, description, data_types.error_domain)?;
        remove_extension_data_records(
            controller,
            &data_types.all,
            &records,
            run_loop,
            &format!("remove {description} all-type records"),
        )?;
        1
    };
    wait_for_zero_persistent_data_for_principals(
        controller,
        data_types,
        run_loop,
        description,
        allowed_principals,
    )?;
    Ok(callback_count)
}

fn wait_for_zero_persistent_data(
    controller: &WKWebExtensionController,
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<(), String> {
    wait_for_zero_persistent_data_for_principals(
        controller,
        data_types,
        run_loop,
        description,
        allowed_principals,
    )
}

fn wait_for_zero_persistent_data_for_principals(
    controller: &WKWebExtensionController,
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<(), String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    for poll in 1..=MAX_DATA_POLLS {
        let records = fetch_extension_data_records(
            controller,
            &data_types.persistent,
            allowed_principals.len(),
            run_loop,
            &format!("verify {description} persistent-byte readback"),
        )?;
        if records.count() == 0 {
            return Ok(());
        }
        validate_record_identity(&records, description, allowed_principals)?;
        let mut first_nonzero = None;
        for index in 0..records.count() {
            let record = records.objectAtIndex(index);
            validate_error_free_record(&record, description)?;
            if unsafe { record.sizeInBytesOfTypes(&data_types.persistent) } != 0 {
                first_nonzero = Some(record);
                break;
            }
        }
        if first_nonzero.is_none() {
            return Ok(());
        }
        if poll == MAX_DATA_POLLS || Instant::now() >= deadline {
            return Err(format!(
                "{description} retained persistent bytes after {poll} polls: {}",
                describe_data_record(
                    first_nonzero
                        .as_ref()
                        .ok_or_else(|| "nonzero persistent record disappeared".to_owned())?,
                    data_types,
                )
            ));
        }
        drain_run_loop_once(run_loop);
    }
    Err(format!(
        "{description} exhausted the persistent-byte poll bound"
    ))
}

fn wait_for_local_storage(
    controller: &WKWebExtensionController,
    data_types: &NativeDataTypes,
    minimum_completed_bytes: usize,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<usize, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    for poll in 1..=MAX_DATA_POLLS {
        let records = fetch_extension_data_records(
            controller,
            &data_types.local,
            allowed_principals.len(),
            run_loop,
            &format!("fetch {description} local-storage evidence"),
        )?;
        let target = inspect_local_records(&records, data_types, description, allowed_principals)?;
        if let Some((local_size, contains_local)) = target {
            if contains_local && local_size >= minimum_completed_bytes {
                return Ok(local_size);
            }
        }
        if poll == MAX_DATA_POLLS || Instant::now() >= deadline {
            let last = if let Some(index) = (0..records.count()).find(|index| {
                unsafe { records.objectAtIndex(*index).uniqueIdentifier() }.to_string()
                    == EXTENSION_PRINCIPAL
            }) {
                describe_data_record(&records.objectAtIndex(index), data_types)
            } else {
                "records=[]".to_owned()
            };
            return Err(format!(
                "timed out waiting for {description} completed local-storage floor {minimum_completed_bytes} after {poll} polls; {last}"
            ));
        }
        drain_run_loop_once(run_loop);
    }
    Err(format!(
        "{description} exhausted the local-storage poll bound"
    ))
}

fn fetch_exact_local_size(
    controller: &WKWebExtensionController,
    data_types: &NativeDataTypes,
    run_loop: &NSRunLoop,
    description: &str,
    allowed_principals: &[&str],
) -> Result<usize, String> {
    let records = fetch_extension_data_records(
        controller,
        &data_types.local,
        allowed_principals.len(),
        run_loop,
        description,
    )?;
    let Some((size, contains_local)) =
        inspect_local_records(&records, data_types, description, allowed_principals)?
    else {
        return Err(format!(
            "{description} omitted the exact persistent extension principal"
        ));
    };
    if size == 0 || !contains_local {
        return Err(format!(
            "{description} did not retain exact nonzero local-storage evidence"
        ));
    }
    Ok(size)
}

fn inspect_local_records(
    records: &NSArray<WKWebExtensionDataRecord>,
    data_types: &NativeDataTypes,
    description: &str,
    allowed_principals: &[&str],
) -> Result<Option<(usize, bool)>, String> {
    validate_record_identity(records, description, allowed_principals)?;
    let mut target = None;
    for index in 0..records.count() {
        let record = records.objectAtIndex(index);
        // Native record errors are terminal. In particular, code 2
        // (LocalStorageFailed) is not normalized into a retry.
        validate_error_free_record(&record, description)?;
        let identifier = unsafe { record.uniqueIdentifier() }.to_string();
        let size = unsafe { record.sizeInBytesOfTypes(&data_types.local) };
        let contains_local =
            unsafe { record.containedDataTypes() }.containsObject(data_types.local_type);
        if identifier == EXTENSION_PRINCIPAL {
            target = Some((size, contains_local));
        } else if size != 0 {
            return Err(format!(
                "{description} companion principal retained nonzero local-storage bytes: {}",
                bounded_text(&identifier),
            ));
        }
    }
    Ok(target)
}

fn profile_record_principal_scopes() -> [[&'static str; 2]; 2] {
    [
        [EXTENSION_PRINCIPAL, super::PROFILE_ROUTING_PRINCIPALS[0]],
        [EXTENSION_PRINCIPAL, super::PROFILE_ROUTING_PRINCIPALS[1]],
    ]
}

fn validate_record_identity(
    records: &NSArray<WKWebExtensionDataRecord>,
    description: &str,
    allowed_principals: &[&str],
) -> Result<(), String> {
    validate_allowed_principals(allowed_principals)?;
    if records.count() > allowed_principals.len() {
        return Err(format!(
            "{description} exceeded its exact extension-principal record bound"
        ));
    }
    for index in 0..records.count() {
        let identifier = unsafe { records.objectAtIndex(index).uniqueIdentifier() };
        if !allowed_principals.contains(&identifier.to_string().as_str()) {
            return Err(format!(
                "{description} observed an unexpected extension principal: {}",
                bounded_text(&identifier.to_string())
            ));
        }
        for prior_index in 0..index {
            let prior = unsafe { records.objectAtIndex(prior_index).uniqueIdentifier() };
            if identifier.isEqualToString(&prior) {
                return Err(format!(
                    "{description} returned duplicate extension-principal records"
                ));
            }
        }
    }
    Ok(())
}

fn validate_allowed_principals(allowed_principals: &[&str]) -> Result<(), String> {
    const MAX_ALLOWED_PRINCIPALS: usize = 2;
    if allowed_principals.is_empty() || allowed_principals.len() > MAX_ALLOWED_PRINCIPALS {
        return Err("extension-data principal cleanup scope is outside its exact bound".into());
    }
    for (index, principal) in allowed_principals.iter().enumerate() {
        if principal.is_empty()
            || principal.len() > 64
            || !principal
                .bytes()
                .all(|byte| byte == b'-' || byte.is_ascii_lowercase())
            || allowed_principals[..index].contains(principal)
        {
            return Err("extension-data principal cleanup scope is noncanonical".into());
        }
    }
    Ok(())
}

fn validate_error_free_record(
    record: &WKWebExtensionDataRecord,
    description: &str,
) -> Result<(), String> {
    let errors = unsafe { record.errors() };
    if errors.count() > MAX_RECORD_ERRORS {
        return Err(format!(
            "{description} returned {} record errors, exceeding bound {MAX_RECORD_ERRORS}",
            errors.count()
        ));
    }
    if errors.count() != 0 {
        return Err(format!(
            "{description} extension-data record has errors: {}",
            describe_record_errors(&errors)
        ));
    }
    Ok(())
}

fn validate_removal_record_errors(
    records: &NSArray<WKWebExtensionDataRecord>,
    description: &str,
    expected_domain: &NSString,
) -> Result<(), String> {
    let expected_domain = expected_domain.to_string();
    for index in 0..records.count() {
        let errors = unsafe { records.objectAtIndex(index).errors() };
        if errors.count() > MAX_RECORD_ERRORS {
            return Err(format!(
                "{description} returned {} removal-read errors, exceeding bound {MAX_RECORD_ERRORS}",
                errors.count()
            ));
        }
        for error_index in 0..errors.count() {
            let error = errors.objectAtIndex(error_index);
            // WebKit can no longer calculate an unloaded in-memory session
            // store. This documented code is admissible only on the all-type
            // removal input; the subsequent local+sync readback must be
            // error-free and byte-zero.
            let unloaded_session = error.domain().to_string() == expected_domain
                && error.code() == WKWebExtensionDataRecordError::SessionStorageFailed.0;
            if !unloaded_session {
                return Err(format!(
                    "{description} all-type removal input has a terminal error: {}",
                    bounded_native_error(&error)
                ));
            }
        }
    }
    Ok(())
}

fn describe_data_record(record: &WKWebExtensionDataRecord, data_types: &NativeDataTypes) -> String {
    let contained = unsafe { record.containedDataTypes() };
    let errors = unsafe { record.errors() };
    format!(
        "id={}, local_bytes={}, persistent_bytes={}, contained_type_count={}, errors=[{}]",
        bounded_text(&unsafe { record.uniqueIdentifier() }.to_string()),
        unsafe { record.sizeInBytesOfTypes(&data_types.local) },
        unsafe { record.sizeInBytesOfTypes(&data_types.persistent) },
        contained.count(),
        describe_record_errors(&errors),
    )
}

fn describe_record_errors(errors: &NSArray<NSError>) -> String {
    let count = errors.count().min(MAX_RECORD_ERRORS);
    let mut descriptions = Vec::with_capacity(count);
    for index in 0..count {
        descriptions.push(bounded_native_error(&errors.objectAtIndex(index)));
    }
    if errors.count() > MAX_RECORD_ERRORS {
        descriptions.push(format!(
            "{} additional error(s) omitted",
            errors.count() - MAX_RECORD_ERRORS
        ));
    }
    descriptions.join("; ")
}

fn bounded_native_error(error: &NSError) -> String {
    bounded_text(&format!(
        "domain={}, code={}, description={}",
        error.domain(),
        error.code(),
        error.localizedDescription()
    ))
}

fn bounded_text(value: &str) -> String {
    let mut characters = value.chars();
    let bounded = characters
        .by_ref()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn wait_for_release(
    inventory: &ReleaseInventory,
    run_loop: &NSRunLoop,
) -> Result<ReleaseCounts, String> {
    if !(1..=2).contains(&inventory.controllers.len())
        || inventory.contexts.len() != inventory.controllers.len()
        || inventory.stores.len() != inventory.controllers.len()
    {
        return Err(format!(
            "extension-storage cycle release inventory mismatch: controllers={}, contexts={}, stores={}",
            inventory.controllers.len(),
            inventory.contexts.len(),
            inventory.stores.len(),
        ));
    }
    let deadline = Instant::now() + RELEASE_TIMEOUT;
    loop {
        let released_controllers = inventory
            .controllers
            .iter()
            .filter(|controller| controller.load().is_none())
            .count();
        let released_contexts = inventory
            .contexts
            .iter()
            .filter(|context| context.load().is_none())
            .count();
        let released_stores = inventory
            .stores
            .iter()
            .filter(|store| store.load().is_none())
            .count();
        if released_controllers == inventory.controllers.len()
            && released_contexts == inventory.contexts.len()
            && released_stores == inventory.stores.len()
        {
            return Ok(ReleaseCounts {
                controllers: released_controllers,
                contexts: released_contexts,
                stores: released_stores,
            });
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "extension-storage native release did not converge: controllers={released_controllers}/{}, contexts={released_contexts}/{}, stores={released_stores}/{}",
                inventory.controllers.len(),
                inventory.contexts.len(),
                inventory.stores.len(),
            ));
        }
        drain_run_loop_once(run_loop);
    }
}

pub(super) fn validate_runtime_extension(extension: &WKWebExtension) -> Result<(), String> {
    let errors = unsafe { extension.errors() };
    if errors.count() != 0 {
        return Err(format!(
            "runtime MV3 extension parsed with {} error(s)",
            errors.count(),
        ));
    }
    if unsafe { extension.manifestVersion() } != 3.0 {
        return Err("runtime extension was not parsed as manifest v3".into());
    }
    if !unsafe { extension.hasBackgroundContent() }
        || unsafe { extension.hasPersistentBackgroundContent() }
    {
        return Err(
            "runtime extension did not expose an on-demand MV3 background service worker".into(),
        );
    }
    if unsafe { extension.hasInjectedContent() } {
        return Err("runtime extension unexpectedly exposes injectable content".into());
    }
    let storage_permission = webkit_string_symbol(
        b"WKWebExtensionPermissionStorage\0",
        "extension storage permission",
    )?;
    let permissions = unsafe { extension.requestedPermissions() };
    if permissions.count() != 1 || !permissions.containsObject(storage_permission) {
        return Err(format!(
            "runtime extension requested a permission set other than exact storage (count={})",
            permissions.count()
        ));
    }
    Ok(())
}

pub(super) fn write_runtime_extensions(root: &Path) -> Result<RuntimeFixturePaths, String> {
    let paths = RuntimeFixturePaths {
        writer: root.join("runtime-writer"),
        verifier_one: root.join("runtime-verifier-one"),
        verifier_two: root.join("runtime-verifier-two"),
        empty: root.join("runtime-empty"),
    };
    for (path, phase) in [
        (&paths.writer, RuntimePhase::Writer),
        (&paths.verifier_one, RuntimePhase::VerifierOne),
        (&paths.verifier_two, RuntimePhase::VerifierTwo),
        (&paths.empty, RuntimePhase::Empty),
    ] {
        std::fs::create_dir(path).map_err(|error| {
            format!(
                "cannot create persistent runtime fixture {}: {error}",
                path.display()
            )
        })?;
        write_runtime_extension(path, phase)?;
    }
    Ok(paths)
}

fn write_runtime_extension(path: &Path, phase: RuntimePhase) -> Result<(), String> {
    let manifest = json!({
        "manifest_version": 3,
        "name": "Zephium WKWebExtension Runtime Probe",
        "description": "Feature-gated background and persistent-data admission fixture.",
        "version": "1.0.0",
        "permissions": ["storage"],
        "background": {
            "service_worker": "background.js",
            "type": "module"
        }
    });
    write_fixture_file(path, "manifest.json", &manifest.to_string())?;
    write_fixture_file(path, "background.js", &runtime_script(phase))?;
    write_fixture_file(
        path,
        "probe.html",
        "<!doctype html><meta charset=\"utf-8\"><title>zephium-storage-pending</title><script type=\"module\" src=\"probe.js\"></script>",
    )?;
    write_fixture_file(path, "probe.js", &runtime_page_script(phase))?;
    write_fixture_file(
        path,
        "probe-verifier-one.html",
        "<!doctype html><meta charset=\"utf-8\"><title>zephium-storage-pending</title><script type=\"module\" src=\"probe-verifier-one.js\"></script>",
    )?;
    write_fixture_file(
        path,
        "probe-verifier-one.js",
        &runtime_page_script(RuntimePhase::VerifierOne),
    )
}

fn runtime_page_script(phase: RuntimePhase) -> String {
    let phase_name = match phase {
        RuntimePhase::Writer => "writer",
        RuntimePhase::VerifierOne => "verifier-one",
        RuntimePhase::VerifierTwo => "verifier-two",
        RuntimePhase::Empty => "empty",
    };
    format!(
        "{}\ndocument.title = {};",
        runtime_script(phase),
        serde_json::to_string(&format!("zephium-storage-pass-{phase_name}"))
            .expect("static page title is serializable")
    )
}

fn runtime_script(phase: RuntimePhase) -> String {
    let state = |phase: &str| {
        json!({
            "schema": 1,
            "token": PROBE_TOKEN,
            "principal": EXTENSION_PRINCIPAL,
            "phase": phase,
        })
    };
    let sentinel = |phase: &str, payload: String| {
        json!({
            "schema": 1,
            "token": PROBE_TOKEN,
            "principal": EXTENSION_PRINCIPAL,
            "phase": phase,
            "payload": payload,
        })
    };
    let writer = state("writer");
    let verifier_one = state("verifier-one");
    let verifier_two = state("verifier-two");
    let writer_sentinel = sentinel("writer", "w".repeat(WRITER_SENTINEL_BYTES));
    let verifier_one_sentinel = sentinel("verifier-one", "x".repeat(VERIFIER_ONE_SENTINEL_BYTES));
    let verifier_two_sentinel = sentinel("verifier-two", "y".repeat(VERIFIER_TWO_SENTINEL_BYTES));
    let (expected_state, expected_sentinel, next_state, next_sentinel) = match phase {
        RuntimePhase::Writer => (
            serde_json::Value::Null,
            serde_json::Value::Null,
            writer,
            writer_sentinel,
        ),
        RuntimePhase::VerifierOne => (writer, writer_sentinel, verifier_one, verifier_one_sentinel),
        RuntimePhase::VerifierTwo => (
            verifier_one,
            verifier_one_sentinel,
            verifier_two,
            verifier_two_sentinel,
        ),
        RuntimePhase::Empty => (
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
    };

    format!(
        r#"const api = globalThis.browser ?? globalThis.chrome;
const principal = {principal};
const stateKey = {state_key};
const sentinelKey = {sentinel_key};
const expectedState = {expected_state};
const expectedSentinel = {expected_sentinel};
const nextState = {next_state};
const nextSentinel = {next_sentinel};
if (!api?.runtime?.id || !api?.storage?.local) {{
    throw new Error('required runtime or storage API is unavailable');
}}
if (api.runtime.id !== principal) {{
    throw new Error('runtime principal mismatch: ' + api.runtime.id);
}}
function assertObject(actual, wanted, fields, label) {{
    if (!actual || typeof actual !== 'object' || Array.isArray(actual)) {{
        throw new Error(label + ' value is not an object');
    }}
    const actualFields = Object.keys(actual).sort();
    if (actualFields.length !== fields.length ||
        actualFields.some((field, index) => field !== fields[index])) {{
        throw new Error(label + ' fields are not exact');
    }}
    for (const field of fields) {{
        if (actual[field] !== wanted[field]) {{
            throw new Error(label + ' mismatch for ' + field);
        }}
    }}
}}
function assertExactStorage(actual, wantedState, wantedSentinel, label) {{
    if (!actual || typeof actual !== 'object' || Array.isArray(actual)) {{
        throw new Error(label + ' storage result is not an object');
    }}
    const keys = Object.keys(actual).sort();
    if (wantedState === null) {{
        if (keys.length !== 0) {{
            throw new Error(label + ' expected empty storage, got ' + keys.join(','));
        }}
        return;
    }}
    const wantedKeys = wantedSentinel === null
        ? [stateKey]
        : [sentinelKey, stateKey].sort();
    if (keys.length !== wantedKeys.length ||
        keys.some((key, index) => key !== wantedKeys[index])) {{
        throw new Error(label + ' storage keys are not exact');
    }}
    assertObject(
        actual[stateKey],
        wantedState,
        ['phase', 'principal', 'schema', 'token'],
        label + ' state'
    );
    if (wantedSentinel !== null) {{
        assertObject(
            actual[sentinelKey],
            wantedSentinel,
            ['payload', 'phase', 'principal', 'schema', 'token'],
            label + ' sentinel'
        );
    }}
}}
const before = await api.storage.local.get(null);
assertExactStorage(before, expectedState, expectedSentinel, 'before');
if (nextState !== null) {{
    await api.storage.local.set({{ [stateKey]: nextState }});
    const postWrite = await api.storage.local.get(null);
    assertExactStorage(postWrite, nextState, expectedSentinel, 'post-write');
    // The sentinel is deliberately written only after the new state has been
    // read back and validated. Native admission requires its phase-specific
    // byte floor, closing the asynchronous tail.
    await api.storage.local.set({{ [sentinelKey]: nextSentinel }});
}}
const after = await api.storage.local.get(null);
assertExactStorage(
    after,
    nextState === null ? expectedState : nextState,
    nextSentinel === null ? expectedSentinel : nextSentinel,
    'after'
);"#,
        principal =
            serde_json::to_string(EXTENSION_PRINCIPAL).expect("static principal is serializable"),
        state_key = serde_json::to_string(STORAGE_KEY).expect("static storage key is serializable"),
        sentinel_key = serde_json::to_string(STORAGE_SENTINEL_KEY)
            .expect("static sentinel key is serializable"),
        expected_state = expected_state,
        expected_sentinel = expected_sentinel,
        next_state = next_state,
        next_sentinel = next_sentinel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_webkit_symbol_names_fail_closed_before_or_during_lookup() {
        assert!(webkit_string_symbol(b"missing-terminator", "test symbol").is_err());
        assert!(webkit_string_symbol(b"embedded\0terminator\0", "test symbol").is_err());
        assert!(
            webkit_string_symbol(b"ZephiumDefinitelyMissingWebKitSymbol\0", "test symbol").is_err()
        );
    }

    #[test]
    fn namespace_lock_metadata_rejects_public_links_and_identity_substitution() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = tempfile::tempdir().expect("temporary lock-test root");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("secure temporary root permissions");
        let uid = unsafe { geteuid() };
        let root_metadata = std::fs::symlink_metadata(root.path()).expect("root metadata");
        assert_eq!(
            validate_private_temp_directory(root.path(), &root_metadata, uid),
            Ok(())
        );

        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))
            .expect("public temporary root permissions");
        let public_root = std::fs::symlink_metadata(root.path()).expect("public root metadata");
        assert!(validate_private_temp_directory(root.path(), &public_root, uid).is_err());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("restore temporary root permissions");

        let lock = root.path().join("lock");
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&lock)
            .expect("create exact lock file");
        let original = file.metadata().expect("opened lock metadata");
        assert_eq!(validate_lock_metadata(&lock, &original, uid), Ok(()));

        let alias = root.path().join("alias");
        std::fs::hard_link(&lock, &alias).expect("create hard-link attack fixture");
        let multiply_linked = file.metadata().expect("multiply linked lock metadata");
        assert!(validate_lock_metadata(&lock, &multiply_linked, uid).is_err());
        assert_eq!(
            validate_same_file(
                &multiply_linked,
                &std::fs::metadata(&alias).expect("hard-link metadata"),
                "alias",
            ),
            Ok(())
        );

        let substitute = root.path().join("substitute");
        File::create(&substitute).expect("create substitute fixture");
        assert!(validate_same_file(
            &multiply_linked,
            &std::fs::metadata(&substitute).expect("substitute metadata"),
            "substitution",
        )
        .is_err());

        let link = root.path().join("symlink");
        symlink(&lock, &link).expect("create symlink attack fixture");
        let link_metadata = std::fs::symlink_metadata(&link).expect("symlink metadata");
        assert!(validate_lock_metadata(&link, &link_metadata, uid).is_err());
    }

    #[test]
    fn persistent_principal_is_chromium_compatible() {
        assert_eq!(validate_principal(), Ok(()));
    }

    #[test]
    fn profile_record_scopes_are_exact_distinct_and_bounded() {
        let [profile_a, profile_b] = profile_record_principal_scopes();
        assert_eq!(profile_a[0], EXTENSION_PRINCIPAL);
        assert_eq!(profile_b[0], EXTENSION_PRINCIPAL);
        assert_eq!(profile_a[1], super::super::PROFILE_ROUTING_PRINCIPALS[0]);
        assert_eq!(profile_b[1], super::super::PROFILE_ROUTING_PRINCIPALS[1]);
        assert_ne!(profile_a[1], profile_b[1]);
        assert_eq!(validate_allowed_principals(&profile_a), Ok(()));
        assert_eq!(validate_allowed_principals(&profile_b), Ok(()));
        assert!(validate_allowed_principals(&[]).is_err());
        assert!(validate_allowed_principals(&[EXTENSION_PRINCIPAL, EXTENSION_PRINCIPAL,]).is_err());
        assert!(
            validate_allowed_principals(&[EXTENSION_PRINCIPAL, profile_a[1], profile_b[1],])
                .is_err()
        );
    }

    #[test]
    fn runtime_scripts_bind_exact_principal_and_state_transitions() {
        let writer = runtime_script(RuntimePhase::Writer);
        let verifier = runtime_script(RuntimePhase::VerifierOne);
        let empty = runtime_script(RuntimePhase::Empty);
        for script in [&writer, &verifier, &empty] {
            assert!(script.contains(EXTENSION_PRINCIPAL));
            assert!(script.contains("assertExactStorage(before"));
            assert!(script.contains("assertExactStorage("));
            assert!(script.contains("api.runtime.id !== principal"));
        }
        assert!(writer.contains("\"phase\":\"writer\""));
        assert!(verifier.contains("\"phase\":\"verifier-one\""));
        assert!(writer.contains("await api.storage.local.set({ [sentinelKey]: nextSentinel })"));
        let post_write_assertion = writer
            .find("assertExactStorage(postWrite")
            .expect("writer has a post-write self-read assertion");
        let final_sentinel_write = writer
            .find("await api.storage.local.set({ [sentinelKey]: nextSentinel })")
            .expect("writer has a final completion-sentinel write");
        assert!(post_write_assertion < final_sentinel_write);
        assert!(empty.contains("const expectedState = null"));
        assert!(empty.contains("const nextState = null"));
    }

    #[test]
    fn native_completion_floors_match_final_sentinel_padding() {
        let floors = [
            RuntimePhase::Writer.minimum_completed_local_bytes(),
            RuntimePhase::VerifierOne.minimum_completed_local_bytes(),
            RuntimePhase::VerifierTwo.minimum_completed_local_bytes(),
        ];
        assert_eq!(
            floors,
            [
                WRITER_SENTINEL_BYTES,
                VERIFIER_ONE_SENTINEL_BYTES,
                VERIFIER_TWO_SENTINEL_BYTES,
            ]
        );
        assert!(floors.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn diagnostics_are_character_bounded() {
        let value = "é".repeat(MAX_DIAGNOSTIC_CHARS + 1);
        let bounded = bounded_text(&value);
        assert_eq!(bounded.chars().count(), MAX_DIAGNOSTIC_CHARS + 1);
        assert!(bounded.ends_with('…'));
    }
}
