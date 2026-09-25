//! Media & Files: native import into the profile's media store, bounded
//! serving of admitted images to privileged chrome, and OS open for the rest.
use std::borrow::Cow;
use std::time::Duration;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use zephium_core::ids::{ProfileId, ResourceId};
use zephium_core::resources::{
    MediaAssetV1, MediaImport, MediaKind, MediaOrigin, ResourceCall, ResourceContent,
    ResourceError, ResourceResponse, MAX_MEDIA_FILE_BYTES, MAX_MEDIA_IMAGE_BYTES,
};
use zephium_ipc::MediaImportV1;

pub(crate) const SCHEME: &str = "zephium-media";

/// Blob paths for the custom scheme; bytes are admitted only through the
/// store actor.
pub(crate) struct MediaBlobs(pub(crate) zephium_store::MediaStore);

fn profile_of(value: &str) -> Option<ProfileId> {
    ProfileId::parse(value).filter(|id| id.to_string() == value)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn media_import(
    caller: WebviewWindow,
    app: AppHandle,
    shell: State<'_, zephium_app::Handle>,
    expected_profile: String,
) -> Result<MediaImportV1, ()> {
    let refused = |error| Ok(MediaImportV1::Refused { error });
    if !super::authorize(&caller, super::CallerPolicy::Main, "media_import")
        || super::shutdown_started(&app)
    {
        return refused(ResourceError::Unavailable);
    }
    let Some(profile) = profile_of(&expected_profile) else {
        return refused(ResourceError::Invalid);
    };
    let picker = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        picker
            .dialog()
            .file()
            .set_title("Add to Work")
            .blocking_pick_file()
    })
    .await
    .map_err(|_| ())?;
    let Some(picked) = picked else {
        return Ok(MediaImportV1::Cancelled);
    };
    let path = match picked {
        tauri_plugin_dialog::FilePath::Path(path) => path,
        tauri_plugin_dialog::FilePath::Url(url) => match url.to_file_path() {
            Ok(path) => path,
            Err(()) => return refused(ResourceError::Invalid),
        },
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let read = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ResourceError> {
        let metadata = std::fs::metadata(&path).map_err(|_| ResourceError::NotFound)?;
        if !metadata.is_file() {
            return Err(ResourceError::Invalid);
        }
        if metadata.len() > u64::from(MAX_MEDIA_FILE_BYTES) {
            return Err(ResourceError::Capacity);
        }
        std::fs::read(&path).map_err(|_| ResourceError::Unavailable)
    })
    .await
    .map_err(|_| ())?;
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(error) => return refused(error),
    };
    let receiver = shell.import_media(
        profile,
        MediaImport {
            request_id: format!("media-import-{}", ResourceId::generate()),
            name,
            origin: MediaOrigin::Imported,
            bytes: std::sync::Arc::new(bytes),
        },
    );
    let reply = tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(30)))
        .await
        .map_err(|_| ())?;
    let Ok(reply) = reply else {
        return refused(ResourceError::OutcomeUnknown);
    };
    match reply.response {
        ResourceResponse::Applied { record, .. } => {
            super::emit_media_changed(&app, &profile.to_string(), &record.id, &record.revision);
            Ok(MediaImportV1::Imported {
                record: Box::new(record),
            })
        }
        ResourceResponse::Error { error } => refused(error),
        _ => refused(ResourceError::Invalid),
    }
}

async fn media_asset(
    shell: &zephium_app::Handle,
    profile: ProfileId,
    id: &str,
) -> Result<MediaAssetV1, ResourceError> {
    if !zephium_core::resources::valid_id(id) {
        return Err(ResourceError::Invalid);
    }
    let receiver = shell.resource_call(profile, ResourceCall::Get { id: id.to_owned() });
    let reply = tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(8)))
        .await
        .map_err(|_| ResourceError::Unavailable)?
        .map_err(|_| ResourceError::Unavailable)?;
    match reply.response {
        ResourceResponse::Record { record } => match record.draft.content {
            ResourceContent::Media { asset } if !record.trashed => Ok(asset),
            _ => Err(ResourceError::NotFound),
        },
        ResourceResponse::Error { error } => Err(error),
        _ => Err(ResourceError::Invalid),
    }
}

