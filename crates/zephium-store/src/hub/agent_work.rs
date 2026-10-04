//! Exclusively fenced, bounded, exact-CAS Work persistence on the Store thread.

#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};
use zephium_agentic::{
    AgentWorkArchivedExtraction, AgentWorkArtifactDescriptor, AgentWorkArtifactPublication,
    AgentWorkArtifactReply, AgentWorkArtifactRequest, AgentWorkDisposition, AgentWorkIncarnation,
    AgentWorkJournalError as Error, AgentWorkJournalMutation, AgentWorkJournalReply as Reply,
    AgentWorkJournalRequest as Request, AgentWorkRecord, AGENT_WORK_RECORD_BYTES,
    MAX_AGENT_WORK_ARTIFACT_BYTES, MAX_AGENT_WORK_ARTIFACT_TOTAL_BYTES,
    MAX_DURABLE_AGENT_WORK_RUNS,
};
use zephium_core::ids::ProfileId;
use zephium_private_fs::LockedPrivateNamespace;

use super::Hub;

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
#[path = "agent_work_artifact_tests.rs"]
mod artifact_tests;
#[cfg(all(
    test,
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
#[path = "agent_work_tests.rs"]
mod tests;
#[cfg(all(
    test,
    windows,
    debug_assertions,
    feature = "windows-namespace-validation"
))]
#[path = "agent_work_windows_tests.rs"]
mod windows_tests;

const _: [(); 96] = [(); AGENT_WORK_RECORD_BYTES];
const _: [(); 1024] = [(); MAX_DURABLE_AGENT_WORK_RUNS];

pub(super) struct WorkOwnership {
    lease: WorkLease,
    incarnation: AgentWorkIncarnation,
}

enum WorkLease {
    Legacy(LockedPrivateNamespace),
    #[cfg(windows)]
    Native(Arc<zephium_private_fs::NativeWorkStorageAnchor>),
}

impl WorkOwnership {
    fn verified<T>(&self, operation: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        match &self.lease {
            WorkLease::Legacy(namespace) => namespace
                .directory()
                .with_verified_path(|_| operation())
                .map_err(|_| Error::Uncertain)?,
            #[cfg(windows)]
            WorkLease::Native(anchor) => {
                anchor.verify().map_err(|_| Error::Uncertain)?;
                let result = operation();
                anchor.verify().map_err(|_| Error::Uncertain)?;
                result
            }
        }
    }
}

// Engine teardown follows Store, and unacknowledged native work may survive
// Store shutdown. Keep this original lock until OS process exit, even when
// the Hub/Store actor has gone. Store replacement cannot reopen admission.
static PROCESS_WORK_FENCE: Mutex<Option<Arc<WorkOwnership>>> = Mutex::new(None);

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
pub(crate) use tests::work_test_guard;
#[cfg(all(test, windows))]
pub(crate) use tests::work_test_storage;

impl Hub {
    fn with_work_connection<T>(
        &mut self,
        operation: impl FnOnce(&mut Connection) -> Result<T, Error>,
    ) -> Result<T, Error> {
        #[cfg(windows)]
        if self.windows_work_storage.is_some() {
            return self
                .windows_work_database()
                .map_err(|_| Error::Unavailable)?
                .with_connection(|connection| Ok(operation(connection)))
                .map_err(|_| Error::Uncertain)?;
        }
        operation(&mut self.meta)
    }

    fn work_profile_retired(&mut self, profile: ProfileId) -> Result<bool, Error> {
        #[cfg(windows)]
        if self.windows_work_storage.is_some() {
            return self.with_work_connection(|connection| {
                super::windows_work_storage::profile_retired(connection, profile)
                    .map_err(|_| Error::Uncertain)
            });
        }
        let _ = profile;
        Ok(false)
    }

