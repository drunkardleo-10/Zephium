//! Mechanical architecture checks for the bounded Terra controller crate.

use std::collections::BTreeSet;
use std::path::Path;

use toml::Value;

const MANIFEST: &str = "crates/zephium-agent-controller/Cargo.toml";
const ROOT: &str = "crates/zephium-agent-controller/src/lib.rs";
const PROBE: &str = "crates/zephium-agent-controller/src/probe.rs";
const TERRA: &str = "crates/zephium-agent-controller/src/terra.rs";
const ACTION: &str = "crates/zephium-agent-controller/src/action.rs";
const WORK: &str = "crates/zephium-agent-controller/src/work.rs";
const FORM: &str = "crates/zephium-agent-controller/src/work_form.rs";
const NAVIGATION: &str = "crates/zephium-agent-controller/src/work_navigation.rs";
const NAVIGATION_POLICY: &str = "crates/zephium-agentic/src/agent_policy/navigation.rs";
const READ: &str = "crates/zephium-agentic/src/semantic_read.rs";
const CONTINUATION: &str = "crates/zephium-agentic/src/agent_provider/continuation.rs";
const POLICY: &str = "crates/zephium-agentic/src/agent_policy.rs";
const QUALIFIER: &str = "crates/zephium-terra-macos-probe/src/main.rs";
const WORK_QUALIFIER: &str = "crates/zephium-terra-macos-probe/src/work_actor.rs";
const ALLOWED_DEPENDENCIES: [&str; 6] = [
    "thiserror",
    "tokio",
    "zephium-agentic",
    "zephium-agent-model-catalog",
    "zephium-agent-provider-transport",
    "zephium-agent-runtime",
];
const FORBIDDEN_TERRA_TOKENS: [&str; 14] = [
    "reqwest",
    "keychain",
    "load_macos_development",
    "zephium_engine",
    "zephium_store",
    "zephium_app",
    "tauri",
    "wry",
    "webkit",
    "rusqlite",
    "tokio::spawn",
    "std::thread",
    "unsafe",
    ".as_str()",
];

/// Checks the release graph and source boundary for the Terra controller.
pub(crate) fn check(repository: &Path) -> Result<(), String> {
    let manifest = read(repository.join(MANIFEST))?;
    let root = read(repository.join(ROOT))?;
    let probe = read(repository.join(PROBE))?;
    let terra = read(repository.join(TERRA))?;
    let action = read(repository.join(ACTION))?;
    validate_manifest(&manifest)?;
    validate_root(&root)?;
    validate_probe(&probe)?;
    validate_terra(&terra)?;
    validate_action(&action)?;
    validate_work(&read(repository.join(WORK))?)?;
    validate_account_refresh(
        &terra,
        &read(repository.join(WORK))?,
        &read(repository.join(POLICY))?,
    )?;
    validate_form(&read(repository.join(FORM))?)?;
    validate_navigation(
        &read(repository.join(NAVIGATION))?,
        &read(repository.join(NAVIGATION_POLICY))?,
        &read(repository.join(CONTINUATION))?,
    )?;
    validate_scoped_extraction(
        &read(repository.join(READ))?,
        &read(repository.join(CONTINUATION))?,
        &read(repository.join(POLICY))?,
    )?;
    validate_workflow_qualifier(&read(repository.join(QUALIFIER))?)?;
    validate_work_actor_qualifier(&read(repository.join(WORK_QUALIFIER))?)?;
    validate_inventory(&repository.join("crates/zephium-agent-controller"))?;
    Ok(())
}

