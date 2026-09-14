use super::*;
fn paragraph() -> DocumentNode {
    DocumentNode {
        kind: "paragraph".into(),
        content: vec![],
        text: None,
        attrs: None,
        marks: vec![],
    }
}
#[test]
fn validates_real_calendar_dates_and_exact_revisions() {
    assert!(valid_date("2028-02-29"));
    assert!(!valid_date("2027-02-29"));
    assert!(!valid_date("2027-13-01"));
    assert_eq!(revision("9007199254740993"), Some(9007199254740993));
    assert_eq!(revision("01"), None);
}
#[test]
fn accepts_empty_paragraphs_but_rejects_executable_or_unknown_document_shapes() {
    let mut doc = NoteDocument {
        version: 1,
        document: DocumentNode {
            kind: "doc".into(),
            content: vec![paragraph()],
            text: None,
            attrs: None,
            marks: vec![],
        },
    };
    assert!(doc.validate());
    doc.document.content[0].kind = "script".into();
    assert!(!doc.validate());
}
#[test]
fn document_limits_bound_depth_and_node_count() {
    let mut doc = NoteDocument {
        version: 1,
        document: DocumentNode {
            kind: "doc".into(),
            content: vec![paragraph(); 4096],
            text: None,
            attrs: None,
            marks: vec![],
        },
    };
    assert!(!doc.validate());
    doc.document.content = vec![paragraph()];
    for _ in 0..18 {
        let child = doc.document.content.remove(0);
        doc.document.content.push(DocumentNode {
            kind: "blockquote".into(),
            content: vec![child],
            text: None,
            attrs: None,
            marks: vec![],
        });
    }
    assert!(!doc.validate());
}

#[test]
#[cfg(feature = "ipc-types")]
fn boxed_mutation_preserves_json_and_specta_contract() {
    use specta::Type as _;
    let wire = serde_json::json!({"kind":"mutate","command":{
        "version":1,"request_id":"request-0000000000000001",
        "intent":{"kind":"trash","id":"resource-1","expected_revision":"1"}
    }});
    let call: ResourceCall = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(call).unwrap(), wire);
    let mut types = specta::Types::default();
    assert_eq!(
        ResourceCommand::definition(&mut types),
        Box::<ResourceCommand>::definition(&mut types)
    );
    assert!(std::mem::size_of::<ResourceCall>() < std::mem::size_of::<ResourceCommand>());
}

#[test]
fn callers_cannot_mint_object_provenance_or_preview_objects() {
    use crate::work::artifact::*;
    let object =
        |provenance: Option<WorkObjectProvenance>, data: WorkArtifactDataV1| ResourceDraft {
            title: "Object".into(),
            pinned: false,
            content: ResourceContent::Object {
                object: WorkObjectV1 {
                    version: 1,
                    data,
                    evidence: vec![],
                    provenance,
                },
            },
            related: vec![],
        };
    let checklist = WorkArtifactDataV1::Checklist {
        items: vec![WorkChecklistItem {
            text: "Do".into(),
            completed: false,
        }],
    };
    let plain = object(None, checklist.clone());
    assert!(plain.validate() && plain.caller_owned());
    let minted = object(
        Some(WorkObjectProvenance {
            objective: 1.into(),
            execution: 2.into(),
            artifact: 3.into(),
            basis: WorkObjectBasis::Original,
            review: crate::work::WorkOutputReview::Mechanical,
        }),
        checklist,
    );
    assert!(minted.validate() && !minted.caller_owned());
    let call = ResourceCall::Mutate {
        command: Box::new(ResourceCommand {
            version: 1,
            request_id: "request-0000000000000009".into(),
            intent: ResourceIntent::Create { draft: minted },
        }),
    };
    assert!(!call.validate());
    let preview = object(
        None,
        WorkArtifactDataV1::BrowserResourcePreview {
            title: "Page".into(),
            url: "https://example.com/".into(),
            summary: "Descriptive".into(),
        },
    );
    assert!(!preview.validate());
}
