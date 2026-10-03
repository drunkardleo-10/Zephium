use super::*;
use zephium_ipc::work::*;

#[cfg_attr(
    not(all(
        feature = "work-product",
        any(target_os = "macos", target_os = "windows")
    )),
    allow(dead_code)
)]
enum Operation {
    Read,
    Present(WorkHumanPageIdV1, WorkHumanRegionV1),
    Continue(WorkHumanPageIdV1, WorkHumanAccountV1),
    Release(WorkHumanPageIdV1),
}
async fn human_command(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    work: WorkId,
    operation: Operation,
) -> WorkHumanResponseV1 {
    let result = async {
        if !crate::authorize(&caller, crate::CallerPolicy::Main, "work_human") {
            return Err(WorkError::Unavailable);
        }
        let profile = selected_work(&app, &expected_profile, work).await?;
        #[cfg(all(
            feature = "work-product",
            any(target_os = "macos", target_os = "windows")
        ))]
        {
            let owner = app.state::<WorkProductState>();
            let browser = &owner.providers.browser;
            let accepted = match operation {
                Operation::Read => false,
                Operation::Present(id, region) => {
                    browser.present_human_page(profile, work, id, region)?;
                    true
                }
                Operation::Continue(id, account) => {
                    browser.continue_human_page(profile, work, id, account)?;
                    true
                }
                Operation::Release(id) => {
                    browser.release_human_page(profile, work, id)?;
                    true
                }
            };
            Ok((accepted, browser.human_pages(profile, work)?))
        }
        #[cfg(not(all(
            feature = "work-product",
            any(target_os = "macos", target_os = "windows")
        )))]
        {
            let _ = (profile, operation);
            Err(WorkError::Unavailable)
        }
    }
    .await;
    let (accepted, pages, error) = match result {
        Ok((accepted, pages)) => (accepted, pages, None),
        Err(error) => (false, Vec::new(), Some(error.into())),
    };
    WorkHumanResponseV1 {
        version: 1,
        profile: expected_profile,
        work,
        accepted,
        pages,
        error,
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_human_pages(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    work: WorkId,
) -> WorkHumanResponseV1 {
    human_command(caller, app, expected_profile, work, Operation::Read).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_human_present(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    work: WorkId,
    page: WorkHumanPageIdV1,
    region: WorkHumanRegionV1,
) -> WorkHumanResponseV1 {
    human_command(
        caller,
        app,
        expected_profile,
        work,
        Operation::Present(page, region),
    )
    .await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_human_continue(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    work: WorkId,
    page: WorkHumanPageIdV1,
    account: WorkHumanAccountV1,
) -> WorkHumanResponseV1 {
    human_command(
        caller,
        app,
        expected_profile,
        work,
        Operation::Continue(page, account),
    )
    .await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_human_release(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    work: WorkId,
    page: WorkHumanPageIdV1,
) -> WorkHumanResponseV1 {
    human_command(
        caller,
        app,
        expected_profile,
        work,
        Operation::Release(page),
    )
    .await
}
