//! Finding files and text inside a granted folder, with ripgrep's own
//! libraries: `.gitignore` and `.ignore` rules, hidden files skipped, binary
//! files and symlinks passed over. Every result is capped and says what it
//! left out, so the agent narrows instead of paging through a tree.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};
use ignore::{WalkBuilder, WalkState};

/// Directories never walked, gitignored or not.
const SKIPPED: [&str; 6] = [".git", "node_modules", "target", ".cache", "dist", "build"];
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_VISITED: usize = 60_000;
const BUDGET: Duration = Duration::from_millis(2500);
pub const MAX_GLOB_FILES: usize = 100;
pub const MAX_GREP_FILES: usize = 100;
pub const MAX_GREP_LINES: usize = 200;
/// One shown line, in characters; the rest of a minified line is noise.
const MAX_LINE_CHARS: usize = 240;
const MAX_OUTPUT_BYTES: usize = 12 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FindError {
    Pattern,
    NotADirectory,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GrepOutput {
    #[default]
    Files,
    Lines,
    Count,
}

pub struct GrepQuery<'a> {
    pub pattern: &'a str,
    pub glob: Option<&'a str>,
    pub output: GrepOutput,
    pub context: u8,
    pub case_insensitive: bool,
    pub limit: Option<usize>,
}

/// What a search found, already in the agent's compact form.
#[derive(Debug, Default)]
pub struct Found {
    pub text: String,
    /// Files (glob, files, count) or matching lines (lines).
    pub shown: usize,
    pub total: usize,
    /// The walk stopped early on its time or entry budget.
    pub partial: bool,
}

fn walker(root: &Path) -> WalkBuilder {
    let mut walk = WalkBuilder::new(root);
    walk.hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .same_file_system(true)
        .max_filesize(Some(MAX_FILE_BYTES))
        .threads(
            std::thread::available_parallelism()
                .map_or(4, |n| n.get())
                .min(8),
        )
        .filter_entry(|entry| {
            !(entry.file_type().is_some_and(|t| t.is_dir())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIPPED.contains(&name)))
        });
    walk
}

/// A glob without a slash matches at any depth, as people mean `*.rs`.
fn matcher(pattern: &str) -> Result<globset::GlobMatcher, FindError> {
    let pattern = pattern.trim().trim_start_matches("./");
    if pattern.is_empty() || pattern.len() > 256 || pattern.contains("..") {
        return Err(FindError::Pattern);
    }
    let full = if pattern.contains('/') {
        pattern.to_owned()
    } else {
        format!("**/{pattern}")
    };
    globset::GlobBuilder::new(&full)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|_| FindError::Pattern)
}

struct Budget {
    started: Instant,
    visited: AtomicUsize,
    stopped: AtomicBool,
}
impl Budget {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            visited: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
        }
    }
    /// Counts one entry; false once the walk ran out of time or entries.
    fn visit(&self) -> bool {
        if self.visited.fetch_add(1, Ordering::Relaxed) >= MAX_VISITED
            || self.started.elapsed() >= BUDGET
        {
            self.stopped.store(true, Ordering::Relaxed);
            return false;
        }
        true
    }
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn relative(root: &Path, path: &Path) -> String {
    shown_path(path.strip_prefix(root).unwrap_or(path))
}

pub(super) fn shown_path(path: &Path) -> String {
    let shown = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    let shown = shown.replace('\\', "/");
    shown
}

/// Newest first, then by path, so what was just changed leads.
fn by_recency(files: &mut [(PathBuf, SystemTime)]) {
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
}

