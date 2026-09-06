//! Nonblocking native-to-runtime settlement mailboxes.

use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use thiserror::Error;
use tokio::sync::Notify;
use zephium_agentic::{
    AgentAuditCompletion, AgentAuditDeliverySettlement, ContextNativeEvent,
    ContextNavigationReplacement, ContextRendererLoss, SemanticActionNativeCompletion,
    SemanticActionNativeSettlement,
};

const FAULT_NONE: u8 = 0;
const FAULT_CLOSED: u8 = 1 << 0;
const FAULT_TERMINAL_OVERFLOW: u8 = 1 << 1;
const FAULT_SIGNAL_OVERFLOW: u8 = 1 << 2;
const INGRESS_CLOSED: usize = 1usize << (usize::BITS - 1);
const INGRESS_COUNT_MASK: usize = !INGRESS_CLOSED;

/// Minimum terminal slots reserved for one shell-admitted run's callback debt.
///
/// The staged shell can retain the three independent terminal callback classes
/// (native, semantic action, durable audit) plus lifecycle/cancellation
/// settlements without allowing a caller to under-provision its safety lane.
pub const MIN_AGENT_RUNTIME_TERMINAL_CAPACITY: usize = 4;
/// Largest terminal callback queue the generic shell will allocate.
pub const MAX_AGENT_RUNTIME_TERMINAL_CAPACITY: usize = 128;
/// Minimum unsolicited signal slots retained by the shell.
pub const MIN_AGENT_RUNTIME_SIGNAL_CAPACITY: usize = 2;
/// Largest unsolicited signal queue the generic shell will allocate.
pub const MAX_AGENT_RUNTIME_SIGNAL_CAPACITY: usize = 64;

/// Fixed limits for one runtime's native callback queues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRuntimeMailboxConfig {
    terminal_capacity: usize,
    signal_capacity: usize,
}

impl AgentRuntimeMailboxConfig {
    /// Conservative fixed capacity suitable for one production runtime shell.
    pub const STANDARD: Self = Self {
        terminal_capacity: 16,
        signal_capacity: 8,
    };

    /// Creates bounded fixed capacities for the terminal and signal lanes.
    pub const fn try_new(terminal_capacity: usize, signal_capacity: usize) -> Option<Self> {
        if terminal_capacity < MIN_AGENT_RUNTIME_TERMINAL_CAPACITY
            || terminal_capacity > MAX_AGENT_RUNTIME_TERMINAL_CAPACITY
            || signal_capacity < MIN_AGENT_RUNTIME_SIGNAL_CAPACITY
            || signal_capacity > MAX_AGENT_RUNTIME_SIGNAL_CAPACITY
        {
            None
        } else {
            Some(Self {
                terminal_capacity,
                signal_capacity,
            })
        }
    }

    /// Capacity reserved for terminal callback obligations.
    pub const fn terminal_capacity(self) -> usize {
        self.terminal_capacity
    }

    /// Capacity reserved for unsolicited navigation/renderer signals.
    pub const fn signal_capacity(self) -> usize {
        self.signal_capacity
    }
}

/// Content-free terminal state which permanently refuses a mailbox.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentRuntimeMailboxFault {
    /// The runtime sealed its native callback intake.
    #[error("agent runtime native mailbox is closed")]
    Closed,
    /// An obligated terminal callback could not be retained.
    #[error("agent runtime terminal mailbox capacity was exhausted")]
    TerminalOverflow,
    /// An unsolicited native signal could not be retained.
    #[error("agent runtime signal mailbox capacity was exhausted")]
    SignalOverflow,
}

