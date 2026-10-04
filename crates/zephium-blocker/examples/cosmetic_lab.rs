//! Offline measurements using the exact licensed release seed.
use std::io::Read;
use std::time::Instant;
use zephium_blocker::CosmeticPolicy;
use zephium_core::blocker::DocumentStyleProvider;

fn main() {
    let sources: Vec<_> = [
        include_bytes!("../../../assets/blocker-seed/v1/easylist.txt.gz").as_slice(),
        include_bytes!("../../../assets/blocker-seed/v1/easyprivacy.txt.gz").as_slice(),
    ]
    .into_iter()
    .map(|gzip| {
        let mut text = String::new();
        flate2::read::GzDecoder::new(gzip)
            .read_to_string(&mut text)
            .unwrap();
        text
    })
    .collect();
    let start = Instant::now();
    let policy = CosmeticPolicy::compile(sources.iter().map(String::as_str)).unwrap();
    let compile_ms = start.elapsed().as_secs_f64() * 1000.;
    let bytes = policy.encode().unwrap();
    // Measure both a cold restore and a second profile while that policy is live.
    let report = policy.report();
    drop(policy);
    let start = Instant::now();
    let recovered = CosmeticPolicy::decode(&bytes).unwrap();
    let decode_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let second_profile = CosmeticPolicy::decode(&bytes).unwrap();
    let shared_decode_ms = start.elapsed().as_secs_f64() * 1000.;
    let shared = std::sync::Arc::ptr_eq(&recovered, &second_profile);
    let start = Instant::now();
    let document_url = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "https://example.com/".into());
    let plan = recovered.document_plan(&document_url).unwrap();
    let query_ms = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "{}",
        serde_json::json!({"document_url":document_url,"report":report,"shared_decode_ms":shared_decode_ms,"shared":shared,"compile_ms":compile_ms,"decode_ms":decode_ms,"query_ms":query_ms,"policy_bytes":bytes.len(),"initial_css_bytes":plan.css.len(),"generic_index_bytes":plan.generic_index.len()})
    );
    if let Some(path) = std::env::args_os().nth(1) {
        std::fs::write(path, serde_json::to_vec(&serde_json::json!({"css":plan.css.as_ref(),"index":serde_json::from_str::<serde_json::Value>(&plan.generic_index).unwrap(),"exceptions":serde_json::from_str::<serde_json::Value>(&plan.exceptions).unwrap()})).unwrap()).unwrap();
    }
}
