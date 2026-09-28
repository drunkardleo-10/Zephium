//! A file's change as review hunks: three lines of context, within the
//! object's limits, and the language its extension names.
use similar::{ChangeTag, TextDiff};
use zephium_core::work::objects::{WorkDiffHunkV1, WorkDiffLineV1, WorkDiffOpV1};

const CONTEXT: usize = 3;
const MAX_HUNKS: usize = 40;
const MAX_HUNK_LINES: usize = 400;
const MAX_LINE_CHARS: usize = 500;

/// The object's language for a path, `text` when none fits.
pub fn language(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.eq_ignore_ascii_case("dockerfile") {
        return "dockerfile";
    }
    if name.eq_ignore_ascii_case("makefile") {
        return "bash";
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "svelte" => "svelte",
        "py" | "pyi" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "mm" | "m" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "sql" => "sql",
        "html" | "htm" | "vue" => "html",
        "css" | "scss" | "sass" | "less" => "css",
        "json" | "jsonc" => "json",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "md" | "mdx" | "markdown" => "markdown",
        _ => "text",
    }
}

fn clean(line: &str) -> String {
    let line = line.trim_end_matches(['\n', '\r']);
    line.chars()
        .take(MAX_LINE_CHARS)
        .map(|c| if c.is_control() && c != '\t' { ' ' } else { c })
        .collect()
}

/// Hunks from `before` to `after`, and the added and removed line counts.
pub fn hunks(before: &str, after: &str) -> (Vec<WorkDiffHunkV1>, u32, u32) {
    let diff = TextDiff::from_lines(before, after);
    let (mut added, mut removed) = (0u32, 0u32);
    let mut hunks = Vec::new();
    for group in diff.grouped_ops(CONTEXT) {
        if hunks.len() == MAX_HUNKS {
            break;
        }
        let Some(first) = group.first() else {
            continue;
        };
        let mut hunk = WorkDiffHunkV1 {
            old_start: first.old_range().start as u32 + 1,
            new_start: first.new_range().start as u32 + 1,
            lines: Vec::new(),
        };
        for op in &group {
            for change in diff.iter_changes(op) {
                let op = match change.tag() {
                    ChangeTag::Equal => WorkDiffOpV1::Ctx,
                    ChangeTag::Insert => {
                        added += 1;
                        WorkDiffOpV1::Add
                    }
                    ChangeTag::Delete => {
                        removed += 1;
                        WorkDiffOpV1::Del
                    }
                };
                if hunk.lines.len() < MAX_HUNK_LINES {
                    hunk.lines.push(WorkDiffLineV1 {
                        op,
                        text: clean(change.value()),
                    });
                }
            }
        }
        if !hunk.lines.is_empty() {
            hunks.push(hunk);
        }
    }
    (hunks, added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_carry_context_and_counts() {
        let before: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let after = before
            .replace("line 5\n", "line five\n")
            .replace("line 18\n", "");
        let (hunks, added, removed) = hunks(&before, &after);
        assert_eq!((added, removed), (1, 2));
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].old_start, 2);
        assert_eq!(hunks[0].lines.len(), 8);
        assert_eq!(hunks[0].lines[3].op, WorkDiffOpV1::Del);
        assert_eq!(hunks[0].lines[4].text, "line five");
        let (created, added, _) = super::hunks("", "one\ntwo\n");
        assert_eq!((created.len(), added), (1, 2));
        assert_eq!(created[0].old_start, 1);
    }

    #[test]
    fn languages() {
        assert_eq!(language("src/lib.rs"), "rust");
        assert_eq!(language("App.TSX"), "typescript");
        assert_eq!(language("docker/Dockerfile"), "dockerfile");
        assert_eq!(language("notes"), "text");
        assert_eq!(language("README.md"), "markdown");
    }
}
