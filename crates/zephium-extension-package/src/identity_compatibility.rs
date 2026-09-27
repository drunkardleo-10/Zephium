//! Pure URL policy for a Chromium-ID-bound WebAuth flow.
//!
//! This module does not grant the `identity` permission or open a WebView. A
//! native host must bind the ID to an authenticated package and the navigation
//! to its original extension context and profile before using this policy.

use crate::ChromiumExtensionId;
use url::Url;

/// Maximum size of a launch URL or captured redirect, including query and
/// fragment. In particular, do not allow an unbounded token-bearing result.
pub const MAX_IDENTITY_URL_BYTES: usize = 8 * 1024;

/// A launch or navigation URL failed the closed WebAuth URL policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityUrlError {
    /// The URL is empty, too long, malformed, or contains control characters.
    InvalidUrl,
    /// The URL uses a scheme other than HTTPS, or has URL user information.
    UnsafeOrigin,
    /// A `chromiumapp.org` callback names a different extension.
    ForeignRedirect,
}

/// Whether one main-frame navigation may load, or completes the auth flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityNavigation<'a> {
    /// Continue loading in the profile-bound, flow-owned WebView.
    Allow,
    /// Cancel navigation before any request to the redirect host and return
    /// the exact original URL to the initiating extension callback.
    Complete(&'a str),
}

/// Reason an active WebAuth flow ended without a matching redirect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityFlowFailure {
    /// Initial or later navigation violated the URL policy.
    UnsafeNavigation,
    /// A noninteractive page loaded without redirecting.
    RedirectNotReached,
    /// The flow-owned window was closed by the user.
    WindowClosed,
    /// The initiating extension context was unloaded or disabled.
    ContextGone,
    /// The flow's one-shot native timeout expired.
    TimedOut,
}

/// Decision returned by a main-thread WebAuth flow owner. A terminal decision
/// must settle its extension callback exactly once and release its WebView.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityFlowDecision<'a> {
    /// Continue the navigation or wait for another event.
    Allow,
    /// Cancel only this navigation, keeping the flow alive.
    CancelNavigation,
    /// A page became visible and may now be presented in interactive mode.
    Present,
    /// Resolve with the exact callback URL before WebKit can load it.
    Resolve(&'a str),
    /// Reject and release the active flow.
    Reject(IdentityFlowFailure),
    /// The flow already settled; a late native callback has no authority.
    Stale,
}

/// One active flow's noninteractive load and exactly-once terminal policy.
/// The native owner supplies a timer, context membership and resource lease.
pub struct IdentityFlow {
    id: ChromiumExtensionId,
    interactive: bool,
    abort_on_load_for_noninteractive: bool,
    presented: bool,
    settled: bool,
}

impl IdentityFlow {
    /// Validates the launch URL and creates a dormant-on-idle flow. The
    /// caller opens one profile-bound WebView only after this succeeds.
    pub fn new(
        id: ChromiumExtensionId,
        launch_url: &str,
        interactive: bool,
        abort_on_load_for_noninteractive: bool,
    ) -> Result<Self, IdentityUrlError> {
        validate_launch_url(launch_url, &id)?;
        Ok(Self {
            id,
            interactive,
            abort_on_load_for_noninteractive,
            presented: false,
            settled: false,
        })
    }

    /// Classifies one navigation before allowing WebKit to send it. Only a
    /// top-level redirect to this exact ID can complete the flow.
    pub fn navigation<'a>(&mut self, url: &'a str, main_frame: bool) -> IdentityFlowDecision<'a> {
        if self.settled {
            return IdentityFlowDecision::Stale;
        }
        if !main_frame {
            return if allow_subframe_navigation(url) {
                IdentityFlowDecision::Allow
            } else {
                IdentityFlowDecision::CancelNavigation
            };
        }
        match classify_main_frame_navigation(url, &self.id) {
            Ok(IdentityNavigation::Allow) => IdentityFlowDecision::Allow,
            Ok(IdentityNavigation::Complete(url)) => {
                self.settled = true;
                IdentityFlowDecision::Resolve(url)
            }
            Err(_) => self.reject(IdentityFlowFailure::UnsafeNavigation),
        }
    }

    /// Handles completion of one top-level page load. Chrome presents the
    /// interactive view only after a page loads; noninteractive mode aborts
    /// then unless a bounded JavaScript redirect grace period was requested.
    pub fn page_loaded(&mut self) -> IdentityFlowDecision<'static> {
        if self.settled {
            return IdentityFlowDecision::Stale;
        }
        if self.interactive {
            if self.presented {
                IdentityFlowDecision::Allow
            } else {
                self.presented = true;
                IdentityFlowDecision::Present
            }
        } else if self.abort_on_load_for_noninteractive {
            self.reject(IdentityFlowFailure::RedirectNotReached)
        } else {
            IdentityFlowDecision::Allow
        }
    }

    /// Handles user cancellation or closing the flow-owned window.
    pub fn window_closed(&mut self) -> IdentityFlowDecision<'static> {
        self.reject(IdentityFlowFailure::WindowClosed)
    }

    /// Handles extension disable, unload, profile teardown or replacement.
    pub fn context_gone(&mut self) -> IdentityFlowDecision<'static> {
        self.reject(IdentityFlowFailure::ContextGone)
    }

    /// Handles the native one-shot timeout.
    pub fn timed_out(&mut self) -> IdentityFlowDecision<'static> {
        self.reject(IdentityFlowFailure::TimedOut)
    }

    fn reject(&mut self, reason: IdentityFlowFailure) -> IdentityFlowDecision<'static> {
        if self.settled {
            IdentityFlowDecision::Stale
        } else {
            self.settled = true;
            IdentityFlowDecision::Reject(reason)
        }
    }
}