impl AgentRuntimeMailboxFault {
    const fn code(self) -> u8 {
        match self {
            Self::Closed => FAULT_CLOSED,
            Self::TerminalOverflow => FAULT_TERMINAL_OVERFLOW,
            Self::SignalOverflow => FAULT_SIGNAL_OVERFLOW,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        if code & FAULT_TERMINAL_OVERFLOW != 0 {
            Some(Self::TerminalOverflow)
        } else if code & FAULT_SIGNAL_OVERFLOW != 0 {
            Some(Self::SignalOverflow)
        } else if code & FAULT_CLOSED != 0 {
            Some(Self::Closed)
        } else {
            None
        }
    }
}

/// A content-free item removed from the native callback mailbox.
pub(crate) enum AgentRuntimeMailboxItem {
    /// A native lifecycle or semantic-runtime terminal settlement.
    NativeTerminal(ContextNativeEvent),
    /// A native semantic-action callback settlement.
    SemanticActionTerminal(SemanticActionNativeSettlement),
    /// A durable audit-delivery callback settlement.
    AuditTerminal(AgentAuditDeliverySettlement),
    #[cfg(test)]
    /// Test-only content-free terminal marker.
    TestTerminal,
    #[cfg(test)]
    /// Test-only content-free signal marker.
    TestSignal,
    /// An unsolicited navigation replacement notification.
    NavigationReplaced(ContextNavigationReplacement),
    /// An unsolicited renderer-loss notification.
    RendererLost(ContextRendererLoss),
}

impl fmt::Debug for AgentRuntimeMailboxItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::NativeTerminal(_) => "NativeTerminal",
            Self::SemanticActionTerminal(_) => "SemanticActionTerminal",
            Self::AuditTerminal(_) => "AuditTerminal",
            #[cfg(test)]
            Self::TestTerminal => "TestTerminal",
            #[cfg(test)]
            Self::TestSignal => "TestSignal",
            Self::NavigationReplaced(_) => "NavigationReplaced",
            Self::RendererLost(_) => "RendererLost",
        };
        formatter.write_str(label)
    }
}

/// Readiness returned by the mailbox's race-free asynchronous wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentRuntimeMailboxWake {
    /// A terminal item is available and must drain before all signals.
    TerminalReady,
    /// No terminal item is available but a signal is ready.
    SignalReady,
    /// A sticky intake failure prevents successful continuation.
    Fault(AgentRuntimeMailboxFault),
}

/// Content-free reason callback intake could not be cleanly claimed.
///
/// This remains crate-private: the controller worker exposes a closed runtime
/// refusal instead of its mailbox implementation details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentRuntimeMailboxCleanClaimRefusal {
    Fault,
    TerminalDebt,
    SignalDebt,
}

enum TerminalItem {
    Native(ContextNativeEvent),
    SemanticAction(SemanticActionNativeSettlement),
    Audit(AgentAuditDeliverySettlement),
    #[cfg(test)]
    Test,
}

enum SignalItem {
    NavigationReplaced(ContextNavigationReplacement),
    RendererLost(ContextRendererLoss),
    #[cfg(test)]
    Test,
}

struct MailboxInner {
    terminal: ArrayQueue<TerminalItem>,
    signals: ArrayQueue<SignalItem>,
    fault: AtomicU8,
    wake: Notify,
    ingress: AtomicUsize,
    ingress_wake: Notify,
}

/// Fixed dual-lane native callback mailbox owned by one runtime.
///
/// Terminal obligations and unsolicited signals never contend for capacity.
/// Any intake fault is sticky and makes later execution fail closed.
#[derive(Clone)]
pub(crate) struct AgentRuntimeMailbox {
    inner: Arc<MailboxInner>,
}

impl AgentRuntimeMailbox {
    /// Creates one fixed-capacity dual-lane mailbox.
    pub(crate) fn new(config: AgentRuntimeMailboxConfig) -> Self {
        Self {
            inner: Arc::new(MailboxInner {
                terminal: ArrayQueue::new(config.terminal_capacity),
                signals: ArrayQueue::new(config.signal_capacity),
                fault: AtomicU8::new(FAULT_NONE),
                wake: Notify::new(),
                ingress: AtomicUsize::new(0),
                ingress_wake: Notify::new(),
            }),
        }
    }

    /// Returns a cloneable native lifecycle/semantic event callback sink.
    pub fn native_event_sink(&self) -> NativeEventSink {
        NativeEventSink {
            mailbox: self.clone(),
        }
    }

