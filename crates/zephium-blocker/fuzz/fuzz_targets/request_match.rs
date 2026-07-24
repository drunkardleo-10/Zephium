#![no_main]
#![deny(unsafe_code)]

use std::fmt::Write as _;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use zephium_blocker::{
    CompileTarget, CompiledRules, Compiler, FilterSource, NetworkRequest, RequestMethod,
    ResourceType, SourceFormat, SourceId,
};

const MAX_PATH_BYTES: usize = 16 * 1024;

fn rules() -> &'static CompiledRules {
    static RULES: OnceLock<CompiledRules> = OnceLock::new();
    RULES.get_or_init(|| {
        Compiler::default()
            .compile(
                CompileTarget::Runtime,
                vec![FilterSource::new(
                    SourceId::new("zephium-fuzz-requests").expect("fixed source id is valid"),
                    SourceFormat::Standard,
                    "||blocked.fuzz.invalid^\n\
                     @@||allowed.fuzz.invalid^$domain=source.invalid\n\
                     ||important.fuzz.invalid^$important\n\
                     ||party.fuzz.invalid^$third-party\n"
                        .to_owned(),
                )],
            )
            .expect("fixed request matcher policy compiles")
    })
}

fn encoded_url(host: &str, bytes: &[u8]) -> String {
    let mut url = format!("https://{host}/");
    for byte in &bytes[..bytes.len().min(MAX_PATH_BYTES)] {
        write!(&mut url, "{byte:02x}").expect("String writes do not fail");
    }
    url
}

fn resource(selector: u8) -> ResourceType {
    const TYPES: [ResourceType; 16] = [
        ResourceType::Beacon,
        ResourceType::Csp,
        ResourceType::Document,
        ResourceType::Dtd,
        ResourceType::Fetch,
        ResourceType::Font,
        ResourceType::Image,
        ResourceType::Media,
        ResourceType::Object,
        ResourceType::Script,
        ResourceType::Stylesheet,
        ResourceType::Subdocument,
        ResourceType::WebSocket,
        ResourceType::Xslt,
        ResourceType::XmlHttpRequest,
        ResourceType::Other,
    ];
    TYPES[usize::from(selector) % TYPES.len()]
}

fn method(selector: u8) -> RequestMethod {
    const METHODS: [RequestMethod; 9] = [
        RequestMethod::Connect,
        RequestMethod::Delete,
        RequestMethod::Get,
        RequestMethod::Head,
        RequestMethod::Options,
        RequestMethod::Patch,
        RequestMethod::Post,
        RequestMethod::Put,
        RequestMethod::Other,
    ];
    METHODS[usize::from(selector) % METHODS.len()]
}

fuzz_target!(|data: &[u8]| {
    let selectors = [
        data.first().copied().unwrap_or(0),
        data.get(1).copied().unwrap_or(0),
        data.get(2).copied().unwrap_or(0),
    ];
    let host = match selectors[0] % 4 {
        0 => "blocked.fuzz.invalid",
        1 => "allowed.fuzz.invalid",
        2 => "important.fuzz.invalid",
        _ => "clean.fuzz.invalid",
    };
    let target = encoded_url(host, data.get(3..).unwrap_or_default());
    let source = encoded_url("source.invalid", data);
    let exact = NetworkRequest::new(
        &target,
        &source,
        resource(selectors[1]),
        method(selectors[2]),
    );
    let first = rules().evaluate(exact);
    let second = rules().evaluate(exact);
    assert_eq!(first, second);

    let independent =
        NetworkRequest::source_independent(&target, resource(selectors[1]), method(selectors[2]));
    let first = rules().evaluate_source_independent(independent);
    let second = rules().evaluate_source_independent(independent);
    assert_eq!(first, second);
});
