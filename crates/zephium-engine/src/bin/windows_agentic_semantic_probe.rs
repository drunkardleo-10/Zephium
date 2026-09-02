//! One-shot physical-Windows qualifier for the production semantic adapter.

#[cfg(any(test, target_os = "windows"))]
mod capture {
    use std::ffi::{OsStr, OsString};
    use std::io::{self, Write as _};
    use std::path::{Path, PathBuf};

    #[cfg(test)]
    use zephium_agentic::WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES;
    use zephium_agentic::{evidence_metadata_is_direct_directory, WindowsSemanticProbeMode};

    const EVIDENCE_DIRECTORY_FLAG: &str = "--evidence-directory";
    const EVIDENCE_DIRECTORY: &str = "eval/agentic-browsing/local-results";

    pub(super) struct Invocation {
        mode: WindowsSemanticProbeMode,
    }

    impl Invocation {
        pub(super) fn parse(arguments: &[OsString]) -> Option<Self> {
            let [argument, flag, directory] = arguments else {
                return None;
            };
            if flag != EVIDENCE_DIRECTORY_FLAG || directory != OsStr::new(EVIDENCE_DIRECTORY) {
                return None;
            }
            argument
                .to_str()
                .and_then(WindowsSemanticProbeMode::from_argument)
                .map(|mode| Self { mode })
        }

        pub(super) const fn mode(&self) -> WindowsSemanticProbeMode {
            self.mode
        }

        #[cfg_attr(test, allow(dead_code))]
        pub(super) fn prepare_sink(&self) -> io::Result<EvidenceSink> {
            EvidenceSink::prepare_review_record(
                Path::new(EVIDENCE_DIRECTORY),
                self.mode.local_result_filename(),
            )
        }
    }

    pub(super) struct EvidenceSink(Option<PendingRecord>);

    struct PendingRecord {
        file: tempfile::NamedTempFile,
        destination: PathBuf,
    }