    /// Returns a cloneable native semantic-action callback sink.
    #[allow(dead_code)]
    pub(crate) fn semantic_action_sink(&self) -> SemanticActionSink {
        SemanticActionSink {
            mailbox: self.clone(),
        }
    }

    /// Returns a cloneable durable-audit callback sink.
    #[allow(dead_code)]
    pub(crate) fn audit_sink(&self) -> AgentAuditSink {
        AgentAuditSink {
            mailbox: self.clone(),
        }
    }

    /// Returns the sticky failure, if native intake has become unsafe.
    pub fn fault(&self) -> Option<AgentRuntimeMailboxFault> {
        AgentRuntimeMailboxFault::from_code(self.inner.fault.load(Ordering::Acquire))
    }

    /// Permanently closes native callback intake and wakes the controller.
    pub(crate) fn close(&self) {
        self.close_ingress_clean();
        self.publish_fault(AgentRuntimeMailboxFault::Closed);
    }

    /// Closes callback ingress without manufacturing a mailbox fault.
    ///
    /// The controller-success linearization uses this only after it has
    /// already closed every controller-owned effect and audit obligation. A
    /// callback which raced before this close is still counted by `ingress`
    /// and must be drained before clean claim can succeed; a later callback is
    /// simply refused and cannot create new debt after the claim point.
    fn close_ingress_clean(&self) {
        let _ = self
            .inner
            .ingress
            .fetch_or(INGRESS_CLOSED, Ordering::AcqRel);
        self.inner.ingress_wake.notify_waiters();
    }

    /// Atomically closes ingress, waits for racing callback writes, and proves
    /// that no callback debt or sticky intake fault remains.
    pub(crate) async fn try_claim_clean_quiescence(
        &self,
    ) -> Result<(), AgentRuntimeMailboxCleanClaimRefusal> {
        self.close_ingress_clean();
        self.wait_for_ingress_drain().await;
        if self.fault().is_some() {
            return Err(AgentRuntimeMailboxCleanClaimRefusal::Fault);
        }
        if !self.inner.terminal.is_empty() {
            return Err(AgentRuntimeMailboxCleanClaimRefusal::TerminalDebt);
        }
        if !self.inner.signals.is_empty() {
            return Err(AgentRuntimeMailboxCleanClaimRefusal::SignalDebt);
        }
        Ok(())
    }

    /// Closes callback intake, waits for any already-admitted callback to
    /// finish its nonblocking queue write, then consumes every retained item.
    pub(crate) async fn close_and_drain(&self) {
        self.close();
        self.wait_for_ingress_drain().await;
        while let Some(item) = self.try_pop() {
            discard_staged_item(item);
        }
    }

    /// Cleanly closes and drains after a committed controller terminal claim.
    ///
    /// The claim already proved no queue debt; this only refuses late ingress
    /// and releases mailbox storage without manufacturing a `Closed` fault.
    pub(crate) async fn close_and_drain_clean(&self) {
        self.close_ingress_clean();
        self.wait_for_ingress_drain().await;
        while let Some(item) = self.try_pop() {
            discard_staged_item(item);
        }
    }

    /// Cleanly refuses any late callback after a terminal claim.
    pub(crate) fn close_clean(&self) {
        self.close_ingress_clean();
    }

    /// Removes exactly one item, always draining terminal work before signals.
    pub(crate) fn try_pop(&self) -> Option<AgentRuntimeMailboxItem> {
        self.try_pop_terminal().or_else(|| self.try_pop_signal())
    }

