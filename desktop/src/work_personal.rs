//! Settings and the canvas over what the agent remembers about the person and
//! the skills it follows.
use super::*;
use zephium_core::ids::ProfileId;
use zephium_core::work::WorkError;
use zephium_ipc::work::{
    WorkMemoryChangeV1, WorkMemoryQueryV1, WorkMemoryRefusalV1, WorkMemoryResponseV1,
    WorkSkillChangeV1, WorkSkillFaultV1, WorkSkillsResponseV1,
};

const PAGE: u16 = 500;

fn admitted(
    caller: &WebviewWindow,
    app: &tauri::AppHandle,
    expected_profile: &str,
    capability: &'static str,
) -> Result<ProfileId, WorkError> {
    if !authorize(caller, CallerPolicy::Main, capability) {
        return Err(WorkError::Unavailable);
    }
    if shutdown_started(app) {
        return Err(WorkError::Shutdown);
    }
    ProfileId::parse(expected_profile)
        .filter(|id| id.to_string() == expected_profile)
        .ok_or(WorkError::Invalid)
}

async fn memories(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    query: WorkMemoryQueryV1,
    change: Option<WorkMemoryChangeV1>,
) -> WorkMemoryResponseV1 {
    let mut refused = None;
    let result = async {
        let profile = admitted(&caller, &app, &expected_profile, "work_memories")?;
        #[cfg(feature = "work-product")]
        {
            use zephium_app::work_personal as personal;
            use zephium_core::work::personal::{check_memory, WorkMemoryFault};
            let shell = app.state::<zephium_app::Handle>();
            let text = match &change {
                Some(
                    WorkMemoryChangeV1::Add { text, .. } | WorkMemoryChangeV1::Edit { text, .. },
                ) => Some(text.as_str()),
                _ => None,
            };
            if let Some(Err(fault)) = text.map(check_memory) {
                refused = Some(match fault {
                    WorkMemoryFault::Empty => WorkMemoryRefusalV1::Empty,
                    WorkMemoryFault::TooLong => WorkMemoryRefusalV1::TooLong,
                    WorkMemoryFault::Lines => WorkMemoryRefusalV1::Lines,
                    WorkMemoryFault::Secret => WorkMemoryRefusalV1::Secret,
                });
            } else {
                match change {
                    Some(WorkMemoryChangeV1::Add { text, memory }) => {
                        personal::remember(&shell, profile, text, memory, None, None).await?;
                    }
                    Some(WorkMemoryChangeV1::Edit { id, text, memory }) => {
                        personal::edit(&shell, profile, id, text, memory).await?;
                    }
                    Some(WorkMemoryChangeV1::Forget { id }) => {
                        personal::forget(&shell, profile, Some(id)).await?;
                    }
                    Some(WorkMemoryChangeV1::ForgetAll) => {
                        personal::forget(&shell, profile, None).await?;
                    }
                    None => {}
                }
            }
            let query_text = query.query.filter(|text| !text.trim().is_empty());
            personal::memories(&shell, profile, query_text, query.work, PAGE).await
        }
        #[cfg(not(feature = "work-product"))]
        {
            let _ = (profile, query, change);
            Err(WorkError::Unavailable)
        }
    }
    .await;
    let (memories, error) = match result {
        Ok(memories) => (memories, None),
        Err(error) => (Vec::new(), Some(error.into())),
    };
    WorkMemoryResponseV1 {
        version: 1,
        profile: expected_profile,
        memories,
        refused,
        error,
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_memories(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    query: WorkMemoryQueryV1,
) -> WorkMemoryResponseV1 {
    memories(caller, app, expected_profile, query, None).await
}

/// Changes one memory, or all of them, and returns the list for `query`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_change_memory(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    query: WorkMemoryQueryV1,
    change: WorkMemoryChangeV1,
) -> WorkMemoryResponseV1 {
    memories(caller, app, expected_profile, query, Some(change)).await
}

#[cfg(feature = "work-product")]
fn row(row: zephium_app::work_personal::skills::SkillRow) -> zephium_ipc::work::WorkSkillRowV1 {
    use zephium_core::work::model::WorkModelRole;
    zephium_ipc::work::WorkSkillRowV1 {
        name: row.name,
        description: row.description,
        builtin: row.builtin,
        customized: row.customized,
        enabled: row.enabled,
        tools: row.tools,
        role: row.role.map(|role| {
            match role {
                WorkModelRole::Page => "page",
                WorkModelRole::Light => "light",
                _ => "lead",
            }
            .to_owned()
        }),
    }
}

#[cfg(feature = "work-product")]
fn fault(
    fault: zephium_app::work_personal::skills::SkillChangeFault,
) -> Result<WorkSkillFaultV1, WorkError> {
    use zephium_app::work_lead::skills::SkillFault;
    use zephium_app::work_personal::skills::SkillChangeFault as Change;
    Ok(match fault {
        Change::Text(SkillFault::NoFrontmatter) => WorkSkillFaultV1::NoFrontmatter,
        Change::Text(SkillFault::Name) => WorkSkillFaultV1::Name,
        Change::Text(SkillFault::Description) => WorkSkillFaultV1::Description,
        Change::Text(SkillFault::Role) => WorkSkillFaultV1::Role,
        Change::Text(SkillFault::TooLarge) => WorkSkillFaultV1::TooLarge,
        Change::Text(SkillFault::Empty) => WorkSkillFaultV1::Empty,
        Change::NotFound => WorkSkillFaultV1::NotFound,
        Change::BuiltIn => WorkSkillFaultV1::BuiltIn,
        Change::Taken => WorkSkillFaultV1::Taken,
        Change::Full => WorkSkillFaultV1::Full,
        Change::Unavailable => return Err(WorkError::Unavailable),
    })
}

enum SkillCall {
    List,
    Read(String),
    Change(WorkSkillChangeV1),
}

async fn skills(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    call: SkillCall,
) -> WorkSkillsResponseV1 {
    let mut text = None;
    let mut refused = None;
    let result: Result<Vec<zephium_ipc::work::WorkSkillRowV1>, WorkError> = (|| {
        let profile = admitted(&caller, &app, &expected_profile, "work_skills")?;
        #[cfg(feature = "work-product")]
        {
            use zephium_app::work_personal::skills;
            let outcome = match call {
                SkillCall::List => Ok(()),
                SkillCall::Read(name) => skills::read(profile, &name).map(|(read, _)| {
                    text = Some(read);
                }),
                SkillCall::Change(WorkSkillChangeV1::Save { previous, text }) => {
                    skills::save(profile, previous.as_deref(), &text).map(|_| ())
                }
                SkillCall::Change(WorkSkillChangeV1::Delete { name }) => {
                    skills::delete(profile, &name)
                }
                SkillCall::Change(WorkSkillChangeV1::SetEnabled { name, enabled }) => {
                    skills::set_enabled(profile, &name, enabled)
                }
            };
            if let Err(change) = outcome {
                refused = Some(fault(change)?);
            }
            Ok(skills::list(profile).into_iter().map(row).collect())
        }
        #[cfg(not(feature = "work-product"))]
        {
            let _ = (profile, call, &mut text, &mut refused);
            Err(WorkError::Unavailable)
        }
    })();
    let (skills, error) = match result {
        Ok(skills) => (skills, None),
        Err(error) => (Vec::new(), Some(error.into())),
    };
    WorkSkillsResponseV1 {
        version: 1,
        profile: expected_profile,
        skills,
        text,
        fault: refused,
        error,
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_skills(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
) -> WorkSkillsResponseV1 {
    skills(caller, app, expected_profile, SkillCall::List).await
}

/// A skill's `SKILL.md`, built-in or the person's, with the list.
#[tauri::command]
#[specta::specta]
pub(crate) async fn work_skill_text(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    name: String,
) -> WorkSkillsResponseV1 {
    skills(caller, app, expected_profile, SkillCall::Read(name)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn work_change_skill(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    expected_profile: String,
    change: WorkSkillChangeV1,
) -> WorkSkillsResponseV1 {
    skills(caller, app, expected_profile, SkillCall::Change(change)).await
}
