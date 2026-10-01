//! The person's own context for the agent: what it remembers about them, and
//! their history, notes and open tabs behind a consent asked once per work.
//! The lead reaches it through [`PersonalTools`]; Settings through the
//! functions here and [`skills`].
mod day;
pub mod skills;
mod tools;

use std::time::Duration;

pub use day::local_day;
pub use tools::PersonalTools;
use zephium_core::{
    ids::ProfileId,
    work::{personal::*, port::*, WorkError, WorkExecutionId, WorkId},
};

const TIMEOUT: Duration = Duration::from_secs(10);
/// Facts the lead's prompt carries before any recall.
const DIGEST_FACTS: u16 = 24;
const DIGEST_CHARS: usize = 1600;

async fn call(
    handle: &crate::Handle,
    profile: ProfileId,
    request: WorkPersonalRequest,
) -> Result<WorkPersonalReply, WorkError> {
    let response = tokio::time::timeout(
        TIMEOUT,
        handle.submit_work_document(WorkRequest::Personal(request), Some(profile))?,
    )
    .await
    .map_err(|_| WorkError::Unavailable)??;
    match response.reply {
        WorkReply::Personal(reply) if response.profile == profile => Ok(reply),
        _ => Err(WorkError::Invalid),
    }
}

/// Newest first; `query` searches the words, `work` keeps one work's.
pub async fn memories(
    handle: &crate::Handle,
    profile: ProfileId,
    query: Option<String>,
    work: Option<WorkId>,
    limit: u16,
) -> Result<Vec<WorkMemoryV1>, WorkError> {
    match call(
        handle,
        profile,
        WorkPersonalRequest::Memories { query, work, limit },
    )
    .await?
    {
        WorkPersonalReply::Memories(memories) => Ok(memories),
        _ => Err(WorkError::Invalid),
    }
}

pub async fn remember(
    handle: &crate::Handle,
    profile: ProfileId,
    text: String,
    kind: WorkMemoryKindV1,
    work: Option<WorkId>,
    execution: Option<WorkExecutionId>,
) -> Result<WorkMemoryV1, WorkError> {
    let request = WorkPersonalRequest::Remember {
        id: new_memory_id(),
        text,
        kind,
        work,
        execution,
        now_ms: now_ms(),
    };
    match call(handle, profile, request).await? {
        WorkPersonalReply::Memory(Some(memory)) => Ok(memory),
        _ => Err(WorkError::Invalid),
    }
}

pub async fn edit(
    handle: &crate::Handle,
    profile: ProfileId,
    id: String,
    text: String,
    kind: WorkMemoryKindV1,
) -> Result<WorkMemoryV1, WorkError> {
    match call(
        handle,
        profile,
        WorkPersonalRequest::Edit { id, text, kind },
    )
    .await?
    {
        WorkPersonalReply::Memory(Some(memory)) => Ok(memory),
        _ => Err(WorkError::NotFound),
    }
}

pub async fn forget(
    handle: &crate::Handle,
    profile: ProfileId,
    id: Option<String>,
) -> Result<(), WorkError> {
    let request = match id {
        Some(id) => WorkPersonalRequest::Forget { id },
        None => WorkPersonalRequest::ForgetAll,
    };
    call(handle, profile, request).await.map(|_| ())
}

async fn used(handle: &crate::Handle, profile: ProfileId, ids: Vec<String>) {
    if !ids.is_empty() {
        let _ = call(
            handle,
            profile,
            WorkPersonalRequest::Used {
                ids,
                now_ms: now_ms(),
            },
        )
        .await;
    }
}

/// The work's standing answer for a source; `set` records one first.
pub async fn consent(
    handle: &crate::Handle,
    profile: ProfileId,
    work: WorkId,
    source: WorkContextSourceV1,
    set: Option<bool>,
) -> Result<Option<bool>, WorkError> {
    match call(
        handle,
        profile,
        WorkPersonalRequest::Consent { work, source, set },
    )
    .await?
    {
        WorkPersonalReply::Consent(answer) => Ok(answer),
        _ => Err(WorkError::Invalid),
    }
}

async fn history(
    handle: &crate::Handle,
    profile: ProfileId,
    query: String,
    limit: u16,
) -> Result<Vec<WorkHistoryHit>, WorkError> {
    match call(
        handle,
        profile,
        WorkPersonalRequest::SearchHistory { query, limit },
    )
    .await?
    {
        WorkPersonalReply::History(hits) => Ok(hits),
        _ => Err(WorkError::Invalid),
    }
}

/// Pages visited in `[since, until)`, newest first, each address once.
async fn recent_history(
    handle: &crate::Handle,
    profile: ProfileId,
    since: i64,
    until: i64,
    limit: u16,
) -> Result<Vec<WorkHistoryHit>, WorkError> {
    match call(
        handle,
        profile,
        WorkPersonalRequest::RecentHistory {
            since,
            until,
            limit,
        },
    )
    .await?
    {
        WorkPersonalReply::History(hits) => Ok(hits),
        _ => Err(WorkError::Invalid),
    }
}

/// What the lead's prompt says it knows about the person before it recalls
/// anything: the facts used or kept most recently, one per line, bounded.
/// `None` when nothing is remembered.
pub async fn digest(handle: &crate::Handle, profile: ProfileId) -> Option<String> {
    let facts = memories(handle, profile, None, None, DIGEST_FACTS)
        .await
        .ok()?;
    let mut digest = String::new();
    for fact in facts {
        let line = format!("- {} ({})\n", fact.text, fact.kind.name());
        if digest.len() + line.len() > DIGEST_CHARS {
            break;
        }
        digest.push_str(&line);
    }
    (!digest.is_empty()).then_some(digest)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}