/// Generates the URL that Chrome's `identity.getRedirectURL` produces for an
/// authenticated extension ID. Chrome prepends `/` to a path without one and
/// uses `/` when the argument is absent. An empty provided path becomes `/`.
pub fn redirect_url(
    id: &ChromiumExtensionId,
    path: Option<&str>,
) -> Result<String, IdentityUrlError> {
    let path = match path {
        Some(path) if path.starts_with('/') => path,
        Some(path) if !path.is_empty() => return redirect_url(id, Some(&format!("/{path}"))),
        _ => "/",
    };
    if path.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(IdentityUrlError::InvalidUrl);
    }
    let result = format!("https://{}.chromiumapp.org{path}", id.as_str());
    if result.len() > MAX_IDENTITY_URL_BYTES {
        return Err(IdentityUrlError::InvalidUrl);
    }
    Ok(result)
}

/// Validates the first provider URL before constructing an auth surface.
pub fn validate_launch_url(url: &str, id: &ChromiumExtensionId) -> Result<(), IdentityUrlError> {
    let parsed = parse_safe_https(url)?;
    if is_chromiumapp_host(&parsed) && !is_own_redirect(&parsed, id) {
        return Err(IdentityUrlError::ForeignRedirect);
    }
    Ok(())
}

/// Classifies a main-frame navigation before WebKit sends the request. The
/// caller must reject subframes aimed at `chromiumapp.org` separately; they
/// cannot complete the top-level flow.
pub fn classify_main_frame_navigation<'a>(
    url: &'a str,
    id: &ChromiumExtensionId,
) -> Result<IdentityNavigation<'a>, IdentityUrlError> {
    let parsed = parse_safe_https(url)?;
    if is_own_redirect(&parsed, id) {
        return Ok(IdentityNavigation::Complete(url));
    }
    if is_chromiumapp_host(&parsed) {
        return Err(IdentityUrlError::ForeignRedirect);
    }
    Ok(IdentityNavigation::Allow)
}

/// Returns whether a subframe may navigate. A callback host is never loaded
/// in a subframe, since this flow only captures top-level redirects.
pub fn allow_subframe_navigation(url: &str) -> bool {
    parse_safe_https(url).is_ok_and(|url| !is_chromiumapp_host(&url))
}

fn parse_safe_https(value: &str) -> Result<Url, IdentityUrlError> {
    if value.is_empty()
        || value.len() > MAX_IDENTITY_URL_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(IdentityUrlError::InvalidUrl);
    }
    let url = Url::parse(value).map_err(|_| IdentityUrlError::InvalidUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(IdentityUrlError::UnsafeOrigin);
    }
    Ok(url)
}

fn is_own_redirect(url: &Url, id: &ChromiumExtensionId) -> bool {
    url.port().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == format!("{}.chromiumapp.org", id.as_str()))
}