/// Files under `root` whose path relative to it matches `pattern`.
pub fn glob(root: &Path, pattern: &str) -> Result<Found, FindError> {
    if !root.is_dir() {
        return Err(FindError::NotADirectory);
    }
    let matcher = matcher(pattern)?;
    let budget = Budget::new();
    let hits = Mutex::new(Vec::new());
    walker(root).build_parallel().run(|| {
        Box::new(|entry| {
            if !budget.visit() {
                return WalkState::Quit;
            }
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            if entry.file_type().is_some_and(|t| t.is_file())
                && matcher.is_match(entry.path().strip_prefix(root).unwrap_or(entry.path()))
            {
                let path = entry.into_path();
                let at = modified(&path);
                if let Ok(mut hits) = hits.lock() {
                    hits.push((path, at));
                }
            }
            WalkState::Continue
        })
    });
    let mut files = hits.into_inner().unwrap_or_default();
    by_recency(&mut files);
    let total = files.len();
    let mut text = String::new();
    for (path, _) in files.iter().take(MAX_GLOB_FILES) {
        text.push_str(&relative(root, path));
        text.push('\n');
    }
    Ok(Found {
        shown: total.min(MAX_GLOB_FILES),
        total,
        partial: budget.stopped(),
        text,
    })
}

#[derive(Default)]
struct FileHits {
    path: PathBuf,
    modified: Option<SystemTime>,
    /// (line number, is a match, text), in file order; `None` marks a gap.
    lines: Vec<Option<(u64, bool, String)>>,
    count: usize,
}

struct Collect<'a> {
    hits: &'a mut FileHits,
    keep_lines: bool,
    cap: usize,
}
impl Collect<'_> {
    fn push(&mut self, number: Option<u64>, matched: bool, bytes: &[u8]) {
        if !self.keep_lines {
            return;
        }
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim_end_matches(['\n', '\r']);
        let mut shown: String = text.chars().take(MAX_LINE_CHARS).collect();
        if shown.len() < text.len() {
            shown.push('…');
        }
        self.hits
            .lines
            .push(Some((number.unwrap_or(0), matched, shown)));
    }
}
impl Sink for Collect<'_> {
    type Error = std::io::Error;
    fn matched(&mut self, _: &Searcher, found: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.hits.count += 1;
        self.push(found.line_number(), true, found.bytes());
        Ok(self.hits.count < self.cap)
    }
    fn context(&mut self, _: &Searcher, found: &SinkContext<'_>) -> Result<bool, Self::Error> {
        self.push(found.line_number(), false, found.bytes());
        Ok(true)
    }
    fn context_break(&mut self, _: &Searcher) -> Result<bool, Self::Error> {
        if self.keep_lines {
            self.hits.lines.push(None);
        }
        Ok(true)
    }
    fn binary_data(&mut self, _: &Searcher, _: u64) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

