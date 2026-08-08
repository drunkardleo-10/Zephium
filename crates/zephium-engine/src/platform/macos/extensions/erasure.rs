//! Exact, bounded erasure of one process-retained persistent controller.
//!
//! This module cannot create or reopen a controller namespace. It consumes
//! only an entry already retained by [`super::PersistentControllerRegistry`]
//! and therefore makes no cross-restart absence claim. Production activation
//! remains disabled until a durable namespace obligation and cleanup scope
//! are joined above this process-local boundary.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2_foundation::{MainThreadMarker, NSArray, NSError, NSSet, NSString};
use objc2_web_kit::{
    WKWebExtensionController, WKWebExtensionDataRecord, WKWebExtensionDataRecordError,
    WKWebExtensionDataType,
};
use zephium_core::extensions::{
    ExtensionNativeOwnershipIdentity, ExtensionRuntimeBackendTarget,
    MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_core::ids::ProfileId;
use zephium_core::ports::engine::ProfileDataErasureOutcome;

use super::controller_registry::{validate_entry, PersistentControllerEntry};

const MAX_RECORD_ERRORS: usize = 3;
const MAX_PERSISTENT_READBACK_POLLS: usize = 512;
const MAX_CONTROLLER_RELEASE_POLLS: usize = 512;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const POLL_DEADLINE: Duration = Duration::from_secs(5);

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ErasurePhase {
    Ready,
    FetchingAllRecords,
    RemovingAllRecords,
    FetchingPersistentReadback,
    ReleasingController,
    ControllerReleased,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PollDisposition {
    Continue { next_poll: usize },
    DeadlineExceeded,
    PollCapExceeded,
}

fn classify_next_poll(
    now: Instant,
    deadline: Instant,
    completed_poll: usize,
    hard_cap: usize,
) -> PollDisposition {
    if now >= deadline {
        PollDisposition::DeadlineExceeded
    } else if completed_poll >= hard_cap {
        PollDisposition::PollCapExceeded
    } else {
        PollDisposition::Continue {
            next_poll: completed_poll + 1,
        }
    }
}

fn new_poll_deadline(now: Instant) -> Result<Instant, ErasureFailure> {
    now.checked_add(POLL_DEADLINE)
        .ok_or(ErasureFailure::PollDeadlineOverflow)
}

/// Exact generation passed through the later named-store verification and
/// consumed only by matching host-side settlement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ControllerErasureTicket {
    pub(super) profile: ProfileId,
    pub(super) generation: u64,
}

impl ControllerErasureTicket {
    pub(super) const fn profile(self) -> ProfileId {
        self.profile
    }

    pub(super) const fn generation(self) -> u64 {
        self.generation
    }
}

/// Registry-retained witness for one physical controller cleanup attempt.
///
/// The callback chain and registry share this object. Until persistent-byte
/// proof succeeds, `owner` retains the exact controller and store even after
/// the public watchdog reports a timeout. A failed proof never becomes an
/// empty retry merely because a callback returned.
pub(super) struct ControllerErasureWitness {
    profile: ProfileId,
    generation: u64,
    attempt: Arc<AtomicBool>,
    phase: Cell<ErasurePhase>,
    owner: RefCell<Option<PersistentControllerEntry>>,
}

impl ControllerErasureWitness {
    pub(super) fn new(
        profile: ProfileId,
        generation: u64,
        attempt: Arc<AtomicBool>,
        owner: Option<PersistentControllerEntry>,
    ) -> Self {
        Self {
            profile,
            generation,
            attempt,
            phase: Cell::new(ErasurePhase::Ready),
            owner: RefCell::new(owner),
        }
    }

    pub(super) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn ticket(&self) -> ControllerErasureTicket {
        ControllerErasureTicket {
            profile: self.profile,
            generation: self.generation,
        }
    }

    pub(super) fn matches_attempt(&self, attempt: &Arc<AtomicBool>) -> bool {
        Arc::ptr_eq(&self.attempt, attempt)
    }

    pub(super) fn attempt_is_active(&self) -> bool {
        self.attempt.load(Ordering::Acquire)
    }

    pub(super) fn controller_release_is_proven(&self) -> bool {
        self.phase.get() == ErasurePhase::ControllerReleased
    }

    pub(super) fn retains_native_owner(&self) -> bool {
        self.owner
            .try_borrow()
            .map_or(true, |owner| owner.is_some())
    }

    pub(super) fn mark_controller_released_for_retry(&self) {
        debug_assert!(!self.retains_native_owner());
        self.phase.set(ErasurePhase::ControllerReleased);
    }

    fn transition(&self, expected: ErasurePhase, next: ErasurePhase) -> bool {
        if self.phase.get() != expected {
            return false;
        }
        self.phase.set(next);
        true
    }

    fn controller(&self) -> Result<Retained<WKWebExtensionController>, ErasureFailure> {
        let owner = self
            .owner
            .try_borrow()
            .map_err(|_| ErasureFailure::ReentrantState)?;
        owner
            .as_ref()
            .map(|owner| owner.controller.clone())
            .ok_or(ErasureFailure::OwnerMissing)
    }

    fn take_owner(&self) -> Result<PersistentControllerEntry, ErasureFailure> {
        self.owner
            .try_borrow_mut()
            .map_err(|_| ErasureFailure::ReentrantState)?
            .take()
            .ok_or(ErasureFailure::OwnerMissing)
    }

    fn fail(&self) -> bool {
        match self.phase.get() {
            ErasurePhase::Failed => false,
            ErasurePhase::ControllerReleased => false,
            _ => {
                self.phase.set(ErasurePhase::Failed);
                true
            }
        }
    }
}

/// Host-to-platform handoff after views have closed.
pub(crate) enum ProfileControllerErasure {
    /// Store supplied no durable namespace scope and the process registry had
    /// no controller entry. Both independently bounded facts agree that this
    /// extension-controller stage is not required.
    NamespaceNotRequired,
    /// Physical data cleanup and release are still required.
    Pending(PersistentControllerErasure),
    /// A prior attempt already proved controller release; only the later
    /// named website-store deletion failed and may be retried.
    ControllerAlreadyReleased(ControllerErasureTicket),
}

/// Move-only physical erasure owner.
pub(crate) struct PersistentControllerErasure {
    witness: Rc<ControllerErasureWitness>,
}

impl PersistentControllerErasure {
    pub(super) fn new(witness: Rc<ControllerErasureWitness>) -> Self {
        Self { witness }
    }

    pub(crate) fn start(
        self,
        completion: Arc<crate::erasure::Completion>,
        released: impl FnOnce(ControllerErasureTicket) + 'static,
    ) {
        let prepared = guarded_native(|| {
            let owner = self
                .witness
                .owner
                .try_borrow()
                .map_err(|_| ErasureFailure::ReentrantState)?;
            let owner = owner.as_ref().ok_or(ErasureFailure::OwnerMissing)?;
            validate_entry(owner).map_err(|_| ErasureFailure::InvalidOwner)?;
            let mtm = MainThreadMarker::new().ok_or(ErasureFailure::MainThreadRequired)?;
            NativeDataTypes::discover(mtm)
        });
        let data_types = match prepared {
            Ok(data_types) => Rc::new(data_types),
            Err(failure) => {
                fail_unsettled(&self.witness, &completion, failure);
                return;
            }
        };
        let machine = ErasureMachine {
            witness: self.witness,
            data_types,
            completion,
            released: Rc::new(RefCell::new(Some(Box::new(released)))),
        };
        run_guarded(&machine, || machine.fetch_all_records());
    }
}

type ReleasedCallback = Box<dyn FnOnce(ControllerErasureTicket)>;

#[derive(Clone)]
struct ErasureMachine {
    witness: Rc<ControllerErasureWitness>,
    data_types: Rc<NativeDataTypes>,
    completion: Arc<crate::erasure::Completion>,
    released: Rc<RefCell<Option<ReleasedCallback>>>,
}

impl ErasureMachine {
    fn fetch_all_records(&self) -> Result<(), ErasureFailure> {
        if !self
            .witness
            .transition(ErasurePhase::Ready, ErasurePhase::FetchingAllRecords)
        {
            return Ok(());
        }
        let controller = self.witness.controller()?;
        let callback_machine = self.clone();
        let fired = Cell::new(false);
        let callback =
            block2::RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
                if fired.replace(true) {
                    return;
                }
                let machine = callback_machine.clone();
                run_guarded(&machine.clone(), || machine.on_all_records(records));
            });
        unsafe {
            controller.fetchDataRecordsOfTypes_completionHandler(&self.data_types.all, &callback);
        }
        Ok(())
    }

    fn on_all_records(
        &self,
        records: NonNull<NSArray<WKWebExtensionDataRecord>>,
    ) -> Result<(), ErasureFailure> {
        if self.witness.phase.get() != ErasurePhase::FetchingAllRecords {
            return Ok(());
        }
        let records = unsafe { Retained::retain(records.as_ptr()) }
            .ok_or(ErasureFailure::ReleasedCallbackValue)?;
        validate_record_inventory(&records, &self.data_types, RecordErrorPolicy::SessionOnly)?;
        if records.count() == 0 {
            if self.witness.transition(
                ErasurePhase::FetchingAllRecords,
                ErasurePhase::FetchingPersistentReadback,
            ) {
                self.start_persistent_readback_polling()?;
            }
            return Ok(());
        }

        if !self.witness.transition(
            ErasurePhase::FetchingAllRecords,
            ErasurePhase::RemovingAllRecords,
        ) {
            return Ok(());
        }
        let controller = self.witness.controller()?;
        let callback_machine = self.clone();
        let fired = Cell::new(false);
        let callback = block2::RcBlock::new(move || {
            if fired.replace(true) {
                return;
            }
            let machine = callback_machine.clone();
            run_guarded(&machine.clone(), || machine.on_remove_completed());
        });
        unsafe {
            controller.removeDataOfTypes_fromDataRecords_completionHandler(
                &self.data_types.all,
                &records,
                &callback,
            );
        }
        Ok(())
    }

    fn on_remove_completed(&self) -> Result<(), ErasureFailure> {
        if self.witness.transition(
            ErasurePhase::RemovingAllRecords,
            ErasurePhase::FetchingPersistentReadback,
        ) {
            self.start_persistent_readback_polling()?;
        }
        Ok(())
    }

    fn start_persistent_readback_polling(&self) -> Result<(), ErasureFailure> {
        let deadline = new_poll_deadline(Instant::now())?;
        self.schedule_persistent_readback(1, deadline)
    }

    fn schedule_persistent_readback(
        &self,
        poll: usize,
        deadline: Instant,
    ) -> Result<(), ErasureFailure> {
        let machine = self.clone();
        dispatch_main_after(machine, POLL_INTERVAL, move |machine| {
            machine.fetch_persistent_readback(poll, deadline)
        })
    }

    fn fetch_persistent_readback(
        &self,
        poll: usize,
        deadline: Instant,
    ) -> Result<(), ErasureFailure> {
        if self.witness.phase.get() != ErasurePhase::FetchingPersistentReadback {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ErasureFailure::PollDeadlineExceeded);
        }
        let controller = self.witness.controller()?;
        let callback_machine = self.clone();
        let fired = Cell::new(false);
        let callback =
            block2::RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
                if fired.replace(true) {
                    return;
                }
                let machine = callback_machine.clone();
                run_guarded(&machine.clone(), || {
                    machine.on_persistent_readback(poll, deadline, records)
                });
            });
        unsafe {
            controller
                .fetchDataRecordsOfTypes_completionHandler(&self.data_types.persistent, &callback);
        }
        Ok(())
    }

    fn on_persistent_readback(
        &self,
        poll: usize,
        deadline: Instant,
        records: NonNull<NSArray<WKWebExtensionDataRecord>>,
    ) -> Result<(), ErasureFailure> {
        if self.witness.phase.get() != ErasurePhase::FetchingPersistentReadback {
            return Ok(());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(ErasureFailure::PollDeadlineExceeded);
        }
        let records = unsafe { Retained::retain(records.as_ptr()) }
            .ok_or(ErasureFailure::ReleasedCallbackValue)?;
        validate_record_inventory(&records, &self.data_types, RecordErrorPolicy::None)?;
        let all_zero = (0..records.count()).all(|index| {
            let record = records.objectAtIndex(index);
            (unsafe { record.sizeInBytesOfTypes(&self.data_types.persistent) }) == 0
        });
        if all_zero {
            return self.begin_controller_release();
        }
        match classify_next_poll(now, deadline, poll, MAX_PERSISTENT_READBACK_POLLS) {
            PollDisposition::Continue { next_poll } => {
                self.schedule_persistent_readback(next_poll, deadline)?;
            }
            PollDisposition::DeadlineExceeded => {
                return Err(ErasureFailure::PollDeadlineExceeded);
            }
            PollDisposition::PollCapExceeded => {
                return Err(ErasureFailure::PollCapExceeded);
            }
        }
        Ok(())
    }

    fn begin_controller_release(&self) -> Result<(), ErasureFailure> {
        if !self.witness.transition(
            ErasurePhase::FetchingPersistentReadback,
            ErasurePhase::ReleasingController,
        ) {
            return Ok(());
        }
        let deadline = new_poll_deadline(Instant::now())?;
        let owner = self.witness.take_owner()?;
        validate_entry(&owner).map_err(|_| ErasureFailure::InvalidOwner)?;
        let weak = Weak::from_retained(&owner.controller);
        drop(owner);
        self.schedule_controller_release_poll(weak, 1, deadline)
    }

    fn schedule_controller_release_poll(
        &self,
        weak: Weak<WKWebExtensionController>,
        poll: usize,
        deadline: Instant,
    ) -> Result<(), ErasureFailure> {
        let machine = self.clone();
        dispatch_main_after(machine, POLL_INTERVAL, move |machine| {
            machine.poll_controller_release(weak, poll, deadline)
        })
    }

    fn poll_controller_release(
        &self,
        weak: Weak<WKWebExtensionController>,
        poll: usize,
        deadline: Instant,
    ) -> Result<(), ErasureFailure> {
        if self.witness.phase.get() != ErasurePhase::ReleasingController {
            return Ok(());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(ErasureFailure::PollDeadlineExceeded);
        }
        if weak.load().is_none() {
            if !self.witness.transition(
                ErasurePhase::ReleasingController,
                ErasurePhase::ControllerReleased,
            ) {
                return Ok(());
            }
            let released = self
                .released
                .try_borrow_mut()
                .map_err(|_| ErasureFailure::ReentrantState)?
                .take()
                .ok_or(ErasureFailure::MissingContinuation)?;
            released(self.witness.ticket());
            return Ok(());
        }
        match classify_next_poll(now, deadline, poll, MAX_CONTROLLER_RELEASE_POLLS) {
            PollDisposition::Continue { next_poll } => {
                self.schedule_controller_release_poll(weak, next_poll, deadline)?;
            }
            PollDisposition::DeadlineExceeded => {
                return Err(ErasureFailure::PollDeadlineExceeded);
            }
            PollDisposition::PollCapExceeded => {
                return Err(ErasureFailure::PollCapExceeded);
            }
        }
        Ok(())
    }

    fn fail_unsettled(&self, failure: ErasureFailure) {
        fail_unsettled(&self.witness, &self.completion, failure);
    }
}

