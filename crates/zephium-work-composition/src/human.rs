use crate::MacosWorkComposition;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use zephium_agentic::{AgentBrowserHumanReason, WorkBrowserHumanRegion};
use zephium_app::{RetainedHumanPhase, RetainedWorkHandle};
use zephium_core::{
    ids::ProfileId,
    work::{WorkAttemptId, WorkError, WorkId, WorkStepId},
};
use zephium_ipc::work::*;

type Key = (ProfileId, WorkId, WorkAttemptId, WorkStepId);
type Notify = Arc<dyn Fn(WorkHumanChangedV1) + Send + Sync>;
const MAX_RETAINED_HUMAN_PAGES: usize = 3;
#[derive(Default)]
pub(crate) struct HumanPages(Mutex<Pages>);
#[derive(Default)]
struct Pages {
    entries: BTreeMap<Key, Page>,
    notify: Option<Notify>,
}
struct Page {
    handle: RetainedWorkHandle,
    projected: Option<WorkHumanPageV1>,
    account: Option<(u32, WorkHumanAccountV1)>,
    released: bool,
}
pub(crate) struct HumanRegistration<'a> {
    pages: &'a HumanPages,
    key: Key,
}
impl HumanPages {
    pub(crate) fn register(
        &self,
        key: Key,
        handle: RetainedWorkHandle,
    ) -> Result<HumanRegistration<'_>, WorkError> {
        let mut pages = self.0.lock().map_err(|_| WorkError::Unavailable)?;
        if pages.entries.len() >= MAX_RETAINED_HUMAN_PAGES || pages.entries.contains_key(&key) {
            return Err(WorkError::Capacity);
        }
        pages.entries.insert(
            key,
            Page {
                handle,
                projected: None,
                account: None,
                released: false,
            },
        );
        Ok(HumanRegistration { pages: self, key })
    }
}
impl HumanRegistration<'_> {
    pub(crate) fn update(&self) {
        let Ok(mut pages) = self.pages.0.lock() else {
            return;
        };
        let Some(page) = pages.entries.get_mut(&self.key) else {
            return;
        };
        let next = snapshot(page, self.key);
        // Remaining time is computed on reads; it is not an invalidation clock.
        let changed = match (&page.projected, &next) {
            (Some(old), Some(new)) => {
                old.id != new.id
                    || old.phase != new.phase
                    || old.document_revision != new.document_revision
                    || old.can_continue != new.can_continue
            }
            (None, None) => false,
            _ => true,
        };
        page.projected = next;
        let notify = pages.notify.clone();
        drop(pages);
        if changed {
            notify_change(notify, self.key);
        }
    }
}
impl Drop for HumanRegistration<'_> {
    fn drop(&mut self) {
        if let Ok(mut pages) = self.pages.0.lock() {
            if let Some(page) = pages.entries.remove(&self.key) {
                page.handle.close();
            }
            let notify = pages.notify.clone();
            drop(pages);
            notify_change(notify, self.key);
        }
    }
}
fn notify_change(notify: Option<Notify>, key: Key) {
    if let Some(notify) = notify {
        notify(WorkHumanChangedV1 {
            profile: key.0.to_string(),
            work: key.1,
        });
    }
}
fn snapshot(page: &Page, key: Key) -> Option<WorkHumanPageV1> {
    let Some(state) = page.handle.human_snapshot() else {
        let mut prior = page.projected.clone()?;
        prior.phase = if page.released || page.handle.is_closed() {
            WorkHumanPhaseV1::Released
        } else {
            WorkHumanPhaseV1::Reading
        };
        prior.can_continue = false;
        prior.remaining_millis = 0;
        return Some(prior);
    };
    Some(WorkHumanPageV1 {
        id: WorkHumanPageIdV1 {
            attempt: key.2,
            step: key.3,
            generation: state.generation,
        },
        phase: if page.released {
            WorkHumanPhaseV1::Released
        } else {
            match state.phase {
                RetainedHumanPhase::WaitingForHuman => WorkHumanPhaseV1::WaitingForHuman,
                RetainedHumanPhase::Presenting => WorkHumanPhaseV1::Presenting,
                RetainedHumanPhase::Presented => WorkHumanPhaseV1::Presented,
                RetainedHumanPhase::Continuing | RetainedHumanPhase::ReadyToResume => {
                    WorkHumanPhaseV1::Continuing
                }
                RetainedHumanPhase::Released => WorkHumanPhaseV1::Released,
            }
        },
        reason: match state.reason {
            AgentBrowserHumanReason::SignIn => WorkHumanReasonV1::SignIn,
            AgentBrowserHumanReason::HumanChallenge => WorkHumanReasonV1::Challenge,
            AgentBrowserHumanReason::Permission => WorkHumanReasonV1::Permission,
            AgentBrowserHumanReason::Verification => WorkHumanReasonV1::Verification,
            AgentBrowserHumanReason::UserDecision => WorkHumanReasonV1::UserDecision,
            AgentBrowserHumanReason::SensitiveEffect => WorkHumanReasonV1::SensitiveEffect,
            AgentBrowserHumanReason::UnsupportedInteraction => {
                WorkHumanReasonV1::UnsupportedInteraction
            }
        },
        remaining_millis: u32::try_from(
            state
                .deadline
                .saturating_duration_since(Instant::now())
                .as_millis(),
        )
        .unwrap_or(u32::MAX),
        document_revision: state.document_revision.to_string(),
        can_continue: state.can_continue && !page.released,
    })
}
impl MacosWorkComposition {
    pub fn release_presented_human_pages(&self) {
        if let Ok(mut pages) = self.human_pages.0.lock() {
            for page in pages.entries.values_mut() {
                if page.handle.human_snapshot().is_some_and(|state| {
                    matches!(
                        state.phase,
                        RetainedHumanPhase::Presenting
                            | RetainedHumanPhase::Presented
                            | RetainedHumanPhase::Continuing
                    )
                }) {
                    page.released = true;
                    page.handle.close();
                }
            }
        }
    }