    pub(super) fn read_agent_work_evidence(
        &mut self,
        profile: ProfileId,
        link: zephium_core::work::artifact::WorkEvidenceLink,
    ) -> Result<zephium_core::work::artifact::WorkEvidencePreviewV1, zephium_core::work::WorkError>
    {
        use zephium_core::work::WorkError;
        if self
            .work_profile_retired(profile)
            .map_err(|_| WorkError::Unavailable)?
        {
            return Err(WorkError::NotFound);
        }
        #[cfg(windows)]
        if self.windows_work_storage.is_some() {
            return self
                .windows_work_database()
                .map_err(|_| WorkError::Unavailable)?
                .with_connection(|connection| Ok(read_work_evidence(connection, profile, link)))
                .map_err(|_| WorkError::Unavailable)?;
        }
        read_work_evidence(&self.meta, profile, link)
    }

    pub(crate) fn agent_work(&mut self, request: Request) -> Result<Reply, Error> {
        if self.recovery_required.is_some() {
            return Err(Error::Unavailable);
        }
        if let Request::Claim = request {
            return self.claim_work();
        }
        let ownership = self.work.as_ref().cloned().ok_or(Error::Fenced)?;
        if let Request::CompareAndSet(mutation) = request {
            if mutation
                .result_profile()
                .is_some_and(|profile| !self.registry.contains(&profile))
            {
                return Err(Error::Fenced);
            }
            if let Some(profile) = mutation.result_profile() {
                if self.work_profile_retired(profile)? {
                    return Err(Error::Fenced);
                }
            }
            if mutation.next().disposition() == AgentWorkDisposition::Running {
                if let Some(profile) = self.with_work_connection(|connection| {
                    result_profile(connection, mutation.next().key())
                })? {
                    if !self.registry.contains(&profile) || self.work_profile_retired(profile)? {
                        return Err(Error::Fenced);
                    }
                }
            }
        }
        let owner = match request {
            Request::Read { owner, .. } => owner,
            Request::CompareAndSet(mutation) => mutation.next().incarnation(),
            Request::Claim => return Err(Error::Fenced),
        };
        if owner != ownership.incarnation {
            return Err(Error::Fenced);
        }
        // Retain the original namespace lock and revalidate its exact identity
        // before and after every SQLite operation, including reconciliation.
        ownership.verified(|| {
            self.with_work_connection(|connection| {
                verify_owner(connection, owner)?;
                match request {
                    Request::Read { key, .. } => read(connection, key).map(Reply::Record),
                    Request::CompareAndSet(mutation) => compare_and_set(connection, mutation, None)
                        .map(|()| Reply::Record(Some(mutation.next()))),
                    Request::Claim => Err(Error::Fenced),
                }
            })
        })
    }

    pub(crate) fn agent_work_artifact(
        &mut self,
        request: AgentWorkArtifactRequest,
    ) -> Result<AgentWorkArtifactReply, Error> {
        if self.recovery_required.is_some() {
            return Err(Error::Unavailable);
        }
        let ownership = self.work.as_ref().cloned().ok_or(Error::Fenced)?;
        let (owner, profile) = match &request {
            AgentWorkArtifactRequest::Publish(publication) => (
                publication.mutation().next().incarnation(),
                publication.descriptor().profile(),
            ),
            AgentWorkArtifactRequest::Read { owner, profile, .. } => (*owner, *profile),
        };
        if ownership.incarnation != owner
            || !self.registry.contains(&profile)
            || self.work_profile_retired(profile)?
        {
            return Err(Error::Fenced);
        }
        ownership.verified(|| {
            self.with_work_connection(|connection| {
                verify_owner(connection, owner)?;
                match request {
                    AgentWorkArtifactRequest::Publish(publication) => {
                        // Data parsing cannot mint a terminal mutation: that owner
                        // remains the original private publication's proof-bearing CAS.
                        AgentWorkArchivedExtraction::decode(
                            publication.descriptor(),
                            publication.body(),
                        )?;
                        compare_and_set(connection, publication.mutation(), Some(&publication))?;
                        Ok(AgentWorkArtifactReply::Published {
                            record: publication.mutation().next(),
                            descriptor: publication.descriptor(),
                        })
                    }
                    AgentWorkArtifactRequest::Read {
                        record, profile, ..
                    } => {
                        if read(connection, record.key())? != Some(record)
                            || record.disposition() != AgentWorkDisposition::Succeeded
                        {
                            return Err(Error::Conflict);
                        }
                        let expected = result_profile(connection, record.key())?;
                        if expected.is_some_and(|expected| expected != profile) {
                            return Err(Error::Fenced);
                        }
                        let body = read_artifact(connection, record.key(), profile)?;
                        if expected.is_some() && body.is_none() {
                            return Err(Error::Uncertain);
                        }
                        Ok(AgentWorkArtifactReply::Read(body))
                    }
                }
            })
        })
    }

