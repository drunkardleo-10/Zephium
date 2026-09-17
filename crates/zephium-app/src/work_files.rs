//! Bounded file steps inside folders the person granted for one run.
//! Every path resolves through a granted root; nothing outside is touched,
//! and what the agent sees is capped to a short excerpt, listing or diff.
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use zephium_core::work::runtime::*;

const MAX_READ_BYTES: u64 = 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_SEARCH_FILES: usize = 2000;
const MAX_SEARCH_HITS: usize = 64;
const SEARCH_BUDGET: Duration = Duration::from_millis(200);
const SKIPPED_DIRS: [&str; 6] = [".git", "node_modules", "target", ".cache", "dist", "build"];
const DENIED_UNDER_HOME: [&str; 7] = [
    ".ssh",
    ".gnupg",
    ".aws",
    ".config/gcloud",
    "Library/Keychains",
    "Library/Application Support/app.zephium",
    "Library/Cookies",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkFileError {
    Denied,
    NotFound,
    NotADirectory,
    NotAFile,
    TooLarge,
    Binary,
    Ambiguous,
    Io,
}
impl WorkFileError {
    /// Closed wording for the step note and the agent.
    pub fn note(self) -> &'static str {
        match self {
            Self::Denied => "Outside the granted folders",
            Self::NotFound => "No such file or folder",
            Self::NotADirectory => "Not a folder",
            Self::NotAFile => "Not a file",
            Self::TooLarge => "Larger than the read limit",
            Self::Binary => "Not a text file",
            Self::Ambiguous => "The passage to replace is missing or not unique",
            Self::Io => "The file could not be accessed",
        }
    }
}

