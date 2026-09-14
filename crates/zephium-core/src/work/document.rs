//! Typed rich-text blocks a model may emit for a document artifact. Rust
//! compiles them into the constrained note schema and derives plain
//! paragraphs, so prose never arrives as Markdown or HTML.
use super::*;
use crate::resources::{DocumentMark, DocumentMarkAttrs, DocumentNode, NoteDocument};

pub const MAX_DOCUMENT_BLOCKS: usize = 128;
pub const MAX_DOCUMENT_SPANS: usize = 64;
pub const MAX_DOCUMENT_ITEMS: usize = 64;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkSpanStyle {
    Plain,
    Bold,
    Italic,
    Code,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDocumentSpan {
    pub text: String,
    pub style: WorkSpanStyle,
    #[serde(default)]
    pub href: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDocumentItem {
    pub spans: Vec<WorkDocumentSpan>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkBlockKind {
    Paragraph,
    Heading,
    Quote,
    Bullets,
    Numbered,
}

/// One block. `level` applies to headings; `items` to lists; `spans` to the
/// rest. Strict structured output supplies every field, so unused ones are
/// empty or null rather than absent.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDocumentBlock {
    pub kind: WorkBlockKind,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub spans: Vec<WorkDocumentSpan>,
    #[serde(default)]
    pub items: Vec<WorkDocumentItem>,
}

/// Compiles blocks into plain paragraphs plus the formatted note document.
/// Links must be allowed web URLs; callers add any origin allowlist.
pub fn compile_blocks(
    blocks: &[WorkDocumentBlock],
) -> Result<(Vec<String>, NoteDocument), WorkError> {
    if blocks.is_empty() || blocks.len() > MAX_DOCUMENT_BLOCKS {
        return Err(WorkError::Invalid);
    }
    let mut paragraphs = Vec::with_capacity(blocks.len());
    let mut content = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block.kind {
            WorkBlockKind::Paragraph | WorkBlockKind::Heading | WorkBlockKind::Quote => {
                if !block.items.is_empty() {
                    return Err(WorkError::Invalid);
                }
                let (text, inline) = compile_spans(&block.spans)?;
                let node = match block.kind {
                    WorkBlockKind::Heading => {
                        let level = block.level.ok_or(WorkError::Invalid)?;
                        if !(1..=3).contains(&level) {
                            return Err(WorkError::Invalid);
                        }
                        DocumentNode {
                            kind: "heading".into(),
                            content: inline,
                            text: None,
                            attrs: Some(crate::resources::DocumentAttrs {
                                level: Some(level),
                                ..Default::default()
                            }),
                            marks: vec![],
                        }
                    }
                    WorkBlockKind::Quote => DocumentNode {
                        kind: "blockquote".into(),
                        content: vec![paragraph(inline)],
                        text: None,
                        attrs: None,
                        marks: vec![],
                    },
                    _ => {
                        if block.level.is_some() {
                            return Err(WorkError::Invalid);
                        }
                        paragraph(inline)
                    }
                };
                paragraphs.push(text);
                content.push(node);
            }
            WorkBlockKind::Bullets | WorkBlockKind::Numbered => {
                if block.items.is_empty()
                    || block.items.len() > MAX_DOCUMENT_ITEMS
                    || !block.spans.is_empty()
                    || block.level.is_some()
                {
                    return Err(WorkError::Invalid);
                }
                let mut text = String::new();
                let mut items = Vec::with_capacity(block.items.len());
                for item in &block.items {
                    let (line, inline) = compile_spans(&item.spans)?;
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&line);
                    items.push(DocumentNode {
                        kind: "listItem".into(),
                        content: vec![paragraph(inline)],
                        text: None,
                        attrs: None,
                        marks: vec![],
                    });
                }
                let ordered = block.kind == WorkBlockKind::Numbered;
                paragraphs.push(text);
                content.push(DocumentNode {
                    kind: if ordered { "orderedList" } else { "bulletList" }.into(),
                    content: items,
                    text: None,
                    attrs: ordered.then(|| crate::resources::DocumentAttrs {
                        start: Some(1),
                        ..Default::default()
                    }),
                    marks: vec![],
                });
            }
        }
    }
    let document = NoteDocument {
        version: 1,
        document: DocumentNode {
            kind: "doc".into(),
            content,
            text: None,
            attrs: None,
            marks: vec![],
        },
    };
    if !document.validate() {
        return Err(WorkError::Invalid);
    }
    Ok((paragraphs, document))
}

fn paragraph(inline: Vec<DocumentNode>) -> DocumentNode {
    DocumentNode {
        kind: "paragraph".into(),
        content: inline,
        text: None,
        attrs: None,
        marks: vec![],
    }
}