/// Opens a non-image asset with the OS default application. The blob is the
/// profile's own snapshot; nothing outside the media store is reachable.
#[tauri::command]
#[specta::specta]
pub(crate) async fn media_open(
    caller: WebviewWindow,
    app: AppHandle,
    shell: State<'_, zephium_app::Handle>,
    expected_profile: String,
    id: String,
) -> Result<bool, ()> {
    if !super::authorize(&caller, super::CallerPolicy::Main, "media_open")
        || super::shutdown_started(&app)
    {
        return Ok(false);
    }
    let Some(profile) = profile_of(&expected_profile) else {
        return Ok(false);
    };
    let Ok(asset) = media_asset(&shell, profile, &id).await else {
        return Ok(false);
    };
    let Some(blobs) = app.try_state::<MediaBlobs>() else {
        return Ok(false);
    };
    let Some(path) = blobs.0.blob_path(profile, &asset.digest) else {
        return Ok(false);
    };
    if !path.is_file() {
        return Ok(false);
    }
    let opened = tokio::task::spawn_blocking(move || open_with_os(&path))
        .await
        .unwrap_or(false);
    Ok(opened)
}

fn open_with_os(path: &std::path::Path) -> bool {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]).arg(path);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    command
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn respond(
    status: u16,
    body: Vec<u8>,
    content_type: &str,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "private, max-age=31536000, immutable")
        .header("Content-Security-Policy", "default-src 'none'; sandbox")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| tauri::http::Response::new(Cow::Borrowed(b"" as &[u8])))
}

/// `zephium-media://localhost/<profile>/<digest>` from privileged main chrome
/// only. Serves admitted image blobs; every other request is 404.
pub(crate) fn serve(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    if ctx.webview_label() != super::MAIN_LABEL {
        return respond(403, Vec::new(), "text/plain");
    }
    let path = request.uri().path().trim_start_matches('/');
    if let Some(rest) = path.strip_prefix("frame/") {
        return serve_page_frame(&ctx, rest);
    }
    let Some((profile, digest)) = path.split_once('/') else {
        return respond(404, Vec::new(), "text/plain");
    };
    let Some(profile) = profile_of(profile) else {
        return respond(404, Vec::new(), "text/plain");
    };
    let Some(blobs) = ctx.app_handle().try_state::<MediaBlobs>() else {
        return respond(503, Vec::new(), "text/plain");
    };
    let Some(bytes) = blobs
        .0
        .read(profile, digest, MAX_MEDIA_IMAGE_BYTES as usize)
    else {
        return respond(404, Vec::new(), "text/plain");
    };
    match sniff_image(&bytes) {
        Some(mime) => respond(200, bytes, mime),
        None => respond(404, Vec::new(), "text/plain"),
    }
}

/// `frame/<attempt>/<step>/<generation>`: the newest frame of one agent page.
/// The generation only busts caches; the bytes are whatever is current.
fn serve_page_frame(
    ctx: &tauri::UriSchemeContext<'_, tauri::Wry>,
    rest: &str,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let mut parts = rest.split('/');
    let ids = (parts.next(), parts.next());
    #[cfg(feature = "work-product")]
    {
        use zephium_core::work::{WorkAttemptId, WorkStepId};
        let (Some(attempt), Some(step)) = (
            ids.0.and_then(WorkAttemptId::parse),
            ids.1.and_then(WorkStepId::parse),
        ) else {
            return respond(404, Vec::new(), "text/plain");
        };
        let Some(state) = ctx
            .app_handle()
            .try_state::<super::work_product::WorkProductState>()
        else {
            return respond(503, Vec::new(), "text/plain");
        };
        match state.page_frame(attempt, step) {
            Some(png) => respond(200, png.as_ref().clone(), "image/png"),
            None => {
                #[cfg(feature = "work-development-traces")]
                super::work_provider::record_diagnostic(format_args!(
                    "work: phase=page_frame served=false attempt={attempt} step={step}"
                ));
                respond(404, Vec::new(), "text/plain")
            }
        }
    }
    #[cfg(not(feature = "work-product"))]
    {
        let _ = (ctx, ids);
        respond(404, Vec::new(), "text/plain")
    }
}

#[allow(dead_code)]
fn kind_is_image(asset: &MediaAssetV1) -> bool {
    asset.kind == MediaKind::Image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sniffing_recognizes_only_admitted_raster_formats() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a...."), Some("image/gif"));
        assert_eq!(
            sniff_image(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(
            sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            None
        );
        assert_eq!(sniff_image(b"%PDF-1.7"), None);
        assert_eq!(sniff_image(b""), None);
    }
}

fn civil_date_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn image_name(url: &tauri::Url) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.rfind(|s| !s.is_empty()).map(str::to_owned))
        .filter(|name| name.len() <= 120)
        .unwrap_or_else(|| "image".into())
}

