//! Folders a request names, asked for in place. A path in the request asks
//! "Read <folder>?" before the lead starts; an allowed folder joins the run's
//! grant at once and stays available to the work's later requests. Folders
//! from earlier requests are available, never assumed.
use std::path::{Path, PathBuf};

use zephium_core::work::{runtime::*, WorkExecutionId};

use super::run::LeadRun;

pub(crate) const ALLOW: &str = "Allow for this work";
pub(crate) const NOT_NOW: &str = "Not now";
/// Paths one request may ask about.
const MAX_ASKED: usize = 3;
/// Markers of a project's root, nearest first wins.
const ROOT_MARKERS: [&str; 7] = [
    ".git",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "go.mod",
    "deno.json",
    "Gemfile",
];

/// What became of a path the request named, for the lead's context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Named {
    /// Readable in this run.
    Granted {
        folder: String,
        path: String,
    },
    Declined {
        folder: String,
    },
    Missing {
        path: String,
    },
    /// Outside what Zephium may read: the home folder itself, private
    /// folders, or anything outside home.
    Refused {
        path: String,
    },
}

/// Absolute or home-relative paths written in the text, in order, once each.
pub(crate) fn paths_in(text: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for word in text.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']')
    }) {
        let word = word.trim_end_matches(['.', ',', ';', ':', '!', '?']);
        let path = if let Some(rest) = word.strip_prefix("~/") {
            match home {
                Some(home) => home.join(rest),
                None => continue,
            }
        } else if word.starts_with('/') && word.len() > 1 && !word.starts_with("//") {
            PathBuf::from(word)
        } else {
            continue;
        };
        // One name is a slash command or a fraction, not a place on disk.
        if path.components().count() < 3 || word.contains("://") {
            continue;
        }
        let path = PathBuf::from(path.to_string_lossy().trim_end_matches('/'));
        if !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

/// The folder to ask for: the path itself when it is a folder, else the
/// nearest project root above the file, else its parent.
pub(crate) fn folder_for(path: &Path, home: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        return Some(path.to_path_buf());
    }
    let parent = path.parent()?;
    let mut at = Some(parent);
    while let Some(dir) = at {
        if dir == home || !dir.starts_with(home) {
            break;
        }
        if ROOT_MARKERS.iter().any(|marker| dir.join(marker).exists()) {
            return Some(dir.to_path_buf());
        }
        at = dir.parent();
    }
    Some(parent.to_path_buf())
}

/// The folder's name as a person reads it.
pub(crate) fn name(folder: &str) -> String {
    Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| folder.to_owned())
}

/// Whether `path` lies in one of `folders`.
fn inside(path: &Path, folders: &[String]) -> Option<String> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    folders
        .iter()
        .find(|folder| {
            let root = std::fs::canonicalize(folder).unwrap_or_else(|_| PathBuf::from(folder));
            canonical.starts_with(&root)
        })
        .cloned()
}

/// Folders the work's earlier requests used: those granted to their runs,
/// and those allowed in answer to a folder question.
pub(crate) fn earlier(
    projection: &WorkRuntimeProjection,
    current: WorkExecutionId,
) -> (Vec<String>, Vec<String>) {
    let mut granted: Vec<String> = Vec::new();
    let mut allowed: Vec<String> = Vec::new();
    let add = |list: &mut Vec<String>, folder: String| {
        if !list.contains(&folder) {
            list.push(folder);
        }
    };
    for execution in projection.executions.iter().filter(|e| e.id != current) {
        for folder in execution
            .agent_grant()
            .map(|grant| grant.folders.clone())
            .unwrap_or_default()
        {
            add(&mut granted, folder);
        }
        for step in &execution.steps {
            if let WorkStepKindV1::Ask {
                purpose: Some(WorkAskPurposeV1::Folder),
                answer: Some(answer),
                ..
            } = &step.kind
            {
                if let Some(folder) = step.local.as_ref().and_then(|l| l.folder.clone()) {
                    if answer == ALLOW {
                        add(&mut allowed, folder);
                    }
                }
            }
        }
    }
    (granted, allowed)
}

/// This request's own folders are the canvas's folders no earlier request
/// had; the rest, and folders allowed in earlier answers, are available.
/// A folder the person took off the canvas is neither.
pub(crate) fn scope(
    canvas: &[String],
    granted: &[String],
    allowed: &[String],
) -> (Vec<String>, Vec<String>) {
    let current: Vec<String> = canvas
        .iter()
        .filter(|folder| !granted.contains(folder) && !allowed.contains(folder))
        .cloned()
        .collect();
    let mut available: Vec<String> = Vec::new();
    for folder in canvas.iter().chain(allowed) {
        if !current.contains(folder) && !available.contains(folder) {
            available.push(folder.clone());
        }
    }
    (current, available)
}

