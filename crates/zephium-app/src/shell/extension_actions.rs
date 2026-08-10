//! Shell ownership of effective extension toolbar actions.

use std::collections::HashMap;

use zephium_core::extensions::{
    ExtensionActionSnapshot, ExtensionActionSnapshotSettlement, ExtensionBrowserSurface,
    ExtensionBrowserSurfaceGeneration,
};
use zephium_core::ids::{ItemId, ProfileId};
use zephium_core::ports::engine::NativeDispatch;

use super::{NativeWork, Shell};

#[derive(Default)]
pub(super) struct ExtensionActionState {
    snapshots: HashMap<ProfileId, ExtensionActionSnapshot>,
    // Also acts as a bounded pending-settlement watchdog. A scheduled read
    // remains here until an exact applied result arrives, so the ordinary
    // maintenance tick repairs a dropped callback without a hot timer.
    retry_profiles: zephium_core::ports::extensions::ExtensionActiveProfiles,
}

impl ExtensionActionState {
    pub(super) fn retire_profile(&mut self, profile: ProfileId) {
        self.snapshots.remove(&profile);
        self.retry_profiles.remove(profile);
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self, profile: ProfileId) -> Option<&ExtensionActionSnapshot> {
        self.snapshots.get(&profile)
    }

    pub(super) fn observe(
        &mut self,
        current_surface: Option<&ExtensionBrowserSurface>,
        profile: ProfileId,
        tab: ItemId,
        surface_generation: ExtensionBrowserSurfaceGeneration,
        settlement: ExtensionActionSnapshotSettlement,
    ) -> ExtensionActionObservation {
        let current = current_surface.is_some_and(|surface| {
            surface.profile() == profile
                && surface.generation() == surface_generation
                && surface
                    .windows()
                    .first()
                    .and_then(zephium_core::extensions::ExtensionBrowserWindow::active)
                    == Some(tab)
        });
        if !current {
            return ExtensionActionObservation::Stale;
        }
        let ExtensionActionSnapshotSettlement::Applied(snapshot) = settlement else {
            return if self.record_refresh(profile) {
                ExtensionActionObservation::Retained
            } else {
                ExtensionActionObservation::CapacityExceeded
            };
        };
        if snapshot.profile() != profile
            || snapshot.tab() != tab
            || snapshot.surface_generation() != surface_generation
        {
            let _ = self.record_refresh(profile);
            return ExtensionActionObservation::Contradictory;
        }
        self.retry_profiles.remove(profile);
        if self
            .snapshots
            .get(&profile)
            .is_some_and(|previous| previous == &snapshot)
        {
            return ExtensionActionObservation::Unchanged;
        }
        self.snapshots.insert(profile, snapshot);
        ExtensionActionObservation::Applied
    }

    fn record_refresh(&mut self, profile: ProfileId) -> bool {
        self.retry_profiles.try_insert(profile)
    }

    fn retry_profiles(&self) -> zephium_core::ports::extensions::ExtensionActiveProfiles {
        self.retry_profiles
    }

    fn clear_projection(&mut self, profile: ProfileId) {
        self.retry_profiles.remove(profile);
        self.snapshots.remove(&profile);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExtensionActionObservation {
    Applied,
    Unchanged,
    Retained,
    Stale,
    Contradictory,
    CapacityExceeded,
}

impl Shell {
    /// Schedules one bounded read for the exact currently-published active
    /// tab. The request is independent of browser-surface publication: action
    /// backpressure must not turn a successfully-routed content view into a
    /// failed browser mutation.
    pub(super) fn refresh_extension_actions(&mut self, profile: ProfileId) -> NativeWork {
        let target = self
            .extension_browser_surfaces
            .published_surface(profile)
            .and_then(|surface| {
                surface
                    .windows()
                    .first()
                    .and_then(zephium_core::extensions::ExtensionBrowserWindow::active)
                    .map(|tab| (tab, surface.generation()))
            });
        let Some((tab, generation)) = target else {
            self.extension_actions.clear_projection(profile);
            return NativeWork::default();
        };
        let mut native = NativeWork::default();
        if !self.extension_actions.record_refresh(profile) {
            native.rejected = true;
            return native;
        }
        let admission = self
            .engine
            .request_extension_actions(profile, tab, generation);
        native.record(admission);
        if admission == NativeDispatch::Unsupported {
            self.extension_actions.clear_projection(profile);
        }
        native
    }

    /// Retries only reads that were synchronously refused, terminally
    /// rejected, or lost after native scheduling. With no pending work this is
    /// a fixed-size empty iteration and performs no allocation or dispatch.
    pub(super) fn retry_extension_actions(&mut self) -> NativeWork {
        let profiles = self.extension_actions.retry_profiles();
        let mut native = NativeWork::default();
        for profile in profiles.iter() {
            native.merge(self.refresh_extension_actions(profile));
        }
        native
    }
}
