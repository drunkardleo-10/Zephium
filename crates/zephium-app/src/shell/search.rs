//! Bounded launcher search and asynchronous history reconciliation.

use super::*;

const MAX_ASYNC_SEARCH_QUERY_BYTES: usize = 4 * 1024;

pub(super) struct PendingSearch {
    pub(super) generation: u64,
    pub(super) profile: ProfileId,
    pub(super) lookup_query: String,
    pub(super) display_query: String,
    pub(super) open_urls: std::collections::HashSet<String>,
    pub(super) base_results: Vec<SearchResult>,
}

#[derive(Default)]
pub(super) struct SearchState {
    pub(super) context: Option<zephium_ipc::SearchContext>,
    pub(super) results: Vec<SearchResult>,
    pub(super) pending: Option<PendingSearch>,
    pub(super) generation: u64,
}

impl Shell {
    fn publish_search(&mut self, mut results: SearchResults) {
        let mut identities = std::collections::HashSet::new();
        results.results.retain(|result| {
            identities.insert(match &result.action {
                SearchAction::ActivateTab { id } => format!("tab:{id}"),
                SearchAction::OpenUrl { url } => format!("url:{url}"),
                SearchAction::RunCommand { id } => format!("command:{id}"),
            })
        });
        self.search.results = results.results.clone();
        (self.emit)(Projection::Search(results));
    }

    pub(super) fn search_scoped(&mut self, query: &str, context: zephium_ipc::SearchContext) {
        if !self.search_context_current(&context) {
            return;
        }
        self.search.context = Some(context);
        self.search(query);
    }

    fn search_context_current(&self, context: &zephium_ipc::SearchContext) -> bool {
        self.windows.focused().is_some_and(|window| {
            window.id.to_string() == context.window_id
                && window.profile.to_string() == context.profile_id
                && window.space.to_string() == context.space_id
        })
    }

    pub(super) fn cancel_scoped_search(&mut self, session_id: &str) {
        if self
            .search
            .context
            .as_ref()
            .is_some_and(|context| context.session_id == session_id)
        {
            self.search.pending = None;
            self.search.context = None;
            self.search.results.clear();
        }
    }

    pub(super) fn operation_run_search_action(
        &mut self,
        context: zephium_ipc::SearchContext,
        action: SearchAction,
    ) -> OperationDisposition {
        if !self.search_context_current(&context)
            || self.search.context.as_ref() != Some(&context)
            || !self
                .search
                .results
                .iter()
                .any(|result| result.action == action)
        {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        match action {
            SearchAction::ActivateTab { id } => ItemId::parse(&id).map_or_else(
                || operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput),
                |id| self.operation_activate(id),
            ),
            SearchAction::OpenUrl { url } => self.operation_open_url(url),
            SearchAction::RunCommand { id } if id.starts_with("theme.") => {
                self.operation_set_app_setting("appearance".into(), id[6..].into())
            }
            SearchAction::RunCommand { id }
                if matches!(id.as_str(), "split.choose" | "sidebar.toggleCompact") =>
            {
                (self.emit)(Projection::UiCommand(id));
                operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
            }
            SearchAction::RunCommand { id } => self.operation_run_command(&id),
        }
    }

