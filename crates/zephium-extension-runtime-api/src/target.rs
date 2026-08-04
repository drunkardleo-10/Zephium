//! Runtime backend selection.

/// The native runtime family selected for an extension package.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeTarget {
    /// A platform-native browser-extension runtime, such as WebView2's
    /// extension support.
    NativeWebExtension,
    /// Zephium's compatibility runtime for the supported extension subset.
    Compatibility,
}
