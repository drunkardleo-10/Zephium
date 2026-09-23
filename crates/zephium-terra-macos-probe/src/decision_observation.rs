use std::io::Write as _;
use zephium_agentic::*;
use zephium_engine::DecisionObservationSite;

use super::ProbeFailure;

pub(super) fn run(site: &std::ffi::OsStr) -> Result<(), ProbeFailure> {
    let (site, name) = match site.to_str() {
        Some("airbnb") => (DecisionObservationSite::Airbnb, "airbnb"),
        Some("yc") => (DecisionObservationSite::Yc, "yc"),
        Some("government") => (DecisionObservationSite::Government, "government"),
        Some("demo-store") => (DecisionObservationSite::DemoStore, "demo-store"),
        Some("book-store") => (DecisionObservationSite::BookStore, "book-store"),
        Some("test-store") => (DecisionObservationSite::TestStore, "test-store"),
        _ => return Err(ProbeFailure::Authority),
    };
    zephium_engine::run_macos_decision_observation_probe(site, move |observation| {
        let budget = SemanticModelEncodingBudget::INITIAL_PROVIDER_EXACT_CONSERVATIVE;
        let observation =
            fit_semantic_observation_for_model(observation, budget).map_err(|_| "projection")?;
        if observation.frames().iter().flat_map(|frame| frame.nodes())
            .any(|node| node.sensitivity() != SemanticSensitivity::Public) {
            return Err("non_public_observation");
        }
        let payload = encode_semantic_observation(&observation, budget)
            .and_then(|encoded| {
                encoded.admit_conservative_utf8(
                    &SemanticTokenizerRevision::try_new("public-eval-utf8-upper-bound-v1".into())
                        .map_err(|_| SemanticModelEncodingError::Invariant)?,
                )
            })
            .map_err(|_| "encoding")?;
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/work-runtime-proof/decision-observations");
        std::fs::create_dir_all(&directory).map_err(|_| "output_directory")?;
        std::fs::write(directory.join(format!("{name}.zsem")), payload.as_str())
            .map_err(|_| "output")?;
        let objective = AgentProviderObjective::try_admit_conservative_utf8(match site {
            DecisionObservationSite::Yc => "Read the duration of the YC program and explain how office hours with YC partners work.",
            DecisionObservationSite::Airbnb => "Find three places to stay in San Francisco for a solo founder during a YC batch.",
            DecisionObservationSite::Government => "Read the official travel guidance available on this government page.",
            DecisionObservationSite::DemoStore
            | DecisionObservationSite::BookStore
            | DecisionObservationSite::TestStore => "Read this product page and report the product name, its displayed price and the product picture.",
        }.into(), &SemanticTokenizerRevision::try_new("public-eval-utf8-upper-bound-v1".into()).map_err(|_| "tokenizer")?)
            .map_err(|_| "objective")?;
        let mut entries = Vec::new();
        for frame in observation.frames() {
            for node in frame.nodes() {
                let mut ops = Vec::new();
                if node.operations().contains(SemanticOperationClass::Scroll) { ops.push(SemanticOperationClass::Scroll); }
                if node.role() == SemanticRole::Link && node.operations().contains(SemanticOperationClass::Click)
                    && node.link_destination().is_some_and(|target| target.as_url().host_str() == frame.frame().origin().as_url().host_str()) {
                    ops.push(SemanticOperationClass::Click);
                }
                if !ops.is_empty() { entries.push((node.reference(), SemanticOperations::try_new(&ops).map_err(|_| "operations")?)); }
            }
        }
        let authority = AgentProviderActionAuthority::try_new(&observation, &entries).ok_or("authority")?;
        let account = AgentContextAccountBinding::new(AgentAccountAttestationId::generate(), observation.request().context(),
            AgentAccountScope::Anonymous, AgentPolicyInstant::from_millis(1));
        if matches!(
            site,
            DecisionObservationSite::DemoStore
                | DecisionObservationSite::BookStore
                | DecisionObservationSite::TestStore
        ) {
            let fields = vec![
                SemanticExtractionFieldSchema::try_text("product_name".into(), true, 512).and_then(SemanticExtractionFieldSchema::with_verbatim_text).map_err(|_| "read_schema")?,
                SemanticExtractionFieldSchema::try_text("displayed_price".into(), true, 64).and_then(SemanticExtractionFieldSchema::with_verbatim_text).map_err(|_| "read_schema")?,
                SemanticExtractionFieldSchema::try_image_url("product_picture".into(), false, 2048).map_err(|_| "read_schema")?,
            ];
            let schema = SemanticExtractionSchema::try_new(SemanticExtractionSchemaId::new(1).ok_or("read_schema")?, fields).map_err(|_| "read_schema")?;
            let (read, _) = DecisionObservation::try_for_read(&observation, &objective, &authority, account, Some(&schema))
                .and_then(DecisionObservation::into_anonymous_eval_requests)
                .map_err(|error| match error {
                    DecisionProjectionError::Capacity => "read_projection_capacity",
                    DecisionProjectionError::Authority => "read_projection_authority",
                })?;
            std::fs::write(directory.join(format!("{name}-read.json")), read.encode().map_err(|_| "decision_encoding")?).map_err(|_| "decision_output")?;
        }
        if matches!(site, DecisionObservationSite::Yc) {
            let fields = vec![
                SemanticExtractionFieldSchema::try_text("program_duration".into(), true, 1024).and_then(SemanticExtractionFieldSchema::with_verbatim_text).map_err(|_| "read_schema")?,
                SemanticExtractionFieldSchema::try_text("office_hours".into(), true, 1024).map_err(|_| "read_schema")?,
            ];
            let schema = SemanticExtractionSchema::try_new(SemanticExtractionSchemaId::new(1).ok_or("read_schema")?, fields).map_err(|_| "read_schema")?;
            let (read, _) = DecisionObservation::try_for_read(&observation, &objective, &authority, account, Some(&schema))
                .and_then(DecisionObservation::into_anonymous_eval_requests).map_err(|_| "read_projection")?;
            std::fs::write(directory.join("yc-read.json"), read.encode().map_err(|_| "decision_encoding")?).map_err(|_| "decision_output")?;
        }
        let (decision, json_comparison) = DecisionObservation::try_new(&observation, &objective, &authority, account)
            .and_then(DecisionObservation::into_anonymous_eval_requests).map_err(|_| "decision_projection")?;
        std::fs::write(directory.join(format!("{name}.json")), decision.encode().map_err(|_| "decision_encoding")?)
            .map_err(|_| "decision_output")?;
        if let Some(comparison) = json_comparison {
            std::fs::write(directory.join(format!("{name}-json.json")), comparison.encode().map_err(|_| "decision_encoding")?)
                .map_err(|_| "decision_output")?;
        }
        let _ = writeln!(
            std::io::stderr(),
            "decision_observation phase=captured site={site:?} frames={} nodes={} state_bytes={}",
            observation.frames().len(),
            observation
                .frames()
                .iter()
                .map(|frame| frame.nodes().len())
                .sum::<usize>(),
            payload.as_str().len()
        );
        Ok(())
    })
    .map_err(|reason| {
        let _ = writeln!(
            std::io::stderr(),
            "decision_observation phase=failed reason={reason}"
        );
        ProbeFailure::Verification
    })
}