    pub(super) fn search(&mut self, query: &str) {
        let Some((profile, space)) = self
            .windows
            .focused()
            .map(|window| (window.profile, window.space))
        else {
            return;
        };
        let q = query.trim();
        let needle = q.to_lowercase();
        let mut stack = Vec::new();
        for placement in [
            Placement::Favorites { profile },
            Placement::Space {
                space,
                section: SpaceSection::Pinned,
            },
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ] {
            stack.extend(self.items.roots(placement).iter().rev().copied());
        }
        let mut tabs = Vec::new();
        while let Some(id) = stack.pop() {
            if self.items.tab(id).is_some() && self.item_in_scope(id, profile, space) {
                tabs.push(id);
            }
            stack.extend(self.items.children(id).iter().rev().copied());
        }
        tabs.sort_by_key(|id| std::cmp::Reverse(self.residency.last_focus.get(id).copied()));
        let mut results = Vec::new();
        self.search.generation = self.search.generation.wrapping_add(1);
        if self.search.generation == 0 {
            self.search.generation = 1;
        }
        let generation = self.search.generation;
        self.search.pending = None;

        if q.is_empty() {
            results.extend(
                tabs.iter()
                    .filter_map(|id| {
                        self.items
                            .tab(*id)
                            .map(|t| tab_result(*id, t, self.favicon_key(t, Some(profile))))
                    })
                    .take(4),
            );
        } else {
            let matched: Vec<(ItemId, &TabState)> = tabs
                .iter()
                .filter_map(|id| self.items.tab(*id).map(|t| (*id, t)))
                .filter(|(_, t)| {
                    t.title.to_lowercase().contains(&needle)
                        || t.url
                            .as_ref()
                            .is_some_and(|u| u.as_str().to_lowercase().contains(&needle))
                })
                .take(4)
                .collect();
            let open_urls: std::collections::HashSet<String> = matched
                .iter()
                .filter_map(|(_, t)| t.url.as_ref().map(ToString::to_string))
                .collect();
            results.extend(
                matched
                    .iter()
                    .map(|(id, t)| tab_result(*id, t, self.favicon_key(t, Some(profile)))),
            );

            if let Some(url) = navigation::classify(q) {
                if navigation::is_query(q) {
                    results.push(SearchResult {
                        kind: "search".into(),
                        title: format!("Search for \"{q}\""),
                        detail: "DuckDuckGo".into(),
                        favicon: None,
                        action: SearchAction::OpenUrl {
                            url: url.to_string(),
                        },
                    });
                } else {
                    results.push(SearchResult {
                        kind: "url".into(),
                        title: format!("Open {url}"),
                        detail: "New Tab".into(),
                        favicon: self.favicon_key_for_url(profile, url.as_str()),
                        action: SearchAction::OpenUrl {
                            url: url.to_string(),
                        },
                    });
                }
            }

            results.extend(
                commands::REGISTRY
                    .iter()
                    .filter(|c| c.id != "launcher.toggle" && c.group != commands::Group::Work)
                    .filter(|c| c.title.to_lowercase().contains(&needle))
                    .take(3)
                    .map(|c| SearchResult {
                        kind: "command".into(),
                        title: c.title.into(),
                        detail: c.accelerator.unwrap_or_default().into(),
                        favicon: None,
                        action: SearchAction::RunCommand { id: c.id.into() },
                    }),
            );

            results.truncate(10);

            // Project local tab/URL/command matches immediately. History is
            // presentation-only and arrives asynchronously; the exact query
            // generation below prevents a slow old result replacing newer UI.
            self.publish_search(SearchResults {
                context: self.search.context.clone(),
                query: query.into(),
                results: results.clone(),
            });
            if q.len() > MAX_ASYNC_SEARCH_QUERY_BYTES {
                return;
            }
            self.search.pending = Some(PendingSearch {
                generation,
                profile,
                lookup_query: q.to_owned(),
                display_query: query.to_owned(),
                open_urls,
                base_results: results,
            });
            if let Some(reads) = &self.store_reads {
                if !reads.request_history(generation, profile, q.to_owned()) {
                    self.search.pending = None;
                }
            } else {
                #[cfg(test)]
                {
                    let hits = self.store.search_history(profile, q, 6);
                    self.on_store_read(StoreReadResult::History {
                        generation,
                        profile,
                        query: q.to_owned(),
                        hits,
                    });
                }
                #[cfg(not(test))]
                {
                    // Production construction always installs the reader.
                    // Keep this defensive branch nonblocking if a future
                    // internal constructor violates that invariant.
                    self.search.pending = None;
                }
            }
            return;
        }

        self.publish_search(SearchResults {
            context: self.search.context.clone(),
            query: query.into(),
            results,
        });
    }

    pub(super) fn on_history_read(
        &mut self,
        generation: u64,
        profile: ProfileId,
        query: String,
        hits: Vec<zephium_core::ports::store::HistoryHit>,
    ) {
        let exact = self.search.pending.as_ref().is_some_and(|pending| {
            pending.generation == generation
                && pending.profile == profile
                && pending.lookup_query == query
        });
        if !exact {
            return;
        }
        let Some(pending) = self.search.pending.take() else {
            return;
        };
        if self
            .windows
            .focused()
            .is_none_or(|window| window.profile != profile)
        {
            return;
        }

        // Treat even our own store adapter as a serialization boundary: cap
        // cardinality, revalidate URLs, sanitize titles, and deduplicate before
        // values reach privileged launcher markup.
        let mut results = pending.base_results;
        let mut seen = pending.open_urls;
        for hit in hits.into_iter().take(6) {
            if !navigation::is_allowed_str(&hit.url) || !seen.insert(hit.url.clone()) {
                continue;
            }
            let title = zephium_core::item::sanitize_page_title(&hit.title);
            results.push(SearchResult {
                kind: "history".into(),
                title: if title.is_empty() {
                    hit.url.clone()
                } else {
                    title
                },
                detail: hit.url.clone(),
                favicon: self.favicon_key_for_url(profile, &hit.url),
                action: SearchAction::OpenUrl { url: hit.url },
            });
        }
        results.truncate(10);
        self.publish_search(SearchResults {
            context: self.search.context.clone(),
            query: pending.display_query,
            results,
        });
    }
}

fn tab_result(id: ItemId, tab: &TabState, favicon: Option<String>) -> SearchResult {
    let detail = tab
        .url
        .as_ref()
        .and_then(|u| u.host_str().map(ToString::to_string))
        .unwrap_or_default();
    SearchResult {
        kind: "tab".into(),
        title: tab.title.clone(),
        detail,
        favicon,
        action: SearchAction::ActivateTab { id: id.to_string() },
    }
}
