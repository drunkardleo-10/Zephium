//! Offline measurements using the exact licensed release seed.
use std::io::Read;
use std::time::Instant;
use zephium_blocker::CosmeticPolicy;

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
    let css = recovered.stylesheet("https://example.com/").unwrap();
    let query_ms = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "{}",
        serde_json::json!({"report":policy.report(),"compile_ms":compile_ms,"decode_ms":decode_ms,"query_ms":query_ms,"policy_bytes":bytes.len(),"generic_css_bytes":css.len()})
    );
    #[cfg(feature = "webkit")]
    if let Some(path) = std::env::args_os().nth(1) {
        let start = Instant::now();
        let json = policy.webkit_top_document_rules().unwrap().unwrap();
        std::fs::write(path, json.as_bytes()).unwrap();
        println!(
            "{}",
            serde_json::json!({"webkit_json_bytes":json.len(),"webkit_emit_ms":start.elapsed().as_secs_f64()*1000.})
        );
    }
}
