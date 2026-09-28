//! Installed Chrome extensions: Web Store download, consent, the per-profile
//! registry on disk, and handing enabled extensions to the shell.
//!
//! Layout under the app data directory:
//! `webext/packages/<id>/<version>.crx` keeps the signed original, and
//! `webext/packages/<id>/<version>-<compat>/` is the tree WebKit loads,
//! rebuilt from the original whenever the compatibility layer changes.
//! `webext/profiles/<profile>.json` records what a profile installed.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State, WebviewWindow};
use zephium_app::Handle;
use zephium_core::ids::ProfileId;

use crate::{authorize, shutdown_started, CallerPolicy};

pub(crate) struct WebExtensions {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    root: PathBuf,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    state: Mutex<Option<Pending>>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Pending {
    profile: ProfileId,
    entry: Entry,
    staged: PathBuf,
    crx: Vec<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Registry {
    extensions: Vec<Entry>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    install: String,
    id: String,
    name: String,
    version: String,
    description: String,
    enabled: bool,
    /// Directory under `packages/<id>/` WebKit loads.
    package: String,
    /// Compatibility layer the package was prepared with.
    compat: String,
    /// Compatibility layer WebKit last started the background with.
    #[serde(default)]
    started: String,
    permissions: Vec<String>,
    hosts: Vec<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
pub(crate) struct WebExtensionReview {
    id: String,
    name: String,
    version: String,
    description: String,
    warnings: Vec<String>,
    icon: Option<String>,
    update: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
pub(crate) struct WebExtensionView {
    id: String,
    name: String,
    version: String,
    description: String,
    enabled: bool,
    icon: Option<String>,
    /// `running`, `starting`, `failed` or `off`.
    state: String,
    error: Option<String>,
    warnings: Vec<String>,
}

impl WebExtensions {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("webext"),
            state: Mutex::new(None),
        }
    }

    /// Restores every profile's extensions at launch.
    pub(crate) fn restore(&self, shell: &Handle) {
        if let Some(data_dir) = self.root.parent() {
            remove_previous_repository(data_dir);
        }
        #[cfg(target_os = "macos")]
        imp::restore(self, shell);
        #[cfg(not(target_os = "macos"))]
        let _ = shell;
    }
}

/// Removes the package repository left by the previous extension stack, in
/// the background and best effort. It sealed its directories read-only, so
/// they are made writable first.
fn remove_previous_repository(data_dir: &Path) {
    let path = data_dir.join("extension-repository-v1");
    if !path.exists() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("zephium-legacy-extension-cleanup".into())
        .spawn(move || {
            unseal(&path);
            if let Err(error) = std::fs::remove_dir_all(&path) {
                eprintln!("extensions: could not remove the previous repository: {error}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("extensions: could not start the previous repository cleanup: {error}");
    }
}

fn unseal(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(if metadata.is_dir() { 0o700 } else { 0o600 })
    };
    #[cfg(not(unix))]
    let permissions = {
        let mut permissions = metadata.permissions();
        // Windows only clears the read-only attribute.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        permissions
    };
    let _ = std::fs::set_permissions(path, permissions);
    if metadata.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                unseal(&entry.path());
            }
        }
    }
}

const UNAVAILABLE: &str = "Extensions are unavailable in this build.";

/// Downloads and verifies the extension on the active store page, and
/// returns what the user is asked to approve.
#[tauri::command]
#[specta::specta]
pub(crate) async fn web_extension_prepare(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    extensions: State<'_, WebExtensions>,
    tab_id: String,
) -> Result<WebExtensionReview, String> {
    if !authorize(&caller, CallerPolicy::Main, "web_extension_prepare")
        || shutdown_started(caller.app_handle())
    {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(target_os = "macos")]
    return imp::prepare(&shell, &extensions, &tab_id).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (shell, extensions, tab_id);
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn web_extension_confirm(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    extensions: State<'_, WebExtensions>,
    id: String,
) -> Result<(), String> {
    if !authorize(&caller, CallerPolicy::Main, "web_extension_confirm")
        || shutdown_started(caller.app_handle())
    {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(target_os = "macos")]
    return imp::confirm(&shell, &extensions, &id);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (shell, extensions, id);
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn web_extension_cancel(caller: WebviewWindow, extensions: State<'_, WebExtensions>) {
    if !authorize(&caller, CallerPolicy::Main, "web_extension_cancel") {
        return;
    }
    if let Ok(mut pending) = extensions.state.lock() {
        if let Some(pending) = pending.take() {
            let _ = std::fs::remove_dir_all(pending.staged);
        }
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn web_extension_list(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    extensions: State<'_, WebExtensions>,
) -> Result<Vec<WebExtensionView>, String> {
    if !authorize(&caller, CallerPolicy::Main, "web_extension_list") {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(target_os = "macos")]
    return imp::list(&shell, &extensions).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (shell, extensions);
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn web_extension_set_enabled(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    extensions: State<'_, WebExtensions>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    if !authorize(&caller, CallerPolicy::Main, "web_extension_set_enabled") {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(target_os = "macos")]
    return imp::set_enabled(&shell, &extensions, &id, enabled).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (shell, extensions, id, enabled);
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn web_extension_uninstall(
    caller: WebviewWindow,
    shell: State<'_, Handle>,
    extensions: State<'_, WebExtensions>,
    id: String,
) -> Result<(), String> {
    if !authorize(&caller, CallerPolicy::Main, "web_extension_uninstall") {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(target_os = "macos")]
    return imp::uninstall(&shell, &extensions, &id).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (shell, extensions, id);
        Err(UNAVAILABLE.into())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use zephium_app::{Handle, WebExtensionStatus};
    use zephium_core::ids::{ExtensionInstallId, ItemId, ProfileId};
    use zephium_core::ports::engine::WebExtensionLoad;
    use zephium_webext::manifest::Manifest;
    use zephium_webext::{archive, crx, permissions, prepare, store, ExtensionId};

    use super::{Entry, Pending, Registry, WebExtensionReview, WebExtensionView, WebExtensions};

    const MAX_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;

    fn compat_revision() -> String {
        // FNV-1a over the layer and the Chrome identity it presents; changing
        // either rebuilds every package from its original.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in zephium_webext_macos::compat::SCRIPT
            .bytes()
            .chain(zephium_webext_macos::compat::CHROME_VERSION.bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{hash:016x}")[..12].to_owned()
    }

    fn compat_layer() -> prepare::CompatLayer {
        prepare::CompatLayer::new(zephium_webext_macos::compat::SCRIPT)
            .with_permissions(&["nativeMessaging"])
    }

    impl WebExtensions {
        fn packages(&self, id: &str) -> PathBuf {
            self.root.join("packages").join(id)
        }

        fn registry_path(&self, profile: ProfileId) -> PathBuf {
            self.root.join("profiles").join(format!("{profile}.json"))
        }

        fn registry(&self, profile: ProfileId) -> Registry {
            std::fs::read(self.registry_path(profile))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        }

        fn save(&self, profile: ProfileId, registry: &Registry) -> Result<(), String> {
            let path = self.registry_path(profile);
            let parent = path.parent().ok_or("invalid registry path")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let temporary = path.with_extension("json.tmp");
            let bytes = serde_json::to_vec_pretty(registry).map_err(|e| e.to_string())?;
            std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())
        }

        fn profiles(&self) -> Vec<ProfileId> {
            std::fs::read_dir(self.root.join("profiles"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    ProfileId::parse(name.strip_suffix(".json")?)
                })
                .collect()
        }

        /// Hands a profile's enabled extensions to the shell, rebuilding any
        /// package prepared by an older compatibility layer.
        fn load(
            &self,
            entry: &Entry,
            install: ExtensionInstallId,
            start_background: bool,
        ) -> WebExtensionLoad {
            WebExtensionLoad {
                install,
                extension_id: entry.id.clone(),
                root: self.packages(&entry.id).join(&entry.package),
                permissions: entry.permissions.clone(),
                match_patterns: entry.hosts.clone(),
                start_background,
            }
        }

        /// Deletes package files no registry refers to: removed extensions,
        /// which are erased by the browser before their files go, and builds
        /// replaced by updates or compatibility-layer changes.
        fn prune(&self) {
            let mut kept: HashMap<String, Vec<String>> = HashMap::new();
            for profile in self.profiles() {
                for entry in self.registry(profile).extensions {
                    let files = kept.entry(entry.id.clone()).or_default();
                    files.push(entry.package.clone());
                    files.push(format!("{}.crx", entry.version));
                }
            }
            let packages = self.root.join("packages");
            std::thread::spawn(move || prune_packages(&packages, &kept));
        }

        fn apply(&self, shell: &Handle, profile: ProfileId, registry: &mut Registry) {
            let revision = compat_revision();
            let mut changed = false;
            let mut loads = Vec::new();
            for entry in registry.extensions.iter_mut() {
                if entry.compat != revision {
                    match self.rebuild(entry, &revision) {
                        Ok(()) => changed = true,
                        Err(error) => {
                            eprintln!("extensions: could not rebuild {}: {error}", entry.id);
                            continue;
                        }
                    }
                }
                if !entry.enabled {
                    continue;
                }
                let Some(install) = ExtensionInstallId::parse(&entry.install) else {
                    continue;
                };
                let start_background = entry.started != revision;
                if start_background {
                    entry.started = revision.clone();
                    changed = true;
                }
                loads.push(self.load(entry, install, start_background));
            }
            if changed {
                let _ = self.save(profile, registry);
            }
            shell.dispatch(zephium_app::Command::SetWebExtensions {
                profile,
                extensions: loads,
            });
        }

        fn rebuild(&self, entry: &mut Entry, revision: &str) -> Result<(), String> {
            let packages = self.packages(&entry.id);
            let bytes = std::fs::read(packages.join(format!("{}.crx", entry.version)))
                .map_err(|e| e.to_string())?;
            let expected = ExtensionId::parse(&entry.id).ok_or("invalid extension id")?;
            let verified = crx::verify(&bytes, Some(&expected)).map_err(|e| e.to_string())?;
            let package = format!("{}-{revision}", entry.version);
            let target = packages.join(&package);
            let _ = std::fs::remove_dir_all(&target);
            archive::extract(verified.zip, &target, &archive::Limits::default())
                .map_err(|e| e.to_string())?;
            if let Err(error) = prepare::prepare(&target, &compat_layer()) {
                let _ = std::fs::remove_dir_all(&target);
                return Err(error.to_string());
            }
            let old = std::mem::replace(&mut entry.package, package);
            if old != entry.package {
                let _ = std::fs::remove_dir_all(packages.join(old));
            }
            entry.compat = revision.to_owned();
            Ok(())
        }
    }

    fn prune_packages(packages: &Path, kept: &HashMap<String, Vec<String>>) {
        for extension in std::fs::read_dir(packages).into_iter().flatten().flatten() {
            let name = extension.file_name().to_string_lossy().into_owned();
            let Some(files) = kept.get(&name) else {
                let _ = std::fs::remove_dir_all(extension.path());
                continue;
            };
            for file in std::fs::read_dir(extension.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                if files.iter().any(|kept| file.file_name() == kept.as_str()) {
                    continue;
                }
                let path = file.path();
                let _ = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
            }
        }
    }

    #[cfg(test)]
    #[test]
    fn pruning_keeps_only_the_packages_registries_use() {
        let packages = tempfile::tempdir().unwrap();
        for path in ["kept/1.2-aaaa", "kept/1.1-aaaa", "removed/1.0-aaaa"] {
            std::fs::create_dir_all(packages.path().join(path)).unwrap();
        }
        for path in ["kept/1.2.crx", "kept/1.1.crx", "removed/1.0.crx"] {
            std::fs::write(packages.path().join(path), b"crx").unwrap();
        }
        let kept = HashMap::from([(
            "kept".to_string(),
            vec!["1.2-aaaa".to_string(), "1.2.crx".to_string()],
        )]);
        prune_packages(packages.path(), &kept);
        let mut left: Vec<_> = std::fs::read_dir(packages.path().join("kept"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["1.2-aaaa", "1.2.crx"]);
        assert!(!packages.path().join("removed").exists());
    }

    pub(super) fn restore(extensions: &WebExtensions, shell: &Handle) {
        for profile in extensions.profiles() {
            let mut registry = extensions.registry(profile);
            extensions.apply(shell, profile, &mut registry);
        }
        extensions.prune();
    }

    async fn target(
        shell: &Handle,
        tab: Option<ItemId>,
    ) -> Result<zephium_app::WebExtensionTarget, String> {
        let receiver = shell.web_extension_target(tab);
        tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(2)))
            .await
            .map_err(|_| "The browser is unavailable.")?
            .map_err(|_| "The browser did not respond.")?
            .ok_or_else(|| "Extensions can't be installed in this window.".to_owned())
    }

    async fn download(id: &ExtensionId) -> Result<Vec<u8>, String> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|_| "Downloads are unavailable.")?;
        let url = store::download_url(id, zephium_webext_macos::compat::CHROME_VERSION);
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|_| "The Chrome Web Store could not be reached.")?;
        if !response.status().is_success() {
            return Err(format!(
                "The Chrome Web Store answered {}.",
                response.status()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PACKAGE_BYTES)
        {
            return Err("The extension is too large.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "The download was interrupted.")?
        {
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > MAX_PACKAGE_BYTES {
                return Err("The extension is too large.".into());
            }
        }
        Ok(bytes)
    }

    fn icon_data_url(dir: &Path, manifest: &Manifest) -> Option<String> {
        use base64::Engine as _;
        let path = manifest.icons().best(48)?.to_owned();
        let bytes = std::fs::read(dir.join(&path))
            .ok()
            .filter(|bytes| bytes.len() < 512 * 1024)?;
        let mime = if path.ends_with(".svg") {
            "image/svg+xml"
        } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
            "image/jpeg"
        } else {
            "image/png"
        };
        Some(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    pub(super) async fn prepare(
        shell: &Handle,
        extensions: &WebExtensions,
        tab_id: &str,
    ) -> Result<WebExtensionReview, String> {
        let tab = ItemId::parse(tab_id).ok_or("Invalid tab.")?;
        let target = target(shell, Some(tab)).await?;
        let listing = target
            .listing_url
            .ok_or("Open an extension's Chrome Web Store page first.")?;
        let id = store::listing_id(&listing).ok_or("This isn't an extension page.")?;
        let bytes = download(&id).await?;

        let root = extensions.root.clone();
        let profile = target.profile;
        let existing = extensions
            .registry(profile)
            .extensions
            .into_iter()
            .find(|entry| entry.id == id.as_str());
        let staged = tauri::async_runtime::spawn_blocking(move || {
            stage(&root, &id, bytes, profile, existing)
        })
        .await
        .map_err(|_| "Installation failed.")??;
        let review = staged.review.clone();
        let mut pending = extensions
            .state
            .lock()
            .map_err(|_| "Installation failed.")?;
        if let Some(previous) = pending.replace(staged.pending) {
            let _ = std::fs::remove_dir_all(previous.staged);
        }
        Ok(review)
    }

    struct Staged {
        review: WebExtensionReview,
        pending: Pending,
    }

    fn stage(
        root: &Path,
        id: &ExtensionId,
        bytes: Vec<u8>,
        profile: ProfileId,
        existing: Option<Entry>,
    ) -> Result<Staged, String> {
        let verified = crx::verify(&bytes, Some(id))
            .map_err(|_| "The package isn't correctly signed by its publisher.")?;
        let staging = root.join("staging");
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let dir = staging.join(format!("{id}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        archive::extract(verified.zip, &dir, &archive::Limits::default())
            .map_err(|error| format!("The package can't be unpacked safely ({error})."))?;
        let result = (|| {
            let manifest = Manifest::load(&dir)
                .map_err(|error| format!("The extension's manifest is invalid ({error})."))?;
            let warnings = permissions::warnings(&manifest);
            let icon = icon_data_url(&dir, &manifest);
            let mut hosts = manifest.host_permissions();
            for script in manifest.content_scripts() {
                hosts.extend(script.matches);
            }
            hosts.sort();
            hosts.dedup();
            let version = manifest
                .version()
                .ok_or("The extension has no version.")?
                .to_owned();
            let name = manifest.name().unwrap_or_else(|| id.to_string());
            let description = manifest.description().unwrap_or_default().to_owned();
            let report = prepare::prepare(&dir, &compat_layer())
                .map_err(|error| format!("The extension can't be prepared ({error})."))?;
            let mut permissions = manifest.permissions();
            permissions.extend(report.added_permissions);
            let revision = compat_revision();
            let entry = Entry {
                install: existing
                    .as_ref()
                    .map(|entry| entry.install.clone())
                    .unwrap_or_else(|| ExtensionInstallId::generate().to_string()),
                id: id.to_string(),
                name: name.clone(),
                version: version.clone(),
                description: description.clone(),
                enabled: true,
                package: format!("{version}-{revision}"),
                compat: revision,
                started: String::new(),
                permissions,
                hosts,
                icon: icon.clone(),
            };
            Ok(Staged {
                review: WebExtensionReview {
                    id: id.to_string(),
                    name,
                    version,
                    description,
                    warnings,
                    icon,
                    update: existing.is_some(),
                },
                pending: Pending {
                    profile,
                    entry,
                    staged: dir.clone(),
                    crx: bytes,
                },
            })
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    }

    pub(super) fn confirm(
        shell: &Handle,
        extensions: &WebExtensions,
        id: &str,
    ) -> Result<(), String> {
        let pending = extensions
            .state
            .lock()
            .map_err(|_| "Installation failed.")?
            .take()
            .filter(|pending| pending.entry.id == id)
            .ok_or("Nothing is waiting to be installed.")?;
        let packages = extensions.packages(id);
        std::fs::create_dir_all(&packages).map_err(|e| e.to_string())?;
        std::fs::write(
            packages.join(format!("{}.crx", pending.entry.version)),
            &pending.crx,
        )
        .map_err(|e| e.to_string())?;
        let target = packages.join(&pending.entry.package);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&pending.staged, &target).map_err(|e| e.to_string())?;

        let mut registry = extensions.registry(pending.profile);
        let previous = registry.extensions.iter().position(|entry| entry.id == id);
        let old_package = previous.map(|index| registry.extensions.remove(index));
        registry.extensions.push(pending.entry);
        extensions.save(pending.profile, &registry)?;
        if let Some(old) = old_package.filter(|old| {
            old.package
                != registry
                    .extensions
                    .last()
                    .map(|e| e.package.clone())
                    .unwrap_or_default()
        }) {
            let _ = std::fs::remove_dir_all(packages.join(old.package));
        }
        extensions.apply(shell, pending.profile, &mut registry);
        Ok(())
    }

    pub(super) async fn list(
        shell: &Handle,
        extensions: &WebExtensions,
    ) -> Result<Vec<WebExtensionView>, String> {
        // Before the session exists there is nothing to list yet; the build
        // still supports extensions.
        let Ok(profile) = target(shell, None).await.map(|target| target.profile) else {
            return Ok(Vec::new());
        };
        let receiver = shell.web_extension_status(profile);
        let status: HashMap<String, WebExtensionStatus> =
            tauri::async_runtime::spawn_blocking(move || {
                receiver.recv_timeout(Duration::from_secs(2))
            })
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|(install, status)| (install.to_string(), status))
            .collect();
        let root = extensions.root.clone();
        Ok(extensions
            .registry(profile)
            .extensions
            .into_iter()
            .map(|entry| {
                let (state, error) = match (entry.enabled, status.get(&entry.install)) {
                    (false, _) => ("off", None),
                    (true, Some(WebExtensionStatus::Running(_))) => ("running", None),
                    (true, Some(WebExtensionStatus::Failed(error))) => {
                        ("failed", Some(error.clone()))
                    }
                    (true, _) => ("starting", None),
                };
                let warnings =
                    Manifest::load(&root.join("packages").join(&entry.id).join(&entry.package))
                        .map(|manifest| permissions::warnings(&manifest))
                        .unwrap_or_default();
                WebExtensionView {
                    id: entry.id,
                    name: entry.name,
                    version: entry.version,
                    description: entry.description,
                    enabled: entry.enabled,
                    icon: entry.icon,
                    state: state.to_owned(),
                    error,
                    warnings,
                }
            })
            .collect())
    }

    pub(super) async fn set_enabled(
        shell: &Handle,
        extensions: &WebExtensions,
        id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let profile = target(shell, None).await?.profile;
        let mut registry = extensions.registry(profile);
        let entry = registry
            .extensions
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("That extension isn't installed.")?;
        entry.enabled = enabled;
        extensions.save(profile, &registry)?;
        extensions.apply(shell, profile, &mut registry);
        Ok(())
    }

    pub(super) async fn uninstall(
        shell: &Handle,
        extensions: &WebExtensions,
        id: &str,
    ) -> Result<(), String> {
        let profile = target(shell, None).await?.profile;
        let mut registry = extensions.registry(profile);
        let Some(position) = registry.extensions.iter().position(|entry| entry.id == id) else {
            return Err("That extension isn't installed.".into());
        };
        let entry = registry.extensions.remove(position);
        extensions.save(profile, &registry)?;
        // Its package files stay until the next launch: the browser may need
        // them to load the extension once more to erase its data.
        if let Some(install) = ExtensionInstallId::parse(&entry.install) {
            shell.dispatch(zephium_app::Command::RemoveWebExtension {
                profile,
                extension: Box::new(extensions.load(&entry, install, false)),
            });
        }
        extensions.apply(shell, profile, &mut registry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn removes_the_previous_repository_even_when_sealed() {
        use std::os::unix::fs::PermissionsExt;
        let data = tempfile::tempdir().unwrap();
        let sealed = data.path().join("extension-repository-v1/objects");
        std::fs::create_dir_all(&sealed).unwrap();
        std::fs::write(sealed.join("package"), b"old").unwrap();
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o500)).unwrap();

        let path = data.path().join("extension-repository-v1");
        unseal(&path);
        std::fs::remove_dir_all(&path).unwrap();
        assert!(!path.exists());
    }
}