fn read(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref();
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn validate_form(source: &str) -> Result<(), String> {
    for required in [
        "impl AgentWorkTask for AgentWorkFormTask",
        "pub fn try_new_local_preparation(",
        "const MAX_GOALS: usize = 8;",
        "MAX_SEMANTIC_VALUE_PREVIEW_BYTES",
        "SemanticScope::Initial",
        "snapshot.completeness() != SemanticCompleteness::Complete",
        "snapshot.generation() <= last.snapshot",
        "matches.next().is_some()",
        "descendant(snapshot, option_index, target_index)",
        "preview.source_bytes() == goal.value.len()",
        "action.source_observation() != baseline.observation",
        "action.source_observation_generation() != baseline.generation",
        "action.frame() != &baseline.frame",
        "action.bound_action().snapshot_generation() != baseline.snapshot",
        "action.bound_action().option_reference() != binding.option",
        "action.fill_text() != Some(&goal.value)",
        "self.bindings.clear();",
        "self.refused = true;",
        "self.account_sample.get()",
        "sample.context() == context",
    ] {
        if !source.contains(required) {
            return Err(format!("trusted form contract lost boundary: {required}"));
        }
    }
    for forbidden in [
        "println!",
        "eprintln!",
        "tracing::",
        "log::",
        "serde",
        "derive(Debug",
        "derive(Clone, Debug",
        "AgentWorkController::",
        "AgentRunPolicy",
        "AgentRuntimeBrowser",
        "AgentProviderTransport",
        "tokio::",
        "std::thread",
        "fn accept_extraction",
        "fn allows_actions_before_extraction",
        "fn allows_subtree_extraction",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "trusted form contract acquired extra authority or content sink: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_account_refresh(terra: &str, work: &str, policy: &str) -> Result<(), String> {
    let refresh = terra
        .split("pub fn refresh_account(")
        .nth(1)
        .and_then(|tail| tail.split("/// Starts one session").next())
        .ok_or("account refresh lost its owned session boundary")?;
    for required in [
        "self.check_live()?",
        "self.failure = Some(error)",
        "self.policy.accounting().reserved_operations() != 0",
        "self.validate_account_update(account, self.account.context())",
        "account.context() != expected_context",
        "account.account() != self.account.account()",
        "account.observed_at() > now",
        "account.observed_at() < self.account.observed_at()",
        "MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS",
        "account != self.account",
        "account.attestation() == self.account.attestation()",
        "self.account_attestations.contains(&account.attestation())",
        "self.account_attestations.len() >= MAX_BROWSER_ACCOUNT_ATTESTATIONS",
    ] {
        if !refresh.contains(required) {
            return Err(format!("account refresh lost authority fence: {required}"));
        }
    }
    let sample = work
        .split("fn refresh_account(")
        .nth(1)
        .and_then(|tail| tail.split("fn task_progress(").next())
        .ok_or("Work lost trusted account sampling boundary")?;
    if sample
        .matches("self.native.check_control(worker, browser)?")
        .count()
        != 2
        || !sample.contains("self.task.attest_account(context, now);")
        || !sample.contains(".refresh_account(account)")
        || !work.contains("session.continue_inspection(")
        || work
            .matches("state.refresh_account(worker, browser)?")
            .count()
            != 5
        || !policy.contains("MAX_AGENT_ACCOUNT_ATTESTATION_AGE_MILLIS: u64 = 30_000;")
    {
        return Err("Work lost per-admission sampling, control or original expiry boundary".into());
    }
    for forbidden in [
        "AgentContextAccountBinding::new",
        "self.deadline =",
        "self.clock =",
        "self.policy =",
        "self.config =",
    ] {
        if refresh.contains(forbidden) || sample.contains(forbidden) {
            return Err(format!(
                "account refresh minted or replaced authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_scoped_extraction(read: &str, continuation: &str, policy: &str) -> Result<(), String> {
    for (source, required) in [
        (
            read,
            &[
                "struct SemanticReadSubtreeProof",
                "predecessor: SemanticObservationAcknowledgement",
                "if &exact != request",
                "frame.generation() <= anchor.snapshot_generation()",
                "root.key() != anchor.capability().node_key()",
                "ZEPHIUM-SEMANTIC-SUBTREE-READ-GUARD-1",
            ][..],
        ),
        (
            continuation,
            &[
                "pub fn begin_extraction_subtree(",
                "read.matches_subtree(&self.baseline, *target)",
                "subtree_target: Option<crate::SemanticReferenceId>",
                "self.correlation.extraction_schema != Some(schema.id())",
            ][..],
        ),
        (
            policy,
            &[
                "fn provider_subtree_read_taints(",
                "!read.matches_subtree(baseline, target)",
                "cohort.source_guard == baseline.guard()",
                "cohort.contains_reference(target)",
                "fragment.provenance().origin() != anchor.origin()",
                "read_taints(read, account)",
            ][..],
        ),
    ] {
        for boundary in required {
            if !source.contains(boundary) {
                return Err(format!("scoped extraction lost proof boundary: {boundary}"));
            }
        }
    }
    Ok(())
}

fn validate_navigation(actor: &str, policy: &str, continuation: &str) -> Result<(), String> {
    for (source, boundaries) in [
        (actor, &[
            "state.navigation_committed || progress != AgentWorkTaskProgress::ReadyForNavigation",
            "if proposed == &target",
            "state.refresh_account(worker, browser)?",
            "retire_for_navigation(observation, &target, &session.config)",
            ".authorize_navigation(", ".dispatch_navigation(permit, operation, now)",
            ".begin_navigation(id, op)", "state.native.snapshot_generation = None;",
            "session.navigation = Some(active);", "ContextNativeRequest::Navigate(request)",
            "terminal.operation() == operation", ".settle_navigation_terminal(&terminal)",
            "Self::observe(state, worker, browser).await?", "state.task_progress(&fresh)?",
            "state.task.attest_account(operation.context(), now)",
            "account.observed_at() < receipt.settled_at()", "account.attestation() == self.account.attestation()",
            "self.validate_account_update(account, receipt.operation().context())?",
            "SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE",
            ".validate_successor(receipt, observation, request, &self.config)",
            "self.drive(prepared.into_transport_input()).await",
        ][..]),
        (policy, &[
            "self.navigation_used || self.navigation.is_some()", "!self.calls.is_empty()", "!self.effects.is_empty()",
            "!baseline.matches(observation)", "!request.automation.can_automate()",
            "observation.frames()[0].frame().origin() != &origin",
            "operation.kind() != ContextOperationKind::Navigate", "!is_document_successor(",
            "terminal.operation() != active.operation", "target != &active.row.target",
            "self.navigation_receipt = Some(receipt)", "AgentNavigationProgressId(hash.finalize().into())",
        ][..]),
        (continuation, &[
            "pub struct AgentProviderNavigationCheckpoint", "pub fn retire_for_navigation(",
            "self.correlation.navigation_target.as_ref() != Some(target)",
            "pub fn validate_successor(", "request.id() <= self.prior_call.call()",
            "receipt.matches_source(&self.baseline, &self.target)",
            "observation.request().context() != successor", "request.account().account() != receipt.account()",
        ][..]),
    ] {
        for boundary in boundaries {
            if !source.contains(boundary) { return Err(format!("navigation lost boundary: {boundary}")); }
        }
    }
    for forbidden in [
        "self.policy =",
        "self.deadline =",
        "self.turns =",
        "self.config =",
        "self.clock =",
        "execute_semantic_action",
        "try_new_with_redirect_policy",
        "AgentProviderTranscript::",
    ] {
        if actor.contains(forbidden) || policy.contains(forbidden) {
            return Err(format!(
                "navigation widened or replaced authority: {forbidden}"
            ));
        }
    }
    let checkpoint = continuation
        .split("pub struct AgentProviderNavigationCheckpoint {")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .ok_or("navigation checkpoint is missing")?;
    for forbidden in ["transcript", "String", "Arc<str>", "correlation:"] {
        if checkpoint.contains(forbidden) {
            return Err(format!(
                "retired checkpoint retained page replay: {forbidden}"
            ));
        }
    }
    let revoke = actor
        .find(".begin_navigation(id, op)")
        .ok_or("navigation lost ref revocation")?;
    let dispatch = actor
        .find("ContextNativeRequest::Navigate(request)")
        .ok_or("navigation lost dispatch")?;
    if revoke >= dispatch {
        return Err("navigation dispatched before old refs were revoked".into());
    }
    let proposal = actor
        .find(".emit(AgentWorkEventKind::ToolProposed(")
        .ok_or("navigation lost pre-authority proposal publication")?;
    let authorize = actor
        .find(".authorize_navigation(")
        .ok_or("navigation lost policy admission")?;
    let audit = actor
        .find("journal.navigation_active(active)")
        .ok_or("navigation lost active audit recording")?;
    if proposal >= authorize || dispatch >= audit {
        return Err("fallible navigation event/audit work can strand an undispatched owner".into());
    }
    Ok(())
}

fn validate_manifest(source: &str) -> Result<(), String> {
    let manifest: Value = toml::from_str(source)
        .map_err(|error| format!("controller manifest is invalid TOML: {error}"))?;
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller package is missing".to_owned())?;
    if package.get("name").and_then(Value::as_str) != Some("zephium-agent-controller")
        || package.get("publish").and_then(Value::as_bool) != Some(false)
        || package.get("build").and_then(Value::as_bool) != Some(false)
    {
        return Err("controller package identity or build policy drifted".to_owned());
    }
    for forbidden in ["dev-dependencies", "build-dependencies", "target"] {
        if manifest.get(forbidden).is_some() {
            return Err(format!("controller may not declare {forbidden}"));
        }
    }
    let dependencies = manifest
        .get("dependencies")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller dependencies are missing".to_owned())?;
    let actual = dependencies
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = ALLOWED_DEPENDENCIES.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err("controller dependency inventory drifted".to_owned());
    }
    let transport = dependencies
        .get("zephium-agent-provider-transport")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller provider façade dependency is malformed".to_owned())?;
    if transport.get("workspace").and_then(Value::as_bool) != Some(true)
        || transport.get("optional").and_then(Value::as_bool) != Some(true)
        || transport.len() != 2
    {
        return Err("controller provider façade must remain optional workspace-only".to_owned());
    }
    let features = manifest
        .get("features")
        .and_then(Value::as_table)
        .ok_or_else(|| "controller features are missing".to_owned())?;
    require_array(features, "default", &[])?;
    require_array(
        features,
        "provider-transport",
        &[
            "dep:zephium-agent-provider-transport",
            "zephium-agent-provider-transport/provider-transport",
        ],
    )?;
    require_array(
        features,
        "probe-harness",
        &[
            "provider-transport",
            "zephium-agent-provider-transport/probe-harness",
        ],
    )?;
    if features.len() != 3 {
        return Err("controller feature inventory drifted".to_owned());
    }
    Ok(())
}

fn require_array(
    features: &toml::map::Map<String, Value>,
    key: &str,
    expected: &[&str],
) -> Result<(), String> {
    let Some(values) = features.get(key).and_then(Value::as_array) else {
        return Err(format!("controller feature {key} is missing"));
    };
    let actual = values.iter().filter_map(Value::as_str).collect::<Vec<_>>();
    if actual != expected {
        return Err(format!("controller feature {key} drifted"));
    }
    Ok(())
}

fn validate_root(source: &str) -> Result<(), String> {
    for required in [
        "compile_error!(\"the agent controller probe harness is forbidden in optimized builds\")",
        "#[cfg(feature = \"provider-transport\")]\nmod terra;",
        "#[cfg(feature = \"provider-transport\")]\npub use terra",
        "#[cfg(feature = \"probe-harness\")]\nmod probe;",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "controller root lost required boundary: {required}"
            ));
        }
    }
    Ok(())
}

fn validate_probe(source: &str) -> Result<(), String> {
    for required in [
        "AgentProviderSettledToolTurn",
        "AgentBrowserToolProposal::Act",
        "SemanticModelActionQualificationExecution",
        "into_actions()",
        "drop(self.continuation)",
        "settle_for_continuation",
        "compute_semantic_diff",
        "TerraProbeVerifiedTransition",
    ] {
        if !source.contains(required) {
            return Err(format!("controller probe lost required bridge: {required}"));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS {
        if source.contains(forbidden) {
            return Err(format!(
                "controller probe contains forbidden authority token: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_terra(source: &str) -> Result<(), String> {
    let read = source
        .split_once("pub async fn continue_after_read(")
        .and_then(|(_, tail)| tail.split_once("pub async fn extract<'a>("))
        .map(|(read, _)| read)
        .ok_or("controller lost bounded read driver")?;
    for required in [
        "self.check_live()?",
        "self.turns >= MAX_BROWSER_MODEL_TURNS",
        "!self.config.permits_baseline_read()",
        "AgentBrowserScopeProposal::Initial",
        "read_semantic_observation",
        "SemanticReadAuthority::Initial",
        "SemanticReadSensitivityLimit::PublicOnly",
        "SemanticReadBudget::STANDARD",
        ".bind_read_request(request, &self.config, &read, payload)",
        "AgentProviderReadContinuationRequestDraft::try_new(bound)",
        "try_prepare_for_provider_exact_count",
        "self.drive(prepared.into_transport_input()).await",
    ] {
        if !read.contains(required) {
            return Err(format!(
                "controller read lost authority boundary: {required}"
            ));
        }
    }
    for forbidden in [
        "invoke_semantic",
        "capture_once",
        "from_fingerprint",
        "begin_expansion",
        "acknowledge(",
    ] {
        if read.contains(forbidden) {
            return Err(format!(
                "controller read acquired new native/ref authority: {forbidden}"
            ));
        }
    }
    let compact: String = source.split_whitespace().collect();
    if compact.contains("pubfntry_finish_unsuccessful(")
        || compact.contains("pub(crate)fntry_finish_unsuccessful(")
        || compact.contains("pub(super)fntry_finish_unsuccessful(")
    {
        return Err("unsuccessful resource closure must stay private to the Work actor".into());
    }
    for required in [
        "impl AgentRuntimeController for TerraTextOnlyController",
        "try_terra_provider_exact_call_config",
        "settle_terra_provider_terminal",
        "AgentProviderAttempt",
        "count_openai_input_tokens",
        "AgentProviderBatchDisposition::Continue",
        "delta.len()",
        "drop(state.objective.take())",
        "drop(state.credential.take())",
        "attempt.abort(AgentProviderAbortReason::ControllerFault)",
        "state.transport.seal();",
        "const MAX_BROWSER_MODEL_TURNS: u8 = 8;",
        "restrict_to_locate_and_act()",
        "pub async fn start_initial(",
        "pub fn try_finish(",
        "fn try_finish_unsuccessful(",
        "self.seal_terminal(false)",
        "else if !unsuccessful",
        "self.retained_terminal.is_some()",
        "self.action_admission_failure.is_some()",
        "!unsuccessful || refusal.human_review().is_none()",
        "AgentBrowserSessionFinishRefusal",
        "self.policy.accounting().reserved_model_tokens() != 0",
        "action_executions: zephium_agentic::SemanticActionExecutionCoordinator::new()",
        "action_settlements: zephium_agentic::SemanticActionSettlementCoordinator::new()",
        "self.action_executions.seal()",
        "self.action_settlements.seal()",
        "continue_after_verified_action",
        "continue_after_locate",
        "locate_semantic_observation",
        "SemanticModelEncodingBudget::LOCATE_RESULT_PROVIDER_EXACT_CONSERVATIVE",
        ".bind_locate_request(model_request, &self.config, &result, payload)",
        "pub async fn extract<'a>(",
        "schema.id().get() != 1",
        "self.drive_terminal(input, Some(output)).await?",
        "SemanticReadSensitivityLimit::PublicOnly",
        "self.extraction_output.take()",
        ".bind_diff_request(request, &self.config, &diff, payload)",
        ".try_prepare_for_provider_exact_count(&mut self.policy, request, &diff)",
        ".try_prove_shutdown()",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "controller Terra path lost required boundary: {required}"
            ));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS {
        if source.contains(forbidden) {
            return Err(format!(
                "controller Terra path contains forbidden authority token: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_inventory(crate_root: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(crate_root)
        .map_err(|error| format!("cannot inspect controller crate root: {error}"))?;
    let root = entries
        .map(|entry| entry.map_err(|error| format!("cannot inspect controller entry: {error}")))
        .collect::<Result<Vec<_>, _>>()?;
    let names = root
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    if names != BTreeSet::from(["Cargo.toml".to_owned(), "src".to_owned()]) {
        return Err("controller crate root inventory drifted".to_owned());
    }
    let source = crate_root.join("src");
    let entries = std::fs::read_dir(&source)
        .map_err(|error| format!("cannot inspect controller source: {error}"))?;
    let names = entries
        .map(|entry| {
            entry.map_err(|error| format!("cannot inspect controller source entry: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    if names
        != BTreeSet::from([
            "action.rs".to_owned(),
            "lib.rs".to_owned(),
            "probe.rs".to_owned(),
            "terra.rs".to_owned(),
            "work.rs".to_owned(),
            "work_tests.rs".to_owned(),
            "work_combined_tests.rs".to_owned(),
            "work_scoped_tests.rs".to_owned(),
            "work_read_tests.rs".to_owned(),
            "work_account_tests.rs".to_owned(),
            "work_navigation.rs".to_owned(),
            "work_navigation_tests.rs".to_owned(),
            "work_form.rs".to_owned(),
            "work_form_tests.rs".to_owned(),
        ])
    {
        return Err("controller source inventory drifted".to_owned());
    }
    Ok(())
}

fn validate_workflow_qualifier(source: &str) -> Result<(), String> {
    let workflow = source
        .split_once("fn run_variable_workflow()")
        .and_then(|(_, source)| source.split_once("fn run_two_action("))
        .map(|(source, _)| source)
        .ok_or_else(|| "generalized native workflow qualifier is missing".to_owned())?;
    for required in [
        "AgentBrowserSession::try_new",
        "AgentBrowserModel::Luna",
        "AgentBrowserRetention::InspectablePublicData",
        ".next_action(",
        ".authorize_action(",
        ".settle_action(",
        ".continue_after_verified_action(",
        "workflow.progress.observe(initial, final_query, language)",
        "workflow.session = Some(refusal.into_session())",
        "terminal.model_receipts().len() != workflow.metrics.len()",
        "workflow.verified < 3",
        "workflow.metrics.len() > 8",
    ] {
        if !workflow.contains(required) {
            return Err(format!(
                "native workflow lost production boundary: {required}"
            ));
        }
    }
    for forbidden in [
        "TerraProbeActionBridge",
        "SemanticModelActionQualificationExecution",
        "for_execution_qualification",
    ] {
        if workflow.contains(forbidden) {
            return Err(format!(
                "native workflow acquired synthetic authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_action(source: &str) -> Result<(), String> {
    let refusal_branch = source
        .split("AgentEffectAuthorization::NeedsHuman(transition) =>")
        .nth(1)
        .and_then(|tail| tail.split("dispatch_semantic_effect").next())
        .ok_or("controller lost exact pre-dispatch refusal branch")?;
    if source.matches("human_review = Some(transition)").count() != 1
        || !refusal_branch.contains("human_review = Some(transition)")
        || !refusal_branch.contains(".needs_human(transition)")
    {
        return Err("controller review classification escaped exact unissued policy branch".into());
    }
    for required in [
        "SemanticActionBatch::bind",
        "authorize_semantic_effect",
        "dispatch_semantic_effect",
        "execution: &mut SemanticActionExecutionCoordinator",
        "settlement: &mut SemanticActionSettlementCoordinator",
        "terminal: Option<SemanticActionBatchResult>",
        "record_success",
        "verify_semantic_action_terminal",
        "settle_verified_semantic_terminal",
        "finalize_accounted_semantic_action_result",
        "self.pending = Some",
        "self.terminal = Some",
        "*proposal_failure = Some(AgentBrowserActionProposalRefusal",
        "proposal: self",
        "let mut human_review = None",
        "human_review = Some(transition)",
        "drop(self.proposal)",
        "AgentBrowserActionFinalizationRefusal",
        "action.settle_budget().millis() < MIN_AGENT_BROWSER_SNAPSHOT_SETTLE_MILLIS",
        "self.retain_failure(failed);",
        "AgentBrowserActionError::Verification(reason)",
    ] {
        if !source.contains(required) {
            return Err(format!("controller action lost boundary: {required}"));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS.into_iter().chain([
        "SemanticModelActionQualificationExecution",
        "for_execution_qualification",
    ]) {
        if source.contains(forbidden) {
            return Err(format!(
                "controller action contains forbidden authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_work(source: &str) -> Result<(), String> {
    let human = source
        .split("pub(crate) fn needs_human(")
        .nth(1)
        .and_then(|tail| tail.split("pub(super) fn model_settled(").next())
        .ok_or("Work actor lost human-refusal ownership boundary")?;
    if human.contains("execution.take()") || human.contains("wait_for_human(") {
        return Err("Work refusal yielded or discarded its original execution token".into());
    }
    let committed = source
        .find("claim.commit_with_shutdown(proof, settlement, provider)")
        .ok_or("Work actor lost original closure commit")?;
    let discarded = source
        .find("AgentBrowserActionProposalRefusal::discard_after_closure")
        .ok_or("Work actor lost original unissued proposal owner")?;
    if discarded < committed {
        return Err("Work actor discarded refusal owner before original closure commit".into());
    }
    for required in [
        "impl AgentRuntimeController for AgentWorkController",
        "self.account_sample.get()",
        "AgentRunSupervisor",
        "AgentAuditLedger",
        "AgentRunMetricClosure::try_close",
        "AgentNativeShutdownResources::new",
        "claim.commit_with_shutdown(proof, settlement, provider)",
        "state.task_progress(&observation)?",
        "let progress = self.task.evaluate(observation)?;",
        "self.task.extraction_schema() != self.extraction_schema.as_ref()",
        "self.task.allows_actions_before_extraction() != self.actions_before_extraction",
        "(input.durable_result || actions_before_extraction || subtree_extraction)",
        "&& extraction_schema.is_none()",
        "self.task.allows_subtree_extraction() != self.subtree_extraction",
        "self.task.allows_baseline_read() != self.baseline_read",
        "session.config = session.config.with_baseline_read()",
        ".begin_extraction_subtree(",
        "Self::capture_once(state, worker, browser, request, frame).await?",
        "expanded.as_ref().map(|_| observation)",
        "session.config.restrict_to_scoped_extraction()",
        "session.config.restrict_to_actions_and_scoped_extraction()",
        "progress == AgentWorkTaskProgress::Complete && self.actions_before_extraction",
        "progress != AgentWorkTaskProgress::ReadyForExtraction",
        "progress == AgentWorkTaskProgress::ReadyForExtraction",
        "session.config.restrict_to_actions_and_extraction()",
        "captured_at = SemanticCaptureInstant::from_millis(now.millis());",
        "session.extract_from(",
        "if session.turns >= super::MAX_BROWSER_MODEL_TURNS",
        "state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete",
        "state.native.revoke(browser)",
        ".begin_action_settlement(",
        "MAX_AGENT_WORK_EVENTS: usize = 64",
        "AgentWorkOutcome::Recovery",
        "worker.try_drain_terminal_claim_refusal_event()",
        "recovery_close: Option<ContextOperationJoin>",
        "Self::begin_recovery_close(state, browser)",
        "AgentWorkOutcome::ClosedUnsuccessfully",
        "session.try_finish_unsuccessful()",
        "state.native.deferred.is_empty()",
        "state.drained.is_some()",
        "(closure.outcome() == AgentRunProgressOutcome::Succeeded) == unsuccessful",
        "self.publish_terminal(worker, cleanup.is_some()).await",
        ".record_human_refusal(",
        "proposal_refusal: action_proposal_failure",
        "AgentBrowserActionProposalRefusal::discard_after_closure",
        "AgentRunProgressOutcome::Failed(AgentSupervisorFailure::PolicyDenied)",
    ] {
        if !source.contains(required) {
            return Err(format!("Work actor lost boundary: {required}"));
        }
    }
    for forbidden in FORBIDDEN_TERRA_TOKENS.into_iter().chain([
        "SemanticModelActionQualificationExecution",
        "for_execution_qualification",
        "TerraProbeActionBridge",
    ]) {
        if source.contains(forbidden) {
            return Err(format!(
                "Work actor acquired forbidden authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_work_actor_qualifier(source: &str) -> Result<(), String> {
    for required in [
        "type Task = AgentWorkFormTask;",
        "AgentWorkFormTask::try_new_local_preparation(",
        "AgentWorkFormPhase::try_new(vec![",
        "AgentWorkFormGoal::fill(",
        "AgentWorkFormGoal::select(",
        "AgentWorkController::try_new_for_probe(",
        "PendingAgentRuntime::spawn_suspended_with_controller(",
        "pending.bind_browser_port(port)",
        "zephium_engine::run_macos_agentic_work_actor_probe(",
        "zephium_store::SqliteStore::open(",
        "AgentBrowserRetention::InspectablePublicData",
        "AgentWorkOutcome::Succeeded(settlement)",
        "AgentBrowserShutdownOutcome::Clean(_)",
        "StoreShutdownOutcome::Clean",
        "settlement.closure().effects()",
        "impl AgentWorkTask for PublicPreparedResultTask",
        "Some(result.observation()) != self.ready",
        "preview.source_bytes() != value.as_str().len()",
        "preview.text() != value.as_str()",
        "self.extraction.accept_extraction(result)",
    ] {
        if !source.contains(required) {
            return Err(format!(
                "Work qualifier lost actual product boundary: {required}"
            ));
        }
    }
    for forbidden in [
        "TerraProbeActionBridge",
        "for_execution_qualification",
        "SemanticModelActionQualificationExecution",
        "impl AgentBrowserPort",
        "set_var(",
        "std::fs::write",
        "request.body()",
        "println!(\"{observation",
        "println!(\"{response",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Work qualifier acquired synthetic/content authority: {forbidden}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_account_refresh, validate_navigation, validate_scoped_extraction};
    use super::{
        validate_action, validate_form, validate_manifest, validate_probe, validate_root,
        validate_terra, validate_work, validate_work_actor_qualifier, validate_workflow_qualifier,
    };

    const MANIFEST: &str = include_str!("../../crates/zephium-agent-controller/Cargo.toml");
    const ROOT: &str = include_str!("../../crates/zephium-agent-controller/src/lib.rs");
    const PROBE: &str = include_str!("../../crates/zephium-agent-controller/src/probe.rs");
    const TERRA: &str = include_str!("../../crates/zephium-agent-controller/src/terra.rs");
    const ACTION: &str = include_str!("../../crates/zephium-agent-controller/src/action.rs");
    const WORK: &str = include_str!("../../crates/zephium-agent-controller/src/work.rs");
    const READ: &str = include_str!("../../crates/zephium-agentic/src/semantic_read.rs");
    const CONTINUATION: &str =
        include_str!("../../crates/zephium-agentic/src/agent_provider/continuation.rs");
    const POLICY: &str = include_str!("../../crates/zephium-agentic/src/agent_policy.rs");
    const QUALIFIER: &str = include_str!("../../crates/zephium-terra-macos-probe/src/main.rs");
    const WORK_QUALIFIER: &str =
        include_str!("../../crates/zephium-terra-macos-probe/src/work_actor.rs");
    const FORM: &str = include_str!("../../crates/zephium-agent-controller/src/work_form.rs");
    const NAVIGATION: &str =
        include_str!("../../crates/zephium-agent-controller/src/work_navigation.rs");
    const NAVIGATION_POLICY: &str =
        include_str!("../../crates/zephium-agentic/src/agent_policy/navigation.rs");

    #[test]
    fn current_controller_boundary_is_valid() {
        validate_manifest(MANIFEST).expect("controller manifest");
        validate_root(ROOT).expect("controller root");
        validate_probe(PROBE).expect("controller probe");
        validate_terra(TERRA).expect("controller Terra path");
        validate_action(ACTION).expect("controller native action path");
        validate_work(WORK).expect("production Work actor");
        validate_account_refresh(TERRA, WORK, POLICY).expect("trusted account sampling");
        validate_form(FORM).expect("production trusted form contract");
        validate_navigation(NAVIGATION, NAVIGATION_POLICY, CONTINUATION)
            .expect("exact document continuation");
        validate_scoped_extraction(READ, CONTINUATION, POLICY).expect("scoped extraction proof");
        validate_workflow_qualifier(QUALIFIER).expect("same-driver native qualification path");
        validate_work_actor_qualifier(WORK_QUALIFIER)
            .expect("actual actor/native/store qualification path");
    }

    #[test]
    fn navigation_cannot_restore_replay_widen_target_or_replace_run_owners() {
        for removed in [
            "state.refresh_account(worker, browser)?",
            ".begin_navigation(id, op)",
            "state.task_progress(&fresh)?",
            "account.observed_at() < receipt.settled_at()",
        ] {
            assert!(validate_navigation(
                &NAVIGATION.replace(removed, "removed"),
                NAVIGATION_POLICY,
                CONTINUATION
            )
            .is_err());
        }
        for removed in [
            "target != &active.row.target",
            "!is_document_successor(",
            "self.navigation_receipt = Some(receipt)",
        ] {
            assert!(validate_navigation(
                NAVIGATION,
                &NAVIGATION_POLICY.replace(removed, "removed"),
                CONTINUATION
            )
            .is_err());
        }
        for forbidden in [
            "self.policy =",
            "self.deadline =",
            "self.turns =",
            "try_new_with_redirect_policy",
        ] {
            assert!(validate_navigation(
                &format!("{NAVIGATION}\n{forbidden}"),
                NAVIGATION_POLICY,
                CONTINUATION
            )
            .is_err());
        }
        assert!(validate_navigation(
            &NAVIGATION.replace(
                "let dispatch = browser.dispatch",
                "journal.navigation_active(active); let dispatch = browser.dispatch"
            ),
            NAVIGATION_POLICY,
            CONTINUATION
        )
        .is_err());
        assert!(validate_navigation(
            NAVIGATION,
            NAVIGATION_POLICY,
            &CONTINUATION.replace(
                "pub struct AgentProviderNavigationCheckpoint {",
                "pub struct AgentProviderNavigationCheckpoint { transcript: String,"
            )
        )
        .is_err());
    }

    #[test]
    fn account_sampling_cannot_renew_time_switch_identity_or_skip_a_boundary() {
        for removed in [
            "self.validate_account_update(account, self.account.context())",
            "account.context() != expected_context",
            "account.account() != self.account.account()",
            "self.account_attestations.contains(&account.attestation())",
            "self.failure = Some(error)",
        ] {
            assert!(
                validate_account_refresh(&TERRA.replace(removed, "removed"), WORK, POLICY).is_err()
            );
        }
        for removed in [
            "state.refresh_account(worker, browser)?",
            "self.task.attest_account(context, now);",
            "self.native.check_control(worker, browser)?",
        ] {
            assert!(
                validate_account_refresh(TERRA, &WORK.replacen(removed, "removed", 1), POLICY)
                    .is_err()
            );
        }
        assert!(validate_account_refresh(
            TERRA,
            WORK,
            &POLICY.replace("u64 = 30_000;", "u64 = 600_000;")
        )
        .is_err());
        assert!(validate_account_refresh(
            &TERRA.replace(
                "pub fn refresh_account(",
                "pub fn refresh_account( /* self.deadline = */"
            ),
            WORK,
            POLICY
        )
        .is_err());
    }

    #[test]
    fn form_task_cannot_lose_value_ref_or_freshness_binding_or_gain_content_sinks() {
        for boundary in [
            "action.fill_text() != Some(&goal.value)",
            "action.bound_action().option_reference() != binding.option",
            "snapshot.generation() <= last.snapshot",
            "self.refused = true;",
            "SemanticScope::Initial",
        ] {
            assert!(validate_form(&FORM.replace(boundary, "removed_boundary")).is_err());
        }
        for forbidden in [
            "tracing::info!",
            "serde::Serialize",
            "AgentRuntimeBrowser",
            "AgentRunPolicy",
            "tokio::spawn",
        ] {
            assert!(validate_form(&format!("{FORM}\n{forbidden}")).is_err());
        }
    }

    #[test]
    fn baseline_read_cannot_lose_policy_or_gain_capture_authority() {
        for required in [
            "!self.config.permits_baseline_read()",
            ".bind_read_request(request, &self.config, &read, payload)",
            "AgentProviderReadContinuationRequestDraft::try_new(bound)",
        ] {
            assert!(validate_terra(&TERRA.replace(required, "removed_boundary")).is_err());
        }
        for forbidden in ["invoke_semantic", "begin_expansion", "from_fingerprint"] {
            assert!(validate_terra(&TERRA.replace(
                "pub async fn continue_after_read(",
                &format!("pub async fn continue_after_read( /* {forbidden} */")
            ))
            .is_err());
        }
        assert!(validate_work(&WORK.replace(
            "self.task.allows_baseline_read() != self.baseline_read",
            "false"
        ))
        .is_err());
    }

    #[test]
    fn scoped_read_cannot_lose_anchor_delivery_or_fresh_read_taint_binding() {
        for boundary in [
            "if &exact != request",
            "frame.generation() <= anchor.snapshot_generation()",
            "root.key() != anchor.capability().node_key()",
            "ZEPHIUM-SEMANTIC-SUBTREE-READ-GUARD-1",
        ] {
            assert!(validate_scoped_extraction(
                &READ.replace(boundary, "removed_boundary"),
                CONTINUATION,
                POLICY
            )
            .is_err());
        }
        for boundary in [
            "read.matches_subtree(&self.baseline, *target)",
            "subtree_target: Option<crate::SemanticReferenceId>",
        ] {
            assert!(validate_scoped_extraction(
                READ,
                &CONTINUATION.replace(boundary, "removed_boundary"),
                POLICY
            )
            .is_err());
        }
        for boundary in [
            "!read.matches_subtree(baseline, target)",
            "cohort.contains_reference(target)",
            "fragment.provenance().origin() != anchor.origin()",
        ] {
            assert!(validate_scoped_extraction(
                READ,
                CONTINUATION,
                &POLICY.replace(boundary, "removed_boundary")
            )
            .is_err());
        }
    }

    #[test]
    fn actor_qualification_requires_actual_runtime_native_and_durable_closure() {
        for boundary in [
            "pending.bind_browser_port(port)",
            "zephium_store::SqliteStore::open(",
            "AgentBrowserShutdownOutcome::Clean(_)",
            "AgentWorkOutcome::Succeeded(settlement)",
            "impl AgentWorkTask for PublicPreparedResultTask",
            "Some(result.observation()) != self.ready",
            "preview.source_bytes() != value.as_str().len()",
            "preview.text() != value.as_str()",
            "self.extraction.accept_extraction(result)",
        ] {
            assert!(validate_work_actor_qualifier(
                &WORK_QUALIFIER.replace(boundary, "removed_boundary")
            )
            .is_err());
        }
        assert!(
            validate_work_actor_qualifier(&format!("{WORK_QUALIFIER}\nimpl AgentBrowserPort"))
                .is_err()
        );
    }

    #[test]
    fn mutation_cannot_reenable_default_transport_or_content_reading() {
        assert!(validate_manifest(
            &MANIFEST.replace("default = []", "default = [\"provider-transport\"]")
        )
        .is_err());
        assert!(validate_terra(&format!("{TERRA}\nlet _ = delta.as_str();")).is_err());
    }

    #[test]
    fn unsuccessful_drain_cannot_bypass_original_debt_or_claim_business_success() {
        for visibility in ["pub ", "pub(crate) ", "pub(super) "] {
            assert!(validate_terra(&TERRA.replace(
                "fn try_finish_unsuccessful(",
                &format!("{visibility}fn try_finish_unsuccessful("),
            ))
            .is_err());
        }
        for boundary in [
            "self.retained_terminal.is_some()",
            "self.action_admission_failure.is_some()",
            "!unsuccessful || refusal.human_review().is_none()",
            "else if !unsuccessful",
        ] {
            assert!(validate_terra(&TERRA.replace(boundary, "removed_boundary")).is_err());
        }
        for boundary in [
            "state.native.deferred.is_empty()",
            "state.drained.is_some()",
            "(closure.outcome() == AgentRunProgressOutcome::Succeeded) == unsuccessful",
            "claim.commit_with_shutdown(proof, settlement, provider)",
            ".record_human_refusal(",
            "proposal_refusal: action_proposal_failure",
            "AgentBrowserActionProposalRefusal::discard_after_closure",
        ] {
            assert!(validate_work(&WORK.replace(boundary, "removed_boundary")).is_err());
        }
    }

    #[test]
    fn combined_task_cannot_bypass_frozen_phase_or_current_result_binding() {
        for boundary in [
            "self.task.extraction_schema() != self.extraction_schema.as_ref()",
            "self.task.allows_actions_before_extraction() != self.actions_before_extraction",
            "(input.durable_result || actions_before_extraction || subtree_extraction)",
            "&& extraction_schema.is_none()",
            "self.task.allows_subtree_extraction() != self.subtree_extraction",
            ".begin_extraction_subtree(",
            "Self::capture_once(state, worker, browser, request, frame).await?",
            "expanded.as_ref().map(|_| observation)",
            "session.config.restrict_to_scoped_extraction()",
            "session.config.restrict_to_actions_and_scoped_extraction()",
            "progress == AgentWorkTaskProgress::Complete && self.actions_before_extraction",
            "progress != AgentWorkTaskProgress::ReadyForExtraction",
            "progress == AgentWorkTaskProgress::ReadyForExtraction",
            "session.config.restrict_to_actions_and_extraction()",
            "captured_at = SemanticCaptureInstant::from_millis(now.millis());",
            "session.extract_from(",
            "if session.turns >= super::MAX_BROWSER_MODEL_TURNS",
            "state.task.accept_extraction(&result)? != AgentWorkTaskProgress::Complete",
        ] {
            assert!(validate_work(&WORK.replace(boundary, "removed_boundary")).is_err());
        }
    }

    #[test]
    fn mutation_cannot_bypass_action_authority_or_drop_pending_ownership() {
        for boundary in [
            "authorize_semantic_effect",
            "dispatch_semantic_effect",
            "verify_semantic_action_terminal",
            "settle_verified_semantic_terminal",
            "finalize_accounted_semantic_action_result",
            "self.pending = Some",
        ] {
            assert!(
                validate_action(&ACTION.replace(boundary, "removed_boundary")).is_err(),
                "{boundary}"
            );
        }
        assert!(validate_action(&format!(
            "{ACTION}\nSemanticModelActionQualificationExecution"
        ))
        .is_err());
    }

    #[test]
    fn workflow_qualifier_cannot_replace_real_policy_or_claim_partial_success() {
        for boundary in [
            "AgentBrowserSession::try_new",
            ".authorize_action(",
            ".settle_action(",
            "workflow.progress.observe(initial, final_query, language)",
            "workflow.session = Some(refusal.into_session())",
            "terminal.model_receipts().len() != workflow.metrics.len()",
        ] {
            assert!(
                validate_workflow_qualifier(&QUALIFIER.replace(boundary, "removed_boundary"))
                    .is_err()
            );
        }
    }
}
