#![no_main]
#![deny(unsafe_code)]

use libfuzzer_sys::fuzz_target;
use zephium_blocker::{CompileTarget, Compiler, FilterSource, SourceFormat, SourceId};

const MAX_INPUT_BYTES: usize = 128 * 1024;

fn source(data: &[u8]) -> FilterSource {
    let mutation = String::from_utf8_lossy(&data[..data.len().min(MAX_INPUT_BYTES)])
        .chars()
        .map(|character| match character {
            '\0' | '\r' | '\n' => ' ',
            other => other,
        })
        .collect::<String>();
    FilterSource::new(
        SourceId::new("zephium-fuzz-webkit").expect("fixed source id is valid"),
        SourceFormat::Standard,
        format!(
            "||baseline.fuzz.invalid^\n\
             ||converted.fuzz.invalid^$script{mutation}\n\
             @@||allowed.fuzz.invalid^$image,domain=source.invalid{mutation}"
        ),
    )
}

fuzz_target!(|data: &[u8]| {
    let first = Compiler::default().compile(CompileTarget::WebKit, vec![source(data)]);
    let second = Compiler::default().compile(CompileTarget::WebKit, vec![source(data)]);
    match (first, second) {
        (Ok(first), Ok(second)) => {
            let first = first.webkit().expect("WebKit target carries JSON");
            let second = second.webkit().expect("WebKit target carries JSON");
            assert_eq!(first.digest(), second.digest());
            assert_eq!(first.rule_count(), second.rule_count());
            assert_eq!(first.json(), second.json());
            assert!(first.json().len() <= 32 * 1024 * 1024);
        }
        (Err(first), Err(second)) => {
            assert_eq!(
                std::mem::discriminant(&first),
                std::mem::discriminant(&second)
            );
            assert_eq!(first.to_string(), second.to_string());
        }
        _ => panic!("identical bounded WebKit input compiled inconsistently"),
    }
});