    pub fn set_human_page_observer(&self, observer: Notify) {
        if let Ok(mut pages) = self.human_pages.0.lock() {
            pages.notify = Some(observer);
        }
    }
    pub fn human_pages(
        &self,
        profile: ProfileId,
        work: WorkId,
    ) -> Result<Vec<WorkHumanPageV1>, WorkError> {
        let pages = self
            .human_pages
            .0
            .lock()
            .map_err(|_| WorkError::Unavailable)?;
        Ok(pages
            .entries
            .iter()
            .filter(|(key, _)| key.0 == profile && key.1 == work)
            .filter_map(|(key, page)| snapshot(page, *key))
            .collect())
    }
    pub fn present_human_page(
        &self,
        profile: ProfileId,
        work: WorkId,
        id: WorkHumanPageIdV1,
        region: WorkHumanRegionV1,
    ) -> Result<(), WorkError> {
        let region =
            WorkBrowserHumanRegion::try_new(region.x, region.y, region.width, region.height)
                .ok_or(WorkError::Invalid)?;
        self.with_human_page(profile, work, id, |page| {
            page.handle.present_human(id.generation, region)
        })
    }
    pub fn continue_human_page(
        &self,
        profile: ProfileId,
        work: WorkId,
        id: WorkHumanPageIdV1,
        account: WorkHumanAccountV1,
    ) -> Result<(), WorkError> {
        self.with_human_page(profile, work, id, |page| {
            if page.account.is_some()
                || !page
                    .handle
                    .human_snapshot()
                    .is_some_and(|state| state.can_continue)
                || !page.handle.continue_human(id.generation)
            {
                return false;
            }
            page.account = Some((id.generation, account));
            true
        })
    }
    pub fn release_human_page(
        &self,
        profile: ProfileId,
        work: WorkId,
        id: WorkHumanPageIdV1,
    ) -> Result<(), WorkError> {
        self.with_human_page(profile, work, id, |page| {
            page.released = true;
            page.handle.close();
            true
        })
    }
    fn with_human_page(
        &self,
        profile: ProfileId,
        work: WorkId,
        id: WorkHumanPageIdV1,
        action: impl FnOnce(&mut Page) -> bool,
    ) -> Result<(), WorkError> {
        let mut pages = self
            .human_pages
            .0
            .lock()
            .map_err(|_| WorkError::Unavailable)?;
        let page = pages
            .entries
            .get_mut(&(profile, work, id.attempt, id.step))
            .ok_or(WorkError::Unavailable)?;
        if page.released
            || !page.handle.human_snapshot().is_some_and(|state| {
                state.generation == id.generation && Instant::now() < state.deadline
            })
            || !action(page)
        {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}
