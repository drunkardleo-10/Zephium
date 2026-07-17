//! The single native engine-event fold into authoritative shell state.

use super::*;

fn valid_native_split_update(current: &Pane, candidate: &Pane) -> bool {
    match (current, candidate) {
        (Pane::Leaf(expected), Pane::Leaf(actual)) => expected == actual,
        (
            Pane::Branch {
                axis: expected_axis,
                a: expected_a,
                b: expected_b,
                ..
            },
            Pane::Branch { axis, ratio, a, b },
        ) => {
            axis == expected_axis
                && ratio.is_finite()
                && (0.05..=0.95).contains(ratio)
                && valid_native_split_update(expected_a, a)
                && valid_native_split_update(expected_b, b)
        }
        _ => false,
    }
}

impl Shell {
    pub(super) fn on_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::RuntimeRestartRequired => {
                if !self.runtime_restart_required {
                    self.runtime_restart_required = true;
                    self.project_runtime_status();
                }
            }
            EngineEvent::SplitChanged { window, tree } => {
                // Native divider drags may update ratios only. Never let a
                // stale or malformed callback mutate topology, swap tabs, or
                // inject a non-finite layout value or cross-window item into
                // Rust-owned state.
                let valid = self
                    .windows
                    .get(window)
                    .and_then(|win| {
                        win.splits.as_ref().map(|current| {
                            valid_native_split_update(current, &tree)
                                && self.pane_in_scope(current, win.profile, win.space)
                                && self.pane_in_scope(&tree, win.profile, win.space)
                        })
                    })
                    .unwrap_or(false);
                if valid {
                    if let Some(win) = self.windows.get_mut(window) {
                        win.splits = Some(tree);
                    }
                    self.schedule_persist();
                    let _ = self.relayout();
                } else {
                    eprintln!("engine: rejected invalid native split tree");
                    let _ = self.relayout();
                }
            }
            EngineEvent::NavState {
                id,
                can_go_back,
                can_go_forward,
            } => {
                self.items.set_nav_flags(id, can_go_back, can_go_forward);
                self.project_tab(id);
            }
            EngineEvent::NewWindowRequested { id, url } => self.open_linked_tab(id, &url),
            EngineEvent::FaviconPixels { id, page_url, rgba } => {
                self.favicon_pixels(id, &page_url, rgba);
            }
            EngineEvent::DiscardSafety {
                id,
                probe,
                can_discard,
            } => self.on_discard_safety(id, probe, can_discard),
            EngineEvent::ViewDiscarded { id, profile, probe } => {
                self.on_view_discarded(id, profile, probe)
            }
            EngineEvent::PermissionRequested { .. } => {}
            EngineEvent::DownloadRequested { .. } => {}
            EngineEvent::PresentationPending {
                id,
                navigation,
                url,
            }
            | EngineEvent::PresentationReady {
                id,
                navigation,
                url,
            } => self.on_presentation_fact(id, navigation, url),
            EngineEvent::NavigationFailed { id, request } => {
                // Only the matching latest intent is affected. The displayed
                // URL was never changed optimistically, so a stale native
                // rejection cannot roll chrome or persistence forward or back.
                if self.items.navigation_failed(id, request) {
                    self.project_tab(id);
                }
            }
            EngineEvent::ZoomSettled {
                id,
                request,
                applied_scale,
                succeeded,
            } => self.on_zoom_settled(id, request, applied_scale, succeeded),
            EngineEvent::NativeActionFailed { id, action } => {
                if self.items.tab(id).is_some_and(TabState::has_view) {
                    let action = match action {
                        NativeAction::Reload => "reload",
                        NativeAction::GoBack => "back",
                        NativeAction::GoForward => "forward",
                    };
                    // The engine separately re-observes authoritative
                    // source/history. Keep this diagnostic bounded and never
                    // include a page-derived URL or native error string.
                    eprintln!("engine: native {action} action failed");
                }
            }
            EngineEvent::ViewCreationFailed { id } => {
                self.on_view_creation_failed(id);
            }
            EngineEvent::ProfileProcessExited { profile, ids } => {
                self.on_profile_process_exit(profile, ids)
            }
            EngineEvent::Crashed { id } => self.on_crashed(id),
            EngineEvent::Captured { .. } => {}
            EngineEvent::HtmlExtracted { .. } => {}
            EngineEvent::ShortcutPressed { .. } => {}
            EngineEvent::TitleChanged { id, title } => {
                self.crash_presentations.remove(&id);
                self.items.set_title(id, title);
                self.project_tab(id);
            }
            EngineEvent::LoadingChanged { id, loading } => {
                if loading {
                    self.cancel_discard_probe(id);
                    self.crash_presentations.remove(&id);
                }
                self.items.set_loading(id, loading);
                self.project_tab(id);
                if !loading {
                    // The first URL observation may precede the renderer's
                    // asynchronous image decode. Poll immediately at load
                    // completion as well as through the bounded timer.
                    self.favicon_load_completed(id);
                    self.engine.warm_spare(self.partition_of(id));
                }
            }
            EngineEvent::UrlChanged { id, url } => {
                self.cancel_discard_probe(id);
                let Ok(committed_url) = url::Url::parse(&url) else {
                    eprintln!("engine: rejected invalid or unknown committed URL event");
                    return;
                };
                if !navigation::is_allowed(&committed_url) {
                    eprintln!("engine: rejected invalid or unknown committed URL event");
                    return;
                }
                let first_committed_url = self.items.tab(id).is_some_and(|tab| tab.url.is_none());
                let replace_stale_title = self
                    .items
                    .tab(id)
                    .and_then(|tab| tab.url.as_ref())
                    .is_none_or(|previous| !same_browser_origin(previous, &committed_url));
                if !self.items.set_committed_url(id, committed_url.clone()) {
                    eprintln!("engine: rejected invalid or unknown committed URL event");
                    return;
                }
                if replace_stale_title {
                    // Browser chrome may be projected before the new document
                    // publishes a title. Never carry a prior origin's trusted
                    // label across the exact URL acknowledgement that unlocks
                    // presentation; use a neutral URL-derived label meanwhile.
                    self.items
                        .set_title(id, neutral_title_for_url(&committed_url));
                }
                self.maybe_discover_favicon(id);
                // History is attributed to the profile that owns the item,
                // not the focused window; incognito profiles never record.
                let recording = self.profile_of_item(id).filter(|p| {
                    self.profiles
                        .get(*p)
                        .is_some_and(|x| x.kind != ProfileKind::Incognito)
                });
                if let Some(profile) = recording.filter(|_| self.should_record_visit(id, &url)) {
                    let title = self
                        .items
                        .tab(id)
                        .map(|t| t.title.clone())
                        .unwrap_or_default();
                    self.store.record_visit(profile, url, title);
                }
                self.schedule_url_checkpoint(id);
                if first_committed_url {
                    // Keep the real privileged New Tab projection and native
                    // frame until the exact presentation eval replaces it.
                    // All generic projections remain URL-free for this item,
                    // so they cannot create an empty gap ahead of that eval.
                    self.presentation.deferred_first_content_layout.insert(id);
                } else {
                    self.project_tab(id);
                }
            }
        }
    }

    fn should_record_visit(&mut self, id: ItemId, url: &str) -> bool {
        const REPEATED_URL_MIN: std::time::Duration = std::time::Duration::from_secs(30);
        const NAVIGATION_MIN: std::time::Duration = std::time::Duration::from_secs(1);
        let now = std::time::Instant::now();
        if let Some((previous, recorded)) = self.last_visits.get(&id) {
            let minimum = if previous == url {
                REPEATED_URL_MIN
            } else {
                NAVIGATION_MIN
            };
            if now.duration_since(*recorded) < minimum {
                return false;
            }
        }
        self.last_visits.insert(id, (url.to_owned(), now));
        true
    }
}

fn same_browser_origin(left: &url::Url, right: &url::Url) -> bool {
    match (origin_of(left), origin_of(right)) {
        (Some(left), Some(right)) => left == right,
        // `about:blank` is the only admitted opaque browser target. Treat it
        // as same-document only when its canonical URL is exactly unchanged.
        (None, None) => left.as_str() == right.as_str(),
        _ => false,
    }
}

fn neutral_title_for_url(url: &url::Url) -> String {
    let Some(host) = url.host_str() else {
        return url.as_str().to_owned();
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}
