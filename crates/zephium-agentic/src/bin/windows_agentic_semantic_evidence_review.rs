//! Offline, content-free review of the seven required physical Windows semantic results.

use std::ffi::OsString;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use zephium_agentic::{
    decode_windows_semantic_probe_response, evidence_metadata_is_direct_directory,
    evidence_metadata_is_direct_file, qualify_windows_semantic_probe_evidence, Platform,
    RuntimeFingerprint, WindowsSemanticProbeAggregate, WindowsSemanticProbeReply,
    MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES, WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES,
};

const REVIEW_SCHEMA_VERSION: u16 = 5;
const REVIEW_SUMMARY_FILENAME: &str = "windows-semantic-review-summary-v5.json";

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct WindowsSemanticPhysicalReview {
    schema_version: u16,
    runtime: RuntimeFingerprint,
    modes: Vec<WindowsSemanticProbeAggregate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewError {
    Arguments,
    Directory,
    MissingRecord,
    RecordType,
    RecordSize,
    RecordRead,
    RecordProtocol,
    RecordRejected,
    RecordIdentity,
    Qualification,
    RuntimeMismatch,
    Output,
}

impl ReviewError {
    const fn message(self) -> &'static str {
        match self {
            Self::Arguments => {
                "expected --directory followed by the ignored result directory and optional --write-summary"
            }
            Self::Directory => "result directory must be one real non-symlink directory",
            Self::MissingRecord => "one required Windows semantic result record is missing",
            Self::RecordType => "one Windows semantic result must be a real non-symlink file",
            Self::RecordSize => "one Windows semantic result violates the JSONL byte ceiling",
            Self::RecordRead => "one Windows semantic result could not be read",
            Self::RecordProtocol => {
                "one Windows semantic result violates the closed response protocol"
            }
            Self::RecordRejected => {
                "one Windows semantic run returned a typed rejection instead of evidence"
            }
            Self::RecordIdentity => {
                "one Windows semantic result does not use the one-shot runner identity"
            }
            Self::Qualification => {
                "one Windows semantic result failed its exact mode qualification"
            }
            Self::RuntimeMismatch => {
                "Windows semantic result runtime fingerprints do not all match"
            }
            Self::Output => {
                "content-free Windows semantic review output could not be encoded or published"
            }
        }
    }
}

fn main() {
    if let Err(error) = run(std::env::args_os().skip(1).collect()) {
        eprintln!(
            "windows-agentic-semantic-evidence-review: {}",
            error.message()
        );
        std::process::exit(2);
    }
}

fn run(arguments: Vec<OsString>) -> Result<(), ReviewError> {
    let [flag, directory, rest @ ..] = arguments.as_slice() else {
        return Err(ReviewError::Arguments);
    };
    if flag != "--directory" {
        return Err(ReviewError::Arguments);
    }
    let write_summary = match rest {
        [] => false,
        [flag] if flag == "--write-summary" => true,
        _ => return Err(ReviewError::Arguments),
    };
    let directory = PathBuf::from(directory);
    validate_directory(&directory)?;
    let review = review_directory(&directory)?;
    let mut output = serde_json::to_vec(&review).map_err(|_| ReviewError::Output)?;
    output.push(b'\n');
    if output.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES {
        return Err(ReviewError::Output);
    }
    if write_summary {
        write_new_record(&directory, REVIEW_SUMMARY_FILENAME, &output)
    } else {
        let mut stdout = io::stdout().lock();
        stdout.write_all(&output).map_err(|_| ReviewError::Output)?;
        stdout.flush().map_err(|_| ReviewError::Output)
    }
}

fn validate_directory(directory: &Path) -> Result<(), ReviewError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|_| ReviewError::Directory)?;
    if !evidence_metadata_is_direct_directory(&metadata) {
        return Err(ReviewError::Directory);
    }
    Ok(())
}

fn review_directory(directory: &Path) -> Result<WindowsSemanticPhysicalReview, ReviewError> {
    let mut runtime = None;
    let mut modes = Vec::with_capacity(WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES.len());
    for mode in WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES {
        let bytes = read_record(directory, mode.local_result_filename())?;
        let response = decode_windows_semantic_probe_response(&bytes)
            .map_err(|_| ReviewError::RecordProtocol)?;
        if response.request_id != 1 {
            return Err(ReviewError::RecordIdentity);
        }
        let WindowsSemanticProbeReply::Completed(evidence) = response.reply else {
            return Err(ReviewError::RecordRejected);
        };
        if evidence.run_id != 1 || evidence.runtime.platform != Platform::Windows {
            return Err(ReviewError::RecordIdentity);
        }
        if let Some(expected) = runtime.as_ref() {
            if expected != &evidence.runtime {
                return Err(ReviewError::RuntimeMismatch);
            }
        } else {
            runtime = Some(evidence.runtime.clone());
        }
        modes.push(
            qualify_windows_semantic_probe_evidence(mode, &evidence)
                .map_err(|_| ReviewError::Qualification)?,
        );
    }

    Ok(WindowsSemanticPhysicalReview {
        schema_version: REVIEW_SCHEMA_VERSION,
        runtime: runtime.ok_or(ReviewError::MissingRecord)?,
        modes,
    })
}

