//! Bounded logical browser-surface facts required by native extension hosts.
//!
//! The application Shell owns this model. Native adapters may bind a
//! [`resident`](ExtensionBrowserTab::resident) tab to an already-existing
//! platform webview, but this projection never authorizes constructing one.
//! In particular, enumerating a discarded tab remains allocation-only Rust
//! work and cannot defeat the browser's native-view residency policy.

use std::collections::HashSet;
use std::sync::Arc;

use crate::ids::{ItemId, ProfileId, WindowId};
use crate::item::page_title_is_sanitized;

/// One browser window per admitted profile remains a common configuration;
/// this independent ceiling also permits multiple windows without allowing a
/// compromised or buggy application projection to grow native delegate state
/// without bound.
pub const MAX_EXTENSION_BROWSER_WINDOWS: usize = crate::session::MAX_SESSION_PROFILES;
/// The extension surface cannot name more logical tabs than the authoritative
/// session aggregate can retain process-wide.
pub const MAX_EXTENSION_BROWSER_TABS: usize = crate::session::MAX_SESSION_ITEMS;

/// Strictly positive, monotonically increasing browser-surface generation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionBrowserSurfaceGeneration(u64);

impl ExtensionBrowserSurfaceGeneration {
    pub const INITIAL: Self = Self(1);

    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// One logical tab identity and its native-document residency expectation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionBrowserTab {
    id: ItemId,
    resident: bool,
    title: Arc<str>,
    url: Option<Arc<str>>,
    loading: bool,
    pinned: bool,
}

impl ExtensionBrowserTab {
    /// Builds one bounded extension-visible tab snapshot.
    ///
    /// Passing the same logical tab as `previous` reuses its immutable title
    /// and URL allocations when those values did not change. Shell may rebuild
    /// the lightweight surface on every committed mutation without copying all
    /// retained tab strings, while the returned value remains an owned fact
    /// with no borrow into mutable session state.
    pub fn from_snapshot(
        previous: Option<&Self>,
        id: ItemId,
        resident: bool,
        title: &str,
        url: Option<&url::Url>,
        loading: bool,
        pinned: bool,
    ) -> Result<Self, ExtensionBrowserSurfaceError> {
        if !page_title_is_sanitized(title) {
            return Err(ExtensionBrowserSurfaceError::InvalidTabTitle);
        }
        if url.is_some_and(|url| !crate::navigation::is_allowed(url)) {
            return Err(ExtensionBrowserSurfaceError::InvalidTabUrl);
        }
        let title = previous
            .filter(|previous| previous.title() == title)
            .map_or_else(
                || Arc::<str>::from(title),
                |previous| previous.title.clone(),
            );
        let url = match (previous, url) {
            (Some(previous), Some(url)) if previous.url() == Some(url.as_str()) => {
                previous.url.clone()
            }
            (_, Some(url)) => Some(Arc::<str>::from(url.as_str())),
            _ => None,
        };
        Ok(Self {
            id,
            resident,
            title,
            url,
            loading,
            pinned,
        })
    }

    pub const fn id(&self) -> ItemId {
        self.id
    }

    /// Whether the Shell currently expects this logical tab to own a native
    /// document. The engine still resolves the physical view independently;
    /// this bit can never mint or recover a native pointer.
    pub const fn resident(&self) -> bool {
        self.resident
    }

    pub const fn discarded(&self) -> bool {
        !self.resident
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub const fn loading(&self) -> bool {
        self.loading
    }

    pub const fn pinned(&self) -> bool {
        self.pinned
    }
}

/// Ordered logical tabs in one browser window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionBrowserWindow {
    id: WindowId,
    private: bool,
    active: Option<ItemId>,
    tabs: Box<[ExtensionBrowserTab]>,
}

impl ExtensionBrowserWindow {
    pub fn new(
        id: WindowId,
        private: bool,
        active: Option<ItemId>,
        tabs: Vec<ExtensionBrowserTab>,
    ) -> Result<Self, ExtensionBrowserSurfaceError> {
        if id == 0 {
            return Err(ExtensionBrowserSurfaceError::InvalidWindowIdentity);
        }
        if tabs.len() > MAX_EXTENSION_BROWSER_TABS {
            return Err(ExtensionBrowserSurfaceError::TooManyTabs);
        }
        let mut unique = HashSet::with_capacity(tabs.len());
        for tab in &tabs {
            if !unique.insert(tab.id()) {
                return Err(ExtensionBrowserSurfaceError::DuplicateTab);
            }
        }
        if active.is_some_and(|active| !unique.contains(&active)) {
            return Err(ExtensionBrowserSurfaceError::ActiveTabMissing);
        }
        Ok(Self {
            id,
            private,
            active,
            tabs: tabs.into_boxed_slice(),
        })
    }