    /// Removes one terminal callback settlement, if present.
    ///
    /// The controller host uses this narrower primitive to make a sticky
    /// intake fault win before an unsolicited signal while still delivering
    /// terminal callback debt first.
    pub(crate) fn try_pop_terminal(&self) -> Option<AgentRuntimeMailboxItem> {
        if let Some(item) = self.inner.terminal.pop() {
            return Some(match item {
                TerminalItem::Native(event) => AgentRuntimeMailboxItem::NativeTerminal(event),
                TerminalItem::SemanticAction(settlement) => {
                    AgentRuntimeMailboxItem::SemanticActionTerminal(settlement)
                }
                TerminalItem::Audit(settlement) => {
                    AgentRuntimeMailboxItem::AuditTerminal(settlement)
                }
                #[cfg(test)]
                TerminalItem::Test => AgentRuntimeMailboxItem::TestTerminal,
            });
        }
        None
    }

    /// Removes one unsolicited native signal, if no terminal priority applies.
    pub(crate) fn try_pop_signal(&self) -> Option<AgentRuntimeMailboxItem> {
        self.inner.signals.pop().map(|item| match item {
            SignalItem::NavigationReplaced(replacement) => {
                AgentRuntimeMailboxItem::NavigationReplaced(replacement)
            }
            SignalItem::RendererLost(loss) => AgentRuntimeMailboxItem::RendererLost(loss),
            #[cfg(test)]
            SignalItem::Test => AgentRuntimeMailboxItem::TestSignal,
        })
    }

    /// Waits without a lost-wake race until work or a sticky failure exists.
    ///
    /// The notification future is registered before queue and fault inspection;
    /// a producer racing in that window leaves either queued work, a sticky
    /// fault, or a stored notification permit for the next await.
    pub(crate) async fn wait_for_work(&self) -> AgentRuntimeMailboxWake {
        loop {
            let mut notified = std::pin::pin!(self.inner.wake.notified());
            notified.as_mut().enable();
            if !self.inner.terminal.is_empty() {
                return AgentRuntimeMailboxWake::TerminalReady;
            }
            if let Some(fault) = self.fault() {
                return AgentRuntimeMailboxWake::Fault(fault);
            }
            if !self.inner.signals.is_empty() {
                return AgentRuntimeMailboxWake::SignalReady;
            }
            notified.await;
        }
    }

    /// Waits for retained callback/signal work while deliberately ignoring a
    /// sticky fault.
    ///
    /// A controller which already owns an accepted durable-audit callback uses
    /// this bounded cleanup path: overflow makes success impossible, but it
    /// must not erase the exact audit settlement that was admitted first.
    pub(crate) async fn wait_for_cleanup_work(&self) {
        loop {
            let mut notified = std::pin::pin!(self.inner.wake.notified());
            notified.as_mut().enable();
            if !self.inner.terminal.is_empty() || !self.inner.signals.is_empty() {
                return;
            }
            notified.await;
        }
    }

    /// Waits without a lost-wake race for a sticky intake failure.
    ///
    /// Unlike [`Self::wait_for_work`], retained callback work does not mask a
    /// fault here. The controller host uses this while a controller awaits
    /// unrelated work, so an overflow still forcibly tears down the one run
    /// rather than relying on the controller to call back into the mailbox.
    pub(crate) async fn wait_for_fault(&self) -> AgentRuntimeMailboxFault {
        loop {
            let mut notified = std::pin::pin!(self.inner.wake.notified());
            notified.as_mut().enable();
            if let Some(fault) = self.fault() {
                return fault;
            }
            notified.await;
        }
    }

    fn publish_terminal(&self, item: TerminalItem) -> Result<(), AgentRuntimeMailboxFault> {
        self.publish(item, true)
    }

    fn publish_signal(&self, item: SignalItem) -> Result<(), AgentRuntimeMailboxFault> {
        self.publish(item, false)
    }

