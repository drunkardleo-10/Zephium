#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

//! Bounded, callback-driven WebView2 cookie transfer.
//!
//! This owner never pumps the message loop, blocks a thread, exposes a cookie
//! field, or writes before every requested origin has completed preflight.
//! Destination mutation is sequential. Legacy transfers clear their disposable
//! destination after a failed write; retained Work seeds delete only journaled
//! new identities. An unproven cleanup must quarantine the automation profile.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Cookie, ICoreWebView2CookieList, ICoreWebView2CookieManager,
    ICoreWebView2Environment, ICoreWebView2Profile2, ICoreWebView2_2,
    COREWEBVIEW2_COOKIE_SAME_SITE_KIND, COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
    COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE, COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
};
use webview2_com::{ClearBrowsingDataCompletedHandler, GetCookiesCompletedHandler};
use windows_core::{Interface as _, HSTRING, PCWSTR, PWSTR};
use wry::WebViewExtWindows as _;
use zephium_agentic::{
    ContextCookieScope, ContextCookieTransferFailure, ContextCookieTransferOutcome,
    ContextCookieTransferRequest, ContextCookieTransferStats, MAX_COOKIES_PER_TRANSFER,
    MAX_COOKIE_BYTES, MAX_COOKIE_TRANSFER_BYTES,
};

use crate::platform::agent_cookie_preflight::{
    map_cookie_transfer_deadline, AgentCookieApplication, AgentCookieFields, AgentCookiePreflight,
    AgentCookiePreflightFailure, AgentCookieSameSite, AgentCookieText,
};

/// Time reserved after enumeration/application for a profile-wide cleanup and
/// cookie-empty verification. The host supplies the absolute outer deadline.
pub(crate) const AGENT_COOKIE_CLEANUP_RESERVE: Duration = Duration::from_secs(10);

/// Derives a profile-scoped cookie manager only from an ordinary Browse view
/// already bound to the selected logical profile's exact environment.
///
/// The host chooses the view through its private item/partition registries;
/// this adapter reattests both Wry's environment and the controller-reported
/// environment before returning native authority. It never reads cookies.
pub(crate) fn selected_profile_cookie_manager(
    view: &wry::WebView,
    expected_environment: &ICoreWebView2Environment,
) -> Result<ICoreWebView2CookieManager, ContextCookieTransferFailure> {
    if !super::same_environment(&view.environment(), expected_environment) {
        return Err(ContextCookieTransferFailure::SourceUnavailable);
    }
    let core = view
        .webview()
        .cast::<ICoreWebView2_2>()
        .map_err(|_| ContextCookieTransferFailure::SourceUnavailable)?;
    // SAFETY: `core` is the live reference-counted interface retained by the
    // exact ordinary Browse view. Both getters return separately AddRef'd COM
    // owners on this same STA and retain no caller pointer.
    let (controller_environment, manager) = unsafe { (core.Environment(), core.CookieManager()) };
    let controller_environment =
        controller_environment.map_err(|_| ContextCookieTransferFailure::SourceUnavailable)?;
    if !super::same_environment(&controller_environment, expected_environment) {
        return Err(ContextCookieTransferFailure::SourceUnavailable);
    }
    manager.map_err(|_| ContextCookieTransferFailure::SourceUnavailable)
}

/// Whether cleanup removed the identities owned by this transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsAgentCookieCleanup {
    NotRequired,
    Proven,
    Unproven,
}

/// Native terminal kept private to the engine host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowsAgentCookieTerminal {
    outcome: ContextCookieTransferOutcome,
    cleanup: WindowsAgentCookieCleanup,
}

impl WindowsAgentCookieTerminal {
    pub(crate) const fn outcome(self) -> ContextCookieTransferOutcome {
        self.outcome
    }

