//! Application-owned, bounded provider/worker lifetimes. Observing a task never
//! starts it; neither a completed callback nor a cached result owns an attempt.
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
};
use zephium_core::{
    ids::ProfileId,
    work::{WorkCommandId, WorkError},
};
use zephium_ipc::work::{WorkOperationStateV1, WorkOperationV1};

type Key = (ProfileId, WorkCommandId);
const MAX_OPERATIONS: usize = 16;
const MAX_ACTIVE: usize = 4;

struct Operation {
    input: WorkOperationV1,
    state: WorkOperationStateV1,
    task: Option<tokio::task::JoinHandle<()>>,
    acknowledged: bool,
}
struct Registry {
    closing: bool,
    ai_enabled: bool,
    work_enabled: bool,
    preference: Option<PendingPreference>,
    entries: BTreeMap<Key, Operation>,
    retired: BTreeMap<Key, WorkOperationV1>,
}
struct PendingPreference {
    operation_id: String,
    ai: bool,
    enabled: bool,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            closing: false,
            ai_enabled: true,
            work_enabled: true,
            preference: None,
            entries: BTreeMap::new(),
            retired: BTreeMap::new(),
        }
    }
}
impl Registry {
    // An acknowledgement may race the task's final destructor. Retain that
    // owner until finished and reclaim it on the next user operation.
    fn reap_acknowledged(&mut self) {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                (entry.acknowledged && entry.task.as_ref().is_none_or(|task| task.is_finished()))
                    .then_some(*key)
            })
            .collect();
        for key in keys {
            if self.retired.len() >= 256 {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.retired.insert(key, entry.input);
            }
        }
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        for operation in self.entries.values() {
            if let Some(task) = &operation.task {
                task.abort();
            }
        }
    }
}
#[derive(Clone, Default)]
pub(crate) struct WorkOperations(Arc<Mutex<Registry>>);

impl WorkOperations {
    pub(crate) fn with_enablement(ai_enabled: bool, work_enabled: bool) -> Self {
        Self(Arc::new(Mutex::new(Registry {
            ai_enabled,
            work_enabled,
            closing: false,
            preference: None,
            entries: BTreeMap::new(),
            retired: BTreeMap::new(),
        })))
    }

    /// Serializes setting queue admission with worker admission. The pending
    /// fence is not a claim that Store has accepted or persisted the value.
    pub(crate) fn preference(
        &self,
        key: &str,
        value: &str,
        dispatch: impl FnOnce() -> zephium_ipc::OperationAdmission,
    ) -> Result<zephium_ipc::OperationAdmission, WorkError> {
        let ai = match key {
            "ai.enabled" => true,
            "work.enabled" => false,
            _ => return Err(WorkError::Invalid),
        };
        if !matches!(value, "true" | "false") {
            return Err(WorkError::Invalid);
        }
        let enabled = value == "true";
        let mut registry = self.0.lock().map_err(|_| WorkError::Unavailable)?;
        registry.reap_acknowledged();
        if registry.closing {
            return Err(WorkError::Shutdown);
        }
        if registry.preference.is_some() {
            return Err(WorkError::Conflict);
        }
        // State publication may precede the task's final destructor. Conversely,
        // an aborted task can remain Pending until observed. Inspect ownership.
        if !enabled
            && registry
                .entries
                .values()
                .any(|entry| entry.task.as_ref().is_some_and(|task| !task.is_finished()))
        {
            return Err(WorkError::Conflict);
        }
        let admission = dispatch();
        if admission.accepted {
            let operation_id = admission
                .operation_id
                .clone()
                .ok_or(WorkError::Unavailable)?;
            registry.preference = Some(PendingPreference {
                operation_id,
                ai,
                enabled,
            });
        }
        Ok(admission)
    }

