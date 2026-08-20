//! Exact per-extension persistent-data erasure inside one profile controller.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::{c_char, c_void};
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSArray, NSError, NSSet, NSString};
use objc2_web_kit::{
    WKWebExtensionController, WKWebExtensionDataRecord, WKWebExtensionDataRecordError,
    WKWebExtensionDataType,
};
use zephium_core::extensions::{
    ExtensionNativeOwnershipIdentity, ExtensionRuntimeBackendTarget,
    MAX_EXTENSION_INSTALLS_PER_PROFILE,
};
use zephium_extension_runtime_api::{
    ExtensionRuntimeHostDataErasureDisposition, ExtensionRuntimeNativeOwnerId,
};

const MAX_RECORDS: usize = MAX_EXTENSION_INSTALLS_PER_PROFILE * 2;
const MAX_RECORD_ERRORS: usize = 3;
const MAX_POLLS: usize = 512;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type Completion = Box<dyn FnOnce(ExtensionRuntimeHostDataErasureDisposition)>;

pub(super) fn begin(
    controller: Retained<WKWebExtensionController>,
    identity: ExtensionRuntimeNativeOwnerId,
    deadline: Instant,
    completion: Completion,
) {
    let prepared = guarded(|| {
        let mtm = MainThreadMarker::new().ok_or(())?;
        Ok((DataTypes::discover(mtm)?, identity_text(identity)?))
    });
    let Ok((types, identity)) = prepared else {
        completion(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
        return;
    };
    let machine = Rc::new(Machine {
        controller,
        types,
        identity,
        deadline,
        completion: RefCell::new(Some(completion)),
        terminal: Cell::new(false),
    });
    run(&machine, || machine.fetch_before_removal());
}

struct Machine {
    controller: Retained<WKWebExtensionController>,
    types: DataTypes,
    identity: Retained<NSString>,
    deadline: Instant,
    completion: RefCell<Option<Completion>>,
    terminal: Cell<bool>,
}

impl Machine {
    fn finish(&self, disposition: ExtensionRuntimeHostDataErasureDisposition) {
        if self.terminal.replace(true) {
            return;
        }
        if let Ok(mut completion) = self.completion.try_borrow_mut() {
            if let Some(completion) = completion.take() {
                completion(disposition);
            }
        }
    }

    fn fetch_before_removal(self: &Rc<Self>) -> Result<(), ()> {
        if Instant::now() >= self.deadline {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
            return Ok(());
        }
        let machine = Rc::clone(self);
        let fired = Cell::new(false);
        let callback =
            block2::RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
                if fired.replace(true) {
                    machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
                    return;
                }
                let records = unsafe { Retained::retain(records.as_ptr()) };
                run(&machine, || {
                    let records = records.ok_or(())?;
                    machine.on_before_removal(records)
                });
            });
        unsafe {
            self.controller
                .fetchDataRecordsOfTypes_completionHandler(&self.types.all, &callback);
        }
        Ok(())
    }

    fn on_before_removal(
        self: &Rc<Self>,
        records: Retained<NSArray<WKWebExtensionDataRecord>>,
    ) -> Result<(), ()> {
        validate_records(&records, &self.types, true)?;
        let mut matching = Vec::new();
        for index in 0..records.count() {
            let record = records.objectAtIndex(index);
            if unsafe { record.uniqueIdentifier() }.isEqualToString(&self.identity) {
                matching.push(record);
            }
        }
        match matching.len() {
            0 => {
                self.finish(ExtensionRuntimeHostDataErasureDisposition::NotPresent);
                Ok(())
            }
            1 => {
                let records = NSArray::from_retained_slice(&matching);
                let machine = Rc::clone(self);
                let fired = Cell::new(false);
                let callback = block2::RcBlock::new(move || {
                    if fired.replace(true) {
                        machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
                        return;
                    }
                    run(&machine, || machine.schedule_readback(1));
                });
                unsafe {
                    self.controller
                        .removeDataOfTypes_fromDataRecords_completionHandler(
                            &self.types.all,
                            &records,
                            &callback,
                        );
                }
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn schedule_readback(self: &Rc<Self>, poll: usize) -> Result<(), ()> {
        if poll > MAX_POLLS || Instant::now() >= self.deadline {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
            return Ok(());
        }
        let when = dispatch2::DispatchTime::try_from(POLL_INTERVAL).map_err(|_| ())?;
        let machine = Rc::clone(self);
        let callback: block2::RcBlock<dyn Fn()> = block2::RcBlock::new(move || {
            run(&machine, || machine.fetch_readback(poll));
        });
        unsafe {
            dispatch2::DispatchQueue::exec_after_with_block(
                when,
                dispatch2::DispatchQueue::main(),
                block2::RcBlock::as_ptr(&callback),
            );
        }
        Ok(())
    }

    fn fetch_readback(self: &Rc<Self>, poll: usize) -> Result<(), ()> {
        if Instant::now() >= self.deadline {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::TimedOut);
            return Ok(());
        }
        let machine = Rc::clone(self);
        let fired = Cell::new(false);
        let callback =
            block2::RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
                if fired.replace(true) {
                    machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
                    return;
                }
                let records = unsafe { Retained::retain(records.as_ptr()) };
                run(&machine, || {
                    let records = records.ok_or(())?;
                    machine.on_readback(records, poll)
                });
            });
        unsafe {
            self.controller
                .fetchDataRecordsOfTypes_completionHandler(&self.types.persistent, &callback);
        }
        Ok(())
    }

    fn on_readback(
        self: &Rc<Self>,
        records: Retained<NSArray<WKWebExtensionDataRecord>>,
        poll: usize,
    ) -> Result<(), ()> {
        validate_records(&records, &self.types, false)?;
        let mut matching = 0;
        let mut nonzero = false;
        for index in 0..records.count() {
            let record = records.objectAtIndex(index);
            if unsafe { record.uniqueIdentifier() }.isEqualToString(&self.identity) {
                matching += 1;
                nonzero |= unsafe { record.sizeInBytesOfTypes(&self.types.persistent) } != 0;
            }
        }
        if matching > 1 {
            return Err(());
        }
        if matching == 0 || !nonzero {
            self.finish(ExtensionRuntimeHostDataErasureDisposition::Erased);
            return Ok(());
        }
        self.schedule_readback(poll.checked_add(1).ok_or(())?)
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if !self.terminal.get() {
            if let Ok(mut completion) = self.completion.try_borrow_mut() {
                if let Some(completion) = completion.take() {
                    completion(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
                }
            }
        }
    }
}