    impl EvidenceSink {
        fn prepare_review_record(directory: &Path, filename: &str) -> io::Result<Self> {
            let metadata = std::fs::symlink_metadata(directory)?;
            if !evidence_metadata_is_direct_directory(&metadata) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "evidence directory is not a real directory",
                ));
            }
            let destination = directory.join(filename);
            match std::fs::symlink_metadata(&destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "evidence record already exists",
                    ));
                }
            }
            let file = tempfile::NamedTempFile::new_in(directory)?;
            Ok(Self(Some(PendingRecord { file, destination })))
        }

        pub(super) fn emit(&mut self, encoded: &[u8]) -> io::Result<()> {
            let mut pending = self.0.take().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "evidence already emitted")
            })?;
            pending.file.write_all(encoded)?;
            pending.file.flush()?;
            pending.file.as_file().sync_all()?;
            pending
                .file
                .persist_noclobber(pending.destination)
                .map(|_| ())
                .map_err(|error| error.error)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn arguments(values: &[&str]) -> Vec<OsString> {
            values.iter().map(OsString::from).collect()
        }

        #[test]
        fn invocation_allows_only_seven_fixed_records_in_the_ignored_directory() {
            for mode in WINDOWS_SEMANTIC_PHYSICAL_REVIEW_MODES {
                let invocation = Invocation::parse(&arguments(&[
                    mode.argument(),
                    "--evidence-directory",
                    EVIDENCE_DIRECTORY,
                ]))
                .expect("fixed review invocation");
                assert_eq!(invocation.mode(), mode);
            }
            assert!(Invocation::parse(&arguments(&["--ci-hidden-fixed-documents"])).is_none());
            assert!(Invocation::parse(&arguments(&[
                "--ci-hidden-fixed-documents",
                "--evidence-directory",
                "arbitrary-output",
            ]))
            .is_none());
            assert!(Invocation::parse(&arguments(&[
                "--unknown",
                "--evidence-directory",
                EVIDENCE_DIRECTORY,
            ]))
            .is_none());
        }

        #[test]
        fn review_record_is_atomic_create_new_and_byte_exact() {
            let directory = tempfile::tempdir().expect("temporary directory");
            let mut sink = EvidenceSink::prepare_review_record(directory.path(), "evidence.jsonl")
                .expect("prepare record");
            let encoded = b"{\"protocol_version\":1}\n";
            sink.emit(encoded).expect("publish record");
            assert_eq!(
                std::fs::read(directory.path().join("evidence.jsonl")).expect("read record"),
                encoded
            );
            assert!(
                EvidenceSink::prepare_review_record(directory.path(), "evidence.jsonl").is_err()
            );
            assert!(sink.emit(encoded).is_err());
        }

        #[cfg(unix)]
        #[test]
        fn review_record_refuses_a_symlink_directory() {
            use std::os::unix::fs::symlink;

            let root = tempfile::tempdir().expect("temporary directory");
            let real = root.path().join("real");
            let alias = root.path().join("alias");
            std::fs::create_dir(&real).expect("real directory");
            symlink(&real, &alias).expect("directory symlink");
            assert!(EvidenceSink::prepare_review_record(&alias, "evidence.jsonl").is_err());
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("windows-agentic-semantic-probe: unsupported platform");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() {
    windows::main();
}

#[cfg(target_os = "windows")]
mod windows {
    use zephium_agentic::{
        encode_windows_semantic_probe_response, qualify_windows_semantic_probe_evidence,
        WindowsSemanticProbeFailure, WindowsSemanticProbeReply, WindowsSemanticProbeResponse,
        WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
    };

    use super::capture::{EvidenceSink, Invocation};

    pub(super) fn main() {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        let invocation = Invocation::parse(&arguments).unwrap_or_else(|| {
            fail("expected one exact hidden semantic mode followed by the fixed evidence-directory pair")
        });
        let mode = invocation.mode();
        let mut sink = invocation
            .prepare_sink()
            .unwrap_or_else(|_| fail("evidence output preflight failed"));
        let evidence = match zephium_engine::run_windows_agentic_semantic_probe(1, mode) {
            Ok(evidence) => evidence,
            Err(failure) => {
                if !emit_reply(&mut sink, WindowsSemanticProbeReply::Rejected(failure)) {
                    fail("machine evidence output failed");
                }
                fail_probe("qualification run failed", failure);
            }
        };
        let qualification = qualify_windows_semantic_probe_evidence(mode, &evidence);
        let summary = format!(
            "windows-agentic-semantic-probe: passed; mode={mode:?}; profile=ephemeral-udf+inprivate; extensions=construction-empty; presentation=hidden; viewport=1280x800-logical; fixture=loopback-only; snapshots={}; world_epochs={}; page_world_bridge=absent; secrets=redacted; suspend_callback={}; suspended_readback={}; resume_readback={}; post_resume_snapshot={}; suspend_ms={}; redirect_chain={}; redirect_chain_hops={}; redirect_limit={}; redirect_limit_hops={}; redirect_recovery={}; location_observed={}; location_rejoined={}; stale_location_join={}; post_location_snapshot={}; peak_pending={}; focus_theft=0; retained_views=0",
            evidence.snapshots,
            evidence.document_epochs,
            evidence.suspend_callback_succeeded,
            evidence.suspended_state_attested,
            evidence.resume_state_attested,
            evidence.post_resume_snapshot_verified,
            evidence.suspend_ms,
            evidence.redirect_chain_verified,
            evidence.redirect_chain_hops_observed,
            evidence.redirect_limit_refused,
            evidence.redirect_limit_hops_observed,
            evidence.redirect_recovery_verified,
            evidence.same_document_replacement_observed,
            evidence.same_document_replacement_rejoined,
            evidence.stale_location_join_refused,
            evidence.post_location_snapshot_verified,
            evidence.peak_pending_invocations,
        );
        if !emit_reply(&mut sink, WindowsSemanticProbeReply::Completed(evidence)) {
            fail("machine evidence output failed");
        }
        match qualification {
            Ok(_) => eprintln!("{summary}"),
            Err(_) => fail("evidence failed its exact mode qualification contract"),
        }
    }

    fn emit_reply(sink: &mut EvidenceSink, reply: WindowsSemanticProbeReply) -> bool {
        let response = WindowsSemanticProbeResponse {
            protocol_version: WINDOWS_SEMANTIC_PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply,
        };
        let Ok(encoded) = encode_windows_semantic_probe_response(&response) else {
            return false;
        };
        sink.emit(&encoded).is_ok()
    }

    fn fail_probe(message: &str, failure: WindowsSemanticProbeFailure) -> ! {
        eprintln!(
            "windows-agentic-semantic-probe: {message}; code={:?}; stage={:?}; retryable={}",
            failure.code, failure.stage, failure.retryable
        );
        std::process::exit(2);
    }

    fn fail(message: &str) -> ! {
        eprintln!("windows-agentic-semantic-probe: {message}");
        std::process::exit(2);
    }
}