    pub(crate) const fn cleanup(self) -> WindowsAgentCookieCleanup {
        self.cleanup
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferPhase {
    CheckingDestination,
    Enumerating(usize),
    Applying {
        in_flight: bool,
    },
    Clearing {
        failure: ContextCookieTransferFailure,
        stats: ContextCookieTransferStats,
    },
    Verifying {
        failure: ContextCookieTransferFailure,
        stats: ContextCookieTransferStats,
    },
    WorkRollback {
        failure: ContextCookieTransferFailure,
        stats: Option<ContextCookieTransferStats>,
    },
}

enum ApplyStep {
    Complete(ContextCookieTransferStats),
    Cookie {
        destination: ICoreWebView2CookieManager,
        cookie: ICoreWebView2Cookie,
        after_apply: ContextCookieTransferStats,
    },
    Failure(
        ContextCookieTransferFailure,
        Option<ContextCookieTransferStats>,
    ),
}

type TransferCompletion = Box<dyn FnOnce(WindowsAgentCookieTerminal) + 'static>;

struct TransferState {
    source: ICoreWebView2CookieManager,
    destination: ICoreWebView2CookieManager,
    destination_profile: ICoreWebView2Profile2,
    scope: ContextCookieScope,
    preflight: Option<AgentCookiePreflight<ICoreWebView2Cookie>>,
    application: Option<AgentCookieApplication<ICoreWebView2Cookie>>,
    phase: TransferPhase,
    application_deadline: Instant,
    terminal_deadline: Instant,
    completion: Option<TransferCompletion>,
    work_seed: Option<WorkSeed>,
}

struct WorkSeed {
    existing: Vec<CookieIdentity>,
    attempted: Vec<ICoreWebView2Cookie>,
}

#[derive(PartialEq, Eq)]
struct CookieIdentity([zeroize::Zeroizing<Vec<u16>>; 3]);

impl CookieIdentity {
    fn bytes(&self) -> usize {
        self.0.iter().map(|field| field.len() * 2).sum()
    }
}

struct TransferShared {
    state: RefCell<Option<TransferState>>,
    cancellation: Cell<Option<ContextCookieTransferFailure>>,
    terminal: Cell<bool>,
    callback_panicked: Rc<dyn Fn()>,
}

/// UI-thread-bound owner for one asynchronous native transfer.
pub(crate) struct WindowsAgentCookieTransfer {
    shared: Rc<TransferShared>,
}

impl WindowsAgentCookieTransfer {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        source: ICoreWebView2CookieManager,
        destination: ICoreWebView2CookieManager,
        destination_profile: ICoreWebView2Profile2,
        request: &ContextCookieTransferRequest,
        admitted_at: Instant,
        completion: impl FnOnce(WindowsAgentCookieTerminal) + 'static,
        callback_panicked: impl Fn() + 'static,
    ) -> Result<Self, ContextCookieTransferFailure> {
        let now = Instant::now();
        let terminal_deadline = map_cookie_transfer_deadline(request.window(), admitted_at, now)
            .ok_or(ContextCookieTransferFailure::TimedOut)?;
        Self::start_scoped(
            source,
            destination,
            destination_profile,
            request.scope().clone(),
            terminal_deadline,
            completion,
            callback_panicked,
        )
    }

    pub(crate) fn start_scoped(
        source: ICoreWebView2CookieManager,
        destination: ICoreWebView2CookieManager,
        destination_profile: ICoreWebView2Profile2,
        scope: ContextCookieScope,
        terminal_deadline: Instant,
        completion: impl FnOnce(WindowsAgentCookieTerminal) + 'static,
        callback_panicked: impl Fn() + 'static,
    ) -> Result<Self, ContextCookieTransferFailure> {
        Self::start_mode(
            source,
            destination,
            destination_profile,
            scope,
            terminal_deadline,
            completion,
            callback_panicked,
            false,
        )
    }

    /// Host admission serializes this initial seed while no Work page can mutate the store.
    pub(crate) fn start_work_scoped(
        source: ICoreWebView2CookieManager,
        destination: ICoreWebView2CookieManager,
        destination_profile: ICoreWebView2Profile2,
        scope: ContextCookieScope,
        terminal_deadline: Instant,
        completion: impl FnOnce(WindowsAgentCookieTerminal) + 'static,
        callback_panicked: impl Fn() + 'static,
    ) -> Result<Self, ContextCookieTransferFailure> {
        Self::start_mode(
            source,
            destination,
            destination_profile,
            scope,
            terminal_deadline,
            completion,
            callback_panicked,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_mode(
        source: ICoreWebView2CookieManager,
        destination: ICoreWebView2CookieManager,
        destination_profile: ICoreWebView2Profile2,
        scope: ContextCookieScope,
        terminal_deadline: Instant,
        completion: impl FnOnce(WindowsAgentCookieTerminal) + 'static,
        callback_panicked: impl Fn() + 'static,
        retained: bool,
    ) -> Result<Self, ContextCookieTransferFailure> {
        let now = Instant::now();
        let Some(application_deadline) =
            terminal_deadline.checked_sub(AGENT_COOKIE_CLEANUP_RESERVE)
        else {
            return Err(ContextCookieTransferFailure::TimedOut);
        };
        if now >= application_deadline {
            return Err(ContextCookieTransferFailure::TimedOut);
        }
        let preflight =
            AgentCookiePreflight::try_new(scope.len()).map_err(map_preflight_failure)?;
        let shared = Rc::new(TransferShared {
            state: RefCell::new(Some(TransferState {
                source,
                destination,
                destination_profile,
                scope,
                preflight: Some(preflight),
                application: None,
                phase: if retained {
                    TransferPhase::CheckingDestination
                } else {
                    TransferPhase::Enumerating(0)
                },
                application_deadline,
                terminal_deadline,
                completion: Some(Box::new(completion)),
                work_seed: retained.then(|| WorkSeed {
                    existing: Vec::new(),
                    attempted: Vec::new(),
                }),
            })),
            cancellation: Cell::new(None),
            terminal: Cell::new(false),
            callback_panicked: Rc::new(callback_panicked),
        });
        if retained {
            check_work_destination(&shared);
        } else {
            start_origin(&shared, 0);
        }
        Ok(Self { shared })
    }

    /// Requests exact cancellation. Native callbacks cannot be revoked, but
    /// after this call they can only drop their owners or drive cleanup.
    pub(crate) fn cancel(&self, failure: ContextCookieTransferFailure) -> bool {
        if !matches!(
            failure,
            ContextCookieTransferFailure::TimedOut
                | ContextCookieTransferFailure::Cancelled
                | ContextCookieTransferFailure::Shutdown
        ) || self.shared.terminal.get()
        {
            return false;
        }
        if self.shared.cancellation.get().is_none() {
            self.shared.cancellation.set(Some(failure));
        }
        drive_cancellation(&self.shared);
        true
    }

    pub(crate) fn is_terminal(&self) -> bool {
        self.shared.terminal.get()
    }
}

impl fmt::Debug for WindowsAgentCookieTransfer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsAgentCookieTransfer")
            .field("terminal", &self.shared.terminal.get())
            .finish()
    }
}

impl Drop for WindowsAgentCookieTransfer {
    fn drop(&mut self) {
        if self.shared.terminal.get() {
            return;
        }
        if self.shared.cancellation.get().is_none() {
            self.shared
                .cancellation
                .set(Some(ContextCookieTransferFailure::Shutdown));
        }
        drive_cancellation(&self.shared);
    }
}

fn start_origin(shared: &Rc<TransferShared>, index: usize) {
    if shared.terminal.get() {
        return;
    }
    if let Some(failure) = shared.cancellation.get() {
        fail_transfer(shared, failure, None);
        return;
    }
    let prepared = {
        let Ok(mut owner) = shared.state.try_borrow_mut() else {
            invoke_callback_panic(shared);
            return;
        };
        let Some(state) = owner.as_mut() else {
            return;
        };
        if Instant::now() >= state.application_deadline
            || index >= state.scope.len()
            || !matches!(state.phase, TransferPhase::Enumerating(expected) if expected == index)
        {
            None
        } else {
            state.phase = TransferPhase::Enumerating(index);
            Some((
                state.source.clone(),
                HSTRING::from(state.scope.origins()[index].as_url().as_str()),
            ))
        }
    };
    let Some((source, origin)) = prepared else {
        fail_transfer(shared, ContextCookieTransferFailure::TimedOut, None);
        return;
    };
    let callback_owner = Rc::clone(shared);
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, cookies| {
        let panic_owner = Rc::clone(&callback_owner);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_origin_completed(&callback_owner, index, result, cookies);
        }))
        .is_err()
        {
            invoke_callback_panic(&panic_owner);
            fail_transfer(
                &panic_owner,
                ContextCookieTransferFailure::EnumerationFailed,
                None,
            );
        }
        Ok(())
    }));
    // SAFETY: `source` and `handler` are live reference-counted COM objects;
    // `origin` remains alive through this call and WebView2 copies the input
    // string for the admitted asynchronous operation.
    let started = unsafe { source.GetCookies(PCWSTR::from_raw(origin.as_ptr()), &handler) };
    if started.is_err() {
        fail_transfer(
            shared,
            ContextCookieTransferFailure::EnumerationFailed,
            None,
        );
    }
}

