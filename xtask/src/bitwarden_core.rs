//! Offline admission for the exact Bitwarden Core source reviewed by Zephium.
//!
//! This boundary deliberately performs no acquisition and emits no adapted
//! artifact. Release automation must present an already checked-out, clean
//! repository at the exact reviewed commit and tag. The per-file digests bind
//! the compatibility preimages which later adaptation is allowed to change.

use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

const PINNED_COMMIT: &str = "adf0337e4a0f788b895933792fc04fa162669eff";
const PINNED_TAG: &str = "browser-v2026.7.0";
const MAX_REVIEWED_SOURCE_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy)]
struct ReviewedSourceFile {
    path: &'static str,
    sha256: &'static str,
    markers: &'static [SourceMarker],
}

#[derive(Clone, Copy)]
struct SourceMarker {
    text: &'static str,
    occurrences: usize,
}

const SOURCE_FILES: &[ReviewedSourceFile] = &[
    ReviewedSourceFile {
        path: "apps/browser/src/background/main.background.ts",
        sha256: "a144b3007cd85258778aec702194c7e3ba20ec0799aececd1538ba8f7f1f373f",
        markers: &[
            SourceMarker {
                text: "const localStorageStorageService = BrowserApi.isManifestVersion(3)",
                occurrences: 1,
            },
            SourceMarker {
                text: "new OffscreenStorageService(this.offscreenDocumentService)",
                occurrences: 1,
            },
            SourceMarker {
                text: "new PrimarySecondaryStorageService(this.storageService, localStorageStorageService)",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/fido2/background/fido2.background.ts",
        sha256: "781599c51ca9b24537c82da7b0eeb82c936fe52ae06bc5d387eb448b66169231",
        markers: &[
            SourceMarker {
                text: "world: \"MAIN\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "world: chrome.scripting.ExecutionWorld.MAIN",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/manifest.v3.json",
        sha256: "8f97a04018ffccc5bd1ff579662f4086b97dd545036f5d5775398a24f264643f",
        markers: &[
            SourceMarker {
                text: "\"offscreen\"",
                occurrences: 2,
            },
            SourceMarker {
                text: "\"sandbox\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "\"overlay/menu-button.html\"",
                occurrences: 2,
            },
            SourceMarker {
                text: "\"overlay/menu-list.html\"",
                occurrences: 2,
            },
            SourceMarker {
                text: "\"overlay/menu.html\"",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/platform/offscreen-document/offscreen-document.service.ts",
        sha256: "2afe5a423a5dec6d1ef92cc35a97f171c4342f540d88fbb2ae077d4252043036",
        markers: &[SourceMarker {
            text: "return typeof chrome.offscreen !== \"undefined\";",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe.service.ts",
        sha256: "04761700c5b876d122267a2903f50a13eea973b41c1228a7c286d98aaec3890a",
        markers: &[
            SourceMarker {
                text: "BrowserApi.getRuntimeURL(\"overlay/menu.html\")",
                occurrences: 1,
            },
            SourceMarker {
                text: "this.iframe.contentWindow?.postMessage(\n      { portKey: this.portKey, ...message },\n      this.extensionOrigin,\n    );",
                occurrences: 1,
            },
        ],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/iframe-content/autofill-inline-menu-iframe-element.ts",
        sha256: "7bc325afdad14744b29e68ecb363c859f79231ae240697303a2591cd012d984b",
        markers: &[SourceMarker {
            text: "this.autofillInlineMenuIframeService.initMenuIframe();",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/pages/button/button.html",
        sha256: "b1b0a514eb26df079f056e2a0c7c21662b37cd4f24ddfc9fc1476c80760c1c50",
        markers: &[SourceMarker {
            text: "<autofill-inline-menu-button></autofill-inline-menu-button>",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/src/autofill/overlay/inline-menu/pages/list/list.html",
        sha256: "181ca75ab2e5bb01caf3b49caf6a6233f3821a4b8a2f121b3a8a84b6765837a0",
        markers: &[SourceMarker {
            text: "<autofill-inline-menu-list></autofill-inline-menu-list>",
            occurrences: 1,
        }],
    },
    ReviewedSourceFile {
        path: "apps/browser/webpack.base.js",
        sha256: "ae27bd62f8e4d34ae5e6fb94e063cada777de2138e0ffd3f3403dbeae8fda625",
        markers: &[
            SourceMarker {
                text: "filename: \"overlay/menu-button.html\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "filename: \"overlay/menu-list.html\"",
                occurrences: 1,
            },
            SourceMarker {
                text: "filename: \"overlay/menu.html\"",
                occurrences: 1,
            },
        ],
    },
];

pub(crate) fn check_source(root: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize source root: {error}"))?;
    if !root.is_dir() {
        return Err("source root is not a directory".into());
    }
    if read_git_boolean(&root, "core.sparseCheckout")? {
        return Err("sparse source checkouts are not release-admissible".into());
    }
    let sparse_definition = run_git(
        &root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/sparse-checkout",
        ],
    )?;
    if Path::new(&sparse_definition).exists() {
        return Err("a sparse-checkout definition is not release-admissible".into());
    }

    let commit = run_git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    require_exact_line("commit", &commit, PINNED_COMMIT)?;
    let tags = run_git(&root, &["tag", "--points-at", "HEAD"])?;
    if !tags.lines().any(|tag| tag == PINNED_TAG) {
        return Err(format!(
            "HEAD is not labelled with the exact reviewed tag {PINNED_TAG}"
        ));
    }
    let status = run_git(
        &root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.is_empty() {
        return Err("source checkout is not clean".into());
    }

    for reviewed in SOURCE_FILES {
        validate_relative_path(reviewed.path)?;
        let path = root.join(reviewed.path);
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("cannot canonicalize {}: {error}", reviewed.path))?;
        if !canonical.starts_with(&root) {
            return Err(format!("{} escapes the source root", reviewed.path));
        }
        let metadata = fs::metadata(&canonical)
            .map_err(|error| format!("cannot inspect {}: {error}", reviewed.path))?;
        if !metadata.is_file() || metadata.len() > MAX_REVIEWED_SOURCE_FILE_BYTES {
            return Err(format!(
                "{} is not a bounded regular source file",
                reviewed.path
            ));
        }
        let bytes = fs::read(&canonical)
            .map_err(|error| format!("cannot read {}: {error}", reviewed.path))?;
        let digest = sha256_hex(&bytes);
        if digest != reviewed.sha256 {
            return Err(format!(
                "{} digest does not match the reviewed source",
                reviewed.path
            ));
        }
        let source =
            std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", reviewed.path))?;
        for marker in reviewed.markers {
            require_occurrences(reviewed.path, source, *marker)?;
        }
    }

    println!(
        "Bitwarden Core source admission passed: commit={PINNED_COMMIT}; tag={PINNED_TAG}; reviewed_files={}",
        SOURCE_FILES.len()
    );
    Ok(())
}

fn run_git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot execute git: {error}"))?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| "cannot capture git output".to_owned())?
        .take(MAX_GIT_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read git output: {error}"))?;
    if bytes.len() as u64 > MAX_GIT_OUTPUT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "git {} exceeded the bounded output allowance",
            arguments.join(" ")
        ));
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for git: {error}"))?;
    if !status.success() {
        return Err(format!(
            "git {} failed with status {}",
            arguments.join(" "),
            status
        ));
    }
    String::from_utf8(bytes)
        .map(|value| value.trim_end_matches(['\r', '\n']).to_owned())
        .map_err(|_| format!("git {} emitted non-UTF-8 output", arguments.join(" ")))
}

fn read_git_boolean(root: &Path, key: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--bool", "--get", key])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("cannot read git configuration: {error}"))?;
    match output.status.code() {
        Some(0) => match output.stdout.as_slice() {
            b"true\n" | b"true\r\n" => Ok(true),
            b"false\n" | b"false\r\n" => Ok(false),
            _ => Err(format!(
                "git configuration {key} is not a canonical boolean"
            )),
        },
        Some(1) => Ok(false),
        _ => Err(format!("cannot read git configuration {key}")),
    }
}

fn require_exact_line(subject: &str, actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected && !actual.contains(['\r', '\n']) {
        Ok(())
    } else {
        Err(format!("{subject} does not match the reviewed value"))
    }
}

fn require_occurrences(path: &str, source: &str, marker: SourceMarker) -> Result<(), String> {
    let actual = source.match_indices(marker.text).count();
    if actual == marker.occurrences {
        Ok(())
    } else {
        Err(format!(
            "{path} marker drifted: expected {} occurrence(s), observed {actual}",
            marker.occurrences
        ))
    }
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let parsed = Path::new(path);
    if path.is_empty()
        || parsed.is_absolute()
        || parsed
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(format!(
            "reviewed path is not portable and relative: {path}"
        ))
    } else {
        Ok(())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing into a String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_inventory_is_unique_portable_and_bounded() {
        let mut paths = SOURCE_FILES
            .iter()
            .map(|file| file.path)
            .collect::<Vec<_>>();
        for path in &paths {
            validate_relative_path(path).unwrap();
        }
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), SOURCE_FILES.len());
        assert!(SOURCE_FILES.iter().all(|file| file.sha256.len() == 64));
    }

    #[test]
    fn exact_marker_cardinality_fails_closed() {
        let marker = SourceMarker {
            text: "needle",
            occurrences: 1,
        };
        assert!(require_occurrences("fixture", "before needle after", marker).is_ok());
        assert!(require_occurrences("fixture", "no match", marker).is_err());
        assert!(require_occurrences("fixture", "needle needle", marker).is_err());
    }

    #[test]
    fn digest_and_exact_line_checks_are_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(require_exact_line("fixture", "value", "value").is_ok());
        assert!(require_exact_line("fixture", "value\nother", "value").is_err());
    }
}
