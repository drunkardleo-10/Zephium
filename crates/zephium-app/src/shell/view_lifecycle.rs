//! Bounded native-view residency, discard, crash, and process recovery.

use super::*;

// A normal browsing set stays warm up to the soft target. Above the pressure
// watermark, hidden pages enter bounded exact-safety probing before the long
// idle grace. Eight additional slots cover the largest visible split/recovery
// batch before the absolute logical admission ceiling. Pages are never force-
// discarded to make room: unsafe pages remain resident and excess creates
// fail synchronously as ordinary hibernated tabs in the model.
pub(super) const LIVE_VIEW_SOFT_LIMIT: usize = 12;
pub(super) const LIVE_VIEW_PRESSURE_LIMIT: usize = 24;
pub(super) const LIVE_VIEW_ABSOLUTE_LIMIT: usize = LIVE_VIEW_PRESSURE_LIMIT + MAX_VISIBLE_PANES;
pub(super) const MAX_CONCURRENT_DISCARD_PROBES: usize = 4;
pub(super) const DISCARD_IDLE_GRACE: std::time::Duration = std::time::Duration::from_secs(15 * 60);
pub(super) const DISCARD_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
pub(super) const DISCARD_PROTECTED_RETRY: std::time::Duration =
    std::time::Duration::from_secs(5 * 60);

#[derive(Clone)]
pub(super) enum PendingDiscardProbe {
    Probing {
        probe: DiscardProbeId,
        committed_url: String,
        deadline: std::time::Instant,
    },
    Closing {
        probe: DiscardProbeId,
        recreate: bool,
        deferred_navigation: Option<String>,
    },
}

pub(super) struct ResidencyState {
    pub(super) recent: Vec<ItemId>,
    pub(super) last_focus: std::collections::HashMap<ItemId, std::time::Instant>,
    pub(super) dormant_min: std::time::Duration,
    pub(super) dormant_sent: Vec<ItemId>,
    pub(super) discard_idle_min: std::time::Duration,
    pub(super) discard_probe_timeout: std::time::Duration,
    pub(super) discard_protected_retry: std::time::Duration,
    pub(super) live_view_soft_limit: usize,
    pub(super) live_view_pressure_limit: usize,
    pub(super) next_discard_probe: u64,
    pub(super) discard_probes: std::collections::HashMap<ItemId, PendingDiscardProbe>,
    pub(super) discard_protected_until: std::collections::HashMap<ItemId, std::time::Instant>,
}

impl Default for ResidencyState {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            last_focus: std::collections::HashMap::new(),
            dormant_min: std::time::Duration::from_secs(5 * 60),
            dormant_sent: Vec::new(),
            discard_idle_min: DISCARD_IDLE_GRACE,
            discard_probe_timeout: DISCARD_PROBE_TIMEOUT,
            discard_protected_retry: DISCARD_PROTECTED_RETRY,
            live_view_soft_limit: LIVE_VIEW_SOFT_LIMIT,
            live_view_pressure_limit: LIVE_VIEW_PRESSURE_LIMIT,
            next_discard_probe: 0,
            discard_probes: std::collections::HashMap::new(),
            discard_protected_until: std::collections::HashMap::new(),
        }
    }
}

#[derive(Default)]
pub(super) struct CrashState {
    pub(super) crashes: std::collections::HashMap<ItemId, std::time::Instant>,
    pub(super) presentations: std::collections::HashSet<ItemId>,
}

impl Shell {
    pub(super) fn on_view_creation_failed(&mut self, id: ItemId) {
        self.zoom.pending.remove(&id);
        self.cancel_pending_presentation(id);
        self.cancel_discard_probe(id);
        self.items.view_creation_failed(id);
        let split_collapsed = self.collapse_failed_split_leaf(id);
        if split_collapsed {
            self.schedule_persist();
        }
        let _ = self.relayout();
        if split_collapsed {
            self.project_items();
        } else {
            self.project_tab(id);
        }
    }

