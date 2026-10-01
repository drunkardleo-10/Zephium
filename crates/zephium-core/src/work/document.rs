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
    // A model's stray level, empty span or bad link is normalized or skipped;
    // only a document with nothing left to show is refused.
    let mut paragraphs = Vec::with_capacity(blocks.len());
    let mut content = Vec::with_capacity(blocks.len());
    for block in blocks.iter().take(MAX_DOCUMENT_BLOCKS) {
        match block.kind {
            WorkBlockKind::Paragraph | WorkBlockKind::Heading | WorkBlockKind::Quote => {
                let Some((text, inline)) = compile_spans(&block.spans) else {
                    continue;
                };
                let node = match block.kind {
                    WorkBlockKind::Heading => DocumentNode {
                        kind: "heading".into(),
                        content: inline,
                        text: None,
                        attrs: Some(crate::resources::DocumentAttrs {
                            level: Some(block.level.unwrap_or(2).clamp(1, 3)),
                            ..Default::default()
                        }),
                        marks: vec![],
                    },
                    WorkBlockKind::Quote => DocumentNode {
                        kind: "blockquote".into(),
                        content: vec![paragraph(inline)],
                        text: None,
                        attrs: None,
                        marks: vec![],
                    },
                    _ => paragraph(inline),
                };
                paragraphs.push(text);
                content.push(node);
            }
            WorkBlockKind::Bullets | WorkBlockKind::Numbered => {
                // A list written as spans without items is one item.
                let lines: Vec<&[WorkDocumentSpan]> = if block.items.is_empty() {
                    vec![block.spans.as_slice()]
                } else {
                    block
                        .items
                        .iter()
                        .take(MAX_DOCUMENT_ITEMS)
                        .map(|item| item.spans.as_slice())
                        .collect()
                };
                let mut text = String::new();
                let mut items = Vec::with_capacity(lines.len());
                for spans in lines {
                    let Some((line, inline)) = compile_spans(spans) else {
                        continue;
                    };
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
                if items.is_empty() {
                    continue;
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
    if content.is_empty() {
        return Err(WorkError::Invalid);
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

/// None when the spans show no text. Overlong text is clipped, a span with
/// control characters is skipped and a disallowed link keeps its text only.
fn compile_spans(spans: &[WorkDocumentSpan]) -> Option<(String, Vec<DocumentNode>)> {
    let mut text = String::new();
    let mut inline = Vec::with_capacity(spans.len());
    for span in spans.iter().take(MAX_DOCUMENT_SPANS) {
        let value = super::agent::clip_text(&span.text, MAX_WORK_TEXT_BYTES);
        if value.is_empty()
            || value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            continue;
        }
        text.push_str(&value);
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
        if let Some(href) = span
            .href
            .as_deref()
            .filter(|href| href.len() <= 2048 && crate::navigation::is_allowed_str(href))
        {
            marks.push(DocumentMark {
                kind: "link".into(),
                attrs: Some(DocumentMarkAttrs {
                    href: Some(href.to_owned()),
                }),
            });
        }
        inline.push(DocumentNode {
            kind: "text".into(),
            content: vec![],
            text: Some(value),
            attrs: None,
            marks,
        });
    }
    if text.trim().is_empty() {
        return None;
    }
    Some((text, inline))
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
    fn blocks_normalize_bad_links_and_levels_and_skip_empty_text() {
        assert!(compile_blocks(&[]).is_err());
        let (paragraphs, document) = compile_blocks(&[
            block(
                WorkBlockKind::Paragraph,
                vec![span("x", WorkSpanStyle::Plain, Some("javascript:alert(1)"))],
            ),
            WorkDocumentBlock {
                level: Some(4),
                ..block(
                    WorkBlockKind::Heading,
                    vec![span(
                        "file",
                        WorkSpanStyle::Plain,
                        Some("file:///etc/hosts"),
                    )],
                )
            },
            WorkDocumentBlock {
                level: Some(1),
                ..block(
                    WorkBlockKind::Paragraph,
                    vec![span("   ", WorkSpanStyle::Plain, None)],
                )
            },
            block(WorkBlockKind::Bullets, vec![]),
            block(
                WorkBlockKind::Bullets,
                vec![span("one item", WorkSpanStyle::Plain, None)],
            ),
        ])
        .unwrap();
        assert_eq!(paragraphs, vec!["x", "file", "one item"]);
        assert!(document_links(&document).is_empty());
        assert_eq!(
            document.document.content[1].attrs.as_ref().unwrap().level,
            Some(3)
        );
        assert_eq!(document.document.content[2].kind, "bulletList");
        assert!(compile_blocks(&[block(
            WorkBlockKind::Paragraph,
            vec![span("   ", WorkSpanStyle::Plain, None)],
        )])
        .is_err());
    }
}