#[cfg(feature = "work-product")]
async fn environment_snapshot(
    shell: &zephium_app::Handle,
    profile: ProfileId,
    id: zephium_core::work::WorkEnvironmentId,
) -> Result<zephium_core::work::environment::WorkEnvironmentSnapshot, ResourceError> {
    use zephium_core::work::{environment::*, port::*};
    let request = shell
        .work_call(
            profile,
            zephium_ipc::work::WorkCallV1::Environment {
                version: 1,
                request: WorkEnvironmentCall::Read { id },
            },
        )
        .map_err(|_| ResourceError::Unavailable)?;
    let projection = tokio::time::timeout(Duration::from_secs(8), request)
        .await
        .map_err(|_| ResourceError::Unavailable)?
        .map_err(|_| ResourceError::NotFound)?;
    match projection.reply {
        WorkReply::Environment(WorkEnvironmentReply::Snapshot { snapshot })
            if projection.profile == profile =>
        {
            Ok(*snapshot)
        }
        _ => Err(ResourceError::NotFound),
    }
}

#[cfg(feature = "work-product")]
async fn environment_edit(
    shell: &zephium_app::Handle,
    profile: ProfileId,
    id: zephium_core::work::WorkEnvironmentId,
    expected: zephium_core::work::WorkRevision,
    edit: zephium_core::work::environment::WorkEnvironmentEdit,
) -> Result<zephium_core::work::environment::WorkEnvironmentSnapshot, ResourceError> {
    use zephium_core::work::{environment::*, port::*, WorkCommandId};
    let request = shell
        .work_call(
            profile,
            zephium_ipc::work::WorkCallV1::Environment {
                version: 1,
                request: WorkEnvironmentCall::Command {
                    command: WorkCommandId::generate(),
                    intent: WorkEnvironmentIntent::Edit { id, expected, edit },
                },
            },
        )
        .map_err(|_| ResourceError::Unavailable)?;
    let projection = tokio::time::timeout(Duration::from_secs(8), request)
        .await
        .map_err(|_| ResourceError::Unavailable)?
        .map_err(|_| ResourceError::Conflict)?;
    match projection.reply {
        WorkReply::Environment(WorkEnvironmentReply::Applied { snapshot, .. })
            if projection.profile == profile =>
        {
            Ok(*snapshot)
        }
        _ => Err(ResourceError::Conflict),
    }
}

/// Admits one public image for a subject already on the canvas: fetch
/// without cookies, bound and decode in the store, mint the Media resource,
/// add it next to the subject, and relate subject → media.
/// Admits a folder the person dropped or chose so the canvas can hold it
/// and later runs can read inside it. The same policy governs the run.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_admit_folder(
    caller: WebviewWindow,
    app: AppHandle,
    expected_profile: String,
    path: String,
) -> Result<zephium_ipc::WorkFolderAdmitV1, ()> {
    use zephium_ipc::WorkFolderAdmitV1;
    if !super::authorize(&caller, super::CallerPolicy::Main, "work_admit_folder")
        || super::shutdown_started(&app)
        || profile_of(&expected_profile).is_none()
    {
        return Ok(WorkFolderAdmitV1::Refused {
            not_a_folder: false,
        });
    }
    Ok(admit_folder(path))
}

/// Opens the folder picker and admits the choice under the run's policy.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_pick_folder(
    caller: WebviewWindow,
    app: AppHandle,
    expected_profile: String,
) -> Result<Option<zephium_ipc::WorkFolderAdmitV1>, ()> {
    if !super::authorize(&caller, super::CallerPolicy::Main, "work_pick_folder")
        || super::shutdown_started(&app)
        || profile_of(&expected_profile).is_none()
    {
        return Ok(None);
    }
    let picker = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        picker
            .dialog()
            .file()
            .set_title("Add a folder to Work")
            .blocking_pick_folder()
    })
    .await
    .map_err(|_| ())?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = match picked {
        tauri_plugin_dialog::FilePath::Path(path) => path,
        tauri_plugin_dialog::FilePath::Url(url) => match url.to_file_path() {
            Ok(path) => path,
            Err(()) => return Ok(None),
        },
    };
    Ok(Some(admit_folder(path.to_string_lossy().into_owned())))
}