struct NativeDataTypes {
    all: Retained<NSSet<WKWebExtensionDataType>>,
    persistent: Retained<NSSet<WKWebExtensionDataType>>,
    local: &'static WKWebExtensionDataType,
    session: &'static WKWebExtensionDataType,
    synchronized: &'static WKWebExtensionDataType,
    error_domain: &'static NSString,
}

impl NativeDataTypes {
    fn discover(mtm: MainThreadMarker) -> Result<Self, ErasureFailure> {
        let local = webkit_string_symbol(b"WKWebExtensionDataTypeLocal\0")?;
        let session = webkit_string_symbol(b"WKWebExtensionDataTypeSession\0")?;
        let synchronized = webkit_string_symbol(b"WKWebExtensionDataTypeSynchronized\0")?;
        let error_domain = webkit_string_symbol(b"WKWebExtensionDataRecordErrorDomain\0")?;
        let all = unsafe { WKWebExtensionController::allExtensionDataTypes(mtm) };
        if all.count() != 3
            || !all.containsObject(local)
            || !all.containsObject(session)
            || !all.containsObject(synchronized)
        {
            return Err(ErasureFailure::UnknownDataTypeInventory);
        }
        Ok(Self {
            all,
            persistent: NSSet::from_slice(&[local, synchronized]),
            local,
            session,
            synchronized,
            error_domain,
        })
    }