fn compile_spans(spans: &[WorkDocumentSpan]) -> Result<(String, Vec<DocumentNode>), WorkError> {
    if spans.is_empty() || spans.len() > MAX_DOCUMENT_SPANS {
        return Err(WorkError::Invalid);
    }
    let mut text = String::new();
    let mut inline = Vec::with_capacity(spans.len());
    for span in spans {
        validate_text(&span.text, MAX_WORK_TEXT_BYTES)?;
        text.push_str(&span.text);
        let mut marks = Vec::new();
        let style = match span.style {
            WorkSpanStyle::Plain => None,
            WorkSpanStyle::Bold => Some("bold"),
            WorkSpanStyle::Italic => Some("italic"),
            WorkSpanStyle::Code => Some("code"),
        };
        if let Some(style) = style {
            marks.push(DocumentMark {
                kind: style.into(),
                attrs: None,
            });
        }
        if let Some(href) = &span.href {
            if !crate::navigation::is_allowed_str(href) || href.len() > 2048 {
                return Err(WorkError::Invalid);
            }
            marks.push(DocumentMark {
                kind: "link".into(),
                attrs: Some(DocumentMarkAttrs {
                    href: Some(href.clone()),
                }),
            });
        }
        inline.push(DocumentNode {
            kind: "text".into(),
            content: vec![],
            text: Some(span.text.clone()),
            attrs: None,
            marks,
        });
    }
    if text.trim().is_empty() {
        return Err(WorkError::Invalid);
    }
    Ok((text, inline))
}

/// Every link target inside a formatted document.
pub fn document_links(document: &NoteDocument) -> Vec<&str> {
    let mut links = Vec::new();
    let mut pending = vec![&document.document];
    while let Some(node) = pending.pop() {
        for mark in &node.marks {
            if mark.kind == "link" {
                if let Some(href) = mark.attrs.as_ref().and_then(|a| a.href.as_deref()) {
                    links.push(href);
                }
            }
        }
        pending.extend(node.content.iter());
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, style: WorkSpanStyle, href: Option<&str>) -> WorkDocumentSpan {
        WorkDocumentSpan {
            text: text.into(),
            style,
            href: href.map(Into::into),
        }
    }
    fn block(kind: WorkBlockKind, spans: Vec<WorkDocumentSpan>) -> WorkDocumentBlock {
        WorkDocumentBlock {
            kind,
            level: None,
            spans,
            items: vec![],
        }
    }

    #[test]
    fn blocks_compile_into_paragraphs_and_a_valid_note_with_links() {
        let blocks = vec![
            WorkDocumentBlock {
                level: Some(2),
                ..block(
                    WorkBlockKind::Heading,
                    vec![span("Summary", WorkSpanStyle::Plain, None)],
                )
            },
            block(
                WorkBlockKind::Paragraph,
                vec![
                    span("See ", WorkSpanStyle::Plain, None),
                    span(
                        "the guide",
                        WorkSpanStyle::Bold,
                        Some("https://docs.example/guide"),
                    ),
                    span(".", WorkSpanStyle::Plain, None),
                ],
            ),
            WorkDocumentBlock {
                items: vec![
                    WorkDocumentItem {
                        spans: vec![span("First", WorkSpanStyle::Plain, None)],
                    },
                    WorkDocumentItem {
                        spans: vec![span("Second", WorkSpanStyle::Code, None)],
                    },
                ],
                ..block(WorkBlockKind::Numbered, vec![])
            },
            block(
                WorkBlockKind::Quote,
                vec![span("Quoted", WorkSpanStyle::Italic, None)],
            ),
        ];
        let (paragraphs, document) = compile_blocks(&blocks).unwrap();
        assert_eq!(
            paragraphs,
            vec!["Summary", "See the guide.", "First\nSecond", "Quoted"]
        );
        assert!(document.validate());
        assert_eq!(
            document_links(&document),
            vec!["https://docs.example/guide"]
        );
        assert_eq!(document.plain_text().lines().count(), 5);
        let link = &document.document.content[1].content[1];
        assert_eq!(link.marks.len(), 2);
        assert_eq!(link.marks[1].kind, "link");
        let ordered = &document.document.content[2];
        assert_eq!(ordered.kind, "orderedList");
        assert_eq!(ordered.content.len(), 2);
    }

    #[test]
    fn blocks_refuse_bad_links_levels_and_empty_text() {
        assert!(compile_blocks(&[]).is_err());
        assert!(compile_blocks(&[block(
            WorkBlockKind::Paragraph,
            vec![span("x", WorkSpanStyle::Plain, Some("javascript:alert(1)"))],
        )])
        .is_err());
        assert!(compile_blocks(&[block(
            WorkBlockKind::Paragraph,
            vec![span("x", WorkSpanStyle::Plain, Some("file:///etc/hosts"))],
        )])
        .is_err());
        assert!(compile_blocks(&[WorkDocumentBlock {
            level: Some(4),
            ..block(
                WorkBlockKind::Heading,
                vec![span("x", WorkSpanStyle::Plain, None)],
            )
        }])
        .is_err());
        assert!(compile_blocks(&[block(
            WorkBlockKind::Paragraph,
            vec![span("   ", WorkSpanStyle::Plain, None)],
        )])
        .is_err());
        assert!(compile_blocks(&[block(WorkBlockKind::Bullets, vec![])]).is_err());
    }
}