    /// Called only after the exact actor disposition is accepted by the result
    /// ledger. A Store-queue refusal releases the fence without changing flags.
    pub(crate) fn preference_processed(&self, disposition: &zephium_ipc::OperationDisposition) {
        let Ok(mut registry) = self.0.lock() else {
            return;
        };
        if registry
            .preference
            .as_ref()
            .is_none_or(|pending| pending.operation_id != disposition.operation_id)
        {
            return;
        }
        let Some(pending) = registry.preference.take() else {
            return;
        };
        if disposition.outcome == zephium_ipc::OperationOutcome::Deferred
            && disposition.reason == zephium_ipc::OperationReason::StoreWorkPending
        {
            if pending.ai {
                registry.ai_enabled = pending.enabled;
            } else {
                registry.work_enabled = pending.enabled;
            }
        }
    }

    pub(crate) fn admit<F>(
        &self,
        key: Key,
        input: WorkOperationV1,
        run: F,
    ) -> Result<WorkOperationStateV1, WorkError>
    where
        F: Future<Output = WorkOperationStateV1> + Send + 'static,
    {
        let mut registry = self.0.lock().map_err(|_| WorkError::Unavailable)?;
        registry.reap_acknowledged();
        if registry.closing {
            return Err(WorkError::Shutdown);
        }
        if let Some(original) = registry.retired.get(&key) {
            return if original == &input {
                Ok(WorkOperationStateV1::Unknown)
            } else {
                Err(WorkError::Conflict)
            };
        }
        if let Some(operation) = registry.entries.get(&key) {
            return if operation.input == input {
                Ok(operation.state.clone())
            } else {
                Err(WorkError::Conflict)
            };
        }
        if registry.preference.is_some() || !registry.ai_enabled || !registry.work_enabled {
            return Err(WorkError::Unavailable);
        }
        let active = registry
            .entries
            .values()
            .filter(|entry| matches!(entry.state, WorkOperationStateV1::Pending { .. }))
            .count();
        if active >= MAX_ACTIVE || registry.entries.len() >= MAX_OPERATIONS {
            return Err(WorkError::Capacity);
        }
        let state = WorkOperationStateV1::Pending { work: input.work() };
        let owner = Arc::downgrade(&self.0);
        // The registry lock prevents completion racing task-handle publication.
        let task = tokio::spawn(async move {
            let state = run.await;
            if let Some(owner) = owner.upgrade() {
                if let Ok(mut registry) = owner.lock() {
                    if let Some(entry) = registry.entries.get_mut(&key) {
                        entry.state = state;
                    }
                }
            }
        });
        registry.entries.insert(
            key,
            Operation {
                input,
                state: state.clone(),
                task: Some(task),
                acknowledged: false,
            },
        );
        Ok(state)
    }

    pub(crate) fn observe(
        &self,
        key: Key,
        work: zephium_core::work::WorkId,
        acknowledge: bool,
    ) -> Result<WorkOperationStateV1, WorkError> {
        let mut registry = self.0.lock().map_err(|_| WorkError::Unavailable)?;
        registry.reap_acknowledged();
        if registry
            .entries
            .get(&key)
            .is_some_and(|entry| entry.input.work() != work)
            || registry
                .retired
                .get(&key)
                .is_some_and(|input| input.work() != work)
        {
            return Err(WorkError::Conflict);
        }
        if let Some(entry) = registry.entries.get_mut(&key) {
            if matches!(entry.state, WorkOperationStateV1::Pending { .. })
                && entry.task.as_ref().is_some_and(|task| task.is_finished())
            {
                entry.state = WorkOperationStateV1::Unknown;
            }
        }
        let state = registry
            .entries
            .get(&key)
            .map(|entry| entry.state.clone())
            .unwrap_or(WorkOperationStateV1::Unknown);
        // Explicit acknowledgement frees a finished observation. It is never
        // automatic eviction or permission to repeat the operation.
        if acknowledge && !matches!(state, WorkOperationStateV1::Pending { .. }) {
            if let Some(entry) = registry.entries.get_mut(&key) {
                entry.acknowledged = true;
            }
            registry.reap_acknowledged();
        }
        Ok(state)
    }