    fn contains_only_known_types(&self, contained: &NSSet<WKWebExtensionDataType>) -> bool {
        let known = usize::from(contained.containsObject(self.local))
            + usize::from(contained.containsObject(self.session))
            + usize::from(contained.containsObject(self.synchronized));
        contained.count() == known
    }
}

#[derive(Clone, Copy)]
enum RecordErrorPolicy {
    None,
    SessionOnly,
}

fn validate_record_inventory(
    records: &NSArray<WKWebExtensionDataRecord>,
    data_types: &NativeDataTypes,
    error_policy: RecordErrorPolicy,
) -> Result<(), ErasureFailure> {
    if records.count() > MAX_EXTENSION_INSTALLS_PER_PROFILE {
        return Err(ErasureFailure::TooManyRecords);
    }
    let mut principals = HashSet::with_capacity(records.count());
    for index in 0..records.count() {
        let record = records.objectAtIndex(index);
        let native_identifier = unsafe { record.uniqueIdentifier() };
        if native_identifier.length() != 32 {
            return Err(ErasureFailure::InvalidPrincipal);
        }
        let identifier = native_identifier.to_string();
        let identity = ExtensionNativeOwnershipIdentity::parse(
            ExtensionRuntimeBackendTarget::MacosNative,
            &identifier,
        )
        .map_err(|_| ErasureFailure::InvalidPrincipal)?;
        if !principals.insert(identity.bytes()) {
            return Err(ErasureFailure::DuplicatePrincipal);
        }
        let contained = unsafe { record.containedDataTypes() };
        if !data_types.contains_only_known_types(&contained) {
            return Err(ErasureFailure::UnknownRecordDataType);
        }
        validate_record_errors(&record, &contained, data_types, error_policy)?;
    }
    Ok(())
}

