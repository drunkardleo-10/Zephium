//! Notes written before notes were files: constrained ProseMirror JSON in the
//! profile database. Each becomes a Markdown file once, under its old id.
use std::collections::HashMap;

use zephium_core::resources::{DocumentNode, NoteDocument};

pub struct LegacyNote {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    pub trashed: bool,
    /// Seconds since the Unix epoch.
    pub updated_at: i64,
    pub document: NoteDocument,
}

/// Escapes text so Markdown reads it back as the same characters.
fn escape(text: &str, line_start: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_start = line_start;
    let chars: Vec<char> = text.chars().collect();
    for (index, &c) in chars.iter().enumerate() {
        let next = chars.get(index + 1).copied();
        let special = match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '~' | '|' => true,
            '#' | '>' if at_start => true,
            '-' | '+' | '=' if at_start => next.is_none_or(char::is_whitespace) || c == '=',
            '.' | ')'
                if index > 0 && chars[..index].iter().all(char::is_ascii_digit) && line_start =>
            {
                next.is_none_or(char::is_whitespace)
            }
            '!' => next == Some('['),
            '&' => true,
            _ => false,
        };
        if special {
            out.push('\\');
        }
        out.push(c);
        at_start = false;
    }
    out
}

fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

fn inline_code(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let ticks = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{text}{pad}{ticks}")
}

struct Converter<'a> {
    titles: &'a HashMap<String, String>,
}

