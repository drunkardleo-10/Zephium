//! Browser-owned pages borrow chrome's full-window presentation, never a page bridge.
use super::*;

pub(super) struct PendingBrowserReturn {
    revision: u64,
    window: WindowId,
    items: ItemsState,
    splits: Option<Pane>,
}

impl Shell {
    pub(super) fn active_browser_page(&self) -> Option<crate::BrowserPage> {
        let window = self.windows.focused()?.id;
        self.browser_page
            .filter(|(owner, _)| *owner == window)
            .map(|(_, page)| page)
    }

    pub(super) fn project_browser_page(&self) {
        let id = self
            .active_browser_page()
            .map_or("browser.return", crate::BrowserPage::command_id);
        (self.emit)(Projection::UiCommand(id.into()));
    }

    pub(super) fn operation_show_browser_page(
        &mut self,
        page: Option<crate::BrowserPage>,
    ) -> OperationDisposition {
        let Some(window) = self.windows.focused().map(|window| window.id) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if page.is_none() && self.active_browser_page().is_some() {
            return self.request_browser_return();
        }
        self.browser_return = None;
        self.browser_after_return = None;
        let previous = self.browser_page;
        self.browser_page = page.map(|page| (window, page));
        self.divider = None;
        if self.relayout() != NativeDispatch::Scheduled {
            self.browser_page = previous;
            let _ = self.relayout();
            return operation_result(
                OperationOutcome::NativeAdmissionFailed,
                OperationReason::NativeDispatchRejected,
            );
        }
        self.project_browser_page();
        // NativeDispatch is scheduling admission, not proof of completed native geometry.
        operation_result(
            OperationOutcome::Deferred,
            OperationReason::NativeWorkPending,
        )
    }
    fn request_browser_return(&mut self) -> OperationDisposition {
        if self.browser_return.is_some() {
            return operation_result(
                OperationOutcome::Deferred,
                OperationReason::NativeWorkPending,
            );
        }

        let Some(items) = self.items_snapshot() else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        let Some((window, splits)) = self.windows.focused().map(|w| (w.id, w.splits.clone()))
        else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        let Some(revision) = self.browser_return_revision.checked_add(1) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        self.browser_return_revision = revision;
        self.browser_return = Some(PendingBrowserReturn {
            revision,
            window,
            items: items.clone(),
            splits,
        });
        let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
            queue: Arc::downgrade(&queue.inner),
        });
        let dispatch = self.chrome.restore_browser_chrome(
            revision,
            items,
            Box::new(move |applied| {
                if let Some(callback) = callback {
                    let _ = callback.dispatch(Command::BrowserChromeRestored { revision, applied });
                }
            }),
        );
        match dispatch {
            ChromePresentationDispatch::Applied => self.browser_chrome_restored(revision, true),
            ChromePresentationDispatch::Rejected => self.browser_chrome_restored(revision, false),
            ChromePresentationDispatch::Scheduled => {}
        }
        operation_result(
            OperationOutcome::Deferred,
            OperationReason::NativeWorkPending,
        )
    }

    pub(super) fn browser_chrome_restored(&mut self, revision: u64, applied: bool) {
        let Some(PendingBrowserReturn {
            revision: expected,
            window,
            items,
            splits,
        }) = self.browser_return.as_ref()
        else {
            return;
        };
        if *expected != revision {
            return;
        }
        let exact = self.windows.focused().is_some_and(|current| {
            current.id == *window
                && current.active.map(|id| id.to_string()) == items.active
                && Some(current.space.to_string()) == items.active_space_id
                && items
                    .profile
                    .as_ref()
                    .is_some_and(|profile| profile.id == current.profile.to_string())
                && items.tabs.iter().all(|old| {
                    ItemId::parse(&old.id)
                        .and_then(|id| self.items.tab(id))
                        .is_some_and(|tab| {
                            tab.title == old.title
                                && tab.url.as_ref().map(|url| url.as_str()) == old.url.as_deref()
                        })
                })
                // Compare native state with its native snapshot. The public
                // projection intentionally omits retained single-leaf trees.
                && &current.splits == splits
        });
        self.browser_return = None;
        if !applied || !exact {
            self.browser_after_return = None;
            self.project_browser_page();
            (self.emit)(Projection::UiCommand("browser.return-failed".into()));
            return;
        }
        let previous = self.browser_page.take();
        if self.relayout() != NativeDispatch::Scheduled {
            self.browser_page = previous;
            let _ = self.relayout();
            self.project_browser_page();
            (self.emit)(Projection::UiCommand("browser.return-failed".into()));
        } else {
            self.project_browser_page();
            if let Some(next) = self.browser_after_return.take() {
                self.handle(*next);
            }
        }
    }
}