    fn claim_work(&mut self) -> Result<Reply, Error> {
        if self.work.is_none() {
            let mut process = PROCESS_WORK_FENCE
                .try_lock()
                .map_err(|_| Error::Unavailable)?;
            if process.is_some() {
                return Err(Error::Fenced);
            }
            // In-memory/transient stores cannot claim durable product admission.
            let _ = self.dir.as_ref().ok_or(Error::Unavailable)?;
            #[cfg(windows)]
            let lease = if self.windows_work_storage.is_some() {
                WorkLease::Native(
                    self.windows_work_database()
                        .map_err(|_| Error::Unavailable)?
                        .anchor
                        .clone(),
                )
            } else {
                WorkLease::Legacy(
                    LockedPrivateNamespace::open_or_create(
                        self.dir
                            .as_ref()
                            .ok_or(Error::Unavailable)?
                            .join("work-execution"),
                    )
                    .map_err(|_| Error::Unavailable)?,
                )
            };
            #[cfg(not(windows))]
            let lease = WorkLease::Legacy(
                LockedPrivateNamespace::open_or_create(
                    self.dir
                        .as_ref()
                        .ok_or(Error::Unavailable)?
                        .join("work-execution"),
                )
                .map_err(|_| Error::Unavailable)?,
            );
            // Keep the exclusive lock even after an ambiguous transaction. A
            // retry may reconcile this exact owner; another owner cannot race it.
            let ownership = Arc::new(WorkOwnership {
                lease,
                incarnation: AgentWorkIncarnation::generate(),
            });
            *process = Some(ownership.clone());
            self.work = Some(ownership);
        }
        let held = self.work.as_ref().cloned().ok_or(Error::Fenced)?;
        let owner = held.incarnation;
        held.verified(|| self.with_work_connection(|connection| {
            let transaction = connection.transaction().map_err(|_| Error::Unavailable)?;
            let mut records = inventory(&transaction)?;
            for record in &mut records {
                let next = record.interrupted(owner)?;
                if next != *record {
                    let changed = transaction.execute(
                        "UPDATE agent_work_runs SET record = ?1 WHERE run_key = ?2 AND record = ?3 AND terminal = 0",
                        params![next.as_bytes().as_slice(), record.key().as_slice(), record.as_bytes().as_slice()],
                    ).map_err(|_| Error::Uncertain)?;
                    if changed != 1 { return Err(Error::Conflict); }
                    *record = next;
                    #[cfg(all(test, any(target_os = "macos", target_os = "linux", target_os = "windows")))]
                    if tests::fault(tests::Fault::RestartPartial) { return Err(Error::Uncertain); }
                }
            }
            transaction.execute(
                "INSERT INTO agent_work_owner(id, incarnation) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET incarnation = excluded.incarnation",
                [owner.bytes().as_slice()],
            ).map_err(|_| Error::Uncertain)?;
            transaction.commit().map_err(|_| Error::Uncertain)?;
            Ok(Reply::Claimed { owner, records })
        }))
    }
}

