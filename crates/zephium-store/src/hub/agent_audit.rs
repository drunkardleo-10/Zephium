//! Transactional idempotent persistence for content-free agent audit batches.

#![deny(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use zephium_agentic::{
    AgentAuditDelivery, AgentAuditSinkFailure, AGENT_AUDIT_RECORD_V1_BYTES,
    MAX_AGENT_AUDIT_DELIVERY_EVENTS,
};

use super::Hub;

/// Hard durable denial-of-service ceiling. Append never evicts audit history.
pub(crate) const MAX_DURABLE_AGENT_AUDIT_EVENTS: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentAuditAppendOutcome {
    Committed,
    Refused(AgentAuditSinkFailure),
    /// Commit acknowledgement or durable identity was ambiguous. The caller
    /// must retain and replay the exact in-flight delivery.
    Uncertain,
}

struct PreparedAgentAuditDelivery {
    manifest: [u8; 16],
    supervisor: [u8; 8],
    delivery: [u8; 8],
    first: [u8; 8],
    last: [u8; 8],
    events: Vec<PreparedAgentAuditEvent>,
}

struct PreparedAgentAuditEvent {
    id: [u8; 8],
    recorded_at: [u8; 8],
    record: [u8; AGENT_AUDIT_RECORD_V1_BYTES],
}

impl PreparedAgentAuditDelivery {
    fn try_from_delivery(delivery: &AgentAuditDelivery) -> Option<Self> {
        let proof = delivery.proof();
        let expected = usize::from(proof.events());
        if expected == 0 || expected > MAX_AGENT_AUDIT_DELIVERY_EVENTS {
            return None;
        }
        let mut events = Vec::new();
        events.try_reserve_exact(expected).ok()?;
        for event in delivery.events() {
            if event.progress().manifest() != delivery.manifest()
                || event.progress().supervisor() != delivery.supervisor()
                || events
                    .last()
                    .is_some_and(|prior: &PreparedAgentAuditEvent| {
                        prior.id >= event.id().get().to_be_bytes()
                    })
            {
                return None;
            }
            events.push(PreparedAgentAuditEvent {
                id: event.id().get().to_be_bytes(),
                recorded_at: event.recorded_at().millis().to_be_bytes(),
                record: *event.persistence_record().as_bytes(),
            });
        }
        if events.len() != expected
            || events
                .first()
                .is_none_or(|event| event.id != proof.first().get().to_be_bytes())
            || events
                .last()
                .is_none_or(|event| event.id != proof.last().get().to_be_bytes())
        {
            return None;
        }
        Some(Self {
            manifest: delivery.manifest().bytes(),
            supervisor: delivery.supervisor().get().to_be_bytes(),
            delivery: proof.id().get().to_be_bytes(),
            first: proof.first().get().to_be_bytes(),
            last: proof.last().get().to_be_bytes(),
            events,
        })
    }
}

impl Hub {
    pub(crate) fn append_agent_audit(
        &mut self,
        delivery: &AgentAuditDelivery,
    ) -> AgentAuditAppendOutcome {
        if self.recovery_required.is_some() {
            return AgentAuditAppendOutcome::Refused(AgentAuditSinkFailure::Unavailable);
        }
        let Some(prepared) = PreparedAgentAuditDelivery::try_from_delivery(delivery) else {
            return AgentAuditAppendOutcome::Refused(AgentAuditSinkFailure::AppendFailed);
        };
        match exact_delivery_exists(&self.meta, &prepared) {
            Ok(Some(true)) => return AgentAuditAppendOutcome::Committed,
            Ok(Some(false)) | Err(_) => return AgentAuditAppendOutcome::Uncertain,
            Ok(None) => {}
        }

        let transaction = match self.meta.transaction() {
            Ok(transaction) => transaction,
            Err(_) => return AgentAuditAppendOutcome::Refused(AgentAuditSinkFailure::Unavailable),
        };
        match durable_capacity(&transaction, prepared.events.len()) {
            Ok(true) => {}
            Ok(false) => {
                return if transaction.rollback().is_ok() {
                    AgentAuditAppendOutcome::Refused(AgentAuditSinkFailure::Capacity)
                } else {
                    AgentAuditAppendOutcome::Uncertain
                };
            }
            Err(_) => {
                return if transaction.rollback().is_ok() {
                    AgentAuditAppendOutcome::Refused(AgentAuditSinkFailure::AppendFailed)
                } else {
                    AgentAuditAppendOutcome::Uncertain
                };
            }
        }
        if insert_delivery(&transaction, &prepared).is_err() {
            return if transaction.rollback().is_ok() {
                AgentAuditAppendOutcome::Refused(AgentAuditSinkFailure::AppendFailed)
            } else {
                AgentAuditAppendOutcome::Uncertain
            };
        }
        match transaction.commit() {
            Ok(()) => AgentAuditAppendOutcome::Committed,
            Err(_) => match exact_delivery_exists(&self.meta, &prepared) {
                Ok(Some(true)) => AgentAuditAppendOutcome::Committed,
                Ok(Some(false) | None) | Err(_) => AgentAuditAppendOutcome::Uncertain,
            },
        }
    }
}

