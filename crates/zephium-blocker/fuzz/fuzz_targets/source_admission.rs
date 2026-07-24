#![no_main]
#![deny(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use zephium_blocker::{CompileTarget, Compiler, FilterSource, SourceFormat, SourceId};

const MAX_INPUT_BYTES: usize = 128 * 1024;
const CHUNK_BYTES: usize = 128;

fn policy(data: &[u8]) -> String {
    let data = &data[..data.len().min(MAX_INPUT_BYTES)];
    let mut output = String::from("||baseline.fuzz.invalid^\n");
    for (index, chunk) in data.chunks(CHUNK_BYTES).enumerate() {
        let mutation = String::from_utf8_lossy(chunk)
            .chars()
            .map(|character| match character {
                '\0' | '\r' | '\n' => ' ',
                other => other,
            })
            .collect::<String>();
        match index % 3 {
            0 => output.push_str(&format!("||case-{index}.fuzz.invalid^{mutation}\n")),
            1 => output.push_str(&format!(
                "@@||case-{index}.fuzz.invalid^$domain=source.invalid{mutation}\n"
            )),
            _ => output.push_str(&format!("! Zephium synthetic mutation {mutation}\n")),
        }
    }
    output
}

fn source(contents: String) -> FilterSource {
    FilterSource::new(
        SourceId::new("zephium-fuzz-source").expect("fixed source id is valid"),
        SourceFormat::Standard,
        contents,
    )
}

fuzz_target!(|data: &[u8]| {
    let contents = policy(data);
    for target in [CompileTarget::Runtime, CompileTarget::WebKit] {
        let first = Compiler::default().compile(target, vec![source(contents.clone())]);
        let second = Compiler::default().compile(target, vec![source(contents.clone())]);
        match (first, second) {
            (Ok(first), Ok(second)) => {
                assert_eq!(first.digest(), second.digest());
                assert_eq!(
                    first.webkit().map(|rules| rules.digest()),
                    second.webkit().map(|rules| rules.digest())
                );
            }
            (Err(first), Err(second)) => {
                assert_eq!(
                    std::mem::discriminant(&first),
                    std::mem::discriminant(&second)
                );
                assert_eq!(first.to_string(), second.to_string());
            }
            _ => panic!("identical bounded source input compiled inconsistently"),
        }
    }
});
