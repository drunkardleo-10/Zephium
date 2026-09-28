//! The macOS implementation: the registry on disk and handing extensions
//! to the shell.

mod install;
mod updates;

pub(in crate::webext) use install::{confirm, prepare};
pub(in crate::webext) use updates::start_updates;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zephium_app::{Handle, WebExtensionStatus};
use zephium_core::ids::{ExtensionInstallId, ItemId, ProfileId};
use zephium_core::ports::engine::WebExtensionLoad;
use zephium_webext::manifest::Manifest;
use zephium_webext::{archive, crx, permissions, prepare, ExtensionId};

use super::{Entry, Registry, WebExtensionView, WebExtensions};

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

pub(in crate::webext) fn restore(extensions: &WebExtensions, shell: &Handle) {
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

pub(in crate::webext) async fn list(
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
        tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(2)))
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
                (true, Some(WebExtensionStatus::Failed(error))) => ("failed", Some(error.clone())),
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

pub(in crate::webext) async fn set_enabled(
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

pub(in crate::webext) fn answer_access(
    shell: &Handle,
    extensions: &WebExtensions,
    request: zephium_ipc::WebExtensionAccessRequestView,
    allowed: bool,
) -> Result<(), String> {
    let profile = ProfileId::parse(&request.profile_id).ok_or("Invalid profile.")?;
    let number: u64 = request.request.parse().map_err(|_| "Invalid request.")?;
    if allowed {
        let mut registry = extensions.registry(profile);
        if let Some(entry) = registry
            .extensions
            .iter_mut()
            .find(|entry| entry.id == request.extension_id)
        {
            for permission in request.permissions {
                if !entry.permissions.contains(&permission) {
                    entry.permissions.push(permission);
                }
            }
            for pattern in request.patterns {
                if !entry.hosts.contains(&pattern) {
                    entry.hosts.push(pattern);
                }
            }
            extensions.save(profile, &registry)?;
        }
    }
    shell.dispatch(zephium_app::Command::AnswerWebExtensionAccess {
        profile,
        request: number,
        allowed,
    });
    Ok(())
}

pub(in crate::webext) async fn uninstall(
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