fn check_work_destination(shared: &Rc<TransferShared>) {
    let destination = shared
        .state
        .try_borrow()
        .ok()
        .and_then(|owner| owner.as_ref().map(|state| state.destination.clone()));
    let Some(destination) = destination else {
        return;
    };
    let owner = shared.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, cookies| {
        let panic_owner = owner.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let checked = result
                .map_err(|_| ContextCookieTransferFailure::EnumerationFailed)
                .and_then(|_| cookie_identities(cookies));
            if owner.terminal.get() {
                return;
            }
            let result = (|| {
                let mut state = owner
                    .state
                    .try_borrow_mut()
                    .map_err(|_| ContextCookieTransferFailure::EnumerationFailed)?;
                let state = state
                    .as_mut()
                    .ok_or(ContextCookieTransferFailure::DestinationUnavailable)?;
                if state.phase != TransferPhase::CheckingDestination {
                    return Err(ContextCookieTransferFailure::EnumerationFailed);
                }
                if Instant::now() >= state.application_deadline {
                    return Err(ContextCookieTransferFailure::TimedOut);
                }
                state
                    .work_seed
                    .as_mut()
                    .ok_or(ContextCookieTransferFailure::DestinationUnavailable)?
                    .existing = checked?;
                state.phase = TransferPhase::Enumerating(0);
                Ok(())
            })();
            match result {
                Ok(()) => start_origin(&owner, 0),
                Err(failure) => fail_transfer(&owner, failure, None),
            }
        }))
        .is_err()
        {
            invoke_callback_panic(&panic_owner);
            fail_transfer(
                &panic_owner,
                ContextCookieTransferFailure::EnumerationFailed,
                None,
            );
        }
        Ok(())
    }));
    // SAFETY: exact retained manager and handler are live; an empty URI requests all cookies.
    if unsafe { destination.GetCookies(PCWSTR::null(), &handler) }.is_err() {
        fail_transfer(
            shared,
            ContextCookieTransferFailure::EnumerationFailed,
            None,
        );
    }
}

fn cookie_identities(
    cookies: Option<ICoreWebView2CookieList>,
) -> Result<Vec<CookieIdentity>, ContextCookieTransferFailure> {
    let cookies = cookies.ok_or(ContextCookieTransferFailure::EnumerationFailed)?;
    let mut count = 0;
    // SAFETY: live callback list and initialized count output.
    unsafe { cookies.Count(&mut count) }
        .map_err(|_| ContextCookieTransferFailure::EnumerationFailed)?;
    if count as usize > MAX_COOKIES_PER_TRANSFER {
        return Err(ContextCookieTransferFailure::LimitExceeded);
    }
    let mut result = Vec::with_capacity(count as usize);
    let mut bytes = 0;
    for index in 0..count {
        // SAFETY: index is within this exact bounded native list.
        let cookie = unsafe { cookies.GetValueAtIndex(index) }
            .map_err(|_| ContextCookieTransferFailure::EnumerationFailed)?;
        let identity = cookie_identity(&cookie)?;
        bytes += identity.bytes();
        if bytes > MAX_COOKIE_TRANSFER_BYTES {
            return Err(ContextCookieTransferFailure::LimitExceeded);
        }
        result.push(identity);
    }
    Ok(result)
}

