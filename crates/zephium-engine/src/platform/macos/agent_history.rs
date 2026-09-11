//! Run-local native history authority for owned agent pages.
//!
//! URL equality and WebKit's boolean `canGoBack` are deliberately insufficient:
//! authority is bound to exact retained `WKBackForwardListItem` identities that
//! this run enrolled after successful GET navigation. Attempt accounting moves
//! before native dispatch; the logical cursor moves only after correlated success.

use std::num::NonZeroUsize;

use objc2::rc::Retained;
use objc2_web_kit::WKBackForwardListItem;
use zephium_agentic::ContextNavigationTarget;

pub(super) const MAX_AGENT_HISTORY_ENTRIES: usize = 32;
pub(super) const MAX_AGENT_HISTORY_ATTEMPTS: u16 = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AgentHistoryEntryIdentity(NonZeroUsize);

impl AgentHistoryEntryIdentity {
    fn from_item(item: &WKBackForwardListItem) -> Option<Self> {
        NonZeroUsize::new(std::ptr::from_ref(item).addr()).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AgentDocumentRuntimeId(u64);

impl AgentDocumentRuntimeId {
    pub(super) const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentHistoryProvenance {
    HostIssuedGet,
}

struct AgentHistoryEntry {
    identity: AgentHistoryEntryIdentity,
    item: Option<Retained<WKBackForwardListItem>>,
    target: ContextNavigationTarget,
    runtime: AgentDocumentRuntimeId,
    provenance: AgentHistoryProvenance,
}

/// Single-use authority for one exact immediate predecessor traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AgentHistoryBackTicket {
    attempt: u16,
    source: AgentHistoryEntryIdentity,
    destination: AgentHistoryEntryIdentity,
    destination_runtime: AgentDocumentRuntimeId,
}

impl AgentHistoryBackTicket {
    pub(super) const fn destination_runtime(&self) -> AgentDocumentRuntimeId {
        self.destination_runtime
    }
}

/// Bounded successful-history projection for one owned page/run lease.
#[derive(Default)]
pub(super) struct AgentHistoryLedger {
    entries: Vec<AgentHistoryEntry>,
    cursor: Option<usize>,
    attempts: u16,
    pending: Option<AgentHistoryBackTicket>,
}

impl AgentHistoryLedger {
    /// Enrolls the exact current native item after a host-issued GET has
    /// completed and all ordinary Work navigation evidence has settled.
    pub(super) fn enroll_get(
        &mut self,
        item: Retained<WKBackForwardListItem>,
        target: ContextNavigationTarget,
        runtime: AgentDocumentRuntimeId,
    ) -> Result<(), ()> {
        if self.pending.is_some() || self.entries.len() >= MAX_AGENT_HISTORY_ENTRIES {
            return Err(());
        }
        let identity = AgentHistoryEntryIdentity::from_item(&item).ok_or(())?;
        if self.entries.iter().any(|entry| entry.identity == identity) {
            return Err(());
        }
        if let Some(cursor) = self.cursor {
            self.entries.truncate(cursor.checked_add(1).ok_or(())?);
        } else if !self.entries.is_empty() {
            return Err(());
        }
        self.entries.push(AgentHistoryEntry {
            identity,
            item: Some(item),
            target,
            runtime,
            provenance: AgentHistoryProvenance::HostIssuedGet,
        });
        self.cursor = self.entries.len().checked_sub(1);
        Ok(())
    }

    /// Freezes a single Back attempt only when native WebKit exposes the exact
    /// enrolled current item and immediate predecessor. This increments the
    /// attempt budget even if subsequent park or native dispatch fails.
    pub(super) fn authorize_back(
        &mut self,
        current: &WKBackForwardListItem,
        predecessor: &WKBackForwardListItem,
    ) -> Result<AgentHistoryBackTicket, ()> {
        self.authorize_back_identities(
            AgentHistoryEntryIdentity::from_item(current).ok_or(())?,
            AgentHistoryEntryIdentity::from_item(predecessor).ok_or(())?,
        )
    }

    fn authorize_back_identities(
        &mut self,
        current: AgentHistoryEntryIdentity,
        predecessor: AgentHistoryEntryIdentity,
    ) -> Result<AgentHistoryBackTicket, ()> {
        if self.pending.is_some() || self.attempts >= MAX_AGENT_HISTORY_ATTEMPTS {
            return Err(());
        }
        let cursor = self.cursor.ok_or(())?;
        let destination_index = cursor.checked_sub(1).ok_or(())?;
        let source = self.entries.get(cursor).ok_or(())?;
        let destination = self.entries.get(destination_index).ok_or(())?;
        if source.identity != current
            || destination.identity != predecessor
            || destination.provenance != AgentHistoryProvenance::HostIssuedGet
        {
            return Err(());
        }
        self.attempts = self.attempts.checked_add(1).ok_or(())?;
        self.pending = Some(AgentHistoryBackTicket {
            attempt: self.attempts,
            source: source.identity,
            destination: destination.identity,
            destination_runtime: destination.runtime,
        });
        self.pending.ok_or(())
    }

    /// Revalidates exact native identity immediately before dispatch.
    pub(super) fn dispatch_item(
        &self,
        ticket: AgentHistoryBackTicket,
        current: &WKBackForwardListItem,
        predecessor: &WKBackForwardListItem,
    ) -> Option<&WKBackForwardListItem> {
        let pending = self.pending.as_ref()?;
        if pending.attempt != ticket.attempt
            || pending.source != ticket.source
            || pending.destination != ticket.destination
            || AgentHistoryEntryIdentity::from_item(current)? != pending.source
            || AgentHistoryEntryIdentity::from_item(predecessor)? != pending.destination
        {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.identity == pending.destination)
            .and_then(|entry| entry.item.as_deref())
    }

    pub(super) fn destination_target(
        &self,
        ticket: AgentHistoryBackTicket,
    ) -> Option<&ContextNavigationTarget> {
        let pending = self.pending.as_ref()?;
        if *pending != ticket {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.identity == pending.destination)
            .map(|entry| &entry.target)
    }

    /// Moves the logical cursor only after traversal evidence joins the exact
    /// destination item. A cold restoration may replace its document runtime.
    pub(super) fn settle_back(
        &mut self,
        ticket: AgentHistoryBackTicket,
        current: &WKBackForwardListItem,
        cold_runtime: Option<AgentDocumentRuntimeId>,
    ) -> Result<(), ()> {
        self.settle_back_identity(
            ticket,
            AgentHistoryEntryIdentity::from_item(current).ok_or(())?,
            cold_runtime,
        )
    }

    fn settle_back_identity(
        &mut self,
        ticket: AgentHistoryBackTicket,
        current: AgentHistoryEntryIdentity,
        cold_runtime: Option<AgentDocumentRuntimeId>,
    ) -> Result<(), ()> {
        let pending = self.pending.take().ok_or(())?;
        if pending.attempt != ticket.attempt
            || pending.source != ticket.source
            || pending.destination != ticket.destination
            || current != pending.destination
        {
            return Err(());
        }
        let cursor = self.cursor.ok_or(())?;
        let destination_index = cursor.checked_sub(1).ok_or(())?;
        let destination = self.entries.get_mut(destination_index).ok_or(())?;
        if destination.identity != pending.destination {
            return Err(());
        }
        if let Some(runtime) = cold_runtime {
            destination.runtime = runtime;
        }
        self.cursor = Some(destination_index);
        Ok(())
    }

    /// Refuses one attempted traversal without moving successful history.
    pub(super) fn refuse_back(&mut self, ticket: AgentHistoryBackTicket) -> bool {
        self.pending.take().is_some_and(|pending| {
            pending.attempt == ticket.attempt
                && pending.source == ticket.source
                && pending.destination == ticket.destination
        })
    }

    /// Clears all native identities and pending authority on lease/run/lifecycle loss.
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.cursor = None;
        self.pending = None;
        self.attempts = 0;
    }

    #[cfg(test)]
    fn cursor(&self) -> Option<usize> {
        self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(value: usize) -> AgentHistoryEntryIdentity {
        AgentHistoryEntryIdentity(NonZeroUsize::new(value).expect("identity"))
    }

    fn entry(value: usize, url: &str, runtime: u64) -> AgentHistoryEntry {
        AgentHistoryEntry {
            identity: identity(value),
            item: None,
            target: ContextNavigationTarget::parse(url).expect("target"),
            runtime: AgentDocumentRuntimeId::new(runtime).expect("runtime"),
            provenance: AgentHistoryProvenance::HostIssuedGet,
        }
    }

    fn ledger(values: &[(usize, &str, u64)]) -> AgentHistoryLedger {
        AgentHistoryLedger {
            entries: values
                .iter()
                .map(|(value, url, runtime)| entry(*value, url, *runtime))
                .collect(),
            cursor: values.len().checked_sub(1),
            attempts: 0,
            pending: None,
        }
    }

    #[test]
    fn constants_bound_retention_and_attempts() {
        assert_eq!(MAX_AGENT_HISTORY_ENTRIES, 32);
        assert_eq!(MAX_AGENT_HISTORY_ATTEMPTS, 128);
        assert!(AgentDocumentRuntimeId::new(0).is_none());
        assert!(AgentDocumentRuntimeId::new(1).is_some());
    }

    #[test]
    fn exact_identity_not_duplicate_url_authorizes_immediate_predecessor() {
        let mut ledger = ledger(&[
            (1, "https://example.test/a", 1),
            (2, "https://example.test/same", 2),
            (3, "https://example.test/same", 3),
        ]);
        assert!(ledger
            .authorize_back_identities(identity(3), identity(1))
            .is_err());
        let ticket = ledger
            .authorize_back_identities(identity(3), identity(2))
            .expect("back");
        assert_eq!(ticket.attempt, 1);
        assert_eq!(ledger.cursor(), Some(2));
    }

    #[test]
    fn refusal_spends_attempt_but_preserves_success_cursor() {
        let mut ledger = ledger(&[
            (1, "https://example.test/a", 1),
            (2, "https://example.test/b", 2),
        ]);
        let ticket = ledger
            .authorize_back_identities(identity(2), identity(1))
            .expect("back");
        assert!(ledger.refuse_back(ticket));
        assert_eq!(ledger.cursor(), Some(1));
        assert_eq!(ledger.attempts, 1);
    }

    #[test]
    fn success_moves_cursor_and_cold_restore_replaces_runtime_only() {
        let mut ledger = ledger(&[
            (1, "https://example.test/a", 1),
            (2, "https://example.test/b", 2),
        ]);
        let ticket = ledger
            .authorize_back_identities(identity(2), identity(1))
            .expect("back");
        ledger
            .settle_back_identity(ticket, identity(1), AgentDocumentRuntimeId::new(9))
            .expect("settle");
        assert_eq!(ledger.cursor(), Some(0));
        assert_eq!(
            ledger.entries[0].runtime,
            AgentDocumentRuntimeId::new(9).unwrap()
        );
        assert_eq!(
            ledger.entries[0].target.as_url().as_str(),
            "https://example.test/a"
        );
    }

    #[test]
    fn wrong_terminal_identity_fails_closed_and_consumes_pending_ticket() {
        let mut ledger = ledger(&[
            (1, "https://example.test/a", 1),
            (2, "https://example.test/b", 2),
        ]);
        let ticket = ledger
            .authorize_back_identities(identity(2), identity(1))
            .expect("back");
        assert!(ledger
            .settle_back_identity(ticket, identity(7), None)
            .is_err());
        assert_eq!(ledger.cursor(), Some(1));
        assert!(ledger.pending.is_none());
    }
}
