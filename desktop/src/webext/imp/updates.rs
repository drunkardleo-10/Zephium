//! Keeping installed extensions current with the Chrome Web Store.

use std::path::Path;
use std::time::Duration;

use zephium_app::Handle;
use zephium_core::ids::ProfileId;
use zephium_webext::{store, ExtensionId};

use super::install::{download, keep_original, stage, store_client, Source};
use crate::webext::{Entry, WebExtensions};

/// The first check waits so launch stays free of network work.
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(5 * 60);
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60 * 60);
const MAX_UPDATE_RESPONSE_BYTES: usize = 1024 * 1024;

/// Keeps installed extensions current with the Chrome Web Store.
pub(in crate::webext) fn start_updates(root: &Path, shell: &Handle) {
    let extensions = WebExtensions {
        root: root.to_path_buf(),
        state: std::sync::Mutex::new(None),
    };
    let shell = shell.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_UPDATE_CHECK).await;
        loop {
            if let Err(error) = check_updates(&extensions, &shell).await {
                eprintln!("extensions: update check failed: {error}");
            }
            tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
        }
    });
}

async fn check_updates(extensions: &WebExtensions, shell: &Handle) -> Result<(), String> {
    let mut installed: std::collections::BTreeMap<String, String> = Default::default();
    for profile in extensions.profiles() {
        for entry in extensions.registry(profile).extensions {
            if !entry.sideloaded {
                installed.entry(entry.id).or_insert(entry.version);
            }
        }
    }
    let entries: Vec<(ExtensionId, String)> = installed
        .iter()
        .filter_map(|(id, version)| Some((ExtensionId::parse(id)?, version.clone())))
        .collect();
    if entries.is_empty() {
        return Ok(());
    }
    let url = store::update_check_url(&entries, zephium_webext_macos::compat::CHROME_VERSION);
    let response = store_client()?
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
    let body = response
        .bytes()
        .await
        .map_err(|_| "The update check was interrupted.")?;
    if body.len() > MAX_UPDATE_RESPONSE_BYTES {
        return Err("The update check answered too much.".into());
    }
    for info in store::parse_update_response(&String::from_utf8_lossy(&body)) {
        let newer = info.status == "ok"
            && info.version.as_deref().is_some_and(|version| {
                installed
                    .get(info.id.as_str())
                    .is_some_and(|current| store::compare_versions(version, current).is_gt())
            });
        if newer {
            if let Err(error) = update(extensions, shell, &info).await {
                eprintln!("extensions: could not update {}: {error}", info.id);
            }
        }
    }
    Ok(())
}

/// Downloads and verifies one update, then applies it everywhere the
/// extension is installed, unless it asks for access the user never
/// granted: then it waits for approval, as in Chrome.
async fn update(
    extensions: &WebExtensions,
    shell: &Handle,
    info: &store::UpdateInfo,
) -> Result<(), String> {
    let bytes = download(&info.id).await?;
    if let Some(hash) = &info.hash_sha256 {
        if !store::matches_sha256(&bytes, hash) {
            return Err("the package doesn't match the published hash".into());
        }
    }
    let holders: Vec<(ProfileId, Entry)> = extensions
        .profiles()
        .into_iter()
        .filter_map(|profile| {
            let entry = extensions
                .registry(profile)
                .extensions
                .into_iter()
                .find(|entry| entry.id == info.id.as_str())?;
            Some((profile, entry))
        })
        .collect();
    let Some((profile, existing)) = holders.first().cloned() else {
        return Ok(());
    };
    let root = extensions.root.clone();
    let source = Source::Store(info.id.clone(), bytes);
    let staged =
        tauri::async_runtime::spawn_blocking(move || stage(&root, source, profile, &[existing]))
            .await
            .map_err(|_| "staging failed")??;
    let pending = staged.pending;
    let fresh = &pending.entry;
    let asks_more = |old: &Entry| {
        fresh
            .permissions
            .iter()
            .any(|name| !old.permissions.contains(name))
            || fresh.hosts.iter().any(|host| !old.hosts.contains(host))
    };
    if holders.iter().any(|(_, old)| asks_more(old)) {
        let _ = std::fs::remove_dir_all(&pending.staged);
        for (profile, _) in &holders {
            let mut registry = extensions.registry(*profile);
            if let Some(entry) = registry
                .extensions
                .iter_mut()
                .find(|entry| entry.id == fresh.id)
            {
                entry.held_update = Some(fresh.version.clone());
            }
            extensions.save(*profile, &registry)?;
        }
        return Ok(());
    }

    let packages = extensions.packages(&fresh.id);
    std::fs::create_dir_all(&packages).map_err(|e| e.to_string())?;
    keep_original(&packages, &fresh.version, &pending.original)?;
    let target = packages.join(&fresh.package);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&pending.staged, &target).map_err(|e| e.to_string())?;
    for (profile, _) in &holders {
        let mut registry = extensions.registry(*profile);
        if let Some(entry) = registry
            .extensions
            .iter_mut()
            .find(|entry| entry.id == fresh.id)
        {
            *entry = Entry {
                install: entry.install.clone(),
                enabled: entry.enabled,
                access: entry.access.clone(),
                ..fresh.clone()
            };
        }
        extensions.save(*profile, &registry)?;
        extensions.apply(shell, *profile, &mut registry);
    }
    Ok(())
}
