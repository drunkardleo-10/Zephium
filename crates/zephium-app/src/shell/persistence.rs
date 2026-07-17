//! Bounded session persistence scheduling and snapshot durability.

use super::*;

pub(super) const PERSIST_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);
pub(super) const PERSIST_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(2);
// URL-only native observations are recoverability checkpoints, not structural
// mutations. Structural changes retain the fast debounce; URL churn is
// globally coalesced to one full snapshot per five minutes.
pub(super) const URL_CHECKPOINT_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(5 * 60);
pub(super) const URL_CHECKPOINT_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(5);

impl Shell {
    pub(super) fn schedule_persist(&mut self) {
        if !self.bootstrapped {
            return;
        }
        self.session_revision = self.session_revision.wrapping_add(1);
        self.schedule_current_session_persist();
    }

    pub(super) fn schedule_url_checkpoint(&mut self, id: ItemId) {
        if !self.bootstrapped || self.items.tab(id).is_none() {
            return;
        }
        self.session_revision = self.session_revision.wrapping_add(1);
        let first_url_dirty = self.url_checkpoint_dirty.is_empty();
        self.url_checkpoint_dirty.insert(id);
        // A pending structural snapshot already includes the newest URL and
        // retains its much shorter durability deadline.
        if self.persist_first_dirty.is_some() || !first_url_dirty {
            return;
        }
        let now = std::time::Instant::now();
        let interval_floor = self
            .last_url_checkpoint
            .checked_add(URL_CHECKPOINT_INTERVAL)
            .unwrap_or(now + URL_CHECKPOINT_INTERVAL);
        let deadline = interval_floor.max(now + URL_CHECKPOINT_DEBOUNCE);
        if let Some(queue) = self.self_queue.as_ref() {
            queue.schedule_persist(deadline);
        } else {
            #[cfg(test)]
            {
                // Deterministic unit shells do not own the production timer.
                self.persist();
            }
            #[cfg(not(test))]
            {
                // Production construction always installs the timer before
                // the actor can receive a URL. If that invariant changes,
                // defer to the exact shutdown snapshot instead of restoring
                // hostile per-URL full rewrites.
                eprintln!("persistence: URL checkpoint timer is unavailable");
            }
        }
    }

    /// Schedules the current authoritative state without advancing its logical
    /// revision. Used after a durable deletion barrier when mutations already
    /// counted by `schedule_persist` need a new post-barrier debounce.
    pub(super) fn schedule_current_session_persist(&mut self) {
        if !self.bootstrapped {
            return;
        }
        let now = std::time::Instant::now();
        let first = *self.persist_first_dirty.get_or_insert(now);
        let deadline = (now + PERSIST_DEBOUNCE).min(first + PERSIST_MAX_AGE);
        if let Some(queue) = self.self_queue.as_ref() {
            queue.schedule_persist(deadline);
        } else {
            // Directly-constructed shells are used by deterministic unit
            // tests and embedders without the production timer thread.
            self.persist();
        }
    }

    pub(super) fn persist(&mut self) {
        self.persist_first_dirty = None;
        // The durable session is loaded by Bootstrap. Before that ordered
        // point, an empty in-memory shell is not authoritative: persisting it
        // during an immediate quit would erase a valid previous session.
        if !self.bootstrapped {
            return;
        }
        self.url_checkpoint_dirty.clear();
        self.last_url_checkpoint = std::time::Instant::now();
        let win = self.windows.focused();
        let state = session::snapshot(
            &self.profiles,
            &self.spaces,
            &self.items,
            win.map(|w| w.space),
            win.and_then(|w| w.active),
            win.and_then(|w| w.splits.as_ref()),
        );
        self.store.save_session(state);
    }
}
