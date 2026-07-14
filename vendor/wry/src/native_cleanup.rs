// Copyright 2020-2026 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Small, platform-independent state machine for fallible native teardown.
//!
//! Native handles live in their platform adapters. This type only records
//! which explicit release contracts are still outstanding, which makes retry
//! and fault-injection semantics testable without a WebView runtime.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CleanupStep {
  ParentSubclass,
  Controller,
  ContainerWindow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CleanupPlan {
  parent_subclass: bool,
  controller: bool,
  container_window: bool,
}

impl CleanupPlan {
  pub(crate) fn new(controller: bool, container_window: bool) -> Self {
    Self {
      parent_subclass: false,
      controller,
      container_window,
    }
  }

  pub(crate) fn set_parent_subclass(&mut self, pending: bool) {
    self.parent_subclass = pending;
  }

  pub(crate) fn set_controller(&mut self, pending: bool) {
    self.controller = pending;
  }

  pub(crate) fn is_complete(self) -> bool {
    !self.parent_subclass && !self.controller && !self.container_window
  }

  pub(crate) fn controller_pending(self) -> bool {
    self.controller
  }

  /// Attempts every outstanding obligation once. Successful steps are
  /// terminally cleared; failed steps remain pending for the next retry.
  pub(crate) fn retry(
    &mut self,
    mut release: impl FnMut(CleanupStep) -> bool,
  ) -> Option<CleanupStep> {
    let mut first_failure = None;
    for (pending, step) in [
      (&mut self.parent_subclass, CleanupStep::ParentSubclass),
      (&mut self.controller, CleanupStep::Controller),
      (&mut self.container_window, CleanupStep::ContainerWindow),
    ] {
      if *pending && release(step) {
        *pending = false;
      } else if *pending && first_failure.is_none() {
        first_failure = Some(step);
      }
    }
    first_failure
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn failed_steps_remain_pending_while_independent_successes_commit() {
    let mut plan = CleanupPlan::new(false, true);
    plan.set_controller(true);
    plan.set_parent_subclass(true);
    assert!(plan.controller_pending());
    let first = plan.retry(|step| step != CleanupStep::Controller);
    assert_eq!(first, Some(CleanupStep::Controller));

    let mut retried = Vec::new();
    assert_eq!(
      plan.retry(|step| {
        retried.push(step);
        true
      }),
      None
    );
    assert_eq!(retried, vec![CleanupStep::Controller]);
    assert!(plan.is_complete());
  }

  #[test]
  fn retry_is_idempotent_after_completion() {
    let mut plan = CleanupPlan::new(true, true);
    assert_eq!(plan.retry(|_| true), None);
    assert!(plan.is_complete());
    assert_eq!(plan.retry(|_| panic!("completed step retried")), None);
  }

  #[test]
  fn first_failure_follows_security_order_but_all_steps_are_attempted() {
    let mut plan = CleanupPlan::new(true, true);
    plan.set_parent_subclass(true);
    let mut attempted = Vec::new();
    assert_eq!(
      plan.retry(|step| {
        attempted.push(step);
        false
      }),
      Some(CleanupStep::ParentSubclass)
    );
    assert_eq!(
      attempted,
      vec![
        CleanupStep::ParentSubclass,
        CleanupStep::Controller,
        CleanupStep::ContainerWindow
      ]
    );
  }
}
