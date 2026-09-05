use super::*;

fn document() -> ArchivedDocument {
    ArchivedDocument {
        version: 1,
        id: [1; 16],
        profile: 2_u128.into(),
        key: [3; 32],
        schema: 1,
        observation: 4,
        generation: 1,
        captured_millis: 5,
        fields: vec![
            ArchivedField {
                name: "label".into(),
                value: ArchivedValue::Text {
                    value: "Fixture label".into(),
                    sources: vec![1],
                },
            },
            ArchivedField {
                name: "active".into(),
                value: ArchivedValue::Boolean {
                    value: true,
                    sources: vec![1],
                },
            },
            ArchivedField {
                name: "count".into(),
                value: ArchivedValue::Unsigned {
                    value: 12,
                    sources: vec![1],
                },
            },
            ArchivedField {
                name: "items".into(),
                value: ArchivedValue::TextList {
                    items: vec![ArchivedText {
                        value: "Fixture label".into(),
                        sources: vec![1],
                    }],
                    sources: vec![1],
                },
            },
        ],
        sources: vec![ArchivedSource {
            id: 1,
            origin: "https://artifact.fixture.invalid/".into(),
            role: "paragraph".into(),
            field: 2,
            context: [4; 16],
            context_generation: 1,
            navigation_epoch: 1,
            frame: 0,
            frame_generation: 1,
            invocation: 1,
            snapshot: 1,
            reference: 1,
            browser_derived: false,
            content: ArchivedSourceContent::Text {
                value: "Fixture label".into(),
            },
        }],
    }
}

fn encode(document: &ArchivedDocument) -> (AgentWorkArtifactDescriptor, Vec<u8>) {
    let bytes = serde_json::to_vec(document).unwrap();
    let descriptor = AgentWorkArtifactDescriptor::decode(
        document.id,
        document.profile,
        document.key,
        Sha256::digest(&bytes).into(),
        bytes.len() as u32,
    )
    .unwrap();
    (descriptor, bytes)
}

#[test]
fn archived_roundtrip_keeps_closed_values_quotes_and_redacted_debug() {
    let (descriptor, bytes) = encode(&document());
    let result = AgentWorkArchivedExtraction::decode(descriptor, &bytes).unwrap();
    assert_eq!(result.descriptor(), descriptor);
    assert_eq!(result.trust(), SemanticExtractionTrust::ModelMapped);
    assert_eq!(result.fields().len(), 4);
    let ArchivedValue::TextList { items, sources } = result.fields()[3].value() else {
        panic!()
    };
    assert_eq!(sources, &[1]);
    assert_eq!(items[0].value(), "Fixture label");
    assert_eq!(
        result.source(items[0].source_ids()[0]).unwrap().origin(),
        "https://artifact.fixture.invalid/"
    );
    assert!(result.source(2).is_none());
    let debug = format!("{result:?} {descriptor:?}");
    for content in [
        "Fixture",
        "artifact.fixture",
        &descriptor.profile().to_string(),
        "digest",
    ] {
        assert!(!debug.contains(content));
    }
}

#[test]
fn archive_identity_digest_length_and_canonical_encoding_are_exact() {
    let (descriptor, mut bytes) = encode(&document());
    bytes[1] ^= 1;
    assert!(AgentWorkArchivedExtraction::decode(descriptor, &bytes).is_err());
    let (descriptor, bytes) = encode(&document());
    for altered in [
        AgentWorkArtifactDescriptor {
            id: [9; 16],
            ..descriptor
        },
        AgentWorkArtifactDescriptor {
            profile: 9_u128.into(),
            ..descriptor
        },
        AgentWorkArtifactDescriptor {
            key: [9; 32],
            ..descriptor
        },
        AgentWorkArtifactDescriptor {
            bytes: descriptor.bytes + 1,
            ..descriptor
        },
    ] {
        assert!(AgentWorkArchivedExtraction::decode(altered, &bytes).is_err());
    }
    let mut spaced = bytes.clone();
    spaced.push(b' ');
    let altered = AgentWorkArtifactDescriptor {
        digest: Sha256::digest(&spaced).into(),
        bytes: spaced.len() as u32,
        ..descriptor
    };
    assert!(AgentWorkArchivedExtraction::decode(altered, &spaced).is_err());
    assert!(AgentWorkArtifactDescriptor::decode(
        [1; 16],
        1_u128.into(),
        [2; 32],
        [3; 32],
        MAX_AGENT_WORK_ARTIFACT_BYTES as u32 + 1
    )
    .is_none());
}

#[test]
fn hostile_archives_cannot_expand_shapes_sources_sensitive_fields_or_bounds() {
    type Corruption = Box<dyn Fn(&mut ArchivedDocument)>;
    let cases: Vec<Corruption> = vec![
        Box::new(|document| document.version = 2),
        Box::new(|document| document.schema = 0),
        Box::new(|document| document.sources[0].role = "password".into()),
        Box::new(|document| document.sources[0].role = "script".into()),
        Box::new(|document| {
            document.sources[0].origin = "https://user:pass@fixture.invalid/path".into()
        }),
        Box::new(|document| document.sources[0].field = 4),
        Box::new(|document| document.sources[0].reference = 0),
        Box::new(|document| {
            document.sources[0].content = ArchivedSourceContent::Text {
                value: "Bearer fixture-secret-value".into(),
            }
        }),
        Box::new(|document| document.fields[0].name = "1invalid".into()),
        Box::new(|document| document.fields[1].name = "label".into()),
        Box::new(|document| {
            document.fields[0].value = ArchivedValue::Text {
                value: "value".into(),
                sources: vec![9],
            }
        }),
        Box::new(|document| {
            document.fields[0].value = ArchivedValue::Text {
                value: "value".into(),
                sources: vec![1, 1],
            }
        }),
        Box::new(|document| {
            document.fields[0].value = ArchivedValue::Text {
                value: "x".repeat(MAX_SEMANTIC_EXTRACTION_TEXT_BYTES + 1),
                sources: vec![1],
            }
        }),
        Box::new(|document| {
            document.fields[0].value = ArchivedValue::Text {
                value: "bad\u{202e}value".into(),
                sources: vec![1],
            }
        }),
    ];
    for (index, alter) in cases.into_iter().enumerate() {
        let mut document = document();
        alter(&mut document);
        let (descriptor, bytes) = encode(&document);
        assert!(
            AgentWorkArchivedExtraction::decode(descriptor, &bytes).is_err(),
            "case {index}"
        );
    }
    let (descriptor, bytes) = encode(&document());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["unknown"] = serde_json::json!({"script":"not executable"});
    let bytes = serde_json::to_vec(&value).unwrap();
    let altered = AgentWorkArtifactDescriptor {
        digest: Sha256::digest(&bytes).into(),
        bytes: bytes.len() as u32,
        ..descriptor
    };
    assert!(AgentWorkArchivedExtraction::decode(altered, &bytes).is_err());
}

#[test]
fn preview_provenance_rejects_false_truncation_and_kind_joins() {
    let mut document = document();
    document.sources[0].field = 3;
    for (original, truncated, accepted) in [
        (3, false, true),
        (8, true, true),
        (8, false, false),
        (2, true, false),
        (3, true, false),
    ] {
        document.sources[0].content = ArchivedSourceContent::Preview {
            value: "abc".into(),
            source_bytes: original,
            truncated,
        };
        let (descriptor, bytes) = encode(&document);
        assert_eq!(
            AgentWorkArchivedExtraction::decode(descriptor, &bytes).is_ok(),
            accepted
        );
    }
}
