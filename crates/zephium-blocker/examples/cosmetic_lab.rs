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
    let start = Instant::now();
    let recovered = CosmeticPolicy::decode(&bytes).unwrap();
    let decode_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let plan = recovered.document_plan("https://example.com/").unwrap();
    let query_ms = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "{}",
        serde_json::json!({"report":policy.report(),"compile_ms":compile_ms,"decode_ms":decode_ms,"query_ms":query_ms,"policy_bytes":bytes.len(),"initial_css_bytes":plan.css.len(),"generic_index_bytes":plan.generic_index.len()})
    );
    if let Some(path) = std::env::args_os().nth(1) {
        std::fs::write(path, serde_json::to_vec(&serde_json::json!({"css":plan.css.as_ref(),"index":serde_json::from_str::<serde_json::Value>(&plan.generic_index).unwrap(),"exceptions":serde_json::from_str::<serde_json::Value>(&plan.exceptions).unwrap()})).unwrap()).unwrap();
    }
}