fn verify_owner(connection: &Connection, owner: AgentWorkIncarnation) -> Result<(), Error> {
    let current: Option<Vec<u8>> = connection
        .query_row(
            "SELECT incarnation FROM agent_work_owner WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| Error::Uncertain)?;
    if current.as_deref() == Some(owner.bytes().as_slice()) {
        Ok(())
    } else {
        Err(Error::Fenced)
    }
}

fn decode(bytes: Vec<u8>, terminal: bool, key: &[u8]) -> Result<AgentWorkRecord, Error> {
    let bytes: [u8; AGENT_WORK_RECORD_BYTES] = bytes.try_into().map_err(|_| Error::Uncertain)?;
    let record = AgentWorkRecord::decode(bytes).ok_or(Error::Uncertain)?;
    if record.disposition().is_terminal() != terminal || record.key() != key {
        return Err(Error::Uncertain);
    }
    Ok(record)
}

fn inventory(connection: &Connection) -> Result<Vec<AgentWorkRecord>, Error> {
    let count: i64 = connection
        .query_row("SELECT count(*) FROM agent_work_runs", [], |row| row.get(0))
        .map_err(|_| Error::Uncertain)?;
    if !(0..=MAX_DURABLE_AGENT_WORK_RUNS as i64).contains(&count) {
        return Err(Error::Capacity);
    }
    let mut records = Vec::new();
    records
        .try_reserve_exact(count as usize)
        .map_err(|_| Error::Capacity)?;
    let mut statement = connection
        .prepare("SELECT run_key, record, terminal FROM agent_work_runs ORDER BY run_key")
        .map_err(|_| Error::Uncertain)?;
    let mut rows = statement.query([]).map_err(|_| Error::Uncertain)?;
    while let Some(row) = rows.next().map_err(|_| Error::Uncertain)? {
        let key: Vec<u8> = row.get(0).map_err(|_| Error::Uncertain)?;
        records.push(decode(
            row.get(1).map_err(|_| Error::Uncertain)?,
            row.get(2).map_err(|_| Error::Uncertain)?,
            &key,
        )?);
    }
    Ok(records)
}

fn read(connection: &Connection, key: [u8; 32]) -> Result<Option<AgentWorkRecord>, Error> {
    let row: Option<(Vec<u8>, bool)> = connection
        .query_row(
            "SELECT record, terminal FROM agent_work_runs WHERE run_key = ?1",
            [key.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| Error::Uncertain)?;
    row.map(|(bytes, terminal)| decode(bytes, terminal, &key))
        .transpose()
}

fn compare_and_set(
    connection: &mut Connection,
    mutation: AgentWorkJournalMutation,
    publication: Option<&AgentWorkArtifactPublication>,
) -> Result<(), Error> {
    compare_and_set_records(
        connection,
        mutation.expected(),
        mutation.next(),
        mutation.result_profile(),
        publication.map(|publication| ArtifactStorageValue {
            descriptor: publication.descriptor(),
            body: publication.body(),
        }),
    )
}

// Storage bytes are not execution authority. Only the public Hub path above
// supplies them from the original core publication and successful mutation.
#[derive(Clone, Copy)]
struct ArtifactStorageValue<'a> {
    descriptor: AgentWorkArtifactDescriptor,
    body: &'a [u8],
}

fn compare_and_set_records(
    connection: &mut Connection,
    expected: Option<AgentWorkRecord>,
    next: AgentWorkRecord,
    result_destination: Option<ProfileId>,
    publication: Option<ArtifactStorageValue<'_>>,
) -> Result<(), Error> {
    if match expected {
        Some(previous) => !next.is_successor_of(previous),
        None => next.disposition() != AgentWorkDisposition::Admitted || next.revision() != 1,
    } {
        return Err(Error::Transition);
    }
    let transaction = connection.transaction().map_err(|_| Error::Unavailable)?;
    #[cfg(all(
        test,
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    if tests::fault(tests::Fault::BeforeWrite) {
        return Err(Error::Uncertain);
    }
    let current = read(&transaction, next.key())?;
    let stored_profile = result_profile(&transaction, next.key())?;
    if expected.is_none() && current.is_some() && stored_profile != result_destination {
        return Err(Error::Conflict);
    }
    if let Some(publication) = publication {
        if next.disposition() != AgentWorkDisposition::Succeeded
            || stored_profile != Some(publication.descriptor.profile())
        {
            return Err(Error::Transition);
        }
        if current == Some(next) {
            let descriptor = publication.descriptor;
            let archived = read_artifact(&transaction, next.key(), descriptor.profile())?
                .ok_or(Error::Uncertain)?;
            if archived.descriptor() != descriptor {
                return Err(Error::Conflict);
            }
            return Ok(());
        }
    } else if next.disposition() == AgentWorkDisposition::Succeeded && stored_profile.is_some() {
        // A durable-result promise cannot silently degrade to execution-only success.
        return Err(Error::Transition);
    }
    // Idempotency is exact bytes only, including process/revision/disposition.
    if current == Some(next) {
        return Ok(());
    }
    if current != expected {
        return Err(Error::Conflict);
    }
    if let Some(publication) = publication {
        let descriptor = publication.descriptor;
        let retained: i64 = transaction
            .query_row(
                "SELECT coalesce(sum(length(body)), 0) FROM agent_work_artifacts",
                [],
                |row| row.get(0),
            )
            .map_err(|_| Error::Uncertain)?;
        if retained < 0
            || retained as usize + publication.body.len() > MAX_AGENT_WORK_ARTIFACT_TOTAL_BYTES
        {
            return Err(Error::Capacity);
        }
        transaction.execute("INSERT INTO agent_work_artifacts(run_key, profile_id, artifact_id, digest, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![next.key().as_slice(), descriptor.profile().to_string(), descriptor.id().as_slice(), descriptor.digest().as_slice(), publication.body]).map_err(|_| Error::Uncertain)?;
        #[cfg(all(
            test,
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        ))]
        if tests::fault(tests::Fault::AfterArtifactWrite) {
            return Err(Error::Uncertain);
        }
    }
    let changed = if let Some(previous) = expected {
        transaction.execute(
            "UPDATE agent_work_runs SET record = ?1, terminal = ?2 WHERE run_key = ?3 AND record = ?4 AND terminal = 0",
            params![next.as_bytes().as_slice(), next.disposition().is_terminal(), next.key().as_slice(), previous.as_bytes().as_slice()],
        )
    } else {
        if inventory(&transaction)?.len() >= MAX_DURABLE_AGENT_WORK_RUNS { return Err(Error::Capacity); }
        transaction.execute(
            "INSERT INTO agent_work_runs(run_key, record, terminal, result_profile) VALUES (?1, ?2, 0, ?3)",
            params![next.key().as_slice(), next.as_bytes().as_slice(), result_destination.map(|profile| profile.to_string())],
        )
    }.map_err(|_| Error::Uncertain)?;
    if changed != 1 {
        return Err(Error::Conflict);
    }
    #[cfg(all(
        test,
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    if tests::fault(tests::Fault::AfterWrite) {
        return Err(Error::Uncertain);
    }
    transaction.commit().map_err(|_| Error::Uncertain)?;
    #[cfg(all(
        test,
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    if tests::fault(tests::Fault::AfterCommit) {
        return Err(Error::Uncertain);
    }
    Ok(())
}