    fn publish<T>(&self, item: T, terminal: bool) -> Result<(), AgentRuntimeMailboxFault>
    where
        T: IntoMailboxItem,
    {
        match self.is_closed() {
            true => return Err(AgentRuntimeMailboxFault::Closed),
            false if !terminal && self.fault().is_some() => return Err(self.fault_or_closed()),
            false => {}
        }
        let reservation = self.reserve_ingress(terminal)?;
        let result = item.push_into(&self.inner);
        drop(reservation);
        match result {
            Ok(()) => {
                // The controller's event wait and its independent fatal-fault
                // monitor can both be registered. Waking only one could let
                // the monitor consume a normal-work wake while the event
                // waiter sleeps beside an already-queued settlement.
                self.inner.wake.notify_waiters();
                match self.fault() {
                    Some(fault) => Err(fault),
                    None => Ok(()),
                }
            }
            Err(()) => {
                let fault = if terminal {
                    AgentRuntimeMailboxFault::TerminalOverflow
                } else {
                    AgentRuntimeMailboxFault::SignalOverflow
                };
                self.publish_fault(fault);
                Err(self.fault_or(fault))
            }
        }
    }

    fn publish_fault(&self, proposed: AgentRuntimeMailboxFault) {
        let _ = self.inner.fault.fetch_or(proposed.code(), Ordering::AcqRel);
        self.inner.wake.notify_waiters();
    }

    fn is_closed(&self) -> bool {
        self.inner.ingress.load(Ordering::Acquire) & INGRESS_CLOSED != 0
    }

    fn reserve_ingress(
        &self,
        terminal: bool,
    ) -> Result<IngressReservation, AgentRuntimeMailboxFault> {
        loop {
            let current = self.inner.ingress.load(Ordering::Acquire);
            if current & INGRESS_CLOSED != 0 {
                return Err(AgentRuntimeMailboxFault::Closed);
            }
            if current & INGRESS_COUNT_MASK == INGRESS_COUNT_MASK {
                let fault = if terminal {
                    AgentRuntimeMailboxFault::TerminalOverflow
                } else {
                    AgentRuntimeMailboxFault::SignalOverflow
                };
                self.publish_fault(fault);
                return Err(self.fault_or(fault));
            }
            match self.inner.ingress.compare_exchange(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(IngressReservation {
                        inner: Arc::clone(&self.inner),
                    })
                }
                Err(_) => continue,
            }
        }
    }

    async fn wait_for_ingress_drain(&self) {
        loop {
            let mut notified = std::pin::pin!(self.inner.ingress_wake.notified());
            notified.as_mut().enable();
            let current = self.inner.ingress.load(Ordering::Acquire);
            if current & INGRESS_CLOSED != 0 && current & INGRESS_COUNT_MASK == 0 {
                return;
            }
            notified.await;
        }
    }

    fn fault_or(&self, fallback: AgentRuntimeMailboxFault) -> AgentRuntimeMailboxFault {
        match self.fault() {
            Some(sticky) => sticky,
            None => fallback,
        }
    }

    fn fault_or_closed(&self) -> AgentRuntimeMailboxFault {
        self.fault_or(AgentRuntimeMailboxFault::Closed)
    }

    #[cfg(test)]
    pub(crate) fn publish_test_terminal(&self) -> Result<(), AgentRuntimeMailboxFault> {
        self.publish_terminal(TerminalItem::Test)
    }

    #[cfg(test)]
    pub(crate) fn publish_test_signal(&self) -> Result<(), AgentRuntimeMailboxFault> {
        self.publish_signal(SignalItem::Test)
    }

    #[cfg(test)]
    pub(crate) fn hold_test_callback(&self) -> Result<impl Drop, AgentRuntimeMailboxFault> {
        self.reserve_ingress(true)
    }
}

struct IngressReservation {
    inner: Arc<MailboxInner>,
}

fn discard_staged_item(item: AgentRuntimeMailboxItem) {
    match item {
        AgentRuntimeMailboxItem::NativeTerminal(event) => drop(event),
        AgentRuntimeMailboxItem::SemanticActionTerminal(settlement) => drop(settlement),
        AgentRuntimeMailboxItem::AuditTerminal(settlement) => {
            let _ = settlement;
        }
        AgentRuntimeMailboxItem::NavigationReplaced(replacement) => drop(replacement),
        AgentRuntimeMailboxItem::RendererLost(loss) => {
            let _ = loss;
        }
        #[cfg(test)]
        AgentRuntimeMailboxItem::TestTerminal | AgentRuntimeMailboxItem::TestSignal => {}
    }
}

