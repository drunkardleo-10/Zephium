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