/// Folders admitted for one run, canonical and policy-checked.
#[derive(Clone, Debug, Default)]
pub struct WorkFileGrant {
    /// Canonical roots, and the roots as the person wrote them.
    roots: Vec<PathBuf>,
    written: Vec<PathBuf>,
}
impl WorkFileGrant {
    /// Admits each folder that is an existing directory under the home
    /// folder and outside the denylist; refused ones come back by name.
    pub fn admit(folders: &[String]) -> (Self, Vec<String>) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut roots = Vec::new();
        let mut written = Vec::new();
        let mut refused = Vec::new();
        for folder in folders {
            match admit_root(folder, home.as_deref()) {
                Some(root) => {
                    if !roots.contains(&root) {
                        roots.push(root);
                    }
                    written.push(PathBuf::from(folder));
                }
                None => refused.push(folder.clone()),
            }
        }
        (Self { roots, written }, refused)
    }
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
    /// Canonical admitted roots, in grant order.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
    /// The canonical target when the path (or, for a new file, its parent)
    /// lies inside a granted root.
    fn resolve(&self, path: &str, may_create: bool) -> Result<PathBuf, WorkFileError> {
        validate_file_path(path).map_err(|_| WorkFileError::Denied)?;
        let candidate = Path::new(path);
        if candidate
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(WorkFileError::Denied);
        }
        // Refuse by the written path first, so nothing outside a root is
        // even probed; the canonical path is checked again for symlinks.
        if !self
            .roots
            .iter()
            .chain(&self.written)
            .any(|root| candidate.starts_with(root))
        {
            return Err(WorkFileError::Denied);
        }
        let resolved = match std::fs::canonicalize(candidate) {
            Ok(resolved) => resolved,
            Err(_) if may_create => {
                let parent = candidate.parent().ok_or(WorkFileError::Denied)?;
                let name = candidate.file_name().ok_or(WorkFileError::Denied)?;
                std::fs::canonicalize(parent)
                    .map_err(|_| WorkFileError::NotFound)?
                    .join(name)
            }
            Err(_) => return Err(WorkFileError::NotFound),
        };
        if self.roots.iter().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(WorkFileError::Denied)
        }
    }
    pub fn list(&self, path: &str) -> Result<WorkFileEvidenceV1, WorkFileError> {
        let dir = self.resolve(path, false)?;
        if !dir.is_dir() {
            return Err(WorkFileError::NotADirectory);
        }
        let mut entries: Vec<(bool, String, u64)> = std::fs::read_dir(&dir)
            .map_err(|_| WorkFileError::Io)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    return None;
                }
                let meta = entry.metadata().ok()?;
                Some((meta.is_dir(), name, meta.len()))
            })
            .collect();
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let total = entries.len();
        let mut text = String::new();
        for (is_dir, name, len) in entries.into_iter().take(MAX_LIST_ENTRIES) {
            if is_dir {
                text.push_str(&format!("{name}/\n"));
            } else {
                text.push_str(&format!("{name}\t{len}\n"));
            }
        }
        let (text, cut) = clip(text);
        Ok(WorkFileEvidenceV1 {
            path: dir.to_string_lossy().into_owned(),
            name: file_name(&dir),
            kind: WorkFileKindV1::Directory,
            bytes: u32::try_from(total).unwrap_or(u32::MAX),
            digest: String::new(),
            text,
            truncated: cut || total > MAX_LIST_ENTRIES,
        })
    }
    pub fn read(&self, path: &str) -> Result<WorkFileEvidenceV1, WorkFileError> {
        let file = self.resolve(path, false)?;
        let (bytes, digest, text) = read_text(&file)?;
        let (kind, text, cut) = match text {
            Some(text) => {
                let (text, cut) = clip(text);
                (WorkFileKindV1::Text, text, cut)
            }
            None => (WorkFileKindV1::Binary, String::new(), false),
        };
        Ok(WorkFileEvidenceV1 {
            path: file.to_string_lossy().into_owned(),
            name: file_name(&file),
            kind,
            bytes,
            digest,
            text,
            truncated: cut,
        })
    }
    /// Case-insensitive literal search over text files below the path.
    pub fn search(&self, path: &str, query: &str) -> Result<WorkFileEvidenceV1, WorkFileError> {
        let root = self.resolve(path, false)?;
        if !root.is_dir() {
            return Err(WorkFileError::NotADirectory);
        }
        let needle = query.to_lowercase();
        if needle.trim().is_empty() {
            return Err(WorkFileError::Ambiguous);
        }
        let started = Instant::now();
        let mut pending = vec![root.clone()];
        let mut visited = 0usize;
        let mut hits = Vec::new();
        let mut cut = false;
        'walk: while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                if started.elapsed() > SEARCH_BUDGET || visited >= MAX_SEARCH_FILES {
                    cut = true;
                    break 'walk;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    if !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_str()) {
                        pending.push(entry.path());
                    }
                    continue;
                }
                if !meta.is_file() || meta.len() > MAX_READ_BYTES {
                    continue;
                }
                visited += 1;
                let Ok(bytes) = std::fs::read(entry.path()) else {
                    continue;
                };
                let Ok(content) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                let relative = entry
                    .path()
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(name);
                for (index, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&needle) {
                        hits.push(format!(
                            "{relative}:{}: {}",
                            index + 1,
                            line.trim().chars().take(200).collect::<String>()
                        ));
                        if hits.len() >= MAX_SEARCH_HITS {
                            cut = true;
                            break 'walk;
                        }
                    }
                }
            }
        }
        let total = hits.len();
        let (text, clipped) = clip(hits.join("\n"));
        Ok(WorkFileEvidenceV1 {
            path: root.to_string_lossy().into_owned(),
            name: file_name(&root),
            kind: WorkFileKindV1::Search,
            bytes: u32::try_from(total).unwrap_or(u32::MAX),
            digest: String::new(),
            text,
            truncated: cut || clipped,
        })
    }
    /// The change a whole-file write would make, for the person to approve.
    pub fn propose_write(&self, path: &str, content: &str) -> Result<String, WorkFileError> {
        let file = self.resolve(path, true)?;
        let current = match read_text(&file) {
            Ok((_, _, Some(text))) => Some(text),
            Ok(_) => return Err(WorkFileError::Binary),
            Err(WorkFileError::NotFound) => None,
            Err(error) => return Err(error),
        };
        Ok(diff(current.as_deref().unwrap_or(""), content))
    }
    pub fn apply_write(
        &self,
        path: &str,
        content: &str,
    ) -> Result<WorkFileEvidenceV1, WorkFileError> {
        let file = self.resolve(path, true)?;
        let text = self.propose_write(path, content)?;
        write_atomic(&file, content.as_bytes())?;
        let (bytes, digest, _) = read_text(&file)?;
        let (text, truncated) = clip(text);
        Ok(WorkFileEvidenceV1 {
            path: file.to_string_lossy().into_owned(),
            name: file_name(&file),
            kind: WorkFileKindV1::Written,
            bytes,
            digest,
            text,
            truncated,
        })
    }
    fn edited(
        &self,
        path: &str,
        old: &str,
        new: &str,
    ) -> Result<(PathBuf, String, String), WorkFileError> {
        let file = self.resolve(path, false)?;
        let (_, _, text) = read_text(&file)?;
        let current = text.ok_or(WorkFileError::Binary)?;
        if current.matches(old).count() != 1 {
            return Err(WorkFileError::Ambiguous);
        }
        let next = current.replacen(old, new, 1);
        Ok((file, current, next))
    }
    pub fn propose_edit(&self, path: &str, old: &str, new: &str) -> Result<String, WorkFileError> {
        let (_, current, next) = self.edited(path, old, new)?;
        Ok(diff(&current, &next))
    }
    pub fn apply_edit(
        &self,
        path: &str,
        old: &str,
        new: &str,
    ) -> Result<WorkFileEvidenceV1, WorkFileError> {
        let (file, current, next) = self.edited(path, old, new)?;
        write_atomic(&file, next.as_bytes())?;
        let (bytes, digest, _) = read_text(&file)?;
        let (text, truncated) = clip(diff(&current, &next));
        Ok(WorkFileEvidenceV1 {
            path: file.to_string_lossy().into_owned(),
            name: file_name(&file),
            kind: WorkFileKindV1::Written,
            bytes,
            digest,
            text,
            truncated,
        })
    }
}

