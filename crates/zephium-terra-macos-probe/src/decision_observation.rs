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
        Some("book-catalog") => (DecisionObservationSite::BookCatalog, "book-catalog"),
        Some("test-store") => (DecisionObservationSite::TestStore, "test-store"),
        Some("airbnb-listing") => (DecisionObservationSite::AirbnbListing, "airbnb-listing"),
        Some("lego-theme") => (DecisionObservationSite::LegoTheme, "lego-theme"),
        Some("consent") => (DecisionObservationSite::Consent, "consent"),
        Some("interstitial") => (DecisionObservationSite::Interstitial, "interstitial"),
        Some("documentation") => (DecisionObservationSite::Documentation, "documentation"),
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
            DecisionObservationSite::AirbnbListing => "Read this listing and report its name, displayed nightly price, displayed monthly total, stay dates or minimum stay, and picture.",
            DecisionObservationSite::LegoTheme => "Collect three LEGO Architecture sets from this catalog with their displayed prices, product links and pictures.",
            DecisionObservationSite::BookCatalog => "Collect three books from this catalog with their displayed prices, availability, product links and pictures.",
            DecisionObservationSite::Consent => "List the product categories this store's home page offers.",
            DecisionObservationSite::Interstitial => "List the clothing categories this store's home page offers.",
            DecisionObservationSite::Documentation => "List every situation in which WAL mode does not work or has drawbacks.",
        }.into(), &SemanticTokenizerRevision::try_new("public-eval-utf8-upper-bound-v1".into()).map_err(|_| "tokenizer")?)
            .map_err(|_| "objective")?;
        let mut entries = Vec::new();
        for frame in observation.frames() {
            // As the Work reading policy does: while a dialog is shown, only
            // its own controls are offered, and a dialog's buttons are.
            let dialog = frame.nodes().iter().position(|node| node.role() == SemanticRole::Dialog);
            let inside = |node: &SemanticNode| {
                let mut parent = node.parent();
                while let Some(index) = parent {
                    if Some(usize::from(index)) == dialog { return true; }
                    parent = frame.nodes().get(usize::from(index)).and_then(SemanticNode::parent);
                }
                false
            };
            for node in frame.nodes() {
                if dialog.is_some() && !inside(node) { continue; }
                let mut ops = Vec::new();
                if node.operations().contains(SemanticOperationClass::Scroll) { ops.push(SemanticOperationClass::Scroll); }
                if node.role() == SemanticRole::Link && node.operations().contains(SemanticOperationClass::Click)
                    && node.link_destination().is_some_and(|target| target.as_url().host_str() == frame.frame().origin().as_url().host_str()) {
                    ops.push(SemanticOperationClass::Click);
                }
                if dialog.is_some() && node.role() == SemanticRole::Button && node.operations().contains(SemanticOperationClass::Click) {
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
        if matches!(site, DecisionObservationSite::AirbnbListing) {
            let verbatim = |name: &str, required| SemanticExtractionFieldSchema::try_text(name.into(), required, 512).and_then(SemanticExtractionFieldSchema::with_verbatim_text);
            let fields = vec![
                verbatim("listing_name", true).map_err(|_| "read_schema")?,
                verbatim("nightly_price", false).map_err(|_| "read_schema")?,
                verbatim("monthly_total_displayed", false).map_err(|_| "read_schema")?,
                verbatim("stay_dates_or_minimum", false).map_err(|_| "read_schema")?,
                SemanticExtractionFieldSchema::try_image_url("listing_picture".into(), false, 2048).map_err(|_| "read_schema")?,
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
        if matches!(site, DecisionObservationSite::BookCatalog) {
            let verbatim = |name: &str| SemanticExtractionFieldSchema::try_text(name.into(), false, 1024).and_then(SemanticExtractionFieldSchema::with_verbatim_text);
            let columns = vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 512).map_err(|_| "read_schema")?,
                verbatim("displayed_price").map_err(|_| "read_schema")?,
                verbatim("availability").map_err(|_| "read_schema")?,
                SemanticExtractionFieldSchema::try_url("product_url".into(), false, 2048).map_err(|_| "read_schema")?,
                SemanticExtractionFieldSchema::try_image_url("image".into(), false, 2048).map_err(|_| "read_schema")?,
            ];
            let rows = SemanticExtractionFieldSchema::try_rows("output_0".into(), true, columns, 3).map_err(|_| "read_schema")?;
            let schema = SemanticExtractionSchema::try_new(SemanticExtractionSchemaId::new(1).ok_or("read_schema")?, vec![rows]).map_err(|_| "read_schema")?;
            let (read, _) = DecisionObservation::try_for_read(&observation, &objective, &authority, account, Some(&schema))
                .and_then(DecisionObservation::into_anonymous_eval_requests)
                .map_err(|_| "read_projection")?;
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
