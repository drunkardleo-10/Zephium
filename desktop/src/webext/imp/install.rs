//! Installing from the Chrome Web Store: download, verification, staging
//! and the user's confirmation.

use std::path::Path;
use std::time::Duration;

use zephium_app::Handle;
use zephium_core::ids::{ExtensionInstallId, ItemId, ProfileId};
use zephium_webext::manifest::Manifest;
use zephium_webext::{archive, crx, permissions, prepare, store, ExtensionId};

use super::{compat_layer, compat_revision, target, MAX_PACKAGE_BYTES};
use crate::webext::{Entry, Pending, WebExtensionReview, WebExtensions};

/// A client that talks only to Google's update and download hosts, where
/// the Web Store serves packages from.
pub(super) fn store_client() -> Result<reqwest::Client, String> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let google = attempt.url().host_str().is_some_and(|host| {
            ["google.com", "googleusercontent.com", "gvt1.com"]
                .iter()
                .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        });
        if attempt.previous().len() >= 5 || !google {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .https_only(true)
        .redirect(policy)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| "Downloads are unavailable.".to_string())
}

pub(super) async fn download(id: &ExtensionId) -> Result<Vec<u8>, String> {
    let client = store_client()?;
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

pub(in crate::webext) async fn prepare(
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
    let staged =
        tauri::async_runtime::spawn_blocking(move || stage(&root, &id, bytes, profile, existing))
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

pub(super) struct Staged {
    pub(super) review: WebExtensionReview,
    pub(super) pending: Pending,
}

pub(super) fn stage(
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
    let dir = staging.join(format!("{id}-{}", ExtensionInstallId::generate()));
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
            held_update: None,
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

pub(in crate::webext) fn confirm(
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