fn cookie_identity(
    cookie: &ICoreWebView2Cookie,
) -> Result<CookieIdentity, ContextCookieTransferFailure> {
    let mut fields = [
        zeroize::Zeroizing::new(Vec::new()),
        zeroize::Zeroizing::new(Vec::new()),
        zeroize::Zeroizing::new(Vec::new()),
    ];
    for (index, field) in fields.iter_mut().enumerate() {
        let mut raw = PWSTR::null();
        // SAFETY: native cookie is live; each getter returns an owned NUL-terminated out-string.
        let result = unsafe {
            match index {
                0 => cookie.Name(&mut raw),
                1 => cookie.Domain(&mut raw),
                _ => cookie.Path(&mut raw),
            }
        };
        let owner = webview2_com::CoTaskMemPWSTR::from(raw);
        result.map_err(|_| ContextCookieTransferFailure::InvalidCookie)?;
        let pointer = owner.as_ref().as_pcwstr().as_ptr();
        if pointer.is_null() {
            return Err(ContextCookieTransferFailure::InvalidCookie);
        }
        for position in 0..=MAX_COOKIE_BYTES {
            // SAFETY: getter's NUL-terminated string stays owned; scanning is policy-bounded.
            let unit = unsafe { pointer.add(position).read() };
            if unit == 0 {
                break;
            }
            if position == MAX_COOKIE_BYTES {
                return Err(ContextCookieTransferFailure::LimitExceeded);
            }
            field.push(unit);
        }
        if field.is_empty() {
            return Err(ContextCookieTransferFailure::InvalidCookie);
        }
    }
    Ok(CookieIdentity(fields))
}