impl Converter<'_> {
    fn inline(&self, nodes: &[DocumentNode]) -> String {
        let mut out = String::new();
        for node in nodes {
            match node.kind.as_str() {
                "text" => {
                    let text = node.text.as_deref().unwrap_or_default();
                    let marks: Vec<&str> = node.marks.iter().map(|m| m.kind.as_str()).collect();
                    if marks.contains(&"code") {
                        out.push_str(&inline_code(text));
                        continue;
                    }
                    let core = text.trim();
                    if core.is_empty() || marks.is_empty() {
                        out.push_str(&escape(text, out.is_empty() || out.ends_with('\n')));
                        continue;
                    }
                    let leading = &text[..text.len() - text.trim_start().len()];
                    let trailing = &text[text.trim_end().len()..];
                    let delimiter = match (marks.contains(&"bold"), marks.contains(&"italic")) {
                        (true, true) => "***",
                        (true, false) => "**",
                        _ => "*",
                    };
                    out.push_str(leading);
                    out.push_str(delimiter);
                    out.push_str(&escape(core, false));
                    out.push_str(delimiter);
                    out.push_str(trailing);
                }
                "hardBreak" => out.push_str("\\\n"),
                "noteReference" => {
                    let id = node
                        .attrs
                        .as_ref()
                        .and_then(|a| a.resource.as_deref())
                        .unwrap_or_default();
                    match self.titles.get(id) {
                        Some(title) => {
                            out.push_str("[[");
                            let title = title.replace(['[', ']', '|', '#', '^'], " ");
                            out.push_str(&title.split_whitespace().collect::<Vec<_>>().join(" "));
                            out.push_str("]]");
                        }
                        None => out.push_str("linked note"),
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn blocks(&self, nodes: &[DocumentNode]) -> String {
        nodes
            .iter()
            .map(|node| self.block(node))
            .filter(|block| !block.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn block(&self, node: &DocumentNode) -> String {
        match node.kind.as_str() {
            "paragraph" => self.inline(&node.content),
            "heading" => {
                let level = node
                    .attrs
                    .as_ref()
                    .and_then(|a| a.level)
                    .unwrap_or(1)
                    .clamp(1, 6);
                format!(
                    "{} {}",
                    "#".repeat(level as usize),
                    self.inline(&node.content)
                )
            }
            "codeBlock" => {
                let text: String = node
                    .content
                    .iter()
                    .filter_map(|n| n.text.as_deref())
                    .collect();
                let fence = fence_for(&text);
                format!("{fence}\n{text}\n{fence}")
            }
            "blockquote" => self
                .blocks(&node.content)
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "bulletList" | "orderedList" => {
                let start = node
                    .attrs
                    .as_ref()
                    .and_then(|a| a.start)
                    .unwrap_or(1)
                    .max(0);
                node.content
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let marker = if node.kind == "bulletList" {
                            "- ".to_string()
                        } else {
                            format!("{}. ", start as i64 + index as i64)
                        };
                        let indent = " ".repeat(marker.len());
                        // A nested list directly under its item's text keeps the list tight.
                        let mut body = String::new();
                        for child in &item.content {
                            let block = self.block(child);
                            if block.is_empty() {
                                continue;
                            }
                            if !body.is_empty() {
                                body.push_str(if child.kind.ends_with("List") {
                                    "\n"
                                } else {
                                    "\n\n"
                                });
                            }
                            body.push_str(&block);
                        }
                        let mut lines = body.lines();
                        let mut text = format!("{marker}{}", lines.next().unwrap_or_default());
                        for line in lines {
                            text.push('\n');
                            if !line.is_empty() {
                                text.push_str(&indent);
                                text.push_str(line);
                            }
                        }
                        text
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => String::new(),
        }
    }
}

/// The note as Markdown, opening with its title as a heading unless the
/// document already begins with that heading.
pub fn to_markdown(note: &LegacyNote, titles: &HashMap<String, String>) -> String {
    let converter = Converter { titles };
    let body = converter.blocks(&note.document.document.content);
    let first = note.document.document.content.first();
    let headed = first.is_some_and(|node| {
        node.kind == "heading"
            && converter.inline(&node.content).trim() == escape(note.title.trim(), false)
    });
    let title = note.title.trim();
    let mut markdown = if headed || title.is_empty() {
        body
    } else if body.is_empty() {
        format!("# {}", escape(title, false))
    } else {
        format!("# {}\n\n{body}", escape(title, false))
    };
    markdown.push('\n');
    markdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::resources::{DocumentAttrs, DocumentMark};

    fn node(kind: &str, content: Vec<DocumentNode>) -> DocumentNode {
        DocumentNode {
            kind: kind.into(),
            content,
            text: None,
            attrs: None,
            marks: Vec::new(),
        }
    }

    fn text(value: &str, marks: &[&str]) -> DocumentNode {
        DocumentNode {
            text: Some(value.into()),
            marks: marks
                .iter()
                .map(|m| DocumentMark {
                    kind: (*m).into(),
                    attrs: None,
                })
                .collect(),
            ..node("text", Vec::new())
        }
    }

    fn with(mut node: DocumentNode, attrs: DocumentAttrs) -> DocumentNode {
        node.attrs = Some(attrs);
        node
    }

    #[test]
    fn converts_every_legacy_node() {
        let reference = "01J9ZQ3V6Q4M8Y2K7T5R1N0B3A";
        let document = node(
            "doc",
            vec![
                node(
                    "paragraph",
                    vec![
                        text("Plain ", &[]),
                        text("bold", &["bold"]),
                        text(" and ", &[]),
                        text("both ", &["bold", "italic"]),
                        text("x`y", &["code"]),
                    ],
                ),
                with(
                    node("heading", vec![text("Part", &[])]),
                    DocumentAttrs {
                        level: Some(2),
                        ..Default::default()
                    },
                ),
                node(
                    "bulletList",
                    vec![node(
                        "listItem",
                        vec![
                            node("paragraph", vec![text("one", &[])]),
                            node(
                                "bulletList",
                                vec![node(
                                    "listItem",
                                    vec![node("paragraph", vec![text("nested", &[])])],
                                )],
                            ),
                        ],
                    )],
                ),
                with(
                    node(
                        "orderedList",
                        vec![node(
                            "listItem",
                            vec![node("paragraph", vec![text("three", &[])])],
                        )],
                    ),
                    DocumentAttrs {
                        start: Some(3),
                        ..Default::default()
                    },
                ),
                node(
                    "blockquote",
                    vec![node(
                        "paragraph",
                        vec![
                            text("quoted", &[]),
                            node("hardBreak", vec![]),
                            text("line", &[]),
                        ],
                    )],
                ),
                node("codeBlock", vec![text("let a = ```;", &[])]),
                node(
                    "paragraph",
                    vec![
                        text("See ", &[]),
                        with(
                            node("noteReference", vec![]),
                            DocumentAttrs {
                                resource: Some(reference.into()),
                                ..Default::default()
                            },
                        ),
                    ],
                ),
                node("paragraph", vec![text("1. not a list *really*", &[])]),
            ],
        );
        let note = LegacyNote {
            id: String::new(),
            title: "Notes".into(),
            pinned: false,
            trashed: false,
            updated_at: 0,
            document: NoteDocument {
                version: 1,
                document,
            },
        };
        let titles = HashMap::from([(reference.to_string(), "Other [draft]".to_string())]);
        assert_eq!(
            to_markdown(&note, &titles),
            "# Notes\n\nPlain **bold** and ***both*** ``x`y``\n\n## Part\n\n- one\n  - nested\n\n3. three\n\n> quoted\\\n> line\n\n````\nlet a = ```;\n````\n\nSee [[Other draft]]\n\n1\\. not a list \\*really\\*\n"
        );
    }

    #[test]
    fn an_existing_title_heading_is_not_repeated() {
        let document = node(
            "doc",
            vec![with(
                node("heading", vec![text("Notes", &[])]),
                DocumentAttrs {
                    level: Some(1),
                    ..Default::default()
                },
            )],
        );
        let note = LegacyNote {
            id: String::new(),
            title: "Notes".into(),
            pinned: false,
            trashed: false,
            updated_at: 0,
            document: NoteDocument {
                version: 1,
                document,
            },
        };
        assert_eq!(to_markdown(&note, &HashMap::new()), "# Notes\n");
    }
}
