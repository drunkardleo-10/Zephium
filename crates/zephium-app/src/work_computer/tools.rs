//! The six tools, their schemas and what each returns to the model.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use zephium_core::work::model::WorkModelTool;
use zephium_core::work::runtime::*;
use zephium_core::work::{WorkError, WorkStepId};

use super::delegate::Delegate;
use super::find::{self, FindError, GrepOutput, GrepQuery};
use super::shell::{self, TestSummary};
use super::{ComputerDiff, ComputerHost, Decision, Settled};
use crate::work_files::{WorkFileError, WorkFileGrant};

const READ_LINES: u32 = 400;
const MAX_READ_LINES: u32 = 2000;
const COMMAND_TIMEOUT: u32 = 120;
const MAX_COMMAND_TIMEOUT: u32 = 600;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// A tool's answer: compact text for the model and whether it failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolReply {
    pub content: String,
    pub is_error: bool,
}
impl ToolReply {
    fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }
    fn fault(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// The helper's tools, as the model sees them.
pub fn definitions() -> Vec<WorkModelTool> {
    let tool = |name: &str, description: &str, schema: Value| WorkModelTool {
        name: name.into(),
        description: description.into(),
        schema,
    };
    let path = json!({"type": "string", "description": "Relative to the working folder, or absolute inside a granted folder."});
    vec![
        tool(
            "glob",
            "Find files by name pattern, newest first: `**/*.rs`, `src/**/*.test.ts`, `Cargo.toml`. A pattern without `/` matches at any depth. Skips .gitignored, hidden and dependency folders. Returns at most 100 paths.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Folder to search; the working folder by default."}
            }, "required": ["pattern"], "additionalProperties": false}),
        ),
        tool(
            "grep",
            "Search file contents with a regular expression (Rust regex: `fn\\s+parse`, `TODO|FIXME`; escape `(){}[].` to match them literally). Smart case unless case_insensitive. output `files` (default) lists matching files newest first; `lines` shows `path:line:text` with `context` lines around each match; `count` gives matches per file. Narrow with `glob` or `path` rather than paging.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Folder or file; the working folder by default."},
                "glob": {"type": "string", "description": "Only files matching this glob, such as `*.ts`."},
                "output": {"type": "string", "enum": ["files", "lines", "count"]},
                "context": {"type": "integer", "minimum": 0, "maximum": 10},
                "case_insensitive": {"type": "boolean"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "At most this many files or lines."}
            }, "required": ["pattern"], "additionalProperties": false}),
        ),
        tool(
            "read",
            "Read a text file as numbered lines (`12: text`); the first 400 lines unless you give offset (1-based) and limit (≤ 2000). Read before you edit or overwrite a file. Read the part you need, not whole large files.",
            json!({"type": "object", "properties": {
                "path": path,
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 2000}
            }, "required": ["path"], "additionalProperties": false}),
        ),
        tool(
            "edit",
            "Replace an exact passage in a file you have read. `old` must match the file exactly, whitespace included, without the line-number prefix, and occur once; include surrounding lines to make it unique, or set replace_all to change every occurrence. Keep edits minimal. The person approves each change before it is written.",
            json!({"type": "object", "properties": {
                "path": path,
                "old": {"type": "string"},
                "new": {"type": "string"},
                "replace_all": {"type": "boolean"}
            }, "required": ["path", "old", "new"], "additionalProperties": false}),
        ),
        tool(
            "write",
            "Create a file, or replace a file you have read, with its complete text. Use edit to change part of an existing file. The person approves it before it is written.",
            json!({"type": "object", "properties": {
                "path": path,
                "text": {"type": "string"}
            }, "required": ["path", "text"], "additionalProperties": false}),
        ),
        tool(
            "bash",
            "Run a shell command in the working folder, or in `cwd` inside a granted folder: tests, builds, git and gh. Reading commands run at once; commands that change files ask the person once per folder; network, privileged or unusual commands ask each time. You get the exit status, duration, test counts and the lines that matter; the full output is kept with the step. Use glob, grep and read to look at files, not cat, find or grep.",
            json!({"type": "object", "properties": {
                "command": {"type": "string"},
                "cwd": {"type": "string"},
                "timeout": {"type": "integer", "minimum": 1, "maximum": 600, "description": "Seconds; 120 by default."}
            }, "required": ["command"], "additionalProperties": false}),
        ),
    ]
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolReply> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolReply::fault(format!("`{key}` is required and is a string.")))
}
fn optional_text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}
fn optional_number(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn file_fault(error: WorkFileError, path: &str) -> ToolReply {
    ToolReply::fault(match error {
        WorkFileError::Denied => format!("{path}: outside the granted folders."),
        WorkFileError::NotFound => format!("{path}: no such file or folder."),
        WorkFileError::Changed => {
            format!("{path} changed since you read it. Read it again before changing it.")
        }
        other => format!("{path}: {}.", other.note().to_lowercase()),
    })
}

fn group_digits(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, c) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn duration(ms: u32) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", f64::from(ms) / 1000.0)
    } else {
        format!("{} m {} s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// A change the person accepted, kept to build one diff per file.
struct Changed {
    before: String,
    after: String,
}

/// One part's tools over one working folder. Reads are remembered by digest,
/// so an edit applies only to the text the model saw.
pub struct ComputerTools {
    files: WorkFileGrant,
    root: PathBuf,
    reads: Mutex<HashMap<PathBuf, String>>,
    changed: Mutex<Vec<(PathBuf, Changed)>>,
    tests: Mutex<Option<TestSummary>>,
    last_output: Mutex<String>,
}

impl ComputerTools {
    /// `root` must lie inside the grant; it is where relative paths start.
    pub fn new(files: WorkFileGrant, root: &str) -> Result<Self, WorkFileError> {
        let root = files.resolve(root, false)?;
        if !root.is_dir() {
            return Err(WorkFileError::NotADirectory);
        }
        Ok(Self {
            files,
            root,
            reads: Mutex::new(HashMap::new()),
            changed: Mutex::new(Vec::new()),
            tests: Mutex::new(None),
            last_output: Mutex::new(String::new()),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// The folder's name, as the part shows it.
    pub fn folder_name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn absolute(&self, path: &str) -> String {
        let path = path.trim();
        if Path::new(path).is_absolute() {
            path.to_owned()
        } else {
            let relative = path.trim_start_matches("./");
            if relative.is_empty() || relative == "." {
                self.root.to_string_lossy().into_owned()
            } else {
                self.root.join(relative).to_string_lossy().into_owned()
            }
        }
    }
    /// How the model sees a path: relative to the working folder when inside it.
    fn shown(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".into(),
            Ok(relative) => relative.to_string_lossy().into_owned(),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    /// The bounded output of the last command.
    pub(super) fn last_output(&self) -> String {
        self.last_output
            .lock()
            .map(|o| o.clone())
            .unwrap_or_default()
    }

    /// Changes made outside the tools (a hand-off), relative to the folder.
    /// Their files must be read again before the model edits them.
    pub(super) fn adopt(&self, changes: &[(String, String, String)]) {
        let (Ok(mut changed), Ok(mut reads)) = (self.changed.lock(), self.reads.lock()) else {
            return;
        };
        for (path, before, after) in changes {
            let file = self.root.join(path);
            reads.remove(&file);
            changed.push((
                file,
                Changed {
                    before: before.clone(),
                    after: after.clone(),
                },
            ));
        }
    }

    /// The last test run's counts.
    pub fn tests(&self) -> Option<TestSummary> {
        self.tests.lock().ok()?.clone()
    }

    /// One diff per changed file, first version to last, with its summary
    /// from `summaries` (path → line) or a plain count.
    pub fn diffs(&self, summaries: &HashMap<String, String>) -> Vec<ComputerDiff> {
        let Ok(changed) = self.changed.lock() else {
            return Vec::new();
        };
        let mut order: Vec<&PathBuf> = Vec::new();
        let mut firsts: HashMap<&PathBuf, &str> = HashMap::new();
        let mut lasts: HashMap<&PathBuf, &str> = HashMap::new();
        for (path, change) in changed.iter() {
            if !firsts.contains_key(path) {
                order.push(path);
                firsts.insert(path, &change.before);
            }
            lasts.insert(path, &change.after);
        }
        order
            .into_iter()
            .filter_map(|path| {
                let (hunks, added, removed) = super::diff::hunks(firsts[path], lasts[path]);
                if hunks.is_empty() {
                    return None;
                }
                let shown = self.shown(path);
                let summary = summaries
                    .get(&shown)
                    .map(|s| s.chars().take(120).collect::<String>())
                    .filter(|s| !s.trim().is_empty() && !s.contains('\n'))
                    .unwrap_or_else(|| format!("+{added} −{removed}"));
                Some(ComputerDiff {
                    language: super::diff::language(&shown),
                    path: shown,
                    summary,
                    hunks,
                    added,
                    removed,
                })
            })
            .collect()
    }

    pub async fn call(&self, host: &dyn ComputerHost, name: &str, args: &Value) -> ToolReply {
        let reply = match name {
            "glob" => self.glob(host, args).await,
            "grep" => self.grep(host, args).await,
            "read" => self.read(host, args).await,
            "edit" => self.edit(host, args).await,
            "write" => self.write(host, args).await,
            "bash" => self.bash(host, args).await,
            _ => Err(ToolReply::fault(format!("There is no `{name}` tool."))),
        };
        match reply {
            Ok(reply) | Err(reply) => reply,
        }
    }

    async fn settle(
        &self,
        host: &dyn ComputerHost,
        step: WorkStepId,
        outcome: Result<WorkFileEvidenceV1, ToolReply>,
        note: impl FnOnce(&WorkFileEvidenceV1) -> String,
    ) -> Result<(WorkFileEvidenceV1, Option<String>), ToolReply> {
        let settled = match &outcome {
            Ok(file) => {
                let note = note(file);
                host.settle(
                    step,
                    WorkStepStatus::Succeeded,
                    Settled::File(Box::new(file.clone())),
                    Some(note),
                )
                .await
            }
            Err(fault) => {
                host.settle(
                    step,
                    WorkStepStatus::Failed,
                    Settled::Nothing,
                    Some(zephium_core::work::agent::clip_text(&fault.content, 200)),
                )
                .await
            }
        };
        let key = settled.map_err(stopped)?;
        outcome.map(|file| (file, key))
    }

    async fn glob(&self, host: &dyn ComputerHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let pattern = text(args, "pattern")?;
        if pattern.trim().is_empty() || pattern.len() > MAX_WORK_FILE_QUERY_BYTES {
            return Err(ToolReply::fault("`pattern` is 1 to 256 bytes."));
        }
        let path = self.absolute(optional_text(args, "path").unwrap_or("."));
        let step = host
            .begin(
                WorkStepKindV1::SearchFiles {
                    path: path.clone(),
                    query: pattern.to_owned(),
                    glob: Some(pattern.to_owned()),
                    regex: None,
                },
                None,
            )
            .await
            .map_err(stopped)?;
        let folder = self.files.resolve(&path, false);
        let root = folder.clone().unwrap_or_default();
        let outcome = match folder {
            Ok(folder) => {
                let owned = pattern.to_owned();
                let found = tokio::task::spawn_blocking(move || find::glob(&folder, &owned)).await;
                match found {
                    Ok(Ok(found)) => Ok(found),
                    Ok(Err(FindError::NotADirectory)) => {
                        Err(ToolReply::fault(format!("{path}: not a folder.")))
                    }
                    Ok(Err(FindError::Pattern)) => Err(ToolReply::fault(
                        "The pattern could not be used; use a glob such as **/*.rs.",
                    )),
                    Err(_) => Err(ToolReply::fault("The search stopped.")),
                }
            }
            Err(error) => Err(file_fault(error, &path)),
        };
        let evidence = outcome.map(|found| {
            let content = listing(
                &found.text,
                found.shown,
                found.total,
                found.partial,
                "file",
                "Narrow the pattern.",
            );
            (
                search_evidence(&root, found.total, &found.text, found.partial),
                content,
            )
        });
        let (evidence, content) = match evidence {
            Ok((evidence, content)) => (Ok(evidence), content),
            Err(fault) => (Err(fault.clone()), fault.content),
        };
        self.settle(host, step, evidence, |e| files_note(e.bytes))
            .await?;
        Ok(ToolReply::ok(content))
    }

    async fn grep(&self, host: &dyn ComputerHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let pattern = text(args, "pattern")?.to_owned();
        if pattern.is_empty() || pattern.len() > MAX_WORK_FILE_QUERY_BYTES {
            return Err(ToolReply::fault("`pattern` is 1 to 256 bytes."));
        }
        let path = self.absolute(optional_text(args, "path").unwrap_or("."));
        let glob = optional_text(args, "glob").map(str::to_owned);
        let output = match optional_text(args, "output") {
            None | Some("files") => GrepOutput::Files,
            Some("lines") => GrepOutput::Lines,
            Some("count") => GrepOutput::Count,
            Some(_) => return Err(ToolReply::fault("`output` is files, lines or count.")),
        };
        let context = optional_number(args, "context").unwrap_or(0).min(10) as u8;
        let case_insensitive = args
            .get("case_insensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let limit = optional_number(args, "limit").map(|n| n.clamp(1, 200) as usize);
        let step = host
            .begin(
                WorkStepKindV1::SearchFiles {
                    path: path.clone(),
                    query: pattern.clone(),
                    glob: glob.clone(),
                    regex: Some(true),
                },
                None,
            )
            .await
            .map_err(stopped)?;
        let target = self.files.resolve(&path, false);
        let outcome = match target {
            Ok(target) => {
                let target_for_walk = target.clone();
                let found = tokio::task::spawn_blocking(move || {
                    find::grep(
                        &target_for_walk,
                        &GrepQuery {
                            pattern: &pattern,
                            glob: glob.as_deref(),
                            output,
                            context,
                            case_insensitive,
                            limit,
                        },
                    )
                })
                .await;
                match found {
                    Ok(Ok(found)) => Ok((target, found)),
                    Ok(Err(_)) => Err(ToolReply::fault(
                        "The pattern could not be used. Check the regular expression and escape literal ( ) [ ] { }.",
                    )),
                    Err(_) => Err(ToolReply::fault("The search stopped.")),
                }
            }
            Err(error) => Err(file_fault(error, &path)),
        };
        let (evidence, content) = match outcome {
            Ok((target, found)) => {
                let content = if found.total == 0 {
                    "No matches.".to_owned()
                } else if output == GrepOutput::Lines {
                    let mut content = found.text.clone();
                    if found.shown < found.total {
                        content.push_str(&format!(
                            "\n({} of {} matching lines; narrow with glob or path.)",
                            found.shown,
                            group_digits(found.total)
                        ));
                    }
                    if found.partial {
                        content
                            .push_str("\n(The search stopped early; results may be incomplete.)");
                    }
                    content
                } else {
                    listing(
                        &found.text,
                        found.shown,
                        found.total,
                        found.partial,
                        "file",
                        "Narrow with glob or path.",
                    )
                };
                (
                    Ok(search_evidence(
                        &target,
                        found.total,
                        &found.text,
                        found.partial,
                    )),
                    content,
                )
            }
            Err(fault) => (Err(fault.clone()), fault.content),
        };
        self.settle(host, step, evidence, |e| match output {
            GrepOutput::Lines => format!("{} matches", e.bytes),
            _ => files_note(e.bytes),
        })
        .await?;
        Ok(ToolReply::ok(content))
    }

    async fn read(&self, host: &dyn ComputerHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let path = self.absolute(text(args, "path")?);
        let offset = optional_number(args, "offset").unwrap_or(1).max(1) as u32;
        let limit = optional_number(args, "limit")
            .map_or(READ_LINES, |n| n.clamp(1, u64::from(MAX_READ_LINES)) as u32);
        let step = host
            .begin(
                WorkStepKindV1::ReadFile {
                    path: path.clone(),
                    offset: Some(offset),
                    limit: Some(limit),
                },
                None,
            )
            .await
            .map_err(stopped)?;
        let outcome = self
            .files
            .read_at(&path, offset, limit)
            .map_err(|error| file_fault(error, &path))
            .and_then(|file| match file.kind {
                WorkFileKindV1::Binary => Err(ToolReply::fault(format!(
                    "{path}: not a text file ({} bytes).",
                    group_digits(file.bytes as usize)
                ))),
                _ => Ok(file),
            });
        let (file, key) = self
            .settle(host, step, outcome, |file| match &file.lines {
                Some(lines) if lines.total > 0 => format!("{} lines", lines.total),
                _ => "Empty".into(),
            })
            .await?;
        if let Ok(resolved) = self.files.resolve(&path, false) {
            if let Ok(mut reads) = self.reads.lock() {
                reads.insert(resolved, file.digest.clone());
            }
        }
        let mut content = if file.text.is_empty() {
            "(empty file)".to_owned()
        } else {
            file.text.clone()
        };
        if let Some(lines) = file.lines {
            if lines.total > 0 && (lines.first > 1 || lines.last < lines.total) {
                content.push_str(&format!(
                    "\n(lines {}–{} of {}",
                    lines.first,
                    lines.last,
                    group_digits(lines.total as usize)
                ));
                if lines.last < lines.total {
                    content.push_str(&format!("; continue with offset {}", lines.last + 1));
                }
                content.push(')');
            }
        }
        if let Some(key) = key {
            content.push_str(&format!("\n(source {key})"));
        }
        Ok(ToolReply::ok(content))
    }

    /// The digest the model saw, required before any change to an existing file.
    fn seen(&self, file: &Path) -> Option<String> {
        self.reads.lock().ok()?.get(file).cloned()
    }

    fn current_text(file: &Path) -> Result<Option<String>, WorkFileError> {
        match std::fs::metadata(file) {
            Err(_) => Ok(None),
            Ok(meta) if !meta.is_file() => Err(WorkFileError::NotAFile),
            Ok(meta) if meta.len() > MAX_FILE_BYTES => Err(WorkFileError::TooLarge),
            Ok(_) => std::fs::read_to_string(file)
                .map(Some)
                .map_err(|_| WorkFileError::Binary),
        }
    }

    async fn edit(&self, host: &dyn ComputerHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let path = self.absolute(text(args, "path")?);
        let old = text(args, "old")?;
        let new = text(args, "new")?;
        let all = args
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if old.is_empty() {
            return Err(ToolReply::fault(
                "`old` is empty. To create a file, use write.",
            ));
        }
        if old == new {
            return Err(ToolReply::fault("`old` and `new` are the same."));
        }
        let file = self
            .files
            .resolve(&path, false)
            .map_err(|e| file_fault(e, &path))?;
        let shown = self.shown(&file);
        let known = self
            .seen(&file)
            .ok_or_else(|| ToolReply::fault(format!("Read {shown} before editing it.")))?;
        let current = Self::current_text(&file)
            .map_err(|e| file_fault(e, &shown))?
            .ok_or_else(|| file_fault(WorkFileError::NotFound, &shown))?;
        let occurrences = current.matches(old).count();
        let kind = match (occurrences, all) {
            (0, _) => {
                return Err(ToolReply::fault(format!(
                    "`old` was not found in {shown}. Match the file exactly, including indentation, without line numbers."
                )))
            }
            (1, _) => WorkStepKindV1::EditFile {
                path: path.clone(),
                old: old.to_owned(),
                new: new.to_owned(),
                replacements: vec![],
                decision: None,
            },
            (n, false) => {
                return Err(ToolReply::fault(format!(
                    "`old` occurs {n} times in {shown}. Include more surrounding lines to make it unique, or set replace_all."
                )))
            }
            (_, true) => WorkStepKindV1::WriteFile {
                path: path.clone(),
                content: current.replace(old, new),
                decision: None,
            },
        };
        self.propose(host, kind, &file, Some(&known), current).await
    }

    async fn write(&self, host: &dyn ComputerHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let path = self.absolute(text(args, "path")?);
        let content = text(args, "text")?;
        let file = self
            .files
            .resolve(&path, true)
            .map_err(|e| file_fault(e, &path))?;
        let shown = self.shown(&file);
        let current = Self::current_text(&file).map_err(|e| file_fault(e, &shown))?;
        let known = match &current {
            Some(_) => Some(self.seen(&file).ok_or_else(|| {
                ToolReply::fault(format!("{shown} exists. Read it before replacing it."))
            })?),
            None => None,
        };
        if current.as_deref() == Some(content) {
            return Ok(ToolReply::ok(format!("{shown} already has this text.")));
        }
        let kind = WorkStepKindV1::WriteFile {
            path: path.clone(),
            content: content.to_owned(),
            decision: None,
        };
        self.propose(
            host,
            kind,
            &file,
            known.as_deref(),
            current.unwrap_or_default(),
        )
        .await
    }

    /// Proposes a change through the grant, waits for the person and applies it.
    async fn propose(
        &self,
        host: &dyn ComputerHost,
        kind: WorkStepKindV1,
        file: &Path,
        known: Option<&str>,
        before: String,
    ) -> Result<ToolReply, ToolReply> {
        let shown = self.shown(file);
        let change = self
            .files
            .prepare(&kind, known)
            .map_err(|e| file_fault(e, &shown))?;
        let after = match &kind {
            WorkStepKindV1::WriteFile { content, .. } => content.clone(),
            WorkStepKindV1::EditFile { old, new, .. } => before.replacen(old.as_str(), new, 1),
            _ => return Err(ToolReply::fault("This change is not supported.")),
        };
        let step = host
            .begin(kind, Some(change.fact()))
            .await
            .map_err(stopped)?;
        match host.decision(step).await.map_err(stopped)? {
            Decision::Approved => {}
            Decision::Declined => {
                host.settle(
                    step,
                    WorkStepStatus::Failed,
                    Settled::Nothing,
                    Some("Declined by the person".into()),
                )
                .await
                .map_err(stopped)?;
                return Ok(ToolReply::fault(format!(
                    "The person declined the change to {shown}. Don't propose it again unchanged; ask or take another approach."
                )));
            }
            Decision::Stopped => {
                let _ = host
                    .settle(
                        step,
                        WorkStepStatus::Cancelled,
                        Settled::Nothing,
                        Some("Stopped".into()),
                    )
                    .await;
                return Err(ToolReply::fault("The run stopped."));
            }
        }
        let applied = self.files.apply(change);
        let (record, _) = self
            .settle(
                host,
                step,
                applied.map_err(|e| file_fault(e, &shown)),
                |_| "Applied".into(),
            )
            .await?;
        if let Ok(mut reads) = self.reads.lock() {
            reads.insert(file.to_path_buf(), record.digest.clone());
        }
        let (_, added, removed) = super::diff::hunks(&before, &after);
        if let Ok(mut changed) = self.changed.lock() {
            changed.push((file.to_path_buf(), Changed { before, after }));
        }
        Ok(ToolReply::ok(format!(
            "Changed {shown} (+{added} −{removed})."
        )))
    }

    async fn bash(&self, host: &dyn ComputerHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let command = text(args, "command")?.trim().to_owned();
        if command.is_empty()
            || command.len() > MAX_WORK_COMMAND_BYTES
            || command.chars().any(|c| c.is_control() && c != '\t')
        {
            return Err(ToolReply::fault(
                "`command` is one line of at most 4096 bytes; join steps with && instead of newlines.",
            ));
        }
        let cwd = self.absolute(optional_text(args, "cwd").unwrap_or("."));
        let timeout = optional_number(args, "timeout").map_or(COMMAND_TIMEOUT, |t| {
            t.clamp(1, u64::from(MAX_COMMAND_TIMEOUT)) as u32
        });
        let directory = match self.files.resolve(&cwd, false) {
            Ok(path) if path.is_dir() => path,
            _ => {
                return Err(ToolReply::fault(format!(
                    "{cwd} is not a folder inside the granted folders."
                )))
            }
        };
        let root = self
            .files
            .roots()
            .iter()
            .filter(|r| directory.starts_with(r))
            .max_by_key(|r| r.components().count())
            .map(|r| r.to_string_lossy().into_owned())
            .ok_or_else(|| ToolReply::fault("The folder is not granted."))?;
        let (class, reason) =
            crate::work_commands::policy::classify(&command, &directory, self.files.roots());
        let scope = match class {
            WorkCommandClassV1::Read => WorkCommandApprovalScopeV1::None,
            WorkCommandClassV1::Write if host.folder_approved(&root).await => {
                WorkCommandApprovalScopeV1::None
            }
            WorkCommandClassV1::Write => WorkCommandApprovalScopeV1::Folder,
            WorkCommandClassV1::Ask => WorkCommandApprovalScopeV1::Command,
        };
        let step = host
            .begin(
                WorkStepKindV1::RunCommand {
                    cwd: directory.to_string_lossy().into_owned(),
                    command: command.clone(),
                    timeout_secs: Some(timeout),
                    decision: None,
                },
                Some(WorkLocalStepV1 {
                    policy: Some(WorkCommandPolicyV1 {
                        class,
                        reason,
                        scope,
                        root,
                    }),
                    ..Default::default()
                }),
            )
            .await
            .map_err(stopped)?;
        if scope != WorkCommandApprovalScopeV1::None {
            match host.decision(step).await.map_err(stopped)? {
                Decision::Approved => {}
                Decision::Declined => {
                    host.settle(
                        step,
                        WorkStepStatus::Failed,
                        Settled::Nothing,
                        Some("Declined".into()),
                    )
                    .await
                    .map_err(stopped)?;
                    return Ok(ToolReply::fault(
                        "The person declined this command. Don't run it again unchanged.",
                    ));
                }
                Decision::Stopped => {
                    let _ = host
                        .settle(
                            step,
                            WorkStepStatus::Cancelled,
                            Settled::Nothing,
                            Some("Stopped".into()),
                        )
                        .await;
                    return Err(ToolReply::fault("The run stopped."));
                }
            }
        }
        // An approval wait can outlive a symlink swap; the folder must still be the one approved.
        if self
            .files
            .resolve(&directory.to_string_lossy(), false)
            .ok()
            .as_ref()
            != Some(&directory)
        {
            host.settle(
                step,
                WorkStepStatus::Failed,
                Settled::Nothing,
                Some("The command folder changed".into()),
            )
            .await
            .map_err(stopped)?;
            return Err(ToolReply::fault("The command folder changed."));
        }
        let result = crate::work_commands::run(
            &directory,
            &command,
            timeout,
            || async {
                tokio::time::timeout(Duration::from_millis(100), host.cancelled())
                    .await
                    .unwrap_or(false)
            },
            |output| async move {
                let _ =
                    tokio::time::timeout(Duration::from_millis(100), host.progress(step, output))
                        .await;
            },
        )
        .await;
        let result = match result {
            Ok(result) => result,
            Err(note) => {
                host.settle(
                    step,
                    WorkStepStatus::Failed,
                    Settled::Nothing,
                    Some(note.into()),
                )
                .await
                .map_err(stopped)?;
                return Err(ToolReply::fault(format!("{note}.")));
            }
        };
        let evidence = result.evidence;
        if let Ok(mut last) = self.last_output.lock() {
            last.clone_from(&evidence.text);
        }
        let tests = shell::tests(&evidence.text);
        if let (Some(tests), Ok(mut last)) = (&tests, self.tests.lock()) {
            *last = Some(tests.clone());
        }
        let mut head = match (evidence.exit, evidence.signal) {
            (Some(code), _) => format!("exit {code}"),
            (None, Some(signal)) => format!("signal {signal}"),
            _ => "no exit status".to_owned(),
        };
        head.push_str(&format!(" · {}", duration(evidence.elapsed_ms)));
        if result.note.starts_with("Stopped") {
            head.push_str(&format!(" · {}", result.note.to_lowercase()));
        }
        let lines = evidence.text.lines().count();
        let mut content = head;
        if let Some(tests) = &tests {
            content.push_str(&format!("\ntests: {}", tests.line()));
        }
        let excerpt = shell::excerpt(&evidence.text);
        if !excerpt.is_empty() {
            content.push('\n');
            content.push_str(&excerpt);
        }
        if lines > 40 || evidence.truncated {
            content.push_str(&format!(
                "\n({} lines of output kept with the step{})",
                group_digits(lines),
                if evidence.truncated {
                    "; the middle was cut"
                } else {
                    ""
                }
            ));
        }
        let note = match &tests {
            Some(tests) if tests.failed > 0 => format!("{} failed", tests.failed),
            Some(tests) => format!("{} passed", tests.passed),
            None => result.note.clone(),
        };
        let key = host
            .settle(
                step,
                if result.succeeded {
                    WorkStepStatus::Succeeded
                } else {
                    WorkStepStatus::Failed
                },
                Settled::Command(Box::new(evidence)),
                Some(note),
            )
            .await
            .map_err(stopped)?;
        if let Some(key) = key {
            content.push_str(&format!("\n(source {key})"));
        }
        Ok(ToolReply {
            content,
            is_error: false,
        })
    }
}

/// The `delegate` tool for the coding agents found on this Mac.
pub(super) fn delegate_tool(delegates: &[Delegate]) -> WorkModelTool {
    let names: Vec<&str> = delegates.iter().map(|d| d.program()).collect();
    WorkModelTool {
        name: "delegate".into(),
        description: "Hand a larger coding job to a coding agent installed on this Mac (codex: OpenAI Codex, which writes only inside the folder; claude: Claude Code, which edits files but runs no commands). It works in the folder by itself for up to ten minutes; you get its summary and the files it changed. The person approves the hand-off first. Use it for multi-file work, not small fixes, and test its work afterwards.".into(),
        schema: json!({"type": "object", "properties": {
            "agent": {"type": "string", "enum": names},
            "task": {"type": "string", "description": "A complete, self-contained brief in one paragraph of at most 3000 characters: the goal, the files involved, constraints and how to verify."}
        }, "required": ["agent", "task"], "additionalProperties": false}),
    }
}

/// Runs a hand-off as one approved command, then reads what changed from git.
pub(super) async fn hand_off(
    host: &dyn ComputerHost,
    tools: &ComputerTools,
    delegates: &[Delegate],
    arguments: &Value,
) -> ToolReply {
    let Some(agent) = arguments["agent"]
        .as_str()
        .and_then(Delegate::parse)
        .filter(|agent| delegates.contains(agent))
    else {
        return ToolReply::fault("That coding agent is not available here.");
    };
    let task: String = arguments["task"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if task.is_empty() || task.len() > 3000 {
        return ToolReply::fault("`task` is one paragraph of 1 to 3000 characters.");
    }
    let folder = tools.root().to_path_buf();
    let snapshot = {
        let folder = folder.clone();
        tokio::task::spawn_blocking(move || super::delegate::Snapshot::take(&folder))
            .await
            .unwrap_or_default()
    };
    let command = super::delegate::command(agent, &folder, &task);
    let reply = tools
        .call(
            host,
            "bash",
            &json!({"command": command, "timeout": MAX_COMMAND_TIMEOUT}),
        )
        .await;
    if reply.is_error {
        return reply;
    }
    let changes = tokio::task::spawn_blocking(move || snapshot.changes(&folder))
        .await
        .unwrap_or_default();
    tools.adopt(&changes);
    let said = super::delegate::final_message(agent, &tools.last_output())
        .unwrap_or_else(|| format!("{} finished without a summary.", agent.name()));
    let mut content = format!("{} said: {said}", agent.name());
    if changes.is_empty() {
        content.push_str("\nNo files changed.");
    } else {
        content.push_str("\nChanged:");
        for (path, before, after) in &changes {
            let (_, added, removed) = super::diff::hunks(before, after);
            content.push_str(&format!("\n{path} (+{added} −{removed})"));
        }
        content.push_str("\nReview with git diff, then run the tests.");
    }
    ToolReply::ok(content)
}

fn stopped(_: WorkError) -> ToolReply {
    ToolReply::fault("The run could not record this step.")
}

fn files_note(count: u32) -> String {
    match count {
        0 => "No files".into(),
        1 => "1 file".into(),
        n => format!("{n} files"),
    }
}

fn listing(
    text: &str,
    shown: usize,
    total: usize,
    partial: bool,
    noun: &str,
    narrow: &str,
) -> String {
    if total == 0 {
        return if partial {
            format!("No {noun}s found before the search stopped.")
        } else {
            format!("No {noun}s found.")
        };
    }
    let mut content = text.trim_end().to_owned();
    if shown < total {
        content.push_str(&format!(
            "\n({shown} of {} {noun}s. {narrow})",
            group_digits(total)
        ));
    }
    if partial {
        content.push_str("\n(The search stopped early; there may be more.)");
    }
    content
}

fn search_evidence(root: &Path, total: usize, text: &str, partial: bool) -> WorkFileEvidenceV1 {
    let (text, cut) = if text.len() > MAX_WORK_FILE_TEXT_BYTES {
        let mut end = MAX_WORK_FILE_TEXT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        (text[..end].to_owned(), true)
    } else {
        (text.to_owned(), false)
    };
    WorkFileEvidenceV1 {
        path: root.to_string_lossy().into_owned(),
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".into()),
        kind: WorkFileKindV1::Search,
        bytes: total.min(u32::MAX as usize) as u32,
        digest: String::new(),
        text,
        truncated: cut || partial,
        before_digest: None,
        after_digest: None,
        lines: None,
    }
}