fn durable_capacity(
    transaction: &Transaction<'_>,
    additional_events: usize,
) -> rusqlite::Result<bool> {
    let (deliveries, events) = transaction.query_row(
        "SELECT delivery_count, event_count FROM agent_audit_state WHERE id = 1",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let maximum = i64::try_from(MAX_DURABLE_AGENT_AUDIT_EVENTS)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    let additional = i64::try_from(additional_events)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    Ok((0..maximum).contains(&deliveries)
        && (0..=maximum).contains(&events)
        && events
            .checked_add(additional)
            .is_some_and(|next| next <= maximum))
}

fn insert_delivery(
    transaction: &Transaction<'_>,
    delivery: &PreparedAgentAuditDelivery,
) -> rusqlite::Result<()> {
    let count = i64::try_from(delivery.events.len())
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
    transaction.execute(
        "INSERT INTO agent_audit_deliveries(
             manifest_id, supervisor_id, delivery_id,
             first_event_id, last_event_id, event_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            delivery.manifest.as_slice(),
            delivery.supervisor.as_slice(),
            delivery.delivery.as_slice(),
            delivery.first.as_slice(),
            delivery.last.as_slice(),
            count,
        ],
    )?;
    for (index, event) in delivery.events.iter().enumerate() {
        let index = i64::try_from(index)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?;
        transaction.execute(
            "INSERT INTO agent_audit_events(
                 manifest_id, supervisor_id, event_id, delivery_id,
                 batch_index, recorded_at, record_version, record
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
            params![
                delivery.manifest.as_slice(),
                delivery.supervisor.as_slice(),
                event.id.as_slice(),
                delivery.delivery.as_slice(),
                index,
                event.recorded_at.as_slice(),
                event.record.as_slice(),
            ],
        )?;
    }
    Ok(())
}