    pub const fn id(&self) -> WindowId {
        self.id
    }

    pub const fn is_private(&self) -> bool {
        self.private
    }

    pub const fn active(&self) -> Option<ItemId> {
        self.active
    }

    pub const fn tabs(&self) -> &[ExtensionBrowserTab] {
        &self.tabs
    }
}

/// Exact logical browser surface for one profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionBrowserSurface {
    profile: ProfileId,
    generation: ExtensionBrowserSurfaceGeneration,
    focused: Option<WindowId>,
    windows: Box<[ExtensionBrowserWindow]>,
}

impl ExtensionBrowserSurface {
    pub fn new(
        profile: ProfileId,
        generation: ExtensionBrowserSurfaceGeneration,
        focused: Option<WindowId>,
        windows: Vec<ExtensionBrowserWindow>,
    ) -> Result<Self, ExtensionBrowserSurfaceError> {
        if windows.len() > MAX_EXTENSION_BROWSER_WINDOWS {
            return Err(ExtensionBrowserSurfaceError::TooManyWindows);
        }
        let mut window_ids = HashSet::with_capacity(windows.len());
        let mut tab_ids = HashSet::new();
        let mut tab_count = 0_usize;
        let mut privacy = None;
        for window in &windows {
            if !window_ids.insert(window.id()) {
                return Err(ExtensionBrowserSurfaceError::DuplicateWindow);
            }
            if privacy
                .replace(window.is_private())
                .is_some_and(|current| current != window.is_private())
            {
                return Err(ExtensionBrowserSurfaceError::MixedPrivacyClass);
            }
            tab_count = tab_count
                .checked_add(window.tabs().len())
                .ok_or(ExtensionBrowserSurfaceError::TooManyTabs)?;
            if tab_count > MAX_EXTENSION_BROWSER_TABS {
                return Err(ExtensionBrowserSurfaceError::TooManyTabs);
            }
            for tab in window.tabs() {
                if !tab_ids.insert(tab.id()) {
                    return Err(ExtensionBrowserSurfaceError::DuplicateTab);
                }
            }
        }
        if focused.is_some_and(|focused| !window_ids.contains(&focused)) {
            return Err(ExtensionBrowserSurfaceError::FocusedWindowMissing);
        }
        Ok(Self {
            profile,
            generation,
            focused,
            windows: windows.into_boxed_slice(),
        })
    }

    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub const fn generation(&self) -> ExtensionBrowserSurfaceGeneration {
        self.generation
    }

    pub const fn focused(&self) -> Option<WindowId> {
        self.focused
    }

    pub const fn windows(&self) -> &[ExtensionBrowserWindow] {
        &self.windows
    }

    pub fn tabs(&self) -> impl Iterator<Item = &ExtensionBrowserTab> {
        self.windows.iter().flat_map(|window| window.tabs())
    }
}

