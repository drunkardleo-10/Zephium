//! Bounded observers of original attempts. No resource, task or execution owner.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use zephium_app::work_runtime::{WorkAttemptObserver, WorkPageFrame};
use zephium_core::ids::ProfileId;
use zephium_core::work::{runtime::*, WorkAttemptId, WorkExecutionId, WorkId, WorkStepId};
use zephium_ipc::work::{WorkPageFrameV1, WorkPageV1, WorkSignalV1};
use zephium_store::{WorkFrameRecord, WorkFrameStore};

const MAX_PAGE_ATTEMPTS: usize = 8;
const MAX_STORED_WORKS: usize = 8;

pub(crate) struct WorkActivity {
    observers: Mutex<BTreeMap<WorkAttemptId, WorkAttemptObserver>>,
    /// Pages of recent attempts, kept after the attempt ends so their last
    /// frame outlives the run for this app session.
    pages: Mutex<BTreeMap<WorkAttemptId, (WorkExecutionId, Vec<WorkPageFrame>)>>,
    frames: Option<Arc<WorkFrameStore>>,
    /// Frames already written this session, by attempt, step and generation.
    persisted: Mutex<BTreeSet<(WorkAttemptId, WorkStepId, u64)>>,
    /// Stored frames of recently read works, listed once per session.
    stored: Mutex<BTreeMap<WorkId, (ProfileId, Vec<WorkFrameRecord>)>>,
}
impl WorkActivity {
    pub(crate) fn new(frames: Option<Arc<WorkFrameStore>>) -> Self {
        Self {
            observers: Mutex::default(),
            pages: Mutex::default(),
            frames,
            persisted: Mutex::default(),
            stored: Mutex::default(),
        }
    }
    #[cfg(target_os = "macos")]
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
            let mut opened = observer.pages();
            if opened.is_empty() {
                continue;
            }
            // A page a long run let go keeps its last frame until it is stored.
            if let (Some((_, known)), Ok(persisted)) =
                (pages.get(&observer.attempt()), self.persisted.lock())
            {
                let dropped: Vec<WorkPageFrame> = known
                    .iter()
                    .filter(|page| {
                        !opened.iter().any(|open| open.step == page.step)
                            && page.frame.as_ref().is_some_and(|frame| {
                                !persisted.contains(&(
                                    observer.attempt(),
                                    page.step,
                                    frame.generation,
                                ))
                            })
                    })
                    .cloned()
                    .map(|page| WorkPageFrame {
                        live: false,
                        ..page
                    })
                    .collect();
                opened.extend(dropped);
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
        let profile = state.work.profile;
        let work = state.work.id;
        let mut live: Vec<WorkPageV1> = pages
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
                    part: None,
                    title: None,
                })
            })
            .collect();
        self.persist(profile, work, &pages);
        let mut stored = self.stored_records(profile, work);
        stored.retain(|record| {
            executions.contains(&record.execution)
                && !live
                    .iter()
                    .any(|page| page.attempt == record.attempt && page.step == record.step)
        });
        live.extend(stored.into_iter().map(|record| WorkPageV1 {
            execution: record.execution,
            attempt: record.attempt,
            step: record.step,
            url: record.url,
            live: false,
            frame: Some(WorkPageFrameV1 {
                generation: record.generation,
                width: record.width,
                height: record.height,
            }),
            part: None,
            title: None,
        }));
        owned(state, &mut live);
        live
    }
    /// Writes the last frame of every settled page once, then remembers it.
    fn persist(
        &self,
        profile: ProfileId,
        work: WorkId,
        pages: &BTreeMap<WorkAttemptId, (WorkExecutionId, Vec<WorkPageFrame>)>,
    ) {
        let Some(frames) = &self.frames else {
            return;
        };
        let Ok(mut persisted) = self.persisted.lock() else {
            return;
        };
        for (attempt, (execution, opened)) in pages {
            for page in opened.iter().filter(|page| !page.live) {
                let Some(frame) = &page.frame else {
                    continue;
                };
                let key = (*attempt, page.step, frame.generation);
                if persisted.contains(&key) {
                    continue;
                }
                persisted.insert(key);
                let record = WorkFrameRecord {
                    work,
                    execution: *execution,
                    attempt: *attempt,
                    step: page.step,
                    url: page.url.clone(),
                    generation: u32::try_from(frame.generation).unwrap_or(u32::MAX),
                    width: frame.width,
                    height: frame.height,
                };
                if frames.put(profile, &record, &frame.png).is_ok() {
                    if let Ok(mut stored) = self.stored.lock() {
                        if let Some((_, records)) = stored.get_mut(&work) {
                            records.retain(|r| {
                                !(r.attempt == record.attempt && r.step == record.step)
                            });
                            records.push(record);
                        }
                    }
                }
            }
        }
    }
    fn stored_records(&self, profile: ProfileId, work: WorkId) -> Vec<WorkFrameRecord> {
        let Some(frames) = &self.frames else {
            return Vec::new();
        };
        let Ok(mut stored) = self.stored.lock() else {
            return Vec::new();
        };
        if let Some((_, records)) = stored.get(&work) {
            return records.clone();
        }
        if stored.len() >= MAX_STORED_WORKS {
            let oldest = stored.keys().next().copied();
            if let Some(oldest) = oldest {
                stored.remove(&oldest);
            }
        }
        let records = frames.list(profile, work);
        stored.insert(work, (profile, records.clone()));
        records
    }
    pub(crate) fn frame(&self, attempt: WorkAttemptId, step: WorkStepId) -> Option<Arc<Vec<u8>>> {
        if let Some(png) = self.pages.lock().ok().and_then(|pages| {
            pages
                .get(&attempt)?
                .1
                .iter()
                .find(|page| page.step == step)?
                .frame
                .as_ref()
                .map(|frame| frame.png.clone())
        }) {
            return Some(png);
        }
        let frames = self.frames.as_ref()?;
        let stored = self.stored.lock().ok()?;
        let (profile, _) = stored.values().find(|(_, records)| {
            records
                .iter()
                .any(|record| record.attempt == attempt && record.step == step)
        })?;
        frames.read(*profile, attempt, step).map(Arc::new)
    }
}

/// Each page's reader and title, from its step: the part it works for, or
/// none for the lead's own read.
fn owned(state: &WorkRuntimeProjection, pages: &mut [WorkPageV1]) {
    for page in pages {
        let Some(step) = state
            .executions
            .iter()
            .find(|execution| execution.id == page.execution)
            .and_then(|execution| execution.steps.iter().find(|step| step.id == page.step))
        else {
            continue;
        };
        page.part = step.part;
        page.title = step
            .local
            .as_ref()
            .and_then(|local| local.page_title.clone())
            .filter(|_| step.account.is_none());
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