struct DataTypes {
    all: Retained<NSSet<WKWebExtensionDataType>>,
    persistent: Retained<NSSet<WKWebExtensionDataType>>,
    local: &'static WKWebExtensionDataType,
    session: &'static WKWebExtensionDataType,
    synchronized: &'static WKWebExtensionDataType,
    error_domain: &'static NSString,
}

impl DataTypes {
    fn discover(mtm: MainThreadMarker) -> Result<Self, ()> {
        let local = symbol(b"WKWebExtensionDataTypeLocal\0")?;
        let session = symbol(b"WKWebExtensionDataTypeSession\0")?;
        let synchronized = symbol(b"WKWebExtensionDataTypeSynchronized\0")?;
        let error_domain = symbol(b"WKWebExtensionDataRecordErrorDomain\0")?;
        let all = unsafe { WKWebExtensionController::allExtensionDataTypes(mtm) };
        if all.count() != 3
            || !all.containsObject(local)
            || !all.containsObject(session)
            || !all.containsObject(synchronized)
        {
            return Err(());
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
}

fn validate_records(
    records: &NSArray<WKWebExtensionDataRecord>,
    types: &DataTypes,
    allow_session_error: bool,
) -> Result<(), ()> {
    if records.count() > MAX_RECORDS {
        return Err(());
    }
    let mut identities = HashSet::with_capacity(records.count());
    for index in 0..records.count() {
        let record = records.objectAtIndex(index);
        let identifier = unsafe { record.uniqueIdentifier() }.to_string();
        let identity = ExtensionNativeOwnershipIdentity::parse(
            ExtensionRuntimeBackendTarget::MacosNative,
            &identifier,
        )
        .map_err(|_| ())?;
        if !identities.insert(identity.bytes()) {
            return Err(());
        }
        let contained = unsafe { record.containedDataTypes() };
        let known = usize::from(contained.containsObject(types.local))
            + usize::from(contained.containsObject(types.session))
            + usize::from(contained.containsObject(types.synchronized));
        if contained.count() != known {
            return Err(());
        }
        let errors = unsafe { record.errors() };
        if errors.count() > MAX_RECORD_ERRORS {
            return Err(());
        }
        for error_index in 0..errors.count() {
            let error: Retained<NSError> = errors.objectAtIndex(error_index);
            if !allow_session_error
                || !error.domain().isEqualToString(types.error_domain)
                || error.code() != WKWebExtensionDataRecordError::SessionStorageFailed.0
            {
                return Err(());
            }
        }
    }
    Ok(())
}

fn identity_text(identity: ExtensionRuntimeNativeOwnerId) -> Result<Retained<NSString>, ()> {
    let bytes = identity.encoded_bytes();
    let text = std::str::from_utf8(&bytes).map_err(|_| ())?;
    Ok(NSString::from_str(text))
}

fn symbol(name: &'static [u8]) -> Result<&'static NSString, ()> {
    if name.last() != Some(&0) || name[..name.len().saturating_sub(1)].contains(&0) {
        return Err(());
    }
    let symbol = unsafe { dlsym((-2_isize) as *mut c_void, name.as_ptr().cast()) };
    let slot = NonNull::new(symbol.cast::<*const NSString>()).ok_or(())?;
    let object = unsafe { slot.as_ptr().read() };
    unsafe { object.as_ref() }.ok_or(())
}

fn run(machine: &Rc<Machine>, operation: impl FnOnce() -> Result<(), ()>) {
    if guarded(operation).is_err() {
        machine.finish(ExtensionRuntimeHostDataErasureDisposition::FailedClosed);
    }
}

fn guarded<T>(operation: impl FnOnce() -> Result<T, ()>) -> Result<T, ()> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| {
        objc2::exception::catch(AssertUnwindSafe(operation))
    })) {
        Ok(Ok(Ok(value))) => Ok(value),
        _ => Err(()),
    }
}
