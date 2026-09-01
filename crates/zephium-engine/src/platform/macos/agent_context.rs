//! Extension-free, hidden WKWebView construction for owned agent contexts.

use raw_window_handle::HasWindowHandle;
use wry::{
    DownloadPolicy, PageClosePolicy, WebView, WebViewBuilder, WebViewBuilderExtDarwin as _,
    WebViewBuilderExtMacos as _,
};
use zephium_agentic::ContextProfileStorageClass;
use zephium_core::ids::ProfileId;

use super::WebsiteDataStore;

/// Closed construction failure mapped to the public native-port taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentOwnedViewConstructionError {
    /// The exact selected-profile storage class or identity was not retained.
    Storage,
    /// Extension or user-script absence could not be proven.
    ExtensionIsolation,
    /// Wry/WebKit refused hidden child-view construction or hardening.
    Native,
}

/// Builds one initially hidden, extension-free selected-profile WKWebView.
///
/// The only initial document is `about:blank`. Network navigation remains
/// denied until a later exact context-navigation adapter is installed, so the
/// caller can attach native content policy before any web request exists.
pub(crate) fn build_owned_agent_view(
    parent: &impl HasWindowHandle,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
) -> Result<WebView, AgentOwnedViewConstructionError> {
    let builder = WebViewBuilder::new()
        .with_url("about:blank")
        .with_visible(false)
        .with_focused(false)
        .with_devtools(false)
        .with_autoplay(false)
        .with_fullscreen_enabled(false)
        .with_picture_in_picture_enabled(false)
        .with_general_autofill_enabled(false)
        .with_navigation_handler(|target| target == "about:blank")
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
        .with_download_policy(DownloadPolicy::DenyWithoutMetadata)
        .with_page_close_policy(PageClosePolicy::Ignore)
        .with_allow_link_preview(false);

    let builder = match (storage_class, ephemeral_store) {
        (ContextProfileStorageClass::Ephemeral, Some(store)) => {
            let configuration = super::new_configuration_with_data_store(store)
                .map_err(|_| AgentOwnedViewConstructionError::Storage)?;
            builder
                .with_incognito(true)
                .with_webview_configuration(configuration)
        }
        (ContextProfileStorageClass::Durable, None) => {
            builder.with_data_store_identifier(profile.bytes())
        }
        (ContextProfileStorageClass::Durable, Some(_))
        | (ContextProfileStorageClass::Ephemeral, None) => {
            return Err(AgentOwnedViewConstructionError::Storage);
        }
    };

    let view = builder
        .build_as_child(parent)
        .map_err(|_| AgentOwnedViewConstructionError::Native)?;
    attest_owned_agent_view(&view, profile, storage_class, ephemeral_store)?;
    Ok(view)
}

fn attest_owned_agent_view(
    view: &WebView,
    profile: ProfileId,
    storage_class: ContextProfileStorageClass,
    ephemeral_store: Option<&WebsiteDataStore>,
) -> Result<(), AgentOwnedViewConstructionError> {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSView};

    let page = super::native_webview(view);
    let configuration = unsafe { page.configuration() };
    if unsafe { configuration.webExtensionController() }.is_some() {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }
    let controller = unsafe { configuration.userContentController() };
    if unsafe { controller.userScripts() }.count() != 0 {
        return Err(AgentOwnedViewConstructionError::ExtensionIsolation);
    }

    let actual_store = unsafe { configuration.websiteDataStore() };
    let persistent = unsafe { actual_store.isPersistent() };
    let identifier = unsafe { actual_store.identifier() }.map(|value| value.as_bytes());
    let storage_valid = match storage_class {
        ContextProfileStorageClass::Durable => {
            ephemeral_store.is_none() && persistent && identifier == Some(profile.bytes())
        }
        ContextProfileStorageClass::Ephemeral => {
            let Some(expected) = ephemeral_store else {
                return Err(AgentOwnedViewConstructionError::Storage);
            };
            !persistent
                && identifier.is_none()
                && Retained::as_ptr(&actual_store) == Retained::as_ptr(expected)
        }
    };
    if !storage_valid {
        return Err(AgentOwnedViewConstructionError::Storage);
    }

    unsafe { page.setInspectable(false) };
    let native_view: &NSView = &page;
    native_view.setTranslatesAutoresizingMaskIntoConstraints(true);
    native_view.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
    native_view.setHidden(true);
    if !native_view.isHidden() {
        return Err(AgentOwnedViewConstructionError::Native);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn construction_source_has_no_model_or_page_program_surface() {
        let source = include_str!("agent_context.rs");
        for forbidden in [
            concat!("with_ipc_", "handler"),
            concat!("with_initialization_", "script"),
            concat!("evaluate_", "script"),
            concat!("with_new_window_req_", "handler"),
            concat!("with_web_extension_", "controller"),
        ] {
            assert!(!source.contains(forbidden));
        }
        assert!(source.contains("with_navigation_handler(|target| target == \"about:blank\")"));
        assert!(source.contains("controller.userScripts()"));
    }
}
