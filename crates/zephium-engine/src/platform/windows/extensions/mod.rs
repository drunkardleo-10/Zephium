//! Profile-bound WebView2 native-extension ownership.
//!
//! This module deliberately exposes no page-world bridge, package selector,
//! or environment constructor. Product startup remains inert until the
//! separately reviewed environment-authority boundary is joined.

mod native;

pub(crate) use native::{
    begin_native_extension_activation, native_extension_cleanup_invariant_failed,
    prepare_native_extension_activation, profile_inventory_is_empty,
    WindowsNativeExtensionActivation, WindowsNativeExtensionFailure, WindowsNativeExtensionOwner,
    WindowsNativeExtensionOwnerReconciliation, WindowsNativeExtensionProfile,
    WindowsNativeExtensionProfileReconciliation, WindowsNativeExtensionRetirement,
};
