//! A separate native top-document sheet; personal hides never enter this cache.
use super::*;
use std::cell::Cell;
use zephium_core::blocker::DeclarativeStyleRules;

pub(super) struct NativeCosmeticState {
    generation: ContentPolicyGeneration,
    wanted: [u8; 32],
    applied: Option<Rc<crate::platform::imp::NativeContentPolicy>>,
}
#[derive(Default)]
pub(super) struct NativeStyleDocument {
    pub(super) current: Cell<Option<[u8; 32]>>,
    pending: Cell<Option<[u8; 32]>>,
    pub(super) committed: Cell<Option<[u8; 32]>>,
}
impl NativeStyleDocument {
    pub(super) fn started(&self, paused: bool) {
        self.pending
            .set(if paused { None } else { self.current.get() });
    }
    pub(super) fn redirected(&self) {
        self.pending.set(None);
    }
    pub(super) fn committed(&self) {
        self.committed.set(self.pending.take());
    }
}
impl EngineHost {
    pub(super) fn request_native_cosmetics(
        &mut self,
        profile: ProfileId,
        generation: ContentPolicyGeneration,
        styles: Option<DeclarativeStyleRules>,
    ) {
        let Some(styles) = styles else {
            self.native_cosmetics.remove(&profile);
            for waiters in self.cosmetic_compilations.values_mut() {
                waiters.retain(|(owner, _)| *owner != profile);
            }
            for (id, view) in &mut self.views {
                if self
                    .partitions
                    .get(id)
                    .is_some_and(|p| p.profile() == profile)
                {
                    view.cosmetic_registration.take();
                    view.native_style_document.current.set(None);
                }
            }
            if let Some(spare) = self
                .spare
                .as_mut()
                .filter(|s| s.partition.profile() == profile)
            {
                spare.view.cosmetic_registration.take();
                spare.view.native_style_document.current.set(None);
            }
            return;
        };
        let digest = *styles.digest().as_bytes();
        let previous = self
            .native_cosmetics
            .remove(&profile)
            .and_then(|state| state.applied);
        let current = previous
            .as_ref()
            .and_then(|p| crate::platform::imp::content_policy_digest(p));
        self.native_cosmetics.insert(
            profile,
            NativeCosmeticState {
                generation,
                wanted: digest,
                applied: previous,
            },
        );
        if current == Some(digest) {
            return;
        }
        if let Some(cached) = self
            .declarative_content_policy_cache
            .get(&digest)
            .and_then(std::rc::Weak::upgrade)
        {
            self.finish_native_cosmetics(profile, generation, digest, Some(cached));
            return;
        }
        if !self.queue_native_artifact(styles.encoded().clone(), digest) {
            return;
        }
        let waiters = self.cosmetic_compilations.entry(digest).or_default();
        waiters.retain(|(owner, _)| *owner != profile);
        if waiters.len() < zephium_core::session::MAX_SESSION_PROFILES {
            waiters.push((profile, generation));
        }
        self.start_next_declarative_content_policy_compilation();
    }
    pub(super) fn finish_native_cosmetics(
        &mut self,
        profile: ProfileId,
        generation: ContentPolicyGeneration,
        digest: [u8; 32],
        native: Option<Rc<crate::platform::imp::NativeContentPolicy>>,
    ) {
        if self
            .native_cosmetics
            .get(&profile)
            .is_none_or(|s| s.generation != generation || s.wanted != digest)
        {
            return;
        }
        let Some(native) = native else { return };
        let mut registrations = Vec::new();
        for (id, view) in &self.views {
            if self
                .partitions
                .get(id)
                .is_some_and(|p| p.profile() == profile)
            {
                let Ok(registration) = crate::platform::imp::install_scoped_content_policy_on_view(
                    view,
                    &native,
                    &view.site_scope.pause,
                ) else {
                    return;
                };
                registrations.push((*id, registration));
            }
        }
        let spare = if let Some(spare) = self
            .spare
            .as_ref()
            .filter(|s| s.partition.profile() == profile)
        {
            let Ok(registration) = crate::platform::imp::install_scoped_content_policy_on_view(
                &spare.view,
                &native,
                &spare.view.site_scope.pause,
            ) else {
                return;
            };
            Some(registration)
        } else {
            None
        };
        for (id, registration) in registrations {
            if let Some(view) = self.views.get_mut(&id) {
                view.cosmetic_registration = Some(registration);
                view.native_style_document.current.set(Some(digest));
            }
        }
        if let (Some(spare), Some(registration)) = (
            self.spare
                .as_mut()
                .filter(|s| s.partition.profile() == profile),
            spare,
        ) {
            spare.view.cosmetic_registration = Some(registration);
            spare.view.native_style_document.current.set(Some(digest));
        }
        if let Some(state) = self.native_cosmetics.get_mut(&profile) {
            state.applied = Some(native);
        }
    }
    pub(super) fn native_cosmetic_policy(
        &self,
        profile: ProfileId,
    ) -> Option<Rc<crate::platform::imp::NativeContentPolicy>> {
        self.native_cosmetics.get(&profile)?.applied.clone()
    }
    pub(super) fn native_cosmetic_digest(&self, profile: ProfileId) -> Option<[u8; 32]> {
        let state = self.native_cosmetics.get(&profile)?;
        let applied = state
            .applied
            .as_ref()
            .and_then(|p| crate::platform::imp::content_policy_digest(p));
        (applied == Some(state.wanted)).then_some(state.wanted)
    }
}
