#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Read-only ordinary-profile authority for the Windows agent cookie bridge.
//!
//! This is the sole seam allowed to join private agent execution to ordinary
//! Browse storage. It exposes only an attested profile-scoped native cookie
//! manager to the sibling host owner; no `ItemId`, cookie field, page value,
//! extension principal, or model-facing representation crosses the seam.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2CookieManager, ICoreWebView2Environment,
};
use zephium_agentic::ContextCookieTransferFailure;
use zephium_core::ids::ProfileId;

use super::EngineHost;

impl EngineHost {
    pub(super) fn selected_profile_cookie_source(
        &self,
        profile: ProfileId,
        expected_environment: &ICoreWebView2Environment,
    ) -> Result<ICoreWebView2CookieManager, ContextCookieTransferFailure> {
        let mut source = None;
        for (id, partition) in &self.partitions {
            if partition.profile() != profile {
                continue;
            }
            let view = self
                .views
                .get(id)
                .ok_or(ContextCookieTransferFailure::SourceUnavailable)?;
            let manager = crate::platform::imp::selected_profile_cookie_manager(
                &view.view,
                expected_environment,
            )?;
            if source.is_none() {
                source = Some(manager);
            }
        }
        if let Some(spare) = self
            .spare
            .as_ref()
            .filter(|spare| spare.partition.profile() == profile)
        {
            let manager = crate::platform::imp::selected_profile_cookie_manager(
                &spare.view.view,
                expected_environment,
            )?;
            if source.is_none() {
                source = Some(manager);
            }
        }
        source.ok_or(ContextCookieTransferFailure::SourceUnavailable)
    }
}