fn exact_delivery_exists(
    connection: &Connection,
    delivery: &PreparedAgentAuditDelivery,
) -> rusqlite::Result<Option<bool>> {
    let exact_delivery = connection
        .query_row(
            "SELECT first_event_id = ?4
                    AND last_event_id = ?5
                    AND event_count = ?6
             FROM agent_audit_deliveries
             WHERE manifest_id = ?1 AND supervisor_id = ?2 AND delivery_id = ?3",
            params![
                delivery.manifest.as_slice(),
                delivery.supervisor.as_slice(),
                delivery.delivery.as_slice(),
                delivery.first.as_slice(),
                delivery.last.as_slice(),
                i64::try_from(delivery.events.len())
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?,
            ],
            |row| row.get::<_, bool>(0),
        )
        .optional()?;
    let Some(true) = exact_delivery else {
        return Ok(exact_delivery.map(|_| false));
    };
    let stored_events = connection.query_row(
        "SELECT count(*) FROM agent_audit_events
         WHERE manifest_id = ?1 AND supervisor_id = ?2 AND delivery_id = ?3",
        params![
            delivery.manifest.as_slice(),
            delivery.supervisor.as_slice(),
            delivery.delivery.as_slice(),
        ],
        |row| row.get::<_, i64>(0),
    )?;
    if stored_events != i64::try_from(delivery.events.len()).unwrap_or(i64::MAX) {
        return Ok(Some(false));
    }
    for (index, event) in delivery.events.iter().enumerate() {
        let exact = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM agent_audit_events
                 WHERE manifest_id = ?1
                   AND supervisor_id = ?2
                   AND event_id = ?3
                   AND delivery_id = ?4
                   AND batch_index = ?5
                   AND recorded_at = ?6
                   AND record_version = 1
                   AND record = ?7
             )",
            params![
                delivery.manifest.as_slice(),
                delivery.supervisor.as_slice(),
                event.id.as_slice(),
                delivery.delivery.as_slice(),
                i64::try_from(index)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))?,
                event.recorded_at.as_slice(),
                event.record.as_slice(),
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if !exact {
            return Ok(Some(false));
        }
    }
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_agentic::{
        AgentAccountScope, AgentAuditDeliveryId, AgentAuditEventId, AgentAuditLedger,
        AgentDelegationSpec, AgentDelegationTopology, AgentEffectScope, AgentPlanNodeAuthority,
        AgentPlanNodeId, AgentPlanNodeScope, AgentPolicyInstant, AgentRunBudget, AgentRunManifest,
        AgentRunManifestId, AgentRunScope, AgentRunSupervisor, AgentSupervisorAttemptId,
        AgentSupervisorId, ContextRunId, SemanticEffectClass, SemanticOrigin, SemanticSensitivity,
    };
    use zephium_core::ids::ProfileId;

    fn manifest_and_node() -> (AgentRunManifest, AgentPlanNodeId) {
        let profile = ProfileId::from(1);
        let origin = SemanticOrigin::parse("https://audit.example.test/private?secret=hidden")
            .expect("origin");
        let effects =
            AgentEffectScope::try_new(&[SemanticEffectClass::Read]).expect("effect scope");
        let node = AgentPlanNodeId::generate();
        let manifest = AgentRunManifest::try_new(
            AgentRunManifestId::generate(),
            ContextRunId::generate(),
            AgentRunScope::try_new(
                vec![profile],
                vec![AgentAccountScope::Anonymous],
                vec![origin.clone()],
                SemanticSensitivity::Public,
                effects,
                Vec::new(),
            )
            .expect("scope"),
            AgentRunBudget::try_new(10, 1_000, 1_000, 1).expect("budget"),
            AgentPolicyInstant::from_millis(100),
            AgentPolicyInstant::from_millis(10_000),
            vec![AgentPlanNodeScope::new(
                node,
                AgentPlanNodeAuthority::try_new(
                    vec![profile],
                    vec![AgentAccountScope::Anonymous],
                    vec![origin],
                    SemanticSensitivity::Public,
                    effects,
                )
                .expect("authority"),
                AgentRunBudget::try_new(10, 1_000, 1_000, 1).expect("node budget"),
                AgentPolicyInstant::from_millis(9_000),
            )],
        )
        .expect("manifest");
        (manifest, node)
    }

    fn delivery(
        manifest: &AgentRunManifest,
        node: AgentPlanNodeId,
        active: bool,
    ) -> AgentAuditDelivery {
        let topology =
            AgentDelegationTopology::try_new(manifest, vec![AgentDelegationSpec::new(node, None)])
                .expect("topology");
        let mut supervisor =
            AgentRunSupervisor::new(AgentSupervisorId::new(1).expect("supervisor"), topology);
        let mut ledger = AgentAuditLedger::try_new(manifest, &supervisor).expect("ledger");
        if active {
            let _execution = supervisor
                .start(node, AgentSupervisorAttemptId::new(1).expect("attempt"))
                .expect("start");
        }
        ledger
            .record_current(
                &supervisor,
                node,
                AgentAuditEventId::new(1).expect("event"),
                AgentPolicyInstant::from_millis(100),
            )
            .expect("record");
        ledger
            .begin_delivery(
                AgentAuditDeliveryId::new(1).expect("delivery"),
                MAX_AGENT_AUDIT_DELIVERY_EVENTS,
            )
            .expect("delivery")
    }

    #[test]
    fn exact_append_and_replay_commit_once_without_content_columns() {
        let (manifest, node) = manifest_and_node();
        let delivery = delivery(&manifest, node, false);
        let expected_record = *delivery
            .events()
            .next()
            .expect("event")
            .persistence_record()
            .as_bytes();
        let directory = tempfile::tempdir().expect("directory");
        let mut hub = Hub::open(directory.path().to_path_buf()).expect("hub");
        assert_eq!(
            hub.append_agent_audit(&delivery),
            AgentAuditAppendOutcome::Committed
        );
        drop(hub);
        let mut hub = Hub::open(directory.path().to_path_buf()).expect("reopen");
        assert_eq!(
            hub.append_agent_audit(&delivery),
            AgentAuditAppendOutcome::Committed
        );
        let counts = hub
            .meta
            .query_row(
                "SELECT delivery_count, event_count FROM agent_audit_state WHERE id = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("counts");
        assert_eq!(counts, (1, 1));
        let stored: Vec<u8> = hub
            .meta
            .query_row("SELECT record FROM agent_audit_events", [], |row| {
                row.get(0)
            })
            .expect("record");
        assert_eq!(stored, expected_record);
        let schema: String = hub
            .meta
            .query_row(
                "SELECT group_concat(name, ',') FROM pragma_table_info('agent_audit_events')",
                [],
                |row| row.get(0),
            )
            .expect("schema");
        for forbidden in ["content", "origin", "url", "prompt", "response", "error"] {
            assert!(!schema.contains(forbidden), "unexpected column: {schema}");
        }
        drop(hub);
        let database = std::fs::read(directory.path().join("meta.sqlite")).expect("database");
        for forbidden in [
            b"audit.example.test".as_slice(),
            b"secret=hidden".as_slice(),
        ] {
            assert!(
                !database
                    .windows(forbidden.len())
                    .any(|window| window == forbidden),
                "page-derived content reached the durable database"
            );
        }
    }

    #[test]
    fn reused_delivery_identity_with_different_progress_is_never_settled() {
        let (manifest, node) = manifest_and_node();
        let queued = delivery(&manifest, node, false);
        let active = delivery(&manifest, node, true);
        let mut hub = Hub::in_memory().expect("hub");
        assert_eq!(
            hub.append_agent_audit(&queued),
            AgentAuditAppendOutcome::Committed
        );
        assert_eq!(
            hub.append_agent_audit(&active),
            AgentAuditAppendOutcome::Uncertain
        );
        let counts = hub
            .meta
            .query_row(
                "SELECT delivery_count, event_count FROM agent_audit_state WHERE id = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("counts");
        assert_eq!(counts, (1, 1));
    }
}
