//! Release-excluded retention selection; original scoped ownership is unchanged.
use super::*;
use std::io::Write as _;

impl RetainedWork {
    pub(super) fn public_claim_refusal_diagnostic(&self) {
        let _ = write_claim_refusal_diagnostic(
            &mut std::io::stdout().lock(),
            &self.inventory,
            self.phase,
            self.record.is_some(),
            self.active.is_some(),
        );
    }

    pub(in crate::work_resources) fn public_retention_diagnostic(&self) {
        let phase = self
            .owner
            .shared
            .lock_rows()
            .ok()
            .and_then(|rows| rows.phase(&self.resource).ok());
        let resource = self.owner.shared.resource(&self.resource).ok();
        let healthy = resource
            .as_ref()
            .is_some_and(|row| self.owner.shared.current(row).is_ok());
        let idle = resource.as_ref().is_some_and(|row| {
            row.flights.load(Ordering::Acquire) == 0 && row.reads.load(Ordering::Acquire) == 0
        });
        let reusable = resource
            .as_ref()
            .is_some_and(|row| row.reusable.load(Ordering::Acquire));
        let _ = writeln!(std::io::stdout().lock(), "work-retained-product-resource: phase={phase:?} original_row_healthy={healthy} idle={idle} reusable={reusable} destruction_started={} content=redacted", self.destruction.is_some() || self.destroyed);
    }
}

fn write_claim_refusal_diagnostic(
    output: &mut impl std::io::Write,
    inventory: &[AgentWorkRecord],
    phase: AdmissionPhase,
    current_record: bool,
    execution_started: bool,
) -> std::io::Result<()> {
    let nonterminal = inventory
        .iter()
        .filter(|record| !record.disposition().is_terminal())
        .count();
    let unresolved = inventory
        .iter()
        .filter(|record| record.debt() != AgentWorkDebt::NONE)
        .count();
    let ordered = !inventory
        .windows(2)
        .any(|pair| pair[0].key() >= pair[1].key());
    writeln!(output, "work-retained-product-admission-refused: stage=claim_inventory phase={phase:?} inventory={} ordered={ordered} nonterminal={nonterminal} unresolved_debt={unresolved} current_record={current_record} execution_started={execution_started} content=redacted", inventory.len())
}

impl StagedActor {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::work_resources) fn for_public_qualification(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        config: AgentRuntimeConfig,
        provider: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<Self, AgentWorkFailure> {
        let lease = browser.binding().lease().clone();
        let (controller, handle, scope) = AgentWorkRetainedController::try_new_for_public_probe(
            input,
            browser,
            provider,
            credential,
            audit.clone(),
            task,
        )?;
        let deadline = controller.deadline()?;
        Ok(Self {
            controller: Box::new(controller),
            handle,
            scope,
            lease,
            audit,
            config,
            deadline,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interrupted() -> AgentWorkRecord {
        let mut bytes = [0; AGENT_WORK_RECORD_BYTES];
        bytes[0] = 1;
        bytes[1] = AgentWorkDisposition::Interrupted as u8;
        bytes[2] = AgentWorkDebt::UNKNOWN.bits();
        bytes[15] = 2;
        bytes[16..32].copy_from_slice(&AgentWorkIncarnation::generate().bytes());
        bytes[47] = 1;
        bytes[63] = 1;
        AgentWorkRecord::decode(bytes).unwrap()
    }

    #[test]
    fn claim_refusal_diagnostic_preserves_and_distinguishes_historical_debt() {
        let original = interrupted();
        for (record, nonterminal) in [
            (original, 1),
            (
                original
                    .transition(AgentWorkDisposition::FreshAdmissionRequired)
                    .unwrap(),
                0,
            ),
            (
                original.transition(AgentWorkDisposition::Rejected).unwrap(),
                0,
            ),
        ] {
            let inventory = vec![record];
            let mut output = Vec::new();
            write_claim_refusal_diagnostic(
                &mut output,
                &inventory,
                AdmissionPhase::Loading,
                false,
                false,
            )
            .unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), format!("work-retained-product-admission-refused: stage=claim_inventory phase=Loading inventory=1 ordered=true nonterminal={nonterminal} unresolved_debt=1 current_record=false execution_started=false content=redacted\n"));
            assert_eq!(inventory, vec![record]);
            assert_eq!(inventory[0].debt(), AgentWorkDebt::UNKNOWN);
        }
    }

    #[test]
    fn claim_refusal_diagnostic_reports_duplicate_inventory_without_identifiers() {
        let record = interrupted();
        let mut output = Vec::new();
        write_claim_refusal_diagnostic(
            &mut output,
            &[record, record],
            AdmissionPhase::Loading,
            false,
            false,
        )
        .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "work-retained-product-admission-refused: stage=claim_inventory phase=Loading inventory=2 ordered=false nonterminal=2 unresolved_debt=2 current_record=false execution_started=false content=redacted\n");
    }
}
