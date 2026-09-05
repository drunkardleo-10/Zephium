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
    AgentWorkDisposition, AgentWorkIncarnation, AgentWorkJournalError as Error,
    AgentWorkJournalReply as Reply, AgentWorkJournalRequest as Request, AgentWorkRecord,
    AGENT_WORK_RECORD_BYTES, MAX_DURABLE_AGENT_WORK_RUNS,
};
use zephium_private_fs::LockedPrivateNamespace;

use super::Hub;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
#[path = "agent_work_tests.rs"]
mod tests;

const _: [(); 96] = [(); AGENT_WORK_RECORD_BYTES];
const _: [(); 1024] = [(); MAX_DURABLE_AGENT_WORK_RUNS];

pub(super) struct WorkOwnership {
    namespace: LockedPrivateNamespace,
    incarnation: AgentWorkIncarnation,
}

// Engine teardown follows Store, and unacknowledged native work may survive
// Store shutdown. Keep this original lock until OS process exit, even when
// the Hub/Store actor has gone. Store replacement cannot reopen admission.
static PROCESS_WORK_FENCE: Mutex<Option<Arc<WorkOwnership>>> = Mutex::new(None);

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(crate) use tests::work_test_guard;

impl Hub {
    pub(crate) fn agent_work(&mut self, request: Request) -> Result<Reply, Error> {
        if self.recovery_required.is_some() {
            return Err(Error::Unavailable);
        }
        if let Request::Claim = request {
            return self.claim_work();
        }
        let ownership = self.work.as_ref().ok_or(Error::Fenced)?;
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
        ownership
            .namespace
            .directory()
            .with_verified_path(|_| {
                verify_owner(&self.meta, owner)?;
                match request {
                    Request::Read { key, .. } => read(&self.meta, key).map(Reply::Record),
                    Request::CompareAndSet(mutation) => {
                        compare_and_set(&mut self.meta, mutation.expected(), mutation.next())
                            .map(|()| Reply::Record(Some(mutation.next())))
                    }
                    Request::Claim => Err(Error::Fenced),
                }
            })
            .map_err(|_| Error::Uncertain)?
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
            let dir = self.dir.as_ref().ok_or(Error::Unavailable)?;
            let namespace = LockedPrivateNamespace::open_or_create(dir.join("work-execution"))
                .map_err(|_| Error::Unavailable)?;
            // Keep the exclusive lock even after an ambiguous transaction. A
            // retry may reconcile this exact owner; another owner cannot race it.
            let ownership = Arc::new(WorkOwnership {
                namespace,
                incarnation: AgentWorkIncarnation::generate(),
            });
            *process = Some(ownership.clone());
            self.work = Some(ownership);
        }
        let held = self.work.as_ref().ok_or(Error::Fenced)?;
        let owner = held.incarnation;
        held.namespace.directory().with_verified_path(|_| {
            let transaction = self.meta.transaction().map_err(|_| Error::Unavailable)?;
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
                    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
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
        }).map_err(|_| Error::Uncertain)?
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
    expected: Option<AgentWorkRecord>,
    next: AgentWorkRecord,
) -> Result<(), Error> {
    if match expected {
        Some(previous) => !next.is_successor_of(previous),
        None => next.disposition() != AgentWorkDisposition::Admitted || next.revision() != 1,
    } {
        return Err(Error::Transition);
    }
    let transaction = connection.transaction().map_err(|_| Error::Unavailable)?;
    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    if tests::fault(tests::Fault::BeforeWrite) {
        return Err(Error::Uncertain);
    }
    let current = read(&transaction, next.key())?;
    // Idempotency is exact bytes only, including process/revision/disposition.
    if current == Some(next) {
        return Ok(());
    }
    if current != expected {
        return Err(Error::Conflict);
    }
    let changed = if let Some(previous) = expected {
        transaction.execute(
            "UPDATE agent_work_runs SET record = ?1, terminal = ?2 WHERE run_key = ?3 AND record = ?4 AND terminal = 0",
            params![next.as_bytes().as_slice(), next.disposition().is_terminal(), next.key().as_slice(), previous.as_bytes().as_slice()],
        )
    } else {
        if inventory(&transaction)?.len() >= MAX_DURABLE_AGENT_WORK_RUNS { return Err(Error::Capacity); }
        transaction.execute(
            "INSERT INTO agent_work_runs(run_key, record, terminal) VALUES (?1, ?2, 0)",
            params![next.key().as_slice(), next.as_bytes().as_slice()],
        )
    }.map_err(|_| Error::Uncertain)?;
    if changed != 1 {
        return Err(Error::Conflict);
    }
    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    if tests::fault(tests::Fault::AfterWrite) {
        return Err(Error::Uncertain);
    }
    transaction.commit().map_err(|_| Error::Uncertain)?;
    #[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
    if tests::fault(tests::Fault::AfterCommit) {
        return Err(Error::Uncertain);
    }
    Ok(())
}