/// Lines matching a regular expression under `root` (a folder or one file).
pub fn grep(root: &Path, query: &GrepQuery<'_>) -> Result<Found, FindError> {
    if query.pattern.is_empty() || query.pattern.len() > 512 {
        return Err(FindError::Pattern);
    }
    let matcher = RegexMatcherBuilder::new()
        .case_smart(!query.case_insensitive)
        .case_insensitive(query.case_insensitive)
        .size_limit(1024 * 1024)
        .dfa_size_limit(2 * 1024 * 1024)
        .build(query.pattern)
        .map_err(|_| FindError::Pattern)?;
    let filter = query.glob.map(matcher_for_filter).transpose()?;
    let lines = query.output == GrepOutput::Lines;
    let context = if lines {
        usize::from(query.context.min(10))
    } else {
        0
    };
    let base = if root.is_file() {
        root.parent().unwrap_or(root).to_path_buf()
    } else {
        root.to_path_buf()
    };
    let budget = Budget::new();
    let found = Mutex::new(Vec::<FileHits>::new());
    let files = AtomicUsize::new(0);
    walker(root).build_parallel().run(|| {
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .before_context(context)
            .after_context(context)
            .binary_detection(BinaryDetection::quit(0))
            .build();
        let matcher = matcher.clone();
        let filter = filter.clone();
        let (budget, found, files, base) = (&budget, &found, &files, &base);
        Box::new(move |entry| {
            if !budget.visit() {
                return WalkState::Quit;
            }
            let Ok(entry) = entry else {
                return WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return WalkState::Continue;
            }
            if filter.as_ref().is_some_and(|f| {
                !f.is_match(entry.path().strip_prefix(base).unwrap_or(entry.path()))
            }) {
                return WalkState::Continue;
            }
            let mut hits = FileHits {
                path: entry.path().to_path_buf(),
                ..Default::default()
            };
            let mut sink = Collect {
                hits: &mut hits,
                keep_lines: lines,
                cap: MAX_GREP_LINES + 1,
            };
            if searcher
                .search_path(&matcher, entry.path(), &mut sink)
                .is_err()
                || hits.count == 0
            {
                return WalkState::Continue;
            }
            hits.modified = Some(modified(&hits.path));
            if let Ok(mut found) = found.lock() {
                found.push(hits);
            }
            // Enough files to rank by recency and still say how many there were.
            if files.fetch_add(1, Ordering::Relaxed) >= 20 * MAX_GREP_FILES {
                budget.stopped.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
            WalkState::Continue
        })
    });
    let mut found = found.into_inner().unwrap_or_default();
    found.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(render(
        &base,
        found,
        query.output,
        query.limit,
        budget.stopped(),
    ))
}

fn matcher_for_filter(glob: &str) -> Result<globset::GlobMatcher, FindError> {
    matcher(glob)
}

fn render(
    base: &Path,
    found: Vec<FileHits>,
    output: GrepOutput,
    limit: Option<usize>,
    partial: bool,
) -> Found {
    let mut text = String::new();
    let mut shown = 0;
    let fits = |text: &String, next: &str| text.len() + next.len() <= MAX_OUTPUT_BYTES;
    match output {
        GrepOutput::Files | GrepOutput::Count => {
            let cap = limit.unwrap_or(MAX_GREP_FILES).clamp(1, MAX_GREP_FILES);
            for hits in found.iter().take(cap) {
                let path = relative(base, &hits.path);
                let line = if output == GrepOutput::Count {
                    let over = if hits.count > MAX_GREP_LINES { "+" } else { "" };
                    format!("{path}:{}{over}\n", hits.count.min(MAX_GREP_LINES))
                } else {
                    format!("{path}\n")
                };
                if !fits(&text, &line) {
                    break;
                }
                text.push_str(&line);
                shown += 1;
            }
            Found {
                text,
                shown,
                total: found.len(),
                partial,
            }
        }
        GrepOutput::Lines => {
            let cap = limit.unwrap_or(MAX_GREP_LINES).clamp(1, MAX_GREP_LINES);
            let total = found.iter().map(|hits| hits.count).sum();
            'files: for hits in &found {
                let path = relative(base, &hits.path);
                let mut block = String::new();
                for line in &hits.lines {
                    match line {
                        None => block.push_str("--\n"),
                        Some((number, matched, line)) => {
                            if *matched {
                                if shown == cap {
                                    break;
                                }
                                shown += 1;
                            }
                            let mark = if *matched { ':' } else { '-' };
                            block.push_str(&format!("{path}{mark}{number}{mark}{line}\n"));
                        }
                    }
                }
                let block = block.trim_end_matches("--\n");
                if !fits(&text, block) {
                    break 'files;
                }
                if !text.is_empty() && !block.is_empty() {
                    text.push_str("--\n");
                }
                text.push_str(block);
                if shown == cap {
                    break;
                }
            }
            Found {
                text,
                shown,
                total,
                partial,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(root.join(".gitignore"), "generated/\n*.log\n").unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn add(a: i32) -> i32 {\n    a + 1\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/deep/util.rs"),
            "// Add helper\nfn helper() {}\nfn add_twice() {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("node_modules/pkg/index.js"),
            "function add() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("generated/out.rs"), "fn add() {}\n").unwrap();
        std::fs::write(root.join("debug.log"), "add\n").unwrap();
        std::fs::write(root.join(".hidden.rs"), "fn add() {}\n").unwrap();
        std::fs::write(root.join("image.bin"), b"add\0\x01\x02").unwrap();
        dir
    }

    #[test]
    fn glob_respects_gitignore_and_sorts_by_recency() {
        let dir = tree();
        let root = dir.path();
        let found = glob(root, "*.rs").unwrap();
        let listed: Vec<&str> = found.text.lines().collect();
        assert_eq!(listed.len(), 2, "{listed:?}");
        assert!(listed.contains(&"src/lib.rs"));
        assert!(listed.contains(&"src/deep/util.rs"));
        let later = SystemTime::now() + Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(root.join("src/deep/util.rs"))
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(
            glob(root, "**/*.rs").unwrap().text.lines().next(),
            Some("src/deep/util.rs")
        );
        assert_eq!(glob(root, "src/*.rs").unwrap().text, "src/lib.rs\n");
        assert!(glob(root, "../*.rs").is_err());
        assert!(glob(root, "[").is_err());
    }

    #[test]
    fn grep_modes_context_and_caps() {
        let dir = tree();
        let root = dir.path();
        let query = |output, context| GrepQuery {
            pattern: "add",
            glob: None,
            output,
            context,
            case_insensitive: false,
            limit: None,
        };
        let files = grep(root, &query(GrepOutput::Files, 0)).unwrap();
        assert_eq!(files.total, 2, "{}", files.text);
        assert!(!files.text.contains("node_modules") && !files.text.contains("generated"));
        let counts = grep(root, &query(GrepOutput::Count, 0)).unwrap();
        assert!(counts.text.contains("src/lib.rs:1\n"));
        // Smart case: lowercase matches "Add" too.
        assert!(
            counts.text.contains("src/deep/util.rs:2\n"),
            "{}",
            counts.text
        );
        let lines = grep(root, &query(GrepOutput::Lines, 1)).unwrap();
        assert!(lines.text.contains("src/lib.rs:1:pub fn add"));
        assert!(lines.text.contains("src/lib.rs-2-    a + 1"));
        let sensitive = grep(
            root,
            &GrepQuery {
                pattern: "Add",
                ..query(GrepOutput::Files, 0)
            },
        )
        .unwrap();
        assert_eq!(sensitive.text, "src/deep/util.rs\n");
        let filtered = grep(
            root,
            &GrepQuery {
                glob: Some("src/deep/*.rs"),
                ..query(GrepOutput::Files, 0)
            },
        )
        .unwrap();
        assert_eq!(filtered.text, "src/deep/util.rs\n");
        let one = grep(&root.join("src/lib.rs"), &query(GrepOutput::Lines, 0)).unwrap();
        assert_eq!(one.text, "lib.rs:1:pub fn add(a: i32) -> i32 {\n");
        assert!(grep(
            root,
            &GrepQuery {
                pattern: "(",
                ..query(GrepOutput::Files, 0)
            }
        )
        .is_err());
    }

    #[test]
    fn grep_caps_lines_and_clips_long_ones() {
        let dir = tempfile::tempdir().unwrap();
        let body: String = (0..500).map(|n| format!("needle {n}\n")).collect();
        std::fs::write(dir.path().join("many.txt"), body).unwrap();
        std::fs::write(
            dir.path().join("wide.txt"),
            format!("needle {}\n", "x".repeat(5000)),
        )
        .unwrap();
        let found = grep(
            dir.path(),
            &GrepQuery {
                pattern: "needle",
                glob: None,
                output: GrepOutput::Lines,
                context: 0,
                case_insensitive: false,
                limit: None,
            },
        )
        .unwrap();
        assert_eq!(found.shown, MAX_GREP_LINES);
        assert!(found.total > MAX_GREP_LINES);
        assert!(found.text.len() <= MAX_OUTPUT_BYTES);
        assert!(found
            .text
            .lines()
            .all(|line| line.chars().count() < MAX_LINE_CHARS + 32));
    }
}