fn on_origin_completed(
    shared: &Rc<TransferShared>,
    index: usize,
    result: windows_core::Result<()>,
    cookies: Option<ICoreWebView2CookieList>,
) {
    if shared.terminal.get() {
        return;
    }
    if result.is_err() || cookies.is_none() {
        fail_transfer(
            shared,
            ContextCookieTransferFailure::EnumerationFailed,
            None,
        );
        return;
    }
    let Some(cookies) = cookies else {
        return;
    };
    let mut count = 0u32;
    let maximum = u32::try_from(MAX_COOKIES_PER_TRANSFER).unwrap_or(0);
    // SAFETY: `cookies` is the live list supplied by the completion callback
    // and `count` is initialized writable storage of the required type.
    if unsafe { cookies.Count(&mut count) }.is_err() || count > maximum {
        fail_transfer(shared, ContextCookieTransferFailure::LimitExceeded, None);
        return;
    }

    let mut failure = None;
    let mut next_origin = None;
    let mut start_application = false;
    {
        let Ok(mut owner) = shared.state.try_borrow_mut() else {
            invoke_callback_panic(shared);
            return;
        };
        let Some(state) = owner.as_mut() else {
            return;
        };
        if !matches!(state.phase, TransferPhase::Enumerating(expected) if expected == index) {
            failure = Some(ContextCookieTransferFailure::EnumerationFailed);
        } else if Instant::now() >= state.application_deadline {
            failure = Some(ContextCookieTransferFailure::TimedOut);
        } else {
            for native_index in 0..count {
                if let Some(cancelled) = shared.cancellation.get() {
                    failure = Some(cancelled);
                    break;
                }
                if Instant::now() >= state.application_deadline {
                    failure = Some(ContextCookieTransferFailure::TimedOut);
                    break;
                }
                // SAFETY: the list count was read from this exact live list and
                // this loop never indexes at or beyond that bounded count.
                let source_cookie = match unsafe { cookies.GetValueAtIndex(native_index) } {
                    Ok(cookie) => cookie,
                    Err(_) => {
                        failure = Some(ContextCookieTransferFailure::EnumerationFailed);
                        break;
                    }
                };
                // SAFETY: both manager and source cookie are live COM owners.
                // CopyCookie creates a detached destination-profile cookie;
                // it does not mutate the destination store.
                let copied = match unsafe { state.destination.CopyCookie(&source_cookie) } {
                    Ok(cookie) => cookie,
                    Err(_) => {
                        failure = Some(ContextCookieTransferFailure::InvalidCookie);
                        break;
                    }
                };
                let fields = match read_cookie_fields(&copied) {
                    Ok(fields) => fields,
                    Err(native_failure) => {
                        failure = Some(native_failure);
                        break;
                    }
                };
                if let Some(seed) = &state.work_seed {
                    match cookie_identity(&copied) {
                        Ok(identity) if !seed.existing.contains(&identity) => {}
                        Ok(_) => {
                            failure = Some(ContextCookieTransferFailure::DestinationUnavailable);
                            break;
                        }
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                let Some(preflight) = state.preflight.as_mut() else {
                    failure = Some(ContextCookieTransferFailure::EnumerationFailed);
                    break;
                };
                if let Err(preflight_failure) = preflight.admit(fields, copied) {
                    failure = Some(map_preflight_failure(preflight_failure));
                    break;
                }
            }
            if failure.is_none() {
                if let Some(preflight) = state.preflight.as_mut() {
                    if let Err(preflight_failure) = preflight.complete_origin() {
                        failure = Some(map_preflight_failure(preflight_failure));
                    } else if index + 1 < state.scope.len() {
                        state.phase = TransferPhase::Enumerating(index + 1);
                        next_origin = Some(index + 1);
                    } else {
                        let application = state
                            .preflight
                            .take()
                            .ok_or(ContextCookieTransferFailure::EnumerationFailed)
                            .and_then(|preflight| {
                                preflight.finish().map_err(map_preflight_failure)
                            });
                        match application {
                            Ok(application) => {
                                state.application = Some(application);
                                state.phase = TransferPhase::Applying { in_flight: false };
                                start_application = true;
                            }
                            Err(native_failure) => failure = Some(native_failure),
                        }
                    }
                } else {
                    failure = Some(ContextCookieTransferFailure::EnumerationFailed);
                }
            }
        }
    }
    if let Some(failure) = failure {
        fail_transfer(shared, failure, None);
    } else if let Some(next_origin) = next_origin {
        start_origin(shared, next_origin);
    } else if start_application {
        apply_preflight(shared);
    }
}

fn apply_preflight(shared: &Rc<TransferShared>) {
    loop {
        if shared.terminal.get() {
            return;
        }
        if let Some(failure) = shared.cancellation.get() {
            fail_transfer(shared, failure, None);
            return;
        }
        let step = {
            let Ok(owner) = shared.state.try_borrow() else {
                invoke_callback_panic(shared);
                return;
            };
            let Some(state) = owner.as_ref() else {
                return;
            };
            if !matches!(state.phase, TransferPhase::Applying { in_flight: false }) {
                ApplyStep::Failure(ContextCookieTransferFailure::ApplicationFailed, None)
            } else if Instant::now() >= state.application_deadline {
                ApplyStep::Failure(ContextCookieTransferFailure::TimedOut, None)
            } else {
                match state.application.as_ref() {
                    Some(application) => match application.current() {
                        Ok(Some(current)) => ApplyStep::Cookie {
                            destination: state.destination.clone(),
                            cookie: current.native().clone(),
                            after_apply: current.after_apply(),
                        },
                        Ok(None) if application.is_complete() => match application.stats() {
                            Ok(stats) => ApplyStep::Complete(stats),
                            Err(_) => ApplyStep::Failure(
                                ContextCookieTransferFailure::ApplicationFailed,
                                None,
                            ),
                        },
                        Ok(None) | Err(_) => ApplyStep::Failure(
                            ContextCookieTransferFailure::ApplicationFailed,
                            None,
                        ),
                    },
                    None => {
                        ApplyStep::Failure(ContextCookieTransferFailure::ApplicationFailed, None)
                    }
                }
            }
        };
        let (destination, cookie, after_apply) = match step {
            ApplyStep::Complete(stats) => {
                complete_terminal(
                    shared,
                    WindowsAgentCookieTerminal {
                        outcome: ContextCookieTransferOutcome::Applied(stats),
                        cleanup: WindowsAgentCookieCleanup::NotRequired,
                    },
                );
                return;
            }
            ApplyStep::Cookie {
                destination,
                cookie,
                after_apply,
            } => (destination, cookie, after_apply),
            ApplyStep::Failure(failure, stats) => {
                fail_transfer(shared, failure, stats);
                return;
            }
        };
        let entered = shared.state.try_borrow_mut().is_ok_and(|mut owner| {
            owner.as_mut().is_some_and(|state| {
                if matches!(state.phase, TransferPhase::Applying { in_flight: false }) {
                    if let Some(seed) = &mut state.work_seed {
                        seed.attempted.push(cookie.clone());
                    }
                    state.phase = TransferPhase::Applying { in_flight: true };
                    true
                } else {
                    false
                }
            })
        });
        if !entered {
            invoke_callback_panic(shared);
            fail_transfer(
                shared,
                ContextCookieTransferFailure::ApplicationFailed,
                None,
            );
            return;
        }
        // SAFETY: `destination` and `cookie` are exact live reference-counted
        // COM owners. The cookie came from this destination manager's
        // CopyCookie preflight and has not escaped the sequential owner.
        if unsafe { destination.AddOrUpdateCookie(&cookie) }.is_err() {
            if let Ok(mut owner) = shared.state.try_borrow_mut() {
                if let Some(state) = owner.as_mut() {
                    state.phase = TransferPhase::Applying { in_flight: false };
                }
            }
            fail_transfer(
                shared,
                shared
                    .cancellation
                    .get()
                    .unwrap_or(ContextCookieTransferFailure::ApplicationFailed),
                None,
            );
            return;
        }
        let recorded = {
            let Ok(mut owner) = shared.state.try_borrow_mut() else {
                invoke_callback_panic(shared);
                fail_transfer(
                    shared,
                    ContextCookieTransferFailure::ApplicationFailed,
                    Some(after_apply),
                );
                return;
            };
            owner.as_mut().is_some_and(|state| {
                state.phase = TransferPhase::Applying { in_flight: false };
                state.application.as_mut().is_some_and(|application| {
                    application.record_current_applied(after_apply).is_ok()
                })
            })
        };
        if !recorded {
            invoke_callback_panic(shared);
            fail_transfer(
                shared,
                ContextCookieTransferFailure::ApplicationFailed,
                Some(after_apply),
            );
            return;
        }
    }
}

fn drive_cancellation(shared: &Rc<TransferShared>) {
    let Some(failure) = shared.cancellation.get() else {
        return;
    };
    if shared.terminal.get() {
        return;
    }
    let phase = shared
        .state
        .try_borrow()
        .ok()
        .and_then(|owner| owner.as_ref().map(|state| state.phase));
    match phase {
        Some(TransferPhase::WorkRollback { failure, stats }) => {
            finish_work_rollback(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
        }
        Some(TransferPhase::Clearing { failure, stats })
        | Some(TransferPhase::Verifying { failure, stats }) => {
            complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven)
        }
        Some(TransferPhase::Applying { in_flight: true }) => {}
        Some(
            TransferPhase::CheckingDestination
            | TransferPhase::Enumerating(_)
            | TransferPhase::Applying { in_flight: false },
        ) => fail_transfer(shared, failure, None),
        None => {}
    }
}

fn fail_transfer(
    shared: &Rc<TransferShared>,
    failure: ContextCookieTransferFailure,
    forced_stats: Option<ContextCookieTransferStats>,
) {
    if shared.terminal.get() {
        return;
    }
    let rollback = shared.state.try_borrow().ok().and_then(|owner| {
        owner
            .as_ref()
            .filter(|state| {
                state
                    .work_seed
                    .as_ref()
                    .is_some_and(|seed| !seed.attempted.is_empty())
            })
            .map(|state| {
                (
                    state.phase,
                    forced_stats.or_else(|| {
                        state
                            .application
                            .as_ref()
                            .and_then(|application| application.stats().ok())
                    }),
                )
            })
    });
    if let Some((phase, stats)) = rollback {
        if let TransferPhase::WorkRollback { failure, stats } = phase {
            finish_work_rollback(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
        } else {
            rollback_work_seed(shared, failure, stats);
        }
        return;
    }
    let decision = {
        let Ok(owner) = shared.state.try_borrow() else {
            return;
        };
        let Some(state) = owner.as_ref() else {
            return;
        };
        match state.phase {
            TransferPhase::Clearing {
                failure: original,
                stats,
            }
            | TransferPhase::Verifying {
                failure: original,
                stats,
            } => Err((original, stats)),
            TransferPhase::CheckingDestination
            | TransferPhase::Enumerating(_)
            | TransferPhase::Applying { .. } => {
                let stats = forced_stats.or_else(|| {
                    state
                        .application
                        .as_ref()
                        .and_then(|application| application.stats().ok())
                });
                match stats.filter(|stats| stats.counts().cookies_applied > 0) {
                    Some(stats) => Err((failure, stats)),
                    None => Ok(()),
                }
            }
            TransferPhase::WorkRollback { .. } => return,
        }
    };
    match decision {
        Ok(()) => complete_terminal(
            shared,
            WindowsAgentCookieTerminal {
                outcome: ContextCookieTransferOutcome::Refused(failure),
                cleanup: WindowsAgentCookieCleanup::NotRequired,
            },
        ),
        Err((failure, stats)) => begin_cleanup(shared, failure, stats),
    }
}

fn begin_cleanup(
    shared: &Rc<TransferShared>,
    failure: ContextCookieTransferFailure,
    stats: ContextCookieTransferStats,
) {
    if shared.terminal.get() {
        return;
    }
    // The triggering cancellation has already been incorporated into the
    // partial outcome. Clear it so cleanup can run; a later outer-deadline
    // cancellation remains able to stop and quarantine this cleanup.
    shared.cancellation.set(None);
    let native = {
        let Ok(mut owner) = shared.state.try_borrow_mut() else {
            invoke_callback_panic(shared);
            return;
        };
        let Some(state) = owner.as_mut() else {
            return;
        };
        state.preflight = None;
        state.application = None;
        state.phase = TransferPhase::Clearing { failure, stats };
        if Instant::now() >= state.terminal_deadline {
            None
        } else {
            Some((state.destination.clone(), state.destination_profile.clone()))
        }
    };
    let Some((destination, profile)) = native else {
        complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
        return;
    };
    // SAFETY: the exact destination manager remains live. This synchronous
    // deletion narrows exposure immediately; Profile2 clear and an empty
    // readback remain the authoritative cleanup proof.
    let _ = unsafe { destination.DeleteAllCookies() };
    if shared.terminal.get() || shared.cancellation.get().is_some() {
        complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
        return;
    }
    let callback_owner = Rc::clone(shared);
    let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
        let panic_owner = Rc::clone(&callback_owner);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_clear_completed(&callback_owner, result);
        }))
        .is_err()
        {
            invoke_callback_panic(&panic_owner);
            finish_cleanup_unproven(&panic_owner);
        }
        Ok(())
    }));
    // SAFETY: `profile` and `handler` are live reference-counted COM owners;
    // the destination view remains retained by the host until this terminal.
    if unsafe { profile.ClearBrowsingDataAll(&handler) }.is_err() {
        complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
    }
}

