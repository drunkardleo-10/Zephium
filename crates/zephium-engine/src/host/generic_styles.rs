//! Bounded generic matching, pulled by native and resolved on the style worker.
//! A visible page owns at most one asynchronous request; DOM mutations resolve
//! it. Unchanged or hidden pages do not cause periodic native/renderer work.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod supported {
    use super::super::content_styles::{DocumentStyleState, StyleKey};
    use super::super::dispatch::with_generic_style;
    use super::super::{permits::EventPermit, EngineHost};
    use crate::navigation_epoch::NavigationEpochTracker;
    use std::sync::{Arc, Mutex};
    use zephium_core::ids::ItemId;

    struct Pull {
        id: ItemId,
        state: Arc<DocumentStyleState>,
        key: StyleKey,
        fingerprint: String,
        visibility_revision: u64,
        charged_bytes: usize,
        token: String,
        navigation: NavigationEpochTracker,
        permit: EventPermit,
        provider: Arc<dyn zephium_core::blocker::DocumentStyleProvider>,
    }
    impl Drop for Pull {
        fn drop(&mut self) {
            super::super::content_styles::release_script(self.charged_bytes);
            self.state.end_generic();
        }
    }
    impl Pull {
        fn attributed(&self, host: &EngineHost) -> bool {
            host.views.get(&self.id).is_some_and(|view| {
                super::super::permits::navigation_callback_matches(
                    &view.event_permit,
                    &view.navigation,
                    &self.permit,
                    &self.navigation,
                    self.key.epoch,
                ) && self
                    .navigation
                    .matches_committed_snapshot(self.key.epoch, &self.key.url)
                    && self.state.generic_current(&self.key, &self.fingerprint)
            })
        }
    }
    struct Batch {
        token: String,
        url: String,
        subscription: String,
        serial: u64,
        tokens: Vec<String>,
    }
    fn decode(text: &str) -> Option<Batch> {
        if text.len() > 128 * 1024 {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let object = value.as_object()?;
        if object.len() != 5 {
            return None;
        }
        let tokens = object.get("tokens")?.as_array()?;
        if tokens.len() > 256 {
            return None;
        }
        let batch = Batch {
            token: object.get("token")?.as_str()?.to_owned(),
            url: object.get("url")?.as_str()?.to_owned(),
            subscription: object.get("subscription")?.as_str()?.to_owned(),
            serial: object.get("serial")?.as_u64()?,
            tokens: tokens
                .iter()
                .map(|token| token.as_str().map(str::to_owned))
                .collect::<Option<_>>()?,
        };
        if batch.token.len() != 32
            || !batch.token.bytes().all(|b| b.is_ascii_hexdigit())
            || batch.url.len() > 32768
            || batch.subscription.len() != 64
            || !batch.subscription.bytes().all(|b| b.is_ascii_hexdigit())
            || batch.serial == 0
            || batch.serial > 9_007_199_254_740_991
            || batch.tokens.is_empty()
            || batch.tokens.len() > 256
            || batch.tokens.iter().map(String::len).sum::<usize>() > 65536
            || batch.tokens.iter().any(|key| {
                key.len() < 2
                    || key.len() > 4097
                    || !matches!(key.as_bytes()[0], b'.' | b'#')
                    || !key.as_bytes()[1..]
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            })
        {
            return None;
        }
        Some(batch)
    }
    impl EngineHost {
        fn generic_visible(&self, id: ItemId) -> bool {
            self.stages.values().any(|stage| stage.wants_visible(id))
        }
        pub(crate) fn refresh_generic_styles(&mut self, id: ItemId) {
            let visible = self.generic_visible(id);
            if let Some(view) = self.views.get(&id) {
                view.content_styles.generic_visibility(visible);
            }
            if !visible {
                return;
            }
            let Some(profile) = self.partitions.get(&id).map(|p| p.profile()) else {
                return;
            };
            if self.erasure_tombstones.contains(&profile) {
                return;
            }
            let Some(view) = self.views.get(&id).filter(|view| view.presentable) else {
                return;
            };
            let Some(provider) = self
                .content_policies
                .get(&profile)
                .and_then(|p| p.applied.as_ref())
                .and_then(|p| p.cosmetics.clone())
            else {
                return;
            };
            let state = view.content_styles.clone();
            let Some((key, fingerprint, token, visibility_revision)) = state.begin_generic() else {
                return;
            };
            if key.subscription != Some(provider.fingerprint())
                || !view
                    .navigation
                    .matches_committed_snapshot(key.epoch, &key.url)
            {
                state.end_generic();
                return;
            }
            let pull = Pull {
                id,
                state,
                key,
                fingerprint,
                visibility_revision,
                charged_bytes: 0,
                token,
                navigation: view.navigation.clone(),
                permit: view.event_permit.clone(),
                provider,
            };
            let params = serde_json::json!([pull.token, pull.key.url, pull.fingerprint]);
            let expression = format!("((p)=>{{const a=globalThis.__zephium_content_style_v1__;return a?a.pullGeneric(p[0],p[1],p[2]):null;}})({params})");
            let pending = Mutex::new(Some(pull));
            // The platform call never blocks its native UI thread. It returns
            // a JSON string only when new bounded tokens exist, or cancellation.
            let _ =
                crate::platform::cosmetic_pull::evaluate(&view.view, &expression, move |result| {
                    let Some(pull) = pending.lock().unwrap_or_else(|p| p.into_inner()).take()
                    else {
                        return;
                    };
                    let _ = with_generic_style(id, move |host| host.generic_tokens(pull, result));
                });
        }
        pub(crate) fn refresh_visible_generic_styles(&mut self) {
            let ids: Vec<_> = self.views.keys().copied().collect();
            for id in ids {
                self.refresh_generic_styles(id);
            }
        }
        fn generic_tokens(&mut self, pull: Pull, result: Option<String>) {
            if !pull.attributed(self) {
                let id = pull.id;
                drop(pull);
                self.refresh_generic_styles(id);
                return;
            }
            if !self.generic_visible(pull.id) {
                return;
            }
            let Some(text) = result else {
                let id = pull.id;
                let changed = pull.state.generic_visibility(true) != pull.visibility_revision;
                drop(pull);
                if changed {
                    self.refresh_generic_styles(id);
                }
                return;
            };
            let Some(worker) = &self.style_worker else {
                return;
            };
            worker.submit(move || {
                let mut pull = pull;
                if !pull.navigation.matches_committed_snapshot(pull.key.epoch, &pull.key.url)
                    || !pull.state.generic_current(&pull.key, &pull.fingerprint) { return None; }
                let batch = decode(&text)?;
                if batch.token != pull.token || batch.url != pull.key.url
                    || batch.subscription != pull.fingerprint { return None; }
                let selectors = pull.provider.generic_selectors(&pull.key.url, &batch.tokens).ok()?;
                let params = serde_json::json!([pull.token, pull.key.url, pull.fingerprint, batch.serial, selectors]);
                let script = format!("((p)=>{{const a=globalThis.__zephium_content_style_v1__;return !!a&&a.applyGeneric(p[0],p[1],p[2],p[3],p[4]);}})({params})");
                if script.len() > 2 * 1024 * 1024 { return None; }
                pull.charged_bytes = super::super::content_styles::charge_script(script.len())?;
                Some(Box::new(move || { let id = pull.id;
                    let _ = with_generic_style(id, move |host| host.deliver_generic(pull, script));
                }))
            });
        }
        fn deliver_generic(&mut self, pull: Pull, script: String) {
            let id = pull.id;
            if !pull.attributed(self) {
                drop(pull);
                self.refresh_generic_styles(id);
                return;
            }
            if !self.generic_visible(id) {
                return;
            }
            let Some(view) = self.views.get(&id) else {
                return;
            };
            let pending = Mutex::new(Some(pull));
            let _ = view.evaluate_script_with_callback(&script, move |result| {
                let Some(pull) = pending.lock().unwrap_or_else(|p| p.into_inner()).take() else {
                    return;
                };
                let _ = with_generic_style(id, move |host| {
                    let current = pull.attributed(host);
                    drop(pull);
                    if result == "true" || !current {
                        host.refresh_generic_styles(id);
                    }
                });
            });
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn token_batches_are_bounded_and_not_a_selector_or_command_channel() {
            let valid = serde_json::json!({"token":"a".repeat(32),"url":"https://example.com/",
                "subscription":"b".repeat(64),"serial":1,"tokens":[".ad","#slot"]});
            assert!(decode(&valid.to_string()).is_some());
            for tokens in [
                serde_json::json!([".ad{}"]),
                serde_json::json!(["#x\ny"]),
                serde_json::json!(vec![".x"; 257]),
                serde_json::json!([]),
            ] {
                let mut bad = valid.clone();
                bad["tokens"] = tokens;
                assert!(decode(&bad.to_string()).is_none());
            }
            let mut bad = valid;
            bad["serial"] = serde_json::json!(0);
            assert!(decode(&bad.to_string()).is_none());
        }
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl super::EngineHost {
    pub(crate) fn refresh_generic_styles(&mut self, _: zephium_core::ids::ItemId) {}
    pub(crate) fn refresh_visible_generic_styles(&mut self) {}
}
