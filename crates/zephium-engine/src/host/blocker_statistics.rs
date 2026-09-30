use super::*;
use zephium_core::blocker::BlockedLoadCounter;

impl EngineHost {
    pub(crate) fn set_blocker_statistics(
        &mut self,
        profile: ProfileId,
        counter: BlockedLoadCounter,
    ) {
        if self.erasure_tombstones.contains(&profile)
            || self.blocker_statistics.len() >= zephium_core::session::MAX_SESSION_PROFILES
        {
            return;
        }
        self.blocker_statistics.entry(profile).or_insert(counter);
    }
    pub(crate) fn collect_blocker_statistics(&self, profile: ProfileId, reset: bool) {
        let Some(counter) = self.blocker_statistics.get(&profile) else {
            return;
        };
        counter.1.store(false, std::sync::atomic::Ordering::Relaxed);
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            for (id, view) in &self.views {
                if self
                    .partitions
                    .get(id)
                    .is_some_and(|p| p.profile() == profile)
                {
                    view.collect_content_block_counter(reset);
                }
            }
            if let Some(spare) = self
                .spare
                .as_ref()
                .filter(|s| s.partition.profile() == profile)
            {
                spare.view.collect_content_block_counter(reset);
            }
        }
        if reset {
            counter.0.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