    // Sleeping-tabs model: hidden views carry a low-memory hint, then a
    // bounded set of exact-generation/epoch probes may authorize full native
    // discard. Positive renderer results are only advisory until this actor
    // rechecks URL, loading, visibility, idle age, and the current budget.
    pub(super) fn maintain_views(&mut self) -> bool {
        self.crash
            .crashes
            .retain(|id, _| self.items.tab(*id).is_some());
        self.crash
            .presentations
            .retain(|id| self.items.tab(*id).is_some());
        self.zoom
            .pending
            .retain(|id, _| self.items.tab(*id).is_some_and(TabState::has_view));
        if self.windows.focused().is_none() {
            return false;
        }
        let shown: std::collections::HashSet<ItemId> = if self.window_visible {
            self.pane_tree()
                .map(|t| t.tabs().into_iter().collect())
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };
        self.residency
            .recent
            .retain(|id| self.items.tab(*id).is_some());
        self.residency
            .last_focus
            .retain(|id, _| self.items.tab(*id).is_some());
        self.last_visits
            .retain(|id, _| self.items.tab(*id).is_some());
        self.residency.discard_protected_until.retain(|id, until| {
            self.items.tab(*id).is_some() && *until > std::time::Instant::now()
        });

        // A timeout command is ordinary coalescible work and may be displaced
        // during a full lifecycle burst. The maintenance pass independently
        // expires the same fixed set so overload cannot strand all probe slots.
        let expired: Vec<ItemId> = self
            .residency
            .discard_probes
            .iter()
            .filter_map(|(id, state)| match state {
                PendingDiscardProbe::Probing { deadline, .. }
                    if *deadline <= std::time::Instant::now() =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        for id in expired {
            self.residency.discard_probes.remove(&id);
            if let Some(queue) = &self.self_queue {
                queue.cancel_discard_probe(id);
            }
            self.protect_discard_candidate(id);
        }

        // Conceptual visible leaves remain protected even while the OS window
        // is minimized. Dormancy may hide them, but discard must not destroy a
        // split the user expects to reappear atomically.
        let protected = self.discard_protected_leaves();
        let live_count = self.items.view_ids().len();
        let invalid_probes: Vec<ItemId> = self
            .residency
            .discard_probes
            .iter()
            .filter_map(|(id, state)| match state {
                PendingDiscardProbe::Probing { committed_url, .. } => {
                    let valid = live_count > self.residency.live_view_soft_limit
                        && !protected.contains(id)
                        && self.items.tab(*id).is_some_and(|tab| {
                            tab.has_view()
                                && !tab.loading
                                && tab
                                    .url
                                    .as_ref()
                                    .is_some_and(|url| url.as_str() == committed_url)
                        });
                    (!valid).then_some(*id)
                }
                PendingDiscardProbe::Closing { .. } => self.items.tab(*id).is_none().then_some(*id),
            })
            .collect();
        for id in invalid_probes {
            if self.items.tab(id).is_none() {
                self.residency.discard_probes.remove(&id);
                if let Some(queue) = &self.self_queue {
                    queue.cancel_discard_probe(id);
                }
            } else {
                self.cancel_discard_probe(id);
            }
        }

        let mut dormant: Vec<ItemId> = self
            .items
            .view_ids()
            .into_iter()
            .filter(|id| {
                !shown.contains(id)
                    && !self.residency.discard_probes.contains_key(id)
                    && self.idle_for(*id, self.residency.dormant_min)
            })
            .collect();
        dormant.sort();
        if dormant != self.residency.dormant_sent {
            self.residency.dormant_sent = dormant.clone();
            self.engine.set_dormant(dormant);
        }

        if live_count <= self.residency.live_view_soft_limit {
            return false;
        }
        let mut candidates: Vec<ItemId> = self
            .items
            .view_ids()
            .into_iter()
            .filter(|id| {
                !protected.contains(id)
                    && !self.residency.discard_probes.contains_key(id)
                    && self
                        .residency
                        .discard_protected_until
                        .get(id)
                        .is_none_or(|until| *until <= std::time::Instant::now())
                    && self.items.tab(*id).is_some_and(|tab| {
                        tab.has_view()
                            && !tab.loading
                            && tab.url.is_some()
                            && (live_count > self.residency.live_view_pressure_limit
                                || self.idle_for(*id, self.residency.discard_idle_min))
                    })
            })
            .collect();
        candidates.sort_by_key(|id| (self.residency.last_focus.get(id).copied(), *id));

        let available =
            MAX_CONCURRENT_DISCARD_PROBES.saturating_sub(self.residency.discard_probes.len());
        for id in candidates.into_iter().take(available) {
            let Some(next) = self.residency.next_discard_probe.checked_add(1) else {
                // Reusing a process-local correlation id could accept a very
                // late callback. Saturation permanently disables new probes.
                break;
            };
            self.residency.next_discard_probe = next;
            let probe = DiscardProbeId(next);
            let deadline = std::time::Instant::now() + self.residency.discard_probe_timeout;
            let Some(committed_url) = self
                .items
                .tab(id)
                .and_then(|tab| tab.url.as_ref())
                .map(ToString::to_string)
            else {
                continue;
            };
            self.residency.discard_probes.insert(
                id,
                PendingDiscardProbe::Probing {
                    probe,
                    committed_url,
                    deadline,
                },
            );
            // A suspended WebView2 cannot reliably execute the DOM query.
            // Replace the desired dormant set first; main-thread FIFO then
            // guarantees resume is requested before the probe evaluation.
            if self.residency.dormant_sent.contains(&id) {
                self.residency.dormant_sent.retain(|dormant| *dormant != id);
                self.engine.set_dormant(self.residency.dormant_sent.clone());
            }
            if !self.engine.probe_discard_safety(id, probe) {
                self.residency.discard_probes.remove(&id);
                self.protect_discard_candidate(id);
                continue;
            }
            if let Some(queue) = &self.self_queue {
                queue.schedule_discard_probe(id, probe, deadline);
            }
        }
        false
    }

    pub(super) fn discard_protected_leaves(&self) -> std::collections::HashSet<ItemId> {
        self.pane_tree()
            .map(|tree| tree.tabs().into_iter().collect())
            .unwrap_or_default()
    }

    fn candidate_is_still_discardable(&self, id: ItemId, committed_url: &str) -> bool {
        let live_count = self.items.view_ids().len();
        live_count > self.residency.live_view_soft_limit
            && !self.discard_protected_leaves().contains(&id)
            && self.items.tab(id).is_some_and(|tab| {
                tab.has_view()
                    && !tab.loading
                    && tab
                        .url
                        .as_ref()
                        .is_some_and(|url| url.as_str() == committed_url)
            })
            && (live_count > self.residency.live_view_pressure_limit
                || self.idle_for(id, self.residency.discard_idle_min))
    }

    fn protect_discard_candidate(&mut self, id: ItemId) {
        self.residency.discard_protected_until.insert(
            id,
            std::time::Instant::now() + self.residency.discard_protected_retry,
        );
    }

    pub(super) fn cancel_discard_probe(&mut self, id: ItemId) {
        match self.residency.discard_probes.get_mut(&id) {
            Some(PendingDiscardProbe::Closing { recreate, .. }) => {
                // Physical close already owns the native generation. Preserve
                // the acknowledgement obligation and recreate after it lands.
                *recreate = true;
            }
            Some(PendingDiscardProbe::Probing { .. }) => {
                self.residency.discard_probes.remove(&id);
                if let Some(queue) = &self.self_queue {
                    queue.cancel_discard_probe(id);
                }
            }
            None => {}
        }
    }

    pub(super) fn recreate_after_inflight_discard(&mut self, id: ItemId) -> bool {
        if let Some(PendingDiscardProbe::Closing { recreate, .. }) =
            self.residency.discard_probes.get_mut(&id)
        {
            *recreate = true;
            true
        } else {
            false
        }
    }

    pub(super) fn on_discard_probe_timeout(&mut self, id: ItemId, probe: DiscardProbeId) {
        let exact = matches!(
            self.residency.discard_probes.get(&id),
            Some(PendingDiscardProbe::Probing { probe: pending, .. }) if *pending == probe
        );
        if exact {
            self.residency.discard_probes.remove(&id);
            self.protect_discard_candidate(id);
            self.maintain_views();
        }
    }

    pub(super) fn on_discard_safety(
        &mut self,
        id: ItemId,
        probe: DiscardProbeId,
        can_discard: bool,
    ) {
        let Some(PendingDiscardProbe::Probing {
            probe: pending,
            committed_url,
            ..
        }) = self.residency.discard_probes.get(&id).cloned()
        else {
            return;
        };
        if pending != probe {
            return;
        }
        if let Some(queue) = &self.self_queue {
            queue.cancel_discard_probe(id);
        }
        if !can_discard || !self.candidate_is_still_discardable(id, &committed_url) {
            self.residency.discard_probes.remove(&id);
            if !can_discard {
                self.protect_discard_candidate(id);
            }
            self.maintain_views();
            return;
        }

        self.residency.discard_probes.insert(
            id,
            PendingDiscardProbe::Closing {
                probe,
                recreate: false,
                deferred_navigation: None,
            },
        );
        // `discard_view` retires the exact native generation at its public
        // boundary. Any queued zoom result is now terminally stale.
        self.zoom.pending.remove(&id);
        if !self.engine.discard_view(id, probe) {
            // Engine dispatch failure after lifecycle retirement is terminal
            // at the native boundary. Keep the closing obligation visible;
            // pretending the old view survived would permit unsafe reuse.
            crate::diagnostic!("engine: native discard was not admitted");
        }
    }

    pub(super) fn on_view_discarded(
        &mut self,
        id: ItemId,
        profile: ProfileId,
        probe: DiscardProbeId,
    ) {
        let Some(PendingDiscardProbe::Closing {
            probe: pending,
            recreate,
            deferred_navigation,
        }) = self.residency.discard_probes.get(&id).cloned()
        else {
            return;
        };
        if pending != probe || self.profile_of_item(id) != Some(profile) {
            return;
        }
        self.zoom.pending.remove(&id);
        self.cancel_pending_presentation(id);
        self.residency.discard_probes.remove(&id);
        if !self.items.mark_view_discarded(id) {
            return;
        }

        let mut effects = if let Some(input) = deferred_navigation {
            self.items.navigate(id, &input)
        } else if recreate || self.discard_protected_leaves().contains(&id) {
            self.items.ensure_view(id)
        } else {
            Vec::new()
        };
        // If a split/focus change happened between native acknowledgement and
        // this actor turn, the final visibility check wins.
        if effects.is_empty() && self.discard_protected_leaves().contains(&id) {
            effects.extend(self.items.ensure_view(id));
        }
        self.apply(effects);
        let _ = self.relayout();
        self.maintain_views();
        self.project_tab(id);
    }

    fn idle_for(&self, id: ItemId, min: std::time::Duration) -> bool {
        self.residency
            .last_focus
            .get(&id)
            .is_none_or(|t| t.elapsed() >= min)
    }

    fn retire_crashed_view(&mut self, id: ItemId) {
        self.zoom.pending.remove(&id);
        let title = self.items.tab(id).map(|tab| tab.title.clone());
        self.items.view_creation_failed(id);
        if let Some(title) = title {
            // `view_creation_failed` owns the generic create-failure label.
            // A renderer crash is different: its label is transient chrome
            // state and must not replace the last committed document title.
            self.items.set_title(id, title);
        }
        self.crash.presentations.insert(id);
    }

    // One automatic relaunch per crash burst: a second death inside the
    // window means the page kills its web process deterministically, and a
    // reload loop would peg the machine.
    pub(super) fn on_crashed(&mut self, id: ItemId) {
        const RETRY_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
        if self.items.tab(id).is_none() {
            return;
        }
        self.cancel_pending_presentation(id);
        self.cancel_discard_probe(id);
        // `Crashed` is emitted only after the engine has physically removed
        // and revoked the exact native-view generation. Sending a second,
        // id-only close here could be reordered behind recovery and destroy
        // the replacement generation.
        self.retire_crashed_view(id);
        self.items.set_loading(id, false);
        let recent = self
            .crash
            .crashes
            .insert(id, std::time::Instant::now())
            .is_some_and(|t| t.elapsed() < RETRY_WINDOW);
        let active = self.windows.focused().and_then(|window| window.active);
        let effects = if !recent && active == Some(id) {
            self.items.ensure_view(id)
        } else {
            Vec::new()
        };
        self.apply(effects);
        if active == Some(id) {
            let _ = self.relayout();
            self.maintain_views();
        }
        // Crash status is runtime truth, not useful session state. Avoid a
        // full O(items) snapshot/relayout for every hidden WebKit view when a
        // shared renderer process reports a burst of per-view terminations.
        self.project_tab(id);
    }

    pub(super) fn on_profile_process_exit(&mut self, profile: ProfileId, ids: Vec<ItemId>) {
        const RETRY_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);
        let visible_order: Vec<ItemId> =
            self.pane_tree().map(|tree| tree.tabs()).unwrap_or_default();
        let visible: std::collections::HashSet<ItemId> = visible_order.iter().copied().collect();
        let mut suppressed_visible = std::collections::HashSet::new();
        let mut effects = Vec::new();
        for id in ids {
            if self.profile_of_item(id) != Some(profile) {
                continue;
            }
            self.cancel_pending_presentation(id);
            self.cancel_discard_probe(id);
            let repeated = self
                .crash
                .crashes
                .insert(id, std::time::Instant::now())
                .is_some_and(|time| time.elapsed() < RETRY_WINDOW);
            self.retire_crashed_view(id);
            // A browser-process loss can report hundreds of tabs at once.
            // Recreate every currently visible split leaf, because native
            // layout admission requires a live token for every leaf. Hidden
            // tabs still recover lazily, avoiding a process/controller storm.
            if visible.contains(&id) && !repeated {
                effects.extend(self.items.ensure_view(id));
            } else if visible.contains(&id) {
                suppressed_visible.insert(id);
            }
        }
        if self
            .windows
            .focused()
            .and_then(|window| window.active)
            .is_some_and(|active| suppressed_visible.contains(&active))
        {
            // A repeatedly crashing active leaf has no native token. Focus a
            // successfully recreated sibling so layout collapses to that live
            // leaf until the protected tab is explicitly activated again.
            if let Some(replacement) = visible_order.into_iter().find(|id| {
                !suppressed_visible.contains(id)
                    && self.items.tab(*id).is_some_and(TabState::has_view)
            }) {
                if let Some(window) = self.windows.focused_mut() {
                    window.active = Some(replacement);
                }
                self.items.set_lifecycle(replacement, Lifecycle::Active);
                self.touch(replacement);
            }
        }
        self.commit(effects);
    }

    pub(super) fn touch(&mut self, id: ItemId) {
        self.cancel_discard_probe(id);
        self.residency
            .last_focus
            .insert(id, std::time::Instant::now());
    }
}
