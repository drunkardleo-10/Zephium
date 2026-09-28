use std::path::Path;
use zephium_webext::prepare;

/// Added to every package: `nativeMessaging` for the browser's own bridges,
/// and `activeTab`, which grants nothing until the user clicks the
/// extension, so "on click" site access works for every extension, as it
/// does in Chrome.
#[cfg(target_os = "macos")]
const ADDED_PERMISSIONS: &[&str] = &["nativeMessaging", "activeTab"];

/// Extensions' own console errors are for whoever is building the browser;
/// in a release they would cost a message to the browser each and help no one.
#[cfg(target_os = "macos")]
const DIAGNOSTICS: bool = cfg!(any(debug_assertions, feature = "webext-qa"));

#[cfg(target_os = "macos")]
pub(super) fn compat_revision() -> String {
    // FNV-1a over the layer and the Chrome identity it presents; changing
    // either rebuilds every package from its original.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in zephium_webext_macos::compat::SCRIPT
        .bytes()
        .chain(zephium_webext::store::CHROME_VERSION.bytes())
        .chain(ADDED_PERMISSIONS.concat().bytes())
        .chain([u8::from(DIAGNOSTICS)])
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")[..12].to_owned()
}

#[cfg(target_os = "macos")]
fn compat_layer() -> prepare::CompatLayer {
    prepare::CompatLayer::new(zephium_webext_macos::compat::SCRIPT)
        .with_permissions(ADDED_PERMISSIONS)
        .with_diagnostics(DIAGNOSTICS)
}

#[cfg(target_os = "windows")]
pub(super) fn compat_revision() -> String {
    "windows-native-v1".into()
}

pub(super) fn prepare_package(dir: &Path) -> Result<prepare::PrepareReport, prepare::PrepareError> {
    #[cfg(target_os = "macos")]
    {
        prepare::prepare(dir, &compat_layer())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = dir;
        Ok(prepare::PrepareReport::default())
    }
}

pub(super) fn record_signed_key(dir: &Path, key: &[u8]) -> Result<(), String> {
    use base64::Engine as _;
    let path = dir.join("manifest.json");
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    manifest["key"] = base64::engine::general_purpose::STANDARD.encode(key).into();
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
