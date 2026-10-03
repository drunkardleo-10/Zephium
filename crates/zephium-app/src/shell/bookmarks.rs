//! Profile-scoped bookmark reads and writes for the Bookmarks panel, and
//! bookmarking the page in front. Every call runs on the store-read worker;
//! the shell only admits, routes and decorates with icons.

use super::*;

use crate::api::{BookmarkCompletion, ImportCompletion};
use crate::store_reads::{BookmarkSurfaceReply, BookmarkWork, ImportWork};
use zephium_core::bookmarks::{BookmarkFailure, BookmarkNode};
use zephium_ipc::{
    BookmarkCall, BookmarkCrumb, BookmarkError, BookmarkResponse, BookmarkView, IconSurface,
};

/// Completions outlive one actor turn; the read queue bounds how many exist.
const MAX_PENDING_BOOKMARK_CALLS: usize = 8;

enum Pending {
    Surface(BookmarkCompletion),
    /// Bookmark This Page; chrome learns the outcome as a UI command.
    AddPage,
}

#[derive(Default)]
pub(super) struct BookmarkState {
    pending: std::collections::HashMap<u64, Pending>,
    imports: std::collections::HashMap<u64, ImportCompletion>,
    next_token: u64,
}

fn error(failure: BookmarkFailure) -> BookmarkError {
    match failure {
        BookmarkFailure::Missing => BookmarkError::Missing,
        BookmarkFailure::Full => BookmarkError::Full,
        BookmarkFailure::Cycle => BookmarkError::Cycle,
        BookmarkFailure::Invalid => BookmarkError::Invalid,
        BookmarkFailure::Unavailable => BookmarkError::Unavailable,
    }
}

impl Shell {
    /// Private profiles keep nothing, and a degraded profile cannot answer.
    fn bookmarks_writable(&self, profile: ProfileId) -> bool {
        self.profiles
            .get(profile)
            .is_some_and(|p| p.kind != zephium_core::profiles::ProfileKind::Incognito)
            && !self.degraded_storage_profiles.contains(&profile)
    }

    fn queue_bookmarks(
        &mut self,
        profile: ProfileId,
        work: BookmarkWork,
        pending: Pending,
    ) -> Result<(), Pending> {
        if self.bookmarks.pending.len() >= MAX_PENDING_BOOKMARK_CALLS {
            return Err(pending);
        }
        let Some(reads) = &self.store_reads else {
            return Err(pending);
        };
        self.bookmarks.next_token = self.bookmarks.next_token.wrapping_add(1);
        let token = self.bookmarks.next_token;
        if !reads.request_bookmarks(token, profile, work) {
            return Err(pending);
        }
        self.bookmarks.pending.insert(token, pending);
        Ok(())
    }

    pub(super) fn bookmark_call(
        &mut self,
        expected_profile: ProfileId,
        call: BookmarkCall,
        done: BookmarkCompletion,
    ) {
        let focused = self
            .windows
            .focused()
            .is_some_and(|window| window.profile == expected_profile);
        if !focused || !call.validate() {
            done.finish(BookmarkResponse::Error {
                error: BookmarkError::Invalid,
            });
            return;
        }
        if !self.bookmarks_writable(expected_profile) {
            done.finish(BookmarkResponse::Error {
                error: BookmarkError::Unavailable,
            });
            return;
        }
        let capacity = self.bookmarks.pending.len() >= MAX_PENDING_BOOKMARK_CALLS;
        if let Err(Pending::Surface(done)) = self.queue_bookmarks(
            expected_profile,
            BookmarkWork::Surface(call),
            Pending::Surface(done),
        ) {
            done.finish(BookmarkResponse::Error {
                error: if capacity {
                    BookmarkError::Capacity
                } else {
                    BookmarkError::Unavailable
                },
            });
        }
    }