/// Reveals an admitted folder, or a file inside one, in Finder.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_reveal_path(
    caller: WebviewWindow,
    app: AppHandle,
    expected_profile: String,
    path: String,
) -> Result<bool, ()> {
    if !super::authorize(&caller, super::CallerPolicy::Main, "work_reveal_path")
        || super::shutdown_started(&app)
        || profile_of(&expected_profile).is_none()
    {
        return Ok(false);
    }
    #[cfg(not(all(feature = "work-product", target_os = "macos")))]
    {
        let _ = path;
        Ok(false)
    }
    #[cfg(all(feature = "work-product", target_os = "macos"))]
    {
        let target = std::path::PathBuf::from(&path);
        let folder = if target.is_dir() {
            target.clone()
        } else {
            match target.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return Ok(false),
            }
        };
        let (grant, _) =
            zephium_app::work_files::WorkFileGrant::admit(&[folder.to_string_lossy().into_owned()]);
        if grant.is_empty() {
            return Ok(false);
        }
        Ok(std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&target)
            .spawn()
            .is_ok())
    }
}

fn admit_folder(path: String) -> zephium_ipc::WorkFolderAdmitV1 {
    use zephium_ipc::WorkFolderAdmitV1;
    let not_a_folder = std::path::Path::new(&path).is_file();
    #[cfg(not(feature = "work-product"))]
    {
        WorkFolderAdmitV1::Refused { not_a_folder }
    }
    #[cfg(feature = "work-product")]
    {
        let (grant, _) = zephium_app::work_files::WorkFileGrant::admit(&[path]);
        let Some(root) = grant.roots().first() else {
            return WorkFolderAdmitV1::Refused { not_a_folder };
        };
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.is_empty() {
            return WorkFolderAdmitV1::Refused { not_a_folder };
        }
        WorkFolderAdmitV1::Admitted {
            path: root.to_string_lossy().into_owned(),
            name,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn media_admit_remote(
    caller: WebviewWindow,
    app: AppHandle,
    shell: State<'_, zephium_app::Handle>,
    expected_profile: String,
    environment: String,
    element: String,
    url: String,
) -> Result<zephium_ipc::MediaAdmitV1, ()> {
    use zephium_ipc::MediaAdmitV1;
    let refused = |error| Ok(MediaAdmitV1::Refused { error });
    if !super::authorize(&caller, super::CallerPolicy::Main, "media_admit_remote")
        || super::shutdown_started(&app)
    {
        return refused(ResourceError::Unavailable);
    }
    let Some(profile) = profile_of(&expected_profile) else {
        return refused(ResourceError::Invalid);
    };
    let started = std::time::Instant::now();
    let mut trace = MediaAdmitTrace::default();
    let result = admit_remote(&app, &shell, profile, environment, element, url, &mut trace).await;
    let outcome = match &result {
        Ok(MediaAdmitV1::Admitted { .. }) => "admitted",
        Ok(MediaAdmitV1::Refused { .. }) => "refused",
        Err(()) => "unknown",
    };
    let error = match &result {
        Ok(MediaAdmitV1::Refused { error }) => Some(*error),
        _ => None,
    };
    super::work_provider::record_diagnostic(format_args!(
        "work: phase=media_admit outcome={outcome} error={error:?} fetch={:?} bytes={} elapsed_ms={}",
        trace.fetch,
        trace.bytes,
        started.elapsed().as_millis()
    ));
    result
}

/// Closed facts about one admission, never the URL.
#[derive(Default)]
struct MediaAdmitTrace {
    bytes: usize,
    #[cfg(feature = "work-product")]
    fetch: Option<zephium_agentic::public_asset::PublicAssetError>,
    #[cfg(not(feature = "work-product"))]
    fetch: Option<()>,
}

async fn admit_remote(
    app: &AppHandle,
    shell: &zephium_app::Handle,
    profile: ProfileId,
    environment: String,
    element: String,
    url: String,
    trace: &mut MediaAdmitTrace,
) -> Result<zephium_ipc::MediaAdmitV1, ()> {
    use zephium_ipc::MediaAdmitV1;
    let refused = |error| Ok(MediaAdmitV1::Refused { error });
    #[cfg(not(feature = "work-product"))]
    {
        let _ = (app, shell, profile, environment, element, url, trace);
        refused(ResourceError::Unavailable)
    }
    #[cfg(feature = "work-product")]
    {
        use zephium_core::work::environment::*;
        use zephium_core::work::{WorkElementId, WorkEnvironmentId};
        let (Some(environment), Some(element)) = (
            WorkEnvironmentId::parse(&environment),
            WorkElementId::parse(&element),
        ) else {
            return refused(ResourceError::Invalid);
        };
        let Ok(parsed) = tauri::Url::parse(&url) else {
            return refused(ResourceError::Invalid);
        };
        if !zephium_agentic::public_asset::public_https(&parsed) {
            return refused(ResourceError::Invalid);
        }
        let snapshot = match environment_snapshot(shell, profile, environment).await {
            Ok(snapshot) => snapshot,
            Err(error) => return refused(error),
        };
        let Some(subject) = snapshot
            .elements
            .iter()
            .find(|candidate| candidate.id == element)
        else {
            return refused(ResourceError::NotFound);
        };
        if !remote_image_target(&subject.reference) {
            return refused(ResourceError::Invalid);
        }
        let area = subject.area;
        let bytes = match zephium_agentic::public_asset::fetch_public_image(parsed.as_str()).await {
            Ok(bytes) => bytes,
            Err(error) => {
                trace.fetch = Some(error);
                return refused(match error {
                    zephium_agentic::public_asset::PublicAssetError::TooLarge => {
                        ResourceError::Capacity
                    }
                    _ => ResourceError::Unavailable,
                });
            }
        };
        trace.bytes = bytes.len();
        let receiver = shell.import_media(
            profile,
            MediaImport {
                request_id: format!("media-admit-{}", ResourceId::generate()),
                name: image_name(&parsed),
                origin: MediaOrigin::Fetched {
                    url: parsed.to_string(),
                    observed_at: civil_date_today(),
                },
                bytes: std::sync::Arc::new(bytes),
            },
        );
        let reply =
            tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(30)))
                .await
                .map_err(|_| ())?;
        let record = match reply.map(|reply| reply.response) {
            Ok(ResourceResponse::Applied { record, .. }) => record,
            Ok(ResourceResponse::Error { error }) => return refused(error),
            _ => return refused(ResourceError::OutcomeUnknown),
        };
        super::emit_media_changed(app, &profile.to_string(), &record.id, &record.revision);
        let Some(resource) = ResourceId::parse(&record.id) else {
            return refused(ResourceError::Invalid);
        };
        let reference = WorkEnvironmentReference::Resource { resource };
        let current = match snapshot
            .elements
            .iter()
            .find(|candidate| candidate.reference == reference)
        {
            Some(existing) => (snapshot.clone(), existing.id),
            None => {
                let added = match environment_edit(
                    shell,
                    profile,
                    environment,
                    snapshot.revision,
                    WorkEnvironmentEdit::Add {
                        reference: reference.clone(),
                        area,
                    },
                )
                .await
                {
                    Ok(added) => added,
                    Err(error) => return refused(error),
                };
                let Some(media) = added
                    .elements
                    .iter()
                    .find(|candidate| candidate.reference == reference)
                else {
                    return refused(ResourceError::OutcomeUnknown);
                };
                let id = media.id;
                (added, id)
            }
        };
        let (snapshot, media_element) = current;
        let related = snapshot.relations.iter().any(|relation| {
            relation.from == element
                && relation.to == media_element
                && relation.kind == WorkRelationKind::Uses
        });
        if !related
            && environment_edit(
                shell,
                profile,
                environment,
                snapshot.revision,
                WorkEnvironmentEdit::Relate {
                    from: element,
                    to: media_element,
                    relation: WorkRelationKind::Uses,
                },
            )
            .await
            .is_err()
        {
            return refused(ResourceError::Conflict);
        }
        Ok(MediaAdmitV1::Admitted {
            element: media_element,
        })
    }
}

#[cfg(feature = "work-product")]
fn remote_image_target(
    reference: &zephium_core::work::environment::WorkEnvironmentReference,
) -> bool {
    use zephium_core::work::environment::WorkEnvironmentReference;
    matches!(
        reference,
        WorkEnvironmentReference::Subject { .. } | WorkEnvironmentReference::Link { .. }
    )
}

#[cfg(test)]
mod date_tests {
    #[cfg(feature = "work-product")]
    #[test]
    fn remote_thumbnails_accept_subjects_and_links_only() {
        use zephium_core::work::environment::WorkEnvironmentReference;
        assert!(super::remote_image_target(
            &WorkEnvironmentReference::Link {
                url: "https://example.com/video".into(),
                title: "example.com".into(),
            }
        ));
        assert!(super::remote_image_target(
            &WorkEnvironmentReference::Subject {
                objective: 1.into(),
                execution: 2.into(),
                artifact: 3.into(),
                index: 0,
            }
        ));
        assert!(!super::remote_image_target(
            &WorkEnvironmentReference::Resource { resource: 4.into() }
        ));
    }
    #[test]
    fn civil_date_is_iso_shaped() {
        let today = super::civil_date_today();
        assert_eq!(today.len(), 10);
        assert!(today.starts_with("20"));
        assert_eq!(&today[4..5], "-");
        assert_eq!(&today[7..8], "-");
    }
}