fn rollback_work_seed(
    shared: &Rc<TransferShared>,
    failure: ContextCookieTransferFailure,
    stats: Option<ContextCookieTransferStats>,
) {
    shared.cancellation.set(None);
    let native = shared.state.try_borrow_mut().ok().and_then(|mut owner| {
        let state = owner.as_mut()?;
        state.preflight = None;
        state.application = None;
        state.phase = TransferPhase::WorkRollback { failure, stats };
        (Instant::now() < state.terminal_deadline).then(|| {
            (
                state.destination.clone(),
                state
                    .work_seed
                    .as_ref()
                    .map(|seed| seed.attempted.clone())
                    .unwrap_or_default(),
            )
        })
    });
    let Some((destination, attempted)) = native else {
        finish_work_rollback(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
        return;
    };
    for cookie in &attempted {
        // SAFETY: the journal contains only copied cookies attempted by this initial seed.
        // No pre-existing identity was admitted, and host admission keeps the store quiescent.
        let _ = unsafe { destination.DeleteCookie(cookie) };
    }
    let owner = shared.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, cookies| {
        let panic_owner = owner.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if owner.terminal.get() {
                return;
            }
            let verified = result.is_ok()
                && cookie_identities(cookies).is_ok_and(|current| {
                    attempted.iter().all(|cookie| {
                        cookie_identity(cookie).is_ok_and(|identity| !current.contains(&identity))
                    })
                });
            let timely = owner.state.try_borrow().is_ok_and(|state| {
                state
                    .as_ref()
                    .is_some_and(|state| Instant::now() < state.terminal_deadline)
            });
            finish_work_rollback(
                &owner,
                failure,
                stats,
                if verified && timely {
                    WindowsAgentCookieCleanup::Proven
                } else {
                    WindowsAgentCookieCleanup::Unproven
                },
            );
        }))
        .is_err()
        {
            invoke_callback_panic(&panic_owner);
            finish_work_rollback(
                &panic_owner,
                failure,
                stats,
                WindowsAgentCookieCleanup::Unproven,
            );
        }
        Ok(())
    }));
    // SAFETY: live retained manager and copied native handler; all-cookie readback verifies only journaled identities.
    if unsafe { destination.GetCookies(PCWSTR::null(), &handler) }.is_err() {
        finish_work_rollback(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
    }
}