fn validate_record_errors(
    record: &WKWebExtensionDataRecord,
    contained: &NSSet<WKWebExtensionDataType>,
    data_types: &NativeDataTypes,
    policy: RecordErrorPolicy,
) -> Result<(), ErasureFailure> {
    let errors = unsafe { record.errors() };
    if errors.count() > MAX_RECORD_ERRORS {
        return Err(ErasureFailure::TooManyRecordErrors);
    }
    match policy {
        RecordErrorPolicy::None if errors.count() != 0 => Err(ErasureFailure::RecordError),
        RecordErrorPolicy::None => Ok(()),
        RecordErrorPolicy::SessionOnly => {
            let mut exact_session_storage_failures = 0;
            for index in 0..errors.count() {
                let error: Retained<NSError> = errors.objectAtIndex(index);
                if error.domain().isEqualToString(data_types.error_domain)
                    && error.code() == WKWebExtensionDataRecordError::SessionStorageFailed.0
                {
                    exact_session_storage_failures += 1;
                }
            }
            let facts = PreRemovalRecordErrorFacts {
                count: errors.count(),
                exact_session_storage_failures,
                contained_session: contained.containsObject(data_types.session),
            };
            pre_removal_record_errors_are_admissible(facts)
                .then_some(())
                .ok_or(ErasureFailure::RecordError)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreRemovalRecordErrorFacts {
    count: usize,
    exact_session_storage_failures: usize,
    contained_session: bool,
}

fn pre_removal_record_errors_are_admissible(facts: PreRemovalRecordErrorFacts) -> bool {
    // WebKit can report the one documented transient session-storage error
    // without listing session storage in `containedDataTypes`. The exact
    // error domain and code are the authority; the contained set is not.
    let _contained_session_is_advisory = facts.contained_session;
    facts.count <= 1 && facts.count == facts.exact_session_storage_failures
}

fn webkit_string_symbol(
    nul_terminated_name: &'static [u8],
) -> Result<&'static NSString, ErasureFailure> {
    if nul_terminated_name.last() != Some(&0)
        || nul_terminated_name[..nul_terminated_name.len().saturating_sub(1)].contains(&0)
    {
        return Err(ErasureFailure::InvalidSymbolName);
    }
    // Darwin defines RTLD_DEFAULT as `(void *)-2`. The exported symbol is a
    // pointer slot containing the Objective-C NSString object.
    let symbol = unsafe {
        dlsym(
            (-2_isize) as *mut c_void,
            nul_terminated_name.as_ptr().cast(),
        )
    };
    let slot = NonNull::new(symbol.cast::<*const NSString>())
        .ok_or(ErasureFailure::MissingRuntimeSymbol)?;
    let object = unsafe { slot.as_ptr().read() };
    unsafe { object.as_ref() }.ok_or(ErasureFailure::MissingRuntimeSymbol)
}

fn run_guarded(machine: &ErasureMachine, operation: impl FnOnce() -> Result<(), ErasureFailure>) {
    if let Err(failure) = guarded_native(operation) {
        machine.fail_unsettled(failure);
    }
}

fn guarded_native<T>(
    operation: impl FnOnce() -> Result<T, ErasureFailure>,
) -> Result<T, ErasureFailure> {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        objc2::exception::catch(AssertUnwindSafe(operation))
    }));
    match result {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(failure))) => Err(failure),
        Ok(Err(_)) => Err(ErasureFailure::NativeException),
        Err(_) => Err(ErasureFailure::RustPanic),
    }
}