fn admit_root(folder: &str, home: Option<&Path>) -> Option<PathBuf> {
    validate_file_path(folder).ok()?;
    let root = std::fs::canonicalize(folder).ok()?;
    if !root.is_dir() {
        return None;
    }
    let home = std::fs::canonicalize(home?).ok()?;
    let relative = root.strip_prefix(&home).ok()?;
    if relative.as_os_str().is_empty() {
        // The whole home folder is never a grant.
        return None;
    }
    let relative = relative.to_string_lossy();
    if DENIED_UNDER_HOME
        .iter()
        .any(|denied| relative == *denied || relative.starts_with(&format!("{denied}/")))
    {
        return None;
    }
    Some(root)
}
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "/".into())
}
/// Bytes, digest and, for UTF-8 text without NUL, the content.
fn read_text(file: &Path) -> Result<(u32, String, Option<String>), WorkFileError> {
    let meta = std::fs::metadata(file).map_err(|_| WorkFileError::NotFound)?;
    if !meta.is_file() {
        return Err(WorkFileError::NotAFile);
    }
    if meta.len() > MAX_READ_BYTES {
        return Err(WorkFileError::TooLarge);
    }
    let bytes = std::fs::read(file).map_err(|_| WorkFileError::Io)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let text = match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Some(text),
        _ => None,
    };
    Ok((u32::try_from(meta.len()).unwrap_or(u32::MAX), digest, text))
}
fn write_atomic(file: &Path, bytes: &[u8]) -> Result<(), WorkFileError> {
    let parent = file.parent().ok_or(WorkFileError::Denied)?;
    let name = file_name(file);
    let temp = parent.join(format!(".{name}.zephium-{}", std::process::id()));
    std::fs::write(&temp, bytes).map_err(|_| WorkFileError::Io)?;
    if std::fs::rename(&temp, file).is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err(WorkFileError::Io);
    }
    Ok(())
}
/// Removed and added lines around each change; enough to judge, never a
/// full copy of both versions.
fn diff(current: &str, next: &str) -> String {
    let a: Vec<&str> = current.lines().collect();
    let b: Vec<&str> = next.lines().collect();
    let common_start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let common_end = a[common_start..]
        .iter()
        .rev()
        .zip(b[common_start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut out = format!(
        "@@ -{},{} +{},{} @@\n",
        common_start + 1,
        a.len() - common_start - common_end,
        common_start + 1,
        b.len() - common_start - common_end
    );
    for line in &a[common_start..a.len() - common_end] {
        out.push_str("- ");
        out.push_str(line);
        out.push('\n');
    }
    for line in &b[common_start..b.len() - common_end] {
        out.push_str("+ ");
        out.push_str(line);
        out.push('\n');
    }
    out
}
/// Keeps the text within the disclosed limit on a character boundary and
/// without control characters other than newline and tab.
fn clip(text: String) -> (String, bool) {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    if cleaned.len() <= MAX_WORK_FILE_TEXT_BYTES {
        return (cleaned, false);
    }
    let mut end = MAX_WORK_FILE_TEXT_BYTES;
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    (cleaned[..end].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_grant() -> (tempfile::TempDir, WorkFileGrant, PathBuf) {
        // The grant policy resolves against $HOME; tests point it at a
        // temporary home so nothing real is touched.
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        let project = home.path().join("Documents").join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
        std::fs::write(project.join("README.md"), "# Project\nhello world\n").unwrap();
        std::fs::write(
            project.join("src/main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        std::fs::write(project.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 1]).unwrap();
        let (grant, refused) = WorkFileGrant::admit(&[
            project.to_string_lossy().into_owned(),
            home.path().join(".ssh").to_string_lossy().into_owned(),
            home.path().to_string_lossy().into_owned(),
            "/etc".into(),
            "relative/path".into(),
        ]);
        assert_eq!(refused.len(), 4);
        (home, grant, project)
    }

    #[test]
    fn steps_stay_inside_granted_folders_and_bounds() {
        let _serial = crate::WORK_RUNTIME_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (home, grant, project) = home_grant();
        let listing = grant.list(&project.to_string_lossy()).unwrap();
        assert_eq!(listing.kind, WorkFileKindV1::Directory);
        assert!(listing.text.starts_with("src/\n"));
        assert!(listing.text.contains("README.md\t"));
        let read = grant
            .read(&project.join("src/main.rs").to_string_lossy())
            .unwrap();
        assert_eq!(read.kind, WorkFileKindV1::Text);
        assert_eq!(read.digest.len(), 64);
        assert!(read.text.contains("println"));
        let binary = grant
            .read(&project.join("logo.png").to_string_lossy())
            .unwrap();
        assert_eq!(binary.kind, WorkFileKindV1::Binary);
        assert!(binary.text.is_empty());
        let hits = grant.search(&project.to_string_lossy(), "HELLO").unwrap();
        assert_eq!(hits.kind, WorkFileKindV1::Search);
        assert!(hits.text.contains("README.md:2: hello world"));
        assert!(hits.text.contains("src/main.rs:2:"));
        for denied in [
            home.path().join(".ssh/id_rsa"),
            home.path().join("Documents/other.txt"),
            project.join("../secret.txt"),
            PathBuf::from("/etc/hosts"),
        ] {
            assert_eq!(
                grant.read(&denied.to_string_lossy()).unwrap_err(),
                WorkFileError::Denied,
                "{}",
                denied.display()
            );
        }
        assert_eq!(
            grant
                .read(&project.join("missing.txt").to_string_lossy())
                .unwrap_err(),
            WorkFileError::NotFound
        );
        let link = home.path().join("Documents/project/escape");
        std::os::unix::fs::symlink(home.path().join(".ssh"), &link).unwrap();
        assert_eq!(
            grant.list(&link.to_string_lossy()).unwrap_err(),
            WorkFileError::Denied
        );
    }

    #[test]
    fn writes_are_proposed_as_diffs_and_applied_atomically() {
        let _serial = crate::WORK_RUNTIME_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_home, grant, project) = home_grant();
        let readme = project.join("README.md").to_string_lossy().into_owned();
        let proposal = grant
            .propose_edit(&readme, "hello world", "hello, world")
            .unwrap();
        assert!(proposal.contains("- hello world\n+ hello, world\n"));
        assert_eq!(
            grant.propose_edit(&readme, "absent", "x").unwrap_err(),
            WorkFileError::Ambiguous
        );
        let applied = grant
            .apply_edit(&readme, "hello world", "hello, world")
            .unwrap();
        assert_eq!(applied.kind, WorkFileKindV1::Written);
        assert_eq!(
            std::fs::read_to_string(&readme).unwrap(),
            "# Project\nhello, world\n"
        );
        let fresh = project.join("notes.txt").to_string_lossy().into_owned();
        assert!(grant
            .propose_write(&fresh, "one\ntwo\n")
            .unwrap()
            .contains("+ one\n+ two\n"));
        grant.apply_write(&fresh, "one\ntwo\n").unwrap();
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "one\ntwo\n");
        assert!(!project.join(".notes.txt.zephium-0").exists());
        assert_eq!(
            grant
                .apply_write(&project.join("../out.txt").to_string_lossy(), "x")
                .unwrap_err(),
            WorkFileError::Denied
        );
        let (text, cut) = clip("a".repeat(MAX_WORK_FILE_TEXT_BYTES + 10));
        assert!(cut && text.len() == MAX_WORK_FILE_TEXT_BYTES);
    }
}