fn finish_work_rollback(
    shared: &Rc<TransferShared>,
    failure: ContextCookieTransferFailure,
    stats: Option<ContextCookieTransferStats>,
    cleanup: WindowsAgentCookieCleanup,
) {
    let outcome = match stats.filter(|stats| stats.counts().cookies_applied > 0) {
        Some(stats) => ContextCookieTransferOutcome::Partial { failure, stats },
        None => ContextCookieTransferOutcome::Refused(failure),
    };
    complete_terminal(shared, WindowsAgentCookieTerminal { outcome, cleanup });
}

fn on_clear_completed(shared: &Rc<TransferShared>, result: windows_core::Result<()>) {
    if shared.terminal.get() {
        return;
    }
    let Some((failure, stats, destination, within_deadline)) =
        shared.state.try_borrow_mut().ok().and_then(|mut owner| {
            let state = owner.as_mut()?;
            let TransferPhase::Clearing { failure, stats } = state.phase else {
                return None;
            };
            state.phase = TransferPhase::Verifying { failure, stats };
            Some((
                failure,
                stats,
                state.destination.clone(),
                Instant::now() < state.terminal_deadline,
            ))
        })
    else {
        invoke_callback_panic(shared);
        return;
    };
    if result.is_err() || !within_deadline || shared.cancellation.get().is_some() {
        complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
        return;
    }
    let callback_owner = Rc::clone(shared);
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, cookies| {
        let panic_owner = Rc::clone(&callback_owner);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_cleanup_verification(&callback_owner, result, cookies);
        }))
        .is_err()
        {
            invoke_callback_panic(&panic_owner);
            finish_cleanup_unproven(&panic_owner);
        }
        Ok(())
    }));
    // SAFETY: null URI is the documented profile-wide cookie query; both the
    // exact destination manager and callback handler are live COM owners.
    if unsafe { destination.GetCookies(PCWSTR::null(), &handler) }.is_err() {
        complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
    }
}

fn on_cleanup_verification(
    shared: &Rc<TransferShared>,
    result: windows_core::Result<()>,
    cookies: Option<ICoreWebView2CookieList>,
) {
    if shared.terminal.get() {
        return;
    }
    let Some((failure, stats, within_deadline)) =
        shared.state.try_borrow().ok().and_then(|owner| {
            let state = owner.as_ref()?;
            let TransferPhase::Verifying { failure, stats } = state.phase else {
                return None;
            };
            Some((failure, stats, Instant::now() < state.terminal_deadline))
        })
    else {
        invoke_callback_panic(shared);
        return;
    };
    let mut count = u32::MAX;
    let empty = if result.is_ok() {
        cookies.is_some_and(|cookies| {
            // SAFETY: `cookies` is the live list supplied by this exact
            // callback and `count` is initialized writable storage.
            unsafe { cookies.Count(&mut count) }.is_ok() && count == 0
        })
    } else {
        false
    };
    complete_partial(
        shared,
        failure,
        stats,
        if empty && within_deadline && shared.cancellation.get().is_none() {
            WindowsAgentCookieCleanup::Proven
        } else {
            WindowsAgentCookieCleanup::Unproven
        },
    );
}

fn finish_cleanup_unproven(shared: &Rc<TransferShared>) {
    let partial = shared.state.try_borrow().ok().and_then(|owner| {
        let state = owner.as_ref()?;
        match state.phase {
            TransferPhase::Clearing { failure, stats }
            | TransferPhase::Verifying { failure, stats } => Some((failure, stats)),
            TransferPhase::CheckingDestination
            | TransferPhase::Enumerating(_)
            | TransferPhase::Applying { .. }
            | TransferPhase::WorkRollback { .. } => None,
        }
    });
    if let Some((failure, stats)) = partial {
        complete_partial(shared, failure, stats, WindowsAgentCookieCleanup::Unproven);
    }
}

fn complete_partial(
    shared: &Rc<TransferShared>,
    failure: ContextCookieTransferFailure,
    stats: ContextCookieTransferStats,
    cleanup: WindowsAgentCookieCleanup,
) {
    complete_terminal(
        shared,
        WindowsAgentCookieTerminal {
            outcome: ContextCookieTransferOutcome::Partial { failure, stats },
            cleanup,
        },
    );
}

