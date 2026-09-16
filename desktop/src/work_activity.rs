//! Bounded observers of original attempts. No resource, task or execution owner.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use zephium_app::work_runtime::{WorkAttemptObserver, WorkPageFrame};
use zephium_core::work::{runtime::*, WorkAttemptId, WorkExecutionId, WorkStepId};
use zephium_ipc::work::{WorkPageFrameV1, WorkPageV1, WorkSignalV1};

const MAX_PAGE_ATTEMPTS: usize = 8;

#[derive(Default)]
pub(crate) struct WorkActivity {
    observers: Mutex<BTreeMap<WorkAttemptId, WorkAttemptObserver>>,
    /// Pages of recent attempts, kept after the attempt ends so their last
    /// frame outlives the run for this app session.
    pages: Mutex<BTreeMap<WorkAttemptId, (WorkExecutionId, Vec<WorkPageFrame>)>>,
}
impl WorkActivity {
    pub(crate) fn track(&self, observer: WorkAttemptObserver) {
        let Ok(mut observers) = self.observers.lock() else {
            return;
        };
        observers.retain(|_, observer| observer.is_alive());
        if observers.len() < MAX_WORK_ATTEMPTS {
            observers.entry(observer.attempt()).or_insert(observer);
        }
    }
    pub(crate) fn read(&self, state: &WorkRuntimeProjection) -> Vec<WorkSignalV1> {
        let Ok(mut observers) = self.observers.lock() else {
            return Vec::new();
        };
        observers.retain(|_, observer| observer.is_alive());
        observers
            .values()
            .filter_map(WorkAttemptObserver::latest)
            .filter(|signal| current(signal, state))
            .collect()
    }
    pub(crate) fn read_pages(&self, state: &WorkRuntimeProjection) -> Vec<WorkPageV1> {
        let (Ok(mut observers), Ok(mut pages)) = (self.observers.lock(), self.pages.lock()) else {
            return Vec::new();
        };
        observers.retain(|_, observer| observer.is_alive());
        for observer in observers.values() {
            let opened = observer.pages();
            if opened.is_empty() {
                continue;
            }
            if pages.len() >= MAX_PAGE_ATTEMPTS && !pages.contains_key(&observer.attempt()) {
                let oldest = pages.keys().next().copied();
                if let Some(oldest) = oldest {
                    pages.remove(&oldest);
                }
            }
            pages.insert(observer.attempt(), (observer.execution(), opened));
        }
        let executions: Vec<_> = state.executions.iter().map(|e| e.id).collect();
        pages.retain(|_, (execution, _)| executions.contains(execution));
        pages
            .iter()
            .flat_map(|(attempt, (execution, opened))| {
                opened.iter().map(|page| WorkPageV1 {
                    execution: *execution,
                    attempt: *attempt,
                    step: page.step,
                    url: page.url.clone(),
                    live: page.live,
                    frame: page.frame.as_ref().map(|frame| WorkPageFrameV1 {
                        generation: u32::try_from(frame.generation).unwrap_or(u32::MAX),
                        width: frame.width,
                        height: frame.height,
                    }),
                })
            })
            .collect()
    }
    pub(crate) fn frame(&self, attempt: WorkAttemptId, step: WorkStepId) -> Option<Arc<Vec<u8>>> {
        let pages = self.pages.lock().ok()?;
        let (_, opened) = pages.get(&attempt)?;
        opened
            .iter()
            .find(|page| page.step == step)?
            .frame
            .as_ref()
            .map(|frame| frame.png.clone())
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