fn fail_unsettled(
    witness: &ControllerErasureWitness,
    completion: &Arc<crate::erasure::Completion>,
    failure: ErasureFailure,
) {
    let phase = witness.phase.get();
    if !witness.fail() {
        return;
    }
    eprintln!(
        "privacy: macOS extension-controller erasure proof failed (phase={phase:?}, code={failure:?})"
    );
    let completion = completion.clone();
    let _ = std::panic::catch_unwind(AssertUnwindSafe(move || {
        completion.report_unsettled(ProfileDataErasureOutcome::Failed);
    }));
}

fn dispatch_main_after(
    machine: ErasureMachine,
    delay: Duration,
    operation: impl FnOnce(ErasureMachine) -> Result<(), ErasureFailure> + 'static,
) -> Result<(), ErasureFailure> {
    let when = dispatch2::DispatchTime::try_from(delay)
        .map_err(|_| ErasureFailure::PollScheduleConversion)?;
    let fallback = machine.clone();
    let task = RefCell::new(Some((machine, operation)));
    let callback: block2::RcBlock<dyn Fn()> = block2::RcBlock::new(move || {
        let task = task.try_borrow_mut().ok().and_then(|mut task| task.take());
        let Some((machine, operation)) = task else {
            fallback.fail_unsettled(ErasureFailure::ReentrantState);
            return;
        };
        run_guarded(&machine.clone(), || operation(machine));
    });
    // SAFETY: dispatch_after copies this heap block. Every captured native
    // object is main-thread-only, and the destination is the main queue. The
    // block is therefore never transferred to a queue that may execute it on
    // another thread despite the intentionally non-Send captures.
    unsafe {
        dispatch2::DispatchQueue::exec_after_with_block(
            when,
            dispatch2::DispatchQueue::main(),
            block2::RcBlock::as_ptr(&callback),
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ErasureFailure {
    MainThreadRequired,
    InvalidOwner,
    OwnerMissing,
    ReentrantState,
    ReleasedCallbackValue,
    MissingContinuation,
    InvalidSymbolName,
    MissingRuntimeSymbol,
    UnknownDataTypeInventory,
    TooManyRecords,
    InvalidPrincipal,
    DuplicatePrincipal,
    UnknownRecordDataType,
    TooManyRecordErrors,
    RecordError,
    PollDeadlineOverflow,
    PollScheduleConversion,
    PollDeadlineExceeded,
    PollCapExceeded,
    NativeException,
    RustPanic,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical_principal(value: &str) -> Result<[u8; 32], ErasureFailure> {
        ExtensionNativeOwnershipIdentity::parse(ExtensionRuntimeBackendTarget::MacosNative, value)
            .map(ExtensionNativeOwnershipIdentity::bytes)
            .map_err(|_| ErasureFailure::InvalidPrincipal)
    }

    #[test]
    fn principal_grammar_is_exact_and_redaction_friendly() {
        assert_eq!(
            canonical_principal("abcdefghijklmnopabcdefghijklmnop").unwrap(),
            *b"abcdefghijklmnopabcdefghijklmnop"
        );
        assert_eq!(
            canonical_principal("abcdefghijklmnopabcdefghijklmn0p"),
            Err(ErasureFailure::InvalidPrincipal)
        );
        assert_eq!(
            canonical_principal("abcdefghijklmnopabcdefghijklmnopa"),
            Err(ErasureFailure::InvalidPrincipal)
        );
    }

    #[test]
    fn duplicate_principal_detection_is_bounded_and_exact() {
        let first = canonical_principal("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let second = canonical_principal("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let mut seen = HashSet::with_capacity(MAX_EXTENSION_INSTALLS_PER_PROFILE);
        assert!(seen.insert(first));
        assert!(seen.insert(second));
        assert!(!seen.insert(first));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn callback_phase_does_not_cross_stale_or_duplicate_transitions() {
        let phase = Cell::new(ErasurePhase::Ready);
        let transition = |expected, next| {
            if phase.get() != expected {
                return false;
            }
            phase.set(next);
            true
        };
        assert!(transition(
            ErasurePhase::Ready,
            ErasurePhase::FetchingAllRecords
        ));
        assert!(!transition(
            ErasurePhase::Ready,
            ErasurePhase::FetchingAllRecords
        ));
        assert!(!transition(
            ErasurePhase::RemovingAllRecords,
            ErasurePhase::FetchingPersistentReadback
        ));
        assert_eq!(phase.get(), ErasurePhase::FetchingAllRecords);
    }

    #[test]
    fn retry_and_poll_bounds_are_finite() {
        const {
            assert!(MAX_PERSISTENT_READBACK_POLLS > 0);
            assert!(MAX_CONTROLLER_RELEASE_POLLS > 0);
            assert!(MAX_PERSISTENT_READBACK_POLLS <= 1_024);
            assert!(MAX_CONTROLLER_RELEASE_POLLS <= 1_024);
        }
        assert_eq!(POLL_INTERVAL, Duration::from_millis(10));
        assert_eq!(POLL_DEADLINE, Duration::from_secs(5));
        assert_eq!(MAX_RECORD_ERRORS, 3);
        assert_eq!(MAX_EXTENSION_INSTALLS_PER_PROFILE, 8);
    }

    #[test]
    fn poll_progress_is_bounded_by_deadline_and_hard_cap() {
        let now = Instant::now();
        let deadline = now.checked_add(POLL_DEADLINE).unwrap();

        assert_eq!(
            classify_next_poll(now, deadline, 7, 10),
            PollDisposition::Continue { next_poll: 8 }
        );
        assert_eq!(
            classify_next_poll(now, deadline, 10, 10),
            PollDisposition::PollCapExceeded
        );
        assert_eq!(
            classify_next_poll(deadline, deadline, 7, 10),
            PollDisposition::DeadlineExceeded
        );
        assert_eq!(
            classify_next_poll(deadline, deadline, 10, 10),
            PollDisposition::DeadlineExceeded
        );
        assert_eq!(new_poll_deadline(now).unwrap(), deadline);
    }

    #[test]
    fn exact_session_error_is_allowed_without_contained_session_storage() {
        assert!(pre_removal_record_errors_are_admissible(
            PreRemovalRecordErrorFacts {
                count: 1,
                exact_session_storage_failures: 1,
                contained_session: false,
            }
        ));
        assert!(!pre_removal_record_errors_are_admissible(
            PreRemovalRecordErrorFacts {
                count: 1,
                exact_session_storage_failures: 0,
                contained_session: true,
            }
        ));
        assert!(!pre_removal_record_errors_are_admissible(
            PreRemovalRecordErrorFacts {
                count: 2,
                exact_session_storage_failures: 2,
                contained_session: false,
            }
        ));
    }

    #[test]
    fn dynamic_symbol_names_reject_interior_nul_and_missing_terminator() {
        assert!(matches!(
            webkit_string_symbol(b"WKWebExtensionDataTypeLocal"),
            Err(ErasureFailure::InvalidSymbolName)
        ));
        assert!(matches!(
            webkit_string_symbol(b"WKWeb\0Extension\0"),
            Err(ErasureFailure::InvalidSymbolName)
        ));
    }
}