fn complete_terminal(shared: &Rc<TransferShared>, terminal: WindowsAgentCookieTerminal) {
    if shared.terminal.replace(true) {
        return;
    }
    let completion = match shared.state.try_borrow_mut() {
        Ok(mut owner) => owner.take().and_then(|mut state| {
            state.preflight = None;
            state.application = None;
            state.completion.take()
        }),
        Err(_) => {
            invoke_callback_panic(shared);
            None
        }
    };
    let Some(completion) = completion else {
        invoke_callback_panic(shared);
        return;
    };
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion(terminal))).is_err() {
        invoke_callback_panic(shared);
    }
}

fn invoke_callback_panic(shared: &Rc<TransferShared>) {
    let callback = Rc::clone(&shared.callback_panicked);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()));
}

fn read_cookie_fields(
    cookie: &ICoreWebView2Cookie,
) -> Result<AgentCookieFields, ContextCookieTransferFailure> {
    let name = read_cookie_text(cookie, CookieTextField::Name)?;
    let value = read_cookie_text(cookie, CookieTextField::Value)?;
    let domain = read_cookie_text(cookie, CookieTextField::Domain)?;
    let path = read_cookie_text(cookie, CookieTextField::Path)?;
    let mut expires = f64::NAN;
    let mut http_only = windows_core::BOOL::default();
    let mut secure = windows_core::BOOL::default();
    let mut session = windows_core::BOOL::default();
    let mut same_site = COREWEBVIEW2_COOKIE_SAME_SITE_KIND(-1);
    // SAFETY: `cookie` is a live COM owner and every out argument is
    // initialized writable storage of the exact binding type.
    unsafe {
        cookie
            .Expires(&mut expires)
            .map_err(|_| ContextCookieTransferFailure::InvalidCookie)?;
        cookie
            .IsHttpOnly(&mut http_only)
            .map_err(|_| ContextCookieTransferFailure::InvalidCookie)?;
        cookie
            .IsSecure(&mut secure)
            .map_err(|_| ContextCookieTransferFailure::InvalidCookie)?;
        cookie
            .IsSession(&mut session)
            .map_err(|_| ContextCookieTransferFailure::InvalidCookie)?;
        cookie
            .SameSite(&mut same_site)
            .map_err(|_| ContextCookieTransferFailure::InvalidCookie)?;
    }
    let same_site = match same_site {
        COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE => AgentCookieSameSite::None,
        COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX => AgentCookieSameSite::Lax,
        COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT => AgentCookieSameSite::Strict,
        _ => return Err(ContextCookieTransferFailure::InvalidCookie),
    };
    AgentCookieFields::try_new(
        name,
        value,
        domain,
        path,
        expires,
        http_only.as_bool(),
        secure.as_bool(),
        same_site,
        session.as_bool(),
    )
    .map_err(map_preflight_failure)
}

#[derive(Clone, Copy)]
enum CookieTextField {
    Name,
    Value,
    Domain,
    Path,
}

fn read_cookie_text(
    cookie: &ICoreWebView2Cookie,
    field: CookieTextField,
) -> Result<AgentCookieText, ContextCookieTransferFailure> {
    let mut raw = PWSTR::null();
    // SAFETY: `cookie` is a live COM owner and `raw` is initialized writable
    // storage. Every branch has the same CoTaskMem ownership convention.
    let result = unsafe {
        match field {
            CookieTextField::Name => cookie.Name(&mut raw),
            CookieTextField::Value => cookie.Value(&mut raw),
            CookieTextField::Domain => cookie.Domain(&mut raw),
            CookieTextField::Path => cookie.Path(&mut raw),
        }
    };
    if result.is_err() {
        drop(webview2_com::CoTaskMemPWSTR::from(raw));
        return Err(ContextCookieTransferFailure::InvalidCookie);
    }
    take_cookie_text(raw).map_err(map_preflight_failure)
}

fn take_cookie_text(raw: PWSTR) -> Result<AgentCookieText, AgentCookiePreflightFailure> {
    let owner = webview2_com::CoTaskMemPWSTR::from(raw);
    let pointer = owner.as_ref().as_pcwstr().as_ptr();
    if pointer.is_null() {
        return AgentCookieText::try_from_utf16(&[]);
    }
    let mut length = 0usize;
    while length <= MAX_COOKIE_BYTES {
        // SAFETY: WebView2's out-string contract is NUL terminated. The scan
        // is capped at the UTF-16 policy maximum plus one distinguishing unit.
        if unsafe { pointer.add(length).read() } == 0 {
            // SAFETY: the bounded scan established this initialized prefix and
            // `owner` retains the CoTaskMem allocation through decoding.
            let units = unsafe { std::slice::from_raw_parts(pointer, length) };
            return AgentCookieText::try_from_utf16(units);
        }
        length = length
            .checked_add(1)
            .ok_or(AgentCookiePreflightFailure::LimitExceeded)?;
    }
    Err(AgentCookiePreflightFailure::LimitExceeded)
}

const fn map_preflight_failure(
    failure: AgentCookiePreflightFailure,
) -> ContextCookieTransferFailure {
    match failure {
        AgentCookiePreflightFailure::InvalidCookie => ContextCookieTransferFailure::InvalidCookie,
        AgentCookiePreflightFailure::LimitExceeded => ContextCookieTransferFailure::LimitExceeded,
        AgentCookiePreflightFailure::ResourceExhausted
        | AgentCookiePreflightFailure::Incomplete => {
            ContextCookieTransferFailure::EnumerationFailed
        }
    }
}

#[cfg(test)]
#[path = "cookie_transfer_tests.rs"]
mod tests;
