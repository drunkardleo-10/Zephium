//! A Work page the agent read discovers its icon the way a tab does: the
//! same renderer-side script and decoder, polled while the document settles.
//! Only a fixed 32x32 raster leaves the page, named by the committed URL.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use zephium_agentic::{ContextId, ContextNavigationTarget};
use zephium_core::ports::engine::EngineEvent;

use super::super::scripts::{decode_favicon_eval_result, FAVICON_JS};
use super::super::EngineHost;

/// Delays before each repeat poll after the first evaluation at commit.
pub(super) const AGENT_FAVICON_POLL_DELAYS: [Duration; 6] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_millis(1500),
    Duration::from_millis(2500),
    Duration::from_secs(4),
];

/// One bounded discovery for one committed document. Dropping it with the
/// context cancels the pending poll timer.
pub(super) struct AgentFaviconPoll {
    target: ContextNavigationTarget,
    found: Arc<AtomicBool>,
    timer: Option<crate::platform::imp::ContentPolicyTimeout>,
}

fn discoverable(target: &ContextNavigationTarget) -> bool {
    matches!(target.as_url().scheme(), "http" | "https")
}

impl EngineHost {
    pub(super) fn start_agent_favicon(&mut self, id: ContextId) {
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            return;
        };
        binding.favicon = binding
            .committed_target
            .clone()
            .filter(discoverable)
            .map(|target| AgentFaviconPoll {
                target,
                found: Arc::new(AtomicBool::new(false)),
                timer: None,
            });
        self.poll_agent_favicon(id, 0);
    }

    fn poll_agent_favicon(&mut self, id: ContextId, attempt: usize) {
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            return;
        };
        let Some(poll) = binding.favicon.as_ref() else {
            return;
        };
        if binding.renderer_lost
            || binding.pending_navigation.is_some()
            || binding.committed_target.as_ref() != Some(&poll.target)
            || poll.found.load(Ordering::Acquire)
        {
            binding.favicon = None;
            return;
        }
        let target = poll.target.clone();
        let found = poll.found.clone();
        let _ = binding
            .view
            .view()
            .evaluate_script_with_callback(FAVICON_JS, move |result| {
                let Some(rgba) = decode_favicon_eval_result(&result) else {
                    return;
                };
                if found.swap(true, Ordering::AcqRel) {
                    return;
                }
                let target = target.clone();
                let _ = crate::host::try_with(move |host| {
                    host.accept_agent_favicon(id, &target, rgba);
                });
            });
        let Some(delay) = AGENT_FAVICON_POLL_DELAYS.get(attempt).copied() else {
            binding.favicon = None;
            return;
        };
        let timer = crate::platform::imp::schedule_content_policy_timeout(delay, move || {
            let _ = crate::host::try_with(move |host| host.poll_agent_favicon(id, attempt + 1));
        });
        if let Some(poll) = binding.favicon.as_mut() {
            poll.timer = timer;
        }
    }

    /// The document that answered must still be the committed one, so a
    /// late raster never lands under a later page's origin.
    fn accept_agent_favicon(
        &mut self,
        id: ContextId,
        target: &ContextNavigationTarget,
        rgba: Vec<u8>,
    ) {
        let Some(binding) = self.agent_contexts.get_mut(&id) else {
            return;
        };
        if binding.committed_target.as_ref() != Some(target) {
            return;
        }
        binding.favicon = None;
        let profile = binding.profile();
        self.sink.emit(EngineEvent::WorkPageFavicon {
            profile,
            page_url: target.as_url().as_str().to_owned(),
            rgba,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_is_bounded_and_only_for_web_documents() {
        let total: Duration = AGENT_FAVICON_POLL_DELAYS.iter().sum();
        assert!(total <= Duration::from_secs(10));
        assert!(discoverable(
            &ContextNavigationTarget::parse("https://example.com/a").unwrap()
        ));
    }
}