fn is_chromiumapp_host(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| host == "chromiumapp.org" || host.ends_with(".chromiumapp.org"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ChromiumExtensionId {
        ChromiumExtensionId::parse("abcdefghijklmnopabcdefghijklmnop").unwrap()
    }

    #[test]
    fn redirect_url_uses_authenticated_id_and_chrome_path_rules() {
        let id = id();
        let root = "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/";
        assert_eq!(redirect_url(&id, None).unwrap(), root);
        assert_eq!(redirect_url(&id, Some("")).unwrap(), root);
        assert_eq!(
            redirect_url(&id, Some("callback")).unwrap(),
            format!("{root}callback")
        );
        assert_eq!(
            redirect_url(&id, Some("/callback")).unwrap(),
            format!("{root}callback")
        );
    }

    #[test]
    fn callback_is_captured_before_load_with_exact_query_and_fragment() {
        let callback = format!(
            "{}?code=a%2Fb#state=opaque",
            redirect_url(&id(), Some("cb")).unwrap()
        );
        assert_eq!(
            classify_main_frame_navigation(&callback, &id()),
            Ok(IdentityNavigation::Complete(callback.as_str()))
        );
        assert!(!allow_subframe_navigation(&callback));
    }

    #[test]
    fn malformed_or_foreign_redirects_cannot_complete_or_load() {
        let id = id();
        for url in [
            "http://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/cb",
            "https://evil@abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/cb",
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org.evil.test/cb",
            "https://other.chromiumapp.org/cb",
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org:444/cb",
            "https://chromiumapp.org/cb",
            "https://sub.abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/cb",
            "javascript:alert(1)",
            "https://example.test/\ncb",
        ] {
            assert!(
                !matches!(
                    classify_main_frame_navigation(url, &id),
                    Ok(IdentityNavigation::Complete(_))
                ),
                "{url}"
            );
        }
        assert_eq!(
            classify_main_frame_navigation("https://other.chromiumapp.org/cb", &id),
            Err(IdentityUrlError::ForeignRedirect)
        );
    }

    #[test]
    fn provider_navigation_is_https_only_and_bounded() {
        let id = id();
        assert_eq!(
            validate_launch_url("https://accounts.example.test/login", &id),
            Ok(())
        );
        assert_eq!(
            classify_main_frame_navigation("https://accounts.example.test/consent", &id),
            Ok(IdentityNavigation::Allow)
        );
        assert_eq!(
            validate_launch_url("http://accounts.example.test/login", &id),
            Err(IdentityUrlError::UnsafeOrigin)
        );
        assert_eq!(
            validate_launch_url(
                &format!(
                    "https://example.test/{}",
                    "x".repeat(MAX_IDENTITY_URL_BYTES)
                ),
                &id
            ),
            Err(IdentityUrlError::InvalidUrl)
        );
    }

    #[test]
    fn noninteractive_flow_aborts_on_load_or_waits_for_explicit_js_grace() {
        let launch = "https://accounts.example.test/login";
        let mut immediate = IdentityFlow::new(id(), launch, false, true).unwrap();
        assert_eq!(
            immediate.navigation(launch, true),
            IdentityFlowDecision::Allow
        );
        assert_eq!(
            immediate.page_loaded(),
            IdentityFlowDecision::Reject(IdentityFlowFailure::RedirectNotReached)
        );
        assert_eq!(immediate.timed_out(), IdentityFlowDecision::Stale);

        let mut grace = IdentityFlow::new(id(), launch, false, false).unwrap();
        assert_eq!(grace.page_loaded(), IdentityFlowDecision::Allow);
        let callback = redirect_url(&id(), Some("done#token=opaque")).unwrap();
        assert_eq!(
            grace.navigation(&callback, true),
            IdentityFlowDecision::Resolve(callback.as_str())
        );
        assert_eq!(grace.context_gone(), IdentityFlowDecision::Stale);
    }

    #[test]
    fn interactive_flow_presents_once_and_closes_once() {
        let mut flow =
            IdentityFlow::new(id(), "https://accounts.example.test/login", true, true).unwrap();
        assert_eq!(flow.page_loaded(), IdentityFlowDecision::Present);
        assert_eq!(flow.page_loaded(), IdentityFlowDecision::Allow);
        let callback = redirect_url(&id(), None).unwrap();
        assert_eq!(
            flow.navigation(&callback, false),
            IdentityFlowDecision::CancelNavigation
        );
        assert_eq!(
            flow.window_closed(),
            IdentityFlowDecision::Reject(IdentityFlowFailure::WindowClosed)
        );
        assert_eq!(
            flow.navigation(&callback, true),
            IdentityFlowDecision::Stale
        );
    }

    #[test]
    fn disable_and_timeout_settle_only_the_active_flow() {
        let launch = "https://accounts.example.test/login";
        let mut unloaded = IdentityFlow::new(id(), launch, true, true).unwrap();
        assert_eq!(
            unloaded.context_gone(),
            IdentityFlowDecision::Reject(IdentityFlowFailure::ContextGone)
        );
        assert_eq!(unloaded.window_closed(), IdentityFlowDecision::Stale);
        let mut expired = IdentityFlow::new(id(), launch, false, false).unwrap();
        assert_eq!(
            expired.timed_out(),
            IdentityFlowDecision::Reject(IdentityFlowFailure::TimedOut)
        );
        assert_eq!(expired.page_loaded(), IdentityFlowDecision::Stale);
    }
}
