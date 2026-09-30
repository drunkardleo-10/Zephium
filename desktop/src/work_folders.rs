//! The folder a run asks for while it works: the system's own folder panel,
//! opened where the run named it, admitted under the same policy as a folder
//! given with a request.
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

/// Opens the folder panel at `start` (or its nearest existing folder) and
/// admits the person's choice.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_choose_folder(
    caller: WebviewWindow,
    app: AppHandle,
    expected_profile: String,
    start: String,
) -> Result<Option<zephium_ipc::WorkFolderAdmitV1>, ()> {
    if !super::authorize(&caller, super::CallerPolicy::Main, "work_choose_folder")
        || super::shutdown_started(&app)
        || zephium_core::ids::ProfileId::parse(&expected_profile)
            .is_none_or(|id| id.to_string() != expected_profile)
    {
        return Ok(None);
    }
    let start = std::path::PathBuf::from(start);
    let at = start
        .ancestors()
        .find(|dir| dir.is_dir())
        .map(std::path::Path::to_path_buf);
    let picker = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        let panel = picker.dialog().file().set_title("Choose a folder for Work");
        match at {
            Some(at) => panel.set_directory(at),
            None => panel,
        }
        .blocking_pick_folder()
    })
    .await
    .map_err(|_| ())?;
    let path = match picked {
        Some(tauri_plugin_dialog::FilePath::Path(path)) => path,
        Some(tauri_plugin_dialog::FilePath::Url(url)) => match url.to_file_path() {
            Ok(path) => path,
            Err(()) => return Ok(None),
        },
        None => return Ok(None),
    };
    Ok(Some(admit(path.to_string_lossy().into_owned())))
}

fn admit(path: String) -> zephium_ipc::WorkFolderAdmitV1 {
    use zephium_ipc::WorkFolderAdmitV1;
    let not_a_folder = std::path::Path::new(&path).is_file();
    #[cfg(not(feature = "work-product"))]
    {
        WorkFolderAdmitV1::Refused { not_a_folder }
    }
    #[cfg(feature = "work-product")]
    {
        let (grant, _) = zephium_app::work_files::WorkFileGrant::admit(&[path]);
        match grant.roots().first() {
            Some(root) => WorkFolderAdmitV1::Admitted {
                path: root.to_string_lossy().into_owned(),
                name: root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            None => WorkFolderAdmitV1::Refused { not_a_folder },
        }
    }
}