fn result_profile(connection: &Connection, key: [u8; 32]) -> Result<Option<ProfileId>, Error> {
    let row: Option<(bool, Option<String>)> = connection.query_row(
        "SELECT result_profile IS NULL, CASE WHEN length(CAST(result_profile AS BLOB)) = 26 THEN result_profile END FROM agent_work_runs WHERE run_key = ?1",
        [key.as_slice()], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(|_| Error::Uncertain)?;
    match row {
        None | Some((true, None)) => Ok(None),
        Some((false, Some(raw))) => ProfileId::parse(&raw)
            .filter(|id| id.to_string() == raw)
            .map(Some)
            .ok_or(Error::Uncertain),
        _ => Err(Error::Uncertain),
    }
}

fn read_artifact(
    connection: &Connection,
    key: [u8; 32],
    profile: ProfileId,
) -> Result<Option<AgentWorkArchivedExtraction>, Error> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = connection
        .query_row(
            "SELECT CASE WHEN length(artifact_id) = 16 THEN artifact_id END,
                CASE WHEN length(digest) = 32 THEN digest END,
                CASE WHEN length(body) BETWEEN 1 AND ?3 THEN body END
         FROM agent_work_artifacts WHERE run_key = ?1 AND profile_id = ?2",
            params![
                key.as_slice(),
                profile.to_string(),
                MAX_AGENT_WORK_ARTIFACT_BYTES
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|_| Error::Uncertain)?;
    row.map(|(id, digest, bytes)| {
        let descriptor = AgentWorkArtifactDescriptor::decode(
            id.try_into().map_err(|_| Error::Uncertain)?,
            profile,
            key,
            digest.try_into().map_err(|_| Error::Uncertain)?,
            bytes.len() as u32,
        )
        .ok_or(Error::Uncertain)?;
        AgentWorkArchivedExtraction::decode(descriptor, &bytes)
    })
    .transpose()
}

/// Historical content access does not claim the process execution fence or
/// reconstruct the original journal owner. The caller has checked membership
/// in the selected profile's Work artifact projection on this same actor.
pub(super) fn read_work_evidence(
    connection: &Connection,
    profile: ProfileId,
    link: zephium_core::work::artifact::WorkEvidenceLink,
) -> Result<zephium_core::work::artifact::WorkEvidencePreviewV1, zephium_core::work::WorkError> {
    use zephium_agentic::ArchivedSourceContent;
    use zephium_core::work::{artifact::WorkEvidencePreviewV1, WorkError};
    let key: Option<Vec<u8>> = connection.query_row(
        "SELECT CASE WHEN length(run_key) = 32 THEN run_key END FROM agent_work_artifacts WHERE profile_id = ?1 AND artifact_id = ?2",
        params![profile.to_string(), link.extraction_id.bytes().as_slice()],
        |row| row.get(0),
    ).optional().map_err(|_| WorkError::Unavailable)?;
    let key = key
        .ok_or(WorkError::NotFound)?
        .try_into()
        .map_err(|_| WorkError::Invalid)?;
    let archive = read_artifact(connection, key, profile)
        .map_err(|_| WorkError::Invalid)?
        .ok_or(WorkError::NotFound)?;
    let source = archive.source(link.source_id).ok_or(WorkError::NotFound)?;
    let (mut text, bytes, mut truncated) = match source.content() {
        ArchivedSourceContent::Text { value } => (value.clone(), value.len() as u64, false),
        ArchivedSourceContent::Preview {
            value,
            source_bytes,
            truncated,
        } => (value.clone(), *source_bytes, *truncated),
        ArchivedSourceContent::Boolean { value } => {
            let value = value.to_string();
            let bytes = value.len() as u64;
            (value, bytes, false)
        }
        ArchivedSourceContent::Ordinal { value } => {
            let value = value.to_string();
            let bytes = value.len() as u64;
            (value, bytes, false)
        }
    };
    if text.len() > 8192 {
        let mut end = 8192;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        truncated = true;
    }
    Ok(WorkEvidencePreviewV1 {
        version: 1,
        link,
        origin: source.origin().into(),
        role: source.role().into(),
        text,
        truncated,
        source_bytes: bytes.to_string(),
        link_destination: source.link_destination().map(str::to_owned),
        source: zephium_core::work::artifact::WorkEvidenceSourceV1::NativeExtraction,
    })
}