/// Asks for each folder the request names that the run cannot read yet,
/// and grants the ones the person allows. A named folder the run can already
/// read becomes this request's own.
pub(crate) async fn ask_in_place(run: &LeadRun, objective: &str) -> Vec<Named> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let mut named = Vec::new();
    for path in paths_in(objective, Some(&home)).into_iter().take(MAX_ASKED) {
        let written = path.to_string_lossy().into_owned();
        let Some(folder) = folder_for(&path, &home) else {
            named.push(Named::Missing { path: written });
            continue;
        };
        let folders = run.folders();
        let known: Vec<String> = folders
            .current
            .iter()
            .chain(&folders.available)
            .cloned()
            .collect();
        if let Some(granted) = inside(&path, &known) {
            run.grant_folder(&granted);
            named.push(Named::Granted {
                folder: granted,
                path: written,
            });
            continue;
        }
        let folder = folder.to_string_lossy().into_owned();
        let (admitted, _) = crate::work_files::WorkFileGrant::admit(std::slice::from_ref(&folder));
        if admitted.is_empty() {
            named.push(Named::Refused { path: written });
            continue;
        }
        if named
            .iter()
            .any(|n| matches!(n, Named::Granted { folder: f, .. } | Named::Declined { folder: f } if *f == folder))
        {
            continue;
        }
        let answer = run
            .ask_with(
                WorkAskPurposeV1::Folder,
                format!("Read {}?", name(&folder)),
                vec![ALLOW.into(), NOT_NOW.into()],
                None,
                Some(WorkLocalStepV1 {
                    folder: Some(folder.clone()),
                    ..Default::default()
                }),
            )
            .await;
        match answer {
            Ok(Some(answer)) if answer == ALLOW => {
                run.grant_folder(&folder);
                named.push(Named::Granted {
                    folder,
                    path: written,
                });
            }
            Ok(Some(_)) => named.push(Named::Declined { folder }),
            // The run stopped while it waited; the lead ends at its first check.
            Ok(None) | Err(_) => break,
        }
    }
    named
}

/// Lines for the lead's context about the folders it may read.
pub(crate) fn context(run: &LeadRun, named: &[Named]) -> String {
    let folders = run.folders();
    let mut out = String::new();
    if !folders.current.is_empty() {
        out.push_str(&format!(
            "\nFolders for this request (read them to answer): {}\n",
            folders.current.join(", ")
        ));
    }
    if !folders.available.is_empty() {
        out.push_str(&format!(
            "Folders from earlier requests, readable but not part of this request; use one only when the request is about it: {}\n",
            folders.available.join(", ")
        ));
    }
    for fact in named {
        match fact {
            Named::Declined { folder } => out.push_str(&format!(
                "The person chose not to share {folder}: answer without it, and say in one sentence what reading it would add.\n"
            )),
            Named::Missing { path } => out.push_str(&format!(
                "{path} does not exist on this Mac: say so in one sentence.\n"
            )),
            Named::Refused { path } => out.push_str(&format!(
                "{path} is outside the folders Zephium may read (the home folder itself or private folders): say so in one sentence.\n"
            )),
            Named::Granted { .. } => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_in_a_request_are_found_once_without_their_punctuation() {
        let home = Path::new("/Users/ana");
        assert_eq!(
            paths_in(
                "Check, what I have here in this project:\n/Users/ana/Dev/Lunios. And ~/Dev/Lunios/ too, `/Users/ana/Dev/Lunios`",
                Some(home)
            ),
            vec![PathBuf::from("/Users/ana/Dev/Lunios")]
        );
        assert!(paths_in("30/40 of 1/2 and /help, https://a.com/b/c", Some(home)).is_empty());
        assert_eq!(
            paths_in("read (/tmp/x/y) please", Some(home)),
            vec![PathBuf::from("/tmp/x/y")]
        );
    }

    #[test]
    fn a_file_is_asked_for_by_its_project() {
        let base = std::env::temp_dir().join(format!("zephium-folders-{}", std::process::id()));
        let project = base.join("Lunios");
        std::fs::create_dir_all(project.join("src/routes")).unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
        std::fs::write(project.join("src/routes/page.svelte"), "").unwrap();
        assert_eq!(
            folder_for(&project.join("src/routes/page.svelte"), &base),
            Some(project.clone())
        );
        assert_eq!(folder_for(&project, &base), Some(project.clone()));
        assert_eq!(folder_for(&project.join("missing"), &base), None);
        assert_eq!(name(&project.to_string_lossy()), "Lunios");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_from_an_earlier_request_is_available_not_current() {
        let at = |name: &str| format!("/Users/ana/Dev/{name}");
        let canvas = vec![at("Lunios"), at("New")];
        let granted = vec![at("Lunios"), at("Removed")];
        let allowed = vec![at("Asked")];
        let (current, available) = scope(&canvas, &granted, &allowed);
        assert_eq!(current, vec![at("New")]);
        assert_eq!(available, vec![at("Lunios"), at("Asked")]);
    }
}