    /// Bookmarks the page in front, once: an address already kept answers
    /// with the bookmark that holds it.
    /// Bookmarks `tab`, or the page in front when there is none.
    pub(super) fn operation_bookmark_page(&mut self, tab: Option<ItemId>) -> OperationDisposition {
        let page = if tab.is_some() {
            tab.filter(|id| self.item_in_focused_scope(*id))
        } else if self.active_browser_page().is_some() {
            self.work_pane_tab()
        } else {
            self.windows.focused().and_then(|window| window.active)
        };
        let Some(profile) = self.windows.focused().map(|window| window.profile) else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let target = page.and_then(|id| self.items.tab(id)).and_then(|tab| {
            let url = tab.url.as_ref()?;
            (tab.content == zephium_core::item::TabContent::Web && navigation::is_allowed(url))
                .then(|| (url.to_string(), tab.title.clone()))
        });
        let Some((url, title)) = target else {
            return operation_result(OperationOutcome::NoOp, OperationReason::InvalidScope);
        };
        if !self.bookmarks_writable(profile) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        match self.queue_bookmarks(
            profile,
            BookmarkWork::AddPage { url, title },
            Pending::AddPage,
        ) {
            Ok(()) => operation_result(OperationOutcome::Applied, OperationReason::MutationApplied),
            Err(_) => operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope),
        }
    }

    pub(super) fn on_bookmarks_read(
        &mut self,
        token: u64,
        profile: ProfileId,
        reply: BookmarkSurfaceReply,
    ) {
        let Some(pending) = self.bookmarks.pending.remove(&token) else {
            return;
        };
        let done = match pending {
            Pending::AddPage => {
                let command = match reply {
                    BookmarkSurfaceReply::Saved(Some(id)) => format!("bookmark.added={id}"),
                    _ => "bookmark.failed".to_owned(),
                };
                (self.emit)(Projection::UiCommand(command));
                return;
            }
            Pending::Surface(done) => done,
        };
        let response = match reply {
            BookmarkSurfaceReply::Listing {
                folder,
                path,
                nodes,
            } => BookmarkResponse::Listing {
                folder: folder.map(|id| id.to_string()),
                path: path
                    .into_iter()
                    .map(|node| BookmarkCrumb {
                        id: node.id.to_string(),
                        title: node.title,
                    })
                    .collect(),
                items: self.bookmark_views(profile, nodes),
            },
            BookmarkSurfaceReply::Results(nodes) => BookmarkResponse::Results {
                items: self.bookmark_views(profile, nodes),
            },
            BookmarkSurfaceReply::Saved(id) => BookmarkResponse::Saved {
                id: id.map(|id| id.to_string()),
            },
            BookmarkSurfaceReply::Failed(failure) => BookmarkResponse::Error {
                error: error(failure),
            },
        };
        self.publish_icons();
        done.finish(response);
    }

    fn bookmark_views(
        &mut self,
        profile: ProfileId,
        nodes: Vec<BookmarkNode>,
    ) -> Vec<BookmarkView> {
        let views: Vec<BookmarkView> = nodes
            .into_iter()
            .map(|node| BookmarkView {
                id: node.id.to_string(),
                icon: node
                    .url
                    .as_deref()
                    .and_then(|url| self.icon_ref_for_url(IconSurface::Chrome, profile, url)),
                url: node.url,
                title: node.title,
                children: node.children,
            })
            .collect();
        self.want_icons(
            IconSurface::Chrome,
            profile,
            views
                .iter()
                .filter(|view| view.icon.is_none())
                .filter_map(|view| view.url.as_deref()),
        );
        views
    }

    pub(super) fn import_into_focused(&mut self, work: ImportWork, done: ImportCompletion) {
        if !self.bootstrapped {
            self.bootstrap();
        }
        let Some(profile) = self
            .windows
            .focused()
            .map(|window| window.profile)
            .filter(|profile| self.bookmarks_writable(*profile))
        else {
            done.finish(None);
            return;
        };
        if let ImportWork::Essentials(sites) = work {
            done.finish(Some(self.keep_imported_sites(profile, sites)));
            return;
        }
        if self.bookmarks.imports.len() >= MAX_PENDING_BOOKMARK_CALLS {
            done.finish(None);
            return;
        }
        let Some(reads) = &self.store_reads else {
            done.finish(None);
            return;
        };
        self.bookmarks.next_token = self.bookmarks.next_token.wrapping_add(1);
        let token = self.bookmarks.next_token;
        if reads.request_import(token, profile, work) {
            self.bookmarks.imports.insert(token, done);
        } else {
            done.finish(None);
        }
    }

    pub(super) fn on_import_read(&mut self, token: u64, added: Option<u32>) {
        if let Some(done) = self.bookmarks.imports.remove(&token) {
            done.finish(added);
        }
    }

    /// Every completion is answered, including when the shell is shutting down
    /// and the read queue will never deliver.
    pub(super) fn fail_pending_bookmark_calls(&mut self) {
        for (_, done) in std::mem::take(&mut self.bookmarks.imports) {
            done.finish(None);
        }
        for (_, pending) in std::mem::take(&mut self.bookmarks.pending) {
            if let Pending::Surface(done) = pending {
                done.finish(BookmarkResponse::Error {
                    error: BookmarkError::Unavailable,
                });
            }
        }
    }
}