impl Drop for IngressReservation {
    fn drop(&mut self) {
        let prior = self.inner.ingress.fetch_sub(1, Ordering::AcqRel);
        if prior & INGRESS_CLOSED != 0 && prior & INGRESS_COUNT_MASK == 1 {
            self.inner.ingress_wake.notify_waiters();
        }
    }
}

trait IntoMailboxItem {
    fn push_into(self, inner: &MailboxInner) -> Result<(), ()>;
}

impl IntoMailboxItem for TerminalItem {
    fn push_into(self, inner: &MailboxInner) -> Result<(), ()> {
        inner.terminal.push(self).map_err(|_| ())
    }
}

impl IntoMailboxItem for SignalItem {
    fn push_into(self, inner: &MailboxInner) -> Result<(), ()> {
        inner.signals.push(self).map_err(|_| ())
    }
}

/// Native event sink safe to retain and invoke from a platform callback.
#[derive(Clone)]
pub struct NativeEventSink {
    mailbox: AgentRuntimeMailbox,
}

impl NativeEventSink {
    /// Classifies and attempts to retain one native event without unwinding.
    pub fn publish(&self, event: ContextNativeEvent) -> Result<(), AgentRuntimeMailboxFault> {
        panic_safe(&self.mailbox, || match event {
            ContextNativeEvent::NavigationReplaced(replacement) => self
                .mailbox
                .publish_signal(SignalItem::NavigationReplaced(replacement)),
            ContextNativeEvent::RendererLost(loss) => {
                self.mailbox.publish_signal(SignalItem::RendererLost(loss))
            }
            event => self.mailbox.publish_terminal(TerminalItem::Native(event)),
        })
    }
}

/// Semantic-action callback sink safe to retain and invoke from native code.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct SemanticActionSink {
    mailbox: AgentRuntimeMailbox,
}

impl SemanticActionSink {
    /// Retains one semantic-action terminal result without unwinding.
    #[allow(dead_code)]
    pub(crate) fn publish(
        &self,
        settlement: SemanticActionNativeSettlement,
    ) -> Result<(), AgentRuntimeMailboxFault> {
        panic_safe(&self.mailbox, || {
            self.mailbox
                .publish_terminal(TerminalItem::SemanticAction(settlement))
        })
    }

    /// Creates the move-only callback required by the native port contract.
    #[allow(dead_code)]
    pub(crate) fn completion(&self) -> SemanticActionNativeCompletion {
        let sink = self.clone();
        Box::new(move |settlement| {
            let _ = sink.publish(settlement);
        })
    }
}

/// Audit-delivery callback sink safe to retain and invoke from a store actor.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct AgentAuditSink {
    mailbox: AgentRuntimeMailbox,
}

impl AgentAuditSink {
    /// Retains one audit terminal result without unwinding.
    #[allow(dead_code)]
    pub(crate) fn publish(
        &self,
        settlement: AgentAuditDeliverySettlement,
    ) -> Result<(), AgentRuntimeMailboxFault> {
        panic_safe(&self.mailbox, || {
            self.mailbox
                .publish_terminal(TerminalItem::Audit(settlement))
        })
    }

    /// Creates the move-only callback required by the audit port contract.
    #[allow(dead_code)]
    pub(crate) fn completion(&self) -> AgentAuditCompletion {
        let sink = self.clone();
        Box::new(move |settlement| {
            let _ = sink.publish(settlement);
        })
    }
}