/// Stable bounded projection rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionBrowserSurfaceError {
    InvalidWindowIdentity,
    InvalidTabTitle,
    InvalidTabUrl,
    TooManyWindows,
    TooManyTabs,
    DuplicateWindow,
    DuplicateTab,
    FocusedWindowMissing,
    ActiveTabMissing,
    MixedPrivacyClass,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(value: u128, resident: bool) -> ExtensionBrowserTab {
        ExtensionBrowserTab::from_snapshot(
            None,
            ItemId::from(value),
            resident,
            "Example",
            Some(&url::Url::parse("https://example.test/").unwrap()),
            false,
            false,
        )
        .unwrap()
    }

    #[test]
    fn surface_preserves_discarded_tabs_without_native_authority() {
        let window = ExtensionBrowserWindow::new(
            7,
            false,
            Some(ItemId::from(1)),
            vec![tab(1, true), tab(2, false)],
        )
        .unwrap();
        let surface = ExtensionBrowserSurface::new(
            ProfileId::from(9),
            ExtensionBrowserSurfaceGeneration::INITIAL,
            Some(7),
            vec![window],
        )
        .unwrap();

        assert_eq!(surface.tabs().count(), 2);
        assert!(!surface.tabs().next().unwrap().discarded());
        assert!(surface.tabs().nth(1).unwrap().discarded());
    }

    #[test]
    fn identities_focus_activity_and_privacy_are_closed() {
        assert_eq!(
            ExtensionBrowserWindow::new(0, false, None, Vec::new()),
            Err(ExtensionBrowserSurfaceError::InvalidWindowIdentity)
        );
        assert_eq!(
            ExtensionBrowserWindow::new(1, false, None, vec![tab(1, true), tab(1, false)]),
            Err(ExtensionBrowserSurfaceError::DuplicateTab)
        );
        assert_eq!(
            ExtensionBrowserWindow::new(1, false, Some(ItemId::from(2)), vec![tab(1, true)]),
            Err(ExtensionBrowserSurfaceError::ActiveTabMissing)
        );

        let regular = ExtensionBrowserWindow::new(1, false, None, vec![tab(1, true)]).unwrap();
        let private = ExtensionBrowserWindow::new(2, true, None, vec![tab(2, false)]).unwrap();
        assert_eq!(
            ExtensionBrowserSurface::new(
                ProfileId::from(9),
                ExtensionBrowserSurfaceGeneration::INITIAL,
                Some(3),
                vec![regular.clone()],
            ),
            Err(ExtensionBrowserSurfaceError::FocusedWindowMissing)
        );
        assert_eq!(
            ExtensionBrowserSurface::new(
                ProfileId::from(9),
                ExtensionBrowserSurfaceGeneration::INITIAL,
                Some(1),
                vec![regular, private],
            ),
            Err(ExtensionBrowserSurfaceError::MixedPrivacyClass)
        );
    }

    #[test]
    fn tab_identity_cannot_alias_across_windows() {
        let first = ExtensionBrowserWindow::new(1, false, None, vec![tab(1, true)]).unwrap();
        let second = ExtensionBrowserWindow::new(2, false, None, vec![tab(1, false)]).unwrap();
        assert_eq!(
            ExtensionBrowserSurface::new(
                ProfileId::from(9),
                ExtensionBrowserSurfaceGeneration::INITIAL,
                None,
                vec![first, second],
            ),
            Err(ExtensionBrowserSurfaceError::DuplicateTab)
        );
    }

    #[test]
    fn generation_is_strictly_positive_and_checked() {
        assert_eq!(ExtensionBrowserSurfaceGeneration::new(0), None);
        assert_eq!(
            ExtensionBrowserSurfaceGeneration::INITIAL.next(),
            ExtensionBrowserSurfaceGeneration::new(2)
        );
        assert_eq!(
            ExtensionBrowserSurfaceGeneration::new(u64::MAX)
                .unwrap()
                .next(),
            None
        );
    }

    #[test]
    fn tab_metadata_is_bounded_and_reuses_unchanged_strings() {
        let url = url::Url::parse("https://example.test/login").unwrap();
        let first = ExtensionBrowserTab::from_snapshot(
            None,
            ItemId::from(1),
            true,
            "Sign in",
            Some(&url),
            true,
            true,
        )
        .unwrap();
        let second = ExtensionBrowserTab::from_snapshot(
            Some(&first),
            ItemId::from(1),
            false,
            "Sign in",
            Some(&url),
            false,
            true,
        )
        .unwrap();

        assert!(Arc::ptr_eq(&first.title, &second.title));
        assert!(Arc::ptr_eq(
            first.url.as_ref().unwrap(),
            second.url.as_ref().unwrap()
        ));
        assert!(second.discarded());
        assert!(!second.loading());
        assert!(second.pinned());
        assert_eq!(second.url(), Some(url.as_str()));

        assert_eq!(
            ExtensionBrowserTab::from_snapshot(
                None,
                ItemId::from(2),
                true,
                "\u{202e}spoofed",
                Some(&url),
                false,
                false,
            ),
            Err(ExtensionBrowserSurfaceError::InvalidTabTitle)
        );
        let file = url::Url::parse("file:///private/secret").unwrap();
        assert_eq!(
            ExtensionBrowserTab::from_snapshot(
                None,
                ItemId::from(2),
                true,
                "Local",
                Some(&file),
                false,
                false,
            ),
            Err(ExtensionBrowserSurfaceError::InvalidTabUrl)
        );
    }
}