fn write_new_record(directory: &Path, filename: &str, bytes: &[u8]) -> Result<(), ReviewError> {
    validate_directory(directory)?;
    let path = directory.join(filename);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| ReviewError::Output)?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    if result.is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(ReviewError::Output);
    }
    Ok(())
}

fn read_record(directory: &Path, filename: &str) -> Result<Vec<u8>, ReviewError> {
    let path = directory.join(filename);
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            ReviewError::MissingRecord
        } else {
            ReviewError::RecordRead
        }
    })?;
    if !evidence_metadata_is_direct_file(&metadata) {
        return Err(ReviewError::RecordType);
    }
    if metadata.len() == 0 || metadata.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES as u64 {
        return Err(ReviewError::RecordSize);
    }
    let file = std::fs::File::open(path).map_err(|_| ReviewError::RecordRead)?;
    let capacity = usize::try_from(metadata.len()).map_err(|_| ReviewError::RecordSize)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ReviewError::RecordRead)?;
    file.take((MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ReviewError::RecordRead)?;
    if bytes.is_empty() || bytes.len() > MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES {
        return Err(ReviewError::RecordSize);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zephium_agentic::{
        encode_windows_semantic_probe_response, EvidenceLabel, WindowsSemanticProbeEvidence,
        WindowsSemanticProbeMode, WindowsSemanticProbeResponse, WindowsSemanticResourceEvidence,
        WindowsSemanticTeardownEvidence, WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "zephium-windows-semantic-evidence-review-test-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create isolated test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn evidence(mode: WindowsSemanticProbeMode) -> WindowsSemanticProbeEvidence {
        let flood = mode == WindowsSemanticProbeMode::HiddenEventFlood;
        let renderer = mode == WindowsSemanticProbeMode::HiddenRendererLoss;
        let suspension = mode == WindowsSemanticProbeMode::HiddenSuspendResume;
        let redirects = mode == WindowsSemanticProbeMode::HiddenRedirectLifecycle;
        let location = mode == WindowsSemanticProbeMode::HiddenLocationReplacement;
        WindowsSemanticProbeEvidence {
            run_id: 1,
            runtime: RuntimeFingerprint {
                platform: Platform::Windows,
                os_version: EvidenceLabel::new("10.0.26100").expect("OS label"),
                engine: EvidenceLabel::new("WebView2").expect("engine label"),
                engine_version: EvidenceLabel::new("140.0.0.0").expect("version label"),
                adapter_revision: EvidenceLabel::new(
                    "semantic-runtime-m3-lifecycle-m2-redirect-location-resources-v2",
                )
                .expect("adapter label"),
            },
            mode,
            ephemeral_profile: true,
            in_private: true,
            extensions_absent: true,
            presentation_hidden: true,
            viewport_width: 1_280,
            viewport_height: 800,
            loopback_only: true,
            snapshots: if renderer {
                1
            } else if location {
                3
            } else {
                2
            },
            document_epochs: if flood {
                3
            } else if renderer || suspension {
                1
            } else {
                2
            },
            first_snapshot_verified: true,
            replacement_snapshot_verified: !renderer && !suspension && !redirects && !location,
            replacement_stale_state_absent: !renderer && !suspension && !redirects && !location,
            page_world_bridge_absent: true,
            secrets_redacted: true,
            event_flood_refused: flood,
            recovered_after_event_flood: flood,
            renderer_loss_observed: renderer,
            renderer_lost_refused: renderer,
            suspend_callback_succeeded: suspension,
            suspended_state_attested: suspension,
            resume_state_attested: suspension,
            post_resume_snapshot_verified: suspension,
            suspend_ms: if suspension { 25 } else { 0 },
            redirect_chain_verified: redirects,
            redirect_chain_hops_observed: if redirects { 2 } else { 0 },
            redirect_limit_refused: redirects,
            redirect_limit_hops_observed: if redirects {
                zephium_agentic::MAX_CONTEXT_NAVIGATION_REDIRECTS as u8
            } else {
                0
            },
            redirect_recovery_verified: redirects,
            same_document_replacement_observed: location,
            same_document_replacement_rejoined: location,
            stale_location_join_refused: location,
            post_location_snapshot_verified: location,
            debugger_attached: mode == WindowsSemanticProbeMode::HiddenDebuggerCoexistence,
            focus_theft_observed: false,
            peak_pending_invocations: 1,
            semantic_work_drained: true,
            resources_before: WindowsSemanticResourceEvidence {
                webview2_processes: 4,
                resident_bytes: 256 * 1_024 * 1_024,
            },
            resources_after: WindowsSemanticResourceEvidence {
                webview2_processes: 5,
                resident_bytes: 320 * 1_024 * 1_024,
            },
            elapsed_ms: 800,
            teardown: WindowsSemanticTeardownEvidence {
                runtime_retired: true,
                view_closed: true,
                browser_process_exited: true,
                profile_removed: true,
                fixture_drained: true,
                work_drained: true,
                retained_native_views: 0,
                cleanup_ms: 50,
            },
        }
    }

    fn write_mode(directory: &Path, mode: WindowsSemanticProbeMode) {
        let response = WindowsSemanticProbeResponse {
            protocol_version: WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply: WindowsSemanticProbeReply::Completed(Box::new(evidence(mode))),
        };
        let bytes = encode_windows_semantic_probe_response(&response).expect("encode response");
        std::fs::write(directory.join(mode.local_result_filename()), bytes).expect("write result");
    }

    #[test]
    fn exact_seven_mode_set_reviews_successfully() {
        let directory = TestDirectory::new();
        for mode in WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES {
            write_mode(&directory.0, mode);
        }
        let review = review_directory(&directory.0).expect("review");
        assert_eq!(review.schema_version, REVIEW_SCHEMA_VERSION);
        assert_eq!(review.modes.len(), 7);
        assert_eq!(review.runtime.platform, Platform::Windows);
    }

    #[test]
    fn substitution_and_runtime_mismatch_fail_closed() {
        let directory = TestDirectory::new();
        for mode in WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES {
            write_mode(&directory.0, mode);
        }
        let mode = WindowsSemanticProbeMode::HiddenEventFlood;
        let response = WindowsSemanticProbeResponse {
            protocol_version: WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply: WindowsSemanticProbeReply::Completed(Box::new(evidence(
                WindowsSemanticProbeMode::HiddenFixedDocuments,
            ))),
        };
        std::fs::write(
            directory.0.join(mode.local_result_filename()),
            encode_windows_semantic_probe_response(&response).expect("encode substitution"),
        )
        .expect("replace result");
        assert_eq!(
            review_directory(&directory.0).err(),
            Some(ReviewError::Qualification)
        );

        write_mode(&directory.0, mode);
        let debugger = WindowsSemanticProbeMode::HiddenDebuggerCoexistence;
        let mut changed = evidence(debugger);
        changed.runtime.engine_version = EvidenceLabel::new("141.0.0.0").expect("version");
        let response = WindowsSemanticProbeResponse {
            protocol_version: WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply: WindowsSemanticProbeReply::Completed(Box::new(changed)),
        };
        std::fs::write(
            directory.0.join(debugger.local_result_filename()),
            encode_windows_semantic_probe_response(&response).expect("encode mismatch"),
        )
        .expect("replace result");
        assert_eq!(
            review_directory(&directory.0).err(),
            Some(ReviewError::RuntimeMismatch)
        );
    }

    #[test]
    fn result_reader_rejects_missing_directory_and_oversized_records() {
        let directory = TestDirectory::new();
        assert_eq!(
            read_record(&directory.0, "missing.jsonl"),
            Err(ReviewError::MissingRecord)
        );

        let nested = directory.0.join("nested.jsonl");
        std::fs::create_dir(&nested).expect("nested directory");
        assert_eq!(
            read_record(&directory.0, "nested.jsonl"),
            Err(ReviewError::RecordType)
        );

        let oversized = directory.0.join("oversized.jsonl");
        std::fs::write(
            &oversized,
            vec![b' '; MAX_WINDOWS_SEMANTIC_PROBE_OUTPUT_BYTES + 1],
        )
        .expect("oversized record");
        assert_eq!(
            read_record(&directory.0, "oversized.jsonl"),
            Err(ReviewError::RecordSize)
        );
    }

    #[test]
    fn summary_writer_is_create_new_and_byte_exact() {
        let directory = TestDirectory::new();
        let bytes = b"{\"schema_version\":5}\n";
        write_new_record(&directory.0, REVIEW_SUMMARY_FILENAME, bytes).expect("write summary");
        assert_eq!(
            std::fs::read(directory.0.join(REVIEW_SUMMARY_FILENAME)).expect("read summary"),
            bytes
        );
        assert_eq!(
            write_new_record(&directory.0, REVIEW_SUMMARY_FILENAME, b"replacement\n"),
            Err(ReviewError::Output)
        );
    }

    #[cfg(unix)]
    #[test]
    fn result_reader_refuses_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new();
        let target = directory.0.join("target.jsonl");
        std::fs::write(&target, b"{}\n").expect("target");
        symlink(&target, directory.0.join("alias.jsonl")).expect("symlink");
        assert_eq!(
            read_record(&directory.0, "alias.jsonl"),
            Err(ReviewError::RecordType)
        );
    }
}