fn panic_safe(
    mailbox: &AgentRuntimeMailbox,
    operation: impl FnOnce() -> Result<(), AgentRuntimeMailboxFault>,
) -> Result<(), AgentRuntimeMailboxFault> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(_) => {
            // A caught callback panic is an intake-boundary failure, not only
            // a diagnostic fault. Close the ingress bit so no later terminal
            // callback can reserve a slot before staged teardown drains it.
            mailbox.close();
            Err(AgentRuntimeMailboxFault::Closed)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn mailbox(terminal: usize, signals: usize) -> AgentRuntimeMailbox {
        let config = AgentRuntimeMailboxConfig::try_new(terminal, signals)
            .expect("test capacities are within the published bounds");
        AgentRuntimeMailbox::new(config)
    }

    #[test]
    fn signals_cannot_crowd_out_reserved_terminal_debt() {
        let mailbox = mailbox(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        );
        assert!(mailbox.publish_test_signal().is_ok());
        assert!(mailbox.publish_test_signal().is_ok());
        assert_eq!(
            mailbox.publish_test_signal(),
            Err(AgentRuntimeMailboxFault::SignalOverflow)
        );
        assert_eq!(
            mailbox.publish_test_terminal(),
            Err(AgentRuntimeMailboxFault::SignalOverflow)
        );
        assert!(matches!(
            mailbox.try_pop(),
            Some(AgentRuntimeMailboxItem::TestTerminal)
        ));
    }

    #[test]
    fn both_overflow_types_are_sticky_and_terminal_loss_wins_diagnostics() {
        let mailbox = mailbox(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        );
        for _ in 0..MIN_AGENT_RUNTIME_SIGNAL_CAPACITY {
            assert!(mailbox.publish_test_signal().is_ok());
        }
        assert_eq!(
            mailbox.publish_test_signal(),
            Err(AgentRuntimeMailboxFault::SignalOverflow)
        );
        for _ in 0..MIN_AGENT_RUNTIME_TERMINAL_CAPACITY {
            assert_eq!(
                mailbox.publish_test_terminal(),
                Err(AgentRuntimeMailboxFault::SignalOverflow)
            );
        }
        assert_eq!(
            mailbox.publish_test_terminal(),
            Err(AgentRuntimeMailboxFault::TerminalOverflow)
        );
        assert_eq!(
            mailbox.fault(),
            Some(AgentRuntimeMailboxFault::TerminalOverflow)
        );
    }

    #[test]
    fn close_rejects_late_terminal_callbacks_after_overflow() {
        let mailbox = mailbox(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        );
        for _ in 0..MIN_AGENT_RUNTIME_SIGNAL_CAPACITY {
            assert!(mailbox.publish_test_signal().is_ok());
        }
        assert!(mailbox.publish_test_signal().is_err());
        mailbox.close();
        assert_eq!(
            mailbox.publish_test_terminal(),
            Err(AgentRuntimeMailboxFault::Closed)
        );
    }

    #[test]
    fn closed_mailbox_wake_is_not_lost_after_registration() {
        let mailbox = mailbox(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime");
        mailbox.close();
        let wake = runtime.block_on(async {
            tokio::time::timeout(Duration::from_millis(50), mailbox.wait_for_work())
                .await
                .expect("closed mailbox must wake deterministically")
        });
        assert_eq!(
            wake,
            AgentRuntimeMailboxWake::Fault(AgentRuntimeMailboxFault::Closed)
        );
    }

    #[test]
    fn callback_panic_is_caught_and_closes_the_mailbox() {
        let mailbox = mailbox(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        );
        let result = panic_safe(&mailbox, || -> Result<(), AgentRuntimeMailboxFault> {
            panic!("test callback panic")
        });
        assert_eq!(result, Err(AgentRuntimeMailboxFault::Closed));
        assert_eq!(mailbox.fault(), Some(AgentRuntimeMailboxFault::Closed));
        assert_eq!(
            mailbox.publish_test_terminal(),
            Err(AgentRuntimeMailboxFault::Closed)
        );
        assert!(mailbox.try_pop().is_none());
    }

    #[test]
    fn public_debug_is_content_free() {
        let mailbox = mailbox(
            MIN_AGENT_RUNTIME_TERMINAL_CAPACITY,
            MIN_AGENT_RUNTIME_SIGNAL_CAPACITY,
        );
        assert!(mailbox.publish_test_terminal().is_ok());
        let item = mailbox.try_pop().expect("test item");
        assert_eq!(format!("{item:?}"), "TestTerminal");
    }
}
