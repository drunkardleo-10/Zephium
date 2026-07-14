// Copyright 2020-2026 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Bounded admission for native requests that outlive their initiating callback.

use std::{
  collections::HashSet,
  sync::{Arc, Mutex, TryLockError},
};

/// Custom-protocol handlers are privileged and asynchronous. A modest ceiling
/// leaves ample room for normal asset/IPC concurrency while preventing a
/// compromised chrome renderer from retaining an unbounded number of native
/// tasks, deferrals, response channels, and request bodies.
pub(crate) const CUSTOM_PROTOCOL_IN_FLIGHT_LIMIT: usize = 32;
pub(crate) const CUSTOM_PROTOCOL_OVERFLOW_STATUS: http::StatusCode =
  http::StatusCode::SERVICE_UNAVAILABLE;

#[derive(Debug)]
struct AdmissionState {
  inner: Mutex<AdmissionInner>,
  limit: usize,
}

#[derive(Debug)]
struct AdmissionInner {
  active: HashSet<u64>,
  next_token: u64,
  sealed: bool,
}

/// A cloneable admission scope. Clones share one exact active-request count.
#[derive(Clone, Debug)]
pub(crate) struct InFlightAdmission {
  state: Arc<AdmissionState>,
}

/// The unique ownership proof for one admitted request.
///
/// This type is deliberately neither `Clone` nor manually releasable. Moving
/// it into a native pending-request record makes every response, cancellation,
/// timeout, or teardown path release the slot through exactly one `Drop`.
#[derive(Debug)]
pub(crate) struct InFlightPermit {
  state: Option<Arc<AdmissionState>>,
  token: u64,
}

impl InFlightAdmission {
  pub(crate) fn new(limit: usize) -> Self {
    Self {
      state: Arc::new(AdmissionState {
        inner: Mutex::new(AdmissionInner {
          active: HashSet::new(),
          next_token: 1,
          sealed: false,
        }),
        limit,
      }),
    }
  }

  /// Acquires one slot without blocking. Once accounting becomes unverifiable,
  /// admission remains fail-closed for the lifetime of this scope.
  pub(crate) fn try_acquire(&self) -> Option<InFlightPermit> {
    let mut inner = match self.state.inner.try_lock() {
      Ok(inner) => inner,
      Err(TryLockError::WouldBlock) => return None,
      Err(TryLockError::Poisoned(error)) => {
        // A panic while mutating accounting makes its history unprovable.
        // Recover the guard only to seal and drain; never resume admission.
        let mut inner = error.into_inner();
        inner.sealed = true;
        inner.active.clear();
        return None;
      }
    };
    if inner.sealed || inner.active.len() >= self.state.limit {
      return None;
    }
    let token = inner.next_token;
    let Some(next_token) = token.checked_add(1) else {
      inner.sealed = true;
      inner.active.clear();
      return None;
    };
    inner.next_token = next_token;
    if token == 0 || !inner.active.insert(token) {
      inner.sealed = true;
      inner.active.clear();
      return None;
    }
    Some(InFlightPermit {
      state: Some(self.state.clone()),
      token,
    })
  }

  /// Revokes every outstanding slot and permanently rejects new work. Native
  /// teardown calls this after cancelling its task registry. Late permit drops
  /// are then recognized as teardown revocations rather than underflow.
  pub(crate) fn seal_and_drain(&self) {
    let mut inner = match self.state.inner.lock() {
      Ok(inner) => inner,
      Err(error) => error.into_inner(),
    };
    inner.sealed = true;
    inner.active.clear();
  }

  #[cfg(test)]
  fn active(&self) -> usize {
    self
      .state
      .inner
      .lock()
      .map(|inner| inner.active.len())
      .unwrap_or_default()
  }
}

impl Drop for InFlightPermit {
  fn drop(&mut self) {
    let Some(state) = self.state.take() else {
      return;
    };
    let mut inner = match state.inner.lock() {
      Ok(inner) => inner,
      Err(error) => {
        let mut inner = error.into_inner();
        inner.sealed = true;
        inner.active.clear();
        return;
      }
    };
    if !inner.active.remove(&self.token) && !inner.sealed {
      // The safe API cannot release a token twice. Treat any unexplained
      // absence as corrupt accounting and fail closed without panicking.
      inner.sealed = true;
      inner.active.clear();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exact_cap_plus_one_and_reuse_after_completion() {
    assert_eq!(
      CUSTOM_PROTOCOL_OVERFLOW_STATUS,
      http::StatusCode::SERVICE_UNAVAILABLE
    );
    let admission = InFlightAdmission::new(CUSTOM_PROTOCOL_IN_FLIGHT_LIMIT);
    let mut permits = Vec::new();
    for _ in 0..CUSTOM_PROTOCOL_IN_FLIGHT_LIMIT {
      permits.push(admission.try_acquire().expect("slot within exact cap"));
    }
    assert_eq!(admission.active(), CUSTOM_PROTOCOL_IN_FLIGHT_LIMIT);
    assert!(admission.try_acquire().is_none());

    drop(permits.pop());
    assert_eq!(admission.active(), CUSTOM_PROTOCOL_IN_FLIGHT_LIMIT - 1);
    let reused = admission.try_acquire().expect("released slot is reusable");
    assert_eq!(admission.active(), CUSTOM_PROTOCOL_IN_FLIGHT_LIMIT);

    drop((permits, reused));
    assert_eq!(admission.active(), 0);
  }

  #[test]
  fn cloned_scopes_and_cross_thread_drop_share_exact_accounting() {
    let admission = InFlightAdmission::new(1);
    let cloned = admission.clone();
    let permit = cloned.try_acquire().expect("shared slot");
    assert!(admission.try_acquire().is_none());

    std::thread::spawn(move || drop(permit))
      .join()
      .expect("permit drop thread");
    assert_eq!(admission.active(), 0);
    assert!(admission.try_acquire().is_some());
  }

  #[test]
  fn teardown_drain_revokes_late_permits_without_underflow_or_reopening() {
    let admission = InFlightAdmission::new(2);
    let first = admission.try_acquire().expect("first slot");
    let second = admission.try_acquire().expect("second slot");
    admission.seal_and_drain();
    assert_eq!(admission.active(), 0);
    assert!(admission.try_acquire().is_none());

    drop((first, second));
    assert_eq!(admission.active(), 0);
    assert!(admission.try_acquire().is_none());
  }
}