    /// Revoke local workers and join their destructors before Shell/Store
    /// shutdown. Attempt drops enqueue abandonment on the original runtime lane;
    /// the normal Shell drain owns native resources and persistence settlement.
    pub(crate) async fn shutdown(&self) -> bool {
        let tasks = {
            let Ok(mut registry) = self.0.lock() else {
                return false;
            };
            registry.closing = true;
            registry
                .entries
                .values_mut()
                .filter_map(|entry| entry.task.take())
                .collect::<Vec<_>>()
        };
        for task in &tasks {
            task.abort();
        }
        let mut clean = true;
        for task in tasks {
            if let Err(error) = task.await {
                clean &= error.is_cancelled();
            }
        }
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::work::{WorkId, WorkRevision};
    fn input(id: u128) -> WorkOperationV1 {
        WorkOperationV1::Plan {
            request: zephium_ipc::work::WorkPlanRequestV1 {
                version: 1,
                work: WorkId::from(id),
                expected_revision: WorkRevision::INITIAL,
                context: None,
            },
        }
    }
    fn queued(id: &str) -> zephium_ipc::OperationAdmission {
        zephium_ipc::OperationAdmission {
            operation_id: Some(id.into()),
            accepted: true,
        }
    }
    fn processed(id: &str, admitted: bool) -> zephium_ipc::OperationDisposition {
        zephium_ipc::OperationDisposition {
            operation_id: id.into(),
            outcome: if admitted {
                zephium_ipc::OperationOutcome::Deferred
            } else {
                zephium_ipc::OperationOutcome::Rejected
            },
            reason: if admitted {
                zephium_ipc::OperationReason::StoreWorkPending
            } else {
                zephium_ipc::OperationReason::StoreAdmissionRejected
            },
        }
    }

    #[tokio::test]
    async fn independent_enablement_blocks_fresh_work_and_preserves_replay_observation() {
        for (ai, work) in [(false, true), (true, false), (false, false)] {
            let owner = WorkOperations::with_enablement(ai, work);
            let key = (1.into(), 2.into());
            assert_eq!(
                owner.admit(key, input(1), async {
                    panic!("disabled request must never poll")
                }),
                Err(WorkError::Unavailable)
            );
            assert_eq!(
                owner.observe(key, 1.into(), false).unwrap(),
                WorkOperationStateV1::Unknown
            );
            assert!(owner.shutdown().await);
        }
        let owner = WorkOperations::default();
        let key = (1.into(), 2.into());
        owner
            .admit(key, input(1), async { WorkOperationStateV1::Unknown })
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while owner.0.lock().unwrap().entries[&key]
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        owner
            .preference("ai.enabled", "false", || queued("pref"))
            .unwrap();
        owner.preference_processed(&processed("pref", true));
        assert_eq!(
            owner
                .admit(key, input(1), async { panic!("replay must never poll") })
                .unwrap(),
            WorkOperationStateV1::Unknown
        );
        owner.observe(key, 1.into(), true).unwrap();
        assert_eq!(
            owner.admit(key, input(1), std::future::pending()).unwrap(),
            WorkOperationStateV1::Unknown
        );
        assert_eq!(
            owner.admit(key, input(2), std::future::pending()),
            Err(WorkError::Conflict)
        );
        assert!(owner.shutdown().await);
    }

    #[tokio::test]
    async fn setting_admission_and_store_refusal_never_publish_false_disabled_state() {
        let owner = WorkOperations::default();
        let rejected = owner
            .preference("ai.enabled", "false", || zephium_ipc::OperationAdmission {
                operation_id: None,
                accepted: false,
            })
            .unwrap();
        assert!(!rejected.accepted);
        assert!(owner.0.lock().unwrap().ai_enabled);
        assert!(owner.0.lock().unwrap().preference.is_none());
        owner
            .preference("ai.enabled", "false", || queued("pref"))
            .unwrap();
        let key = (1.into(), 2.into());
        assert_eq!(
            owner.admit(key, input(1), std::future::pending()),
            Err(WorkError::Unavailable)
        );
        owner.preference_processed(&processed("unrelated", true));
        assert!(owner.0.lock().unwrap().preference.is_some());
        owner.preference_processed(&processed("pref", false));
        assert!(owner.0.lock().unwrap().ai_enabled);
        assert!(owner.0.lock().unwrap().preference.is_none());
        owner.admit(key, input(1), std::future::pending()).unwrap();
        assert!(owner.shutdown().await);
    }

    #[tokio::test]
    async fn active_task_must_finish_before_disabling_even_after_terminal_publication() {
        let owner = WorkOperations::default();
        let key = (1.into(), 2.into());
        owner.admit(key, input(1), std::future::pending()).unwrap();
        // Model the interval between result publication and task destruction.
        owner.0.lock().unwrap().entries.get_mut(&key).unwrap().state =
            WorkOperationStateV1::Unknown;
        owner.observe(key, 1.into(), true).unwrap();
        assert!(owner.0.lock().unwrap().entries.contains_key(&key));
        for setting in ["ai.enabled", "work.enabled"] {
            assert!(matches!(
                owner.preference(setting, "false", || panic!(
                    "active refusal must precede dispatch"
                )),
                Err(WorkError::Conflict)
            ));
        }
        owner.0.lock().unwrap().entries[&key]
            .task
            .as_ref()
            .unwrap()
            .abort();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !owner.0.lock().unwrap().entries[&key]
                .task
                .as_ref()
                .unwrap()
                .is_finished()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        owner
            .preference("work.enabled", "false", || queued("pref"))
            .unwrap();
        assert!(owner.0.lock().unwrap().retired.contains_key(&key));
        assert!(!owner.0.lock().unwrap().entries.contains_key(&key));
        assert!(owner.shutdown().await);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disable_and_fresh_worker_share_one_atomic_admission_gate() {
        let owner = WorkOperations::default();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let admission_owner = owner.clone();
        let admission_barrier = barrier.clone();
        let worker = tokio::spawn(async move {
            admission_barrier.wait();
            admission_owner.admit((1.into(), 2.into()), input(1), std::future::pending())
        });
        let preference_owner = owner.clone();
        let disable = tokio::spawn(async move {
            barrier.wait();
            preference_owner.preference("work.enabled", "false", || queued("pref"))
        });
        let worker = worker.await.unwrap();
        let disable = disable.await.unwrap();
        assert_ne!(
            worker.is_ok(),
            disable.is_ok(),
            "exactly one admission must win"
        );
        // Shutdown must drain without waiting for an unobserved setting result.
        assert!(owner.shutdown().await);
    }
    #[tokio::test]
    async fn duplicate_admission_observes_original_job_and_changed_operands_conflict() {
        let owner = WorkOperations::default();
        let key = (ProfileId::from(1), WorkCommandId::from(2));
        owner.admit(key, input(1), std::future::pending()).unwrap();
        assert!(matches!(
            owner.admit(key, input(1), async { panic!("duplicate must never poll") }),
            Ok(WorkOperationStateV1::Pending { .. })
        ));
        assert_eq!(
            owner.admit(key, input(2), std::future::pending()),
            Err(WorkError::Conflict)
        );
        assert!(owner.shutdown().await);
        assert_eq!(
            owner.admit(key, input(1), std::future::pending()),
            Err(WorkError::Shutdown)
        );
    }
    #[tokio::test]
    async fn bounded_keyed_workers_are_joined_before_shutdown_returns() {
        let owner = WorkOperations::default();
        let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        struct Guard(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        for id in 0..MAX_ACTIVE {
            let guard = Guard(dropped.clone());
            owner
                .admit(
                    (ProfileId::from(1), WorkCommandId::from(id as u128)),
                    input(id as u128),
                    async move {
                        let _guard = guard;
                        std::future::pending().await
                    },
                )
                .unwrap();
        }
        assert_eq!(
            owner.admit(
                (ProfileId::from(2), WorkCommandId::from(1)),
                input(1),
                std::future::pending()
            ),
            Err(WorkError::Capacity)
        );
        assert!(owner.shutdown().await);
        assert_eq!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            MAX_ACTIVE
        );
    }
}
