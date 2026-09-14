//! Bounded observers of original attempts. No resource, task or execution owner.
use std::{collections::BTreeMap, sync::Mutex};
use zephium_app::work_runtime::WorkAttemptObserver;
use zephium_core::work::{runtime::*, WorkAttemptId};
use zephium_ipc::work::WorkSignalV1;

#[derive(Default)]
pub(crate) struct WorkActivity(Mutex<BTreeMap<WorkAttemptId, WorkAttemptObserver>>);
impl WorkActivity {
    pub(crate) fn track(&self, observer: WorkAttemptObserver) {
        let Ok(mut observers) = self.0.lock() else {
            return;
        };
        observers.retain(|_, observer| observer.is_alive());
        if observers.len() < MAX_WORK_ATTEMPTS {
            observers.entry(observer.attempt()).or_insert(observer);
        }
    }
    pub(crate) fn read(&self, state: &WorkRuntimeProjection) -> Vec<WorkSignalV1> {
        let Ok(mut observers) = self.0.lock() else {
            return Vec::new();
        };
        observers.retain(|_, observer| observer.is_alive());
        observers
            .values()
            .filter_map(WorkAttemptObserver::latest)
            .filter(|signal| current(signal, state))
            .collect()
    }
}

fn current(signal: &WorkSignalV1, state: &WorkRuntimeProjection) -> bool {
    signal.version == 1
        && signal.profile == state.work.profile.to_string()
        && signal.work == state.work.id
        && signal.basis_revision == state.work.revision
        && !state.interrupted.contains(&signal.execution)
        && state
            .owners
            .iter()
            .any(|entry| entry.execution == signal.execution && entry.owner == signal.owner)
        && state.executions.iter().any(|execution| {
            execution.id == signal.execution
                && matches!(
                    execution.status,
                    WorkExecutionStatus::Running | WorkExecutionStatus::CancelRequested
                )
                && execution.attempts.iter().any(|attempt| {
                    attempt.id == signal.attempt
                        && attempt.node == signal.node
                        && attempt.status == WorkAttemptStatus::Running
                })
        })
}
