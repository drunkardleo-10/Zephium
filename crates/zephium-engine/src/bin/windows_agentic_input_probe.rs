//! One-shot physical-Windows qualification runner for the release-excluded probe.

#[cfg(any(test, target_os = "windows"))]
mod capture {
    use std::ffi::{OsStr, OsString};
    use std::io::{self, Write as _};
    use std::path::{Path, PathBuf};

    use zephium_agentic::WindowsProbeMode;
    #[cfg(test)]
    use zephium_agentic::WINDOWS_PHYSICAL_REVIEW_MODES;

    const EVIDENCE_DIRECTORY_FLAG: &str = "--evidence-directory";
    const EVIDENCE_DIRECTORY: &str = "eval/agentic-browsing/local-results";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum EvidenceOutput {
        Stdout,
        ReviewRecord,
    }

    pub(super) struct Invocation {
        mode: WindowsProbeMode,
        output: EvidenceOutput,
    }

    impl Invocation {
        pub(super) fn parse(arguments: &[OsString]) -> Option<Self> {
            match arguments {
                [argument] => argument
                    .to_str()
                    .and_then(WindowsProbeMode::from_argument)
                    .filter(|mode| !mode.requires_visible_focus_authorization())
                    .map(|mode| Self {
                        mode,
                        output: EvidenceOutput::Stdout,
                    }),
                [argument, gate] if gate == "--allow-visible-focused" => argument
                    .to_str()
                    .and_then(WindowsProbeMode::from_argument)
                    .filter(|mode| mode.requires_visible_focus_authorization())
                    .map(|mode| Self {
                        mode,
                        output: EvidenceOutput::Stdout,
                    }),
                [argument, flag, directory]
                    if flag == EVIDENCE_DIRECTORY_FLAG
                        && directory == OsStr::new(EVIDENCE_DIRECTORY) =>
                {
                    argument
                        .to_str()
                        .and_then(WindowsProbeMode::from_argument)
                        .filter(|mode| {
                            !mode.requires_visible_focus_authorization()
                                && mode.local_result_filename().is_some()
                        })
                        .map(|mode| Self {
                            mode,
                            output: EvidenceOutput::ReviewRecord,
                        })
                }
                _ => None,
            }
        }

        pub(super) const fn mode(&self) -> WindowsProbeMode {
            self.mode
        }

        pub(super) fn prepare_sink(&self) -> io::Result<EvidenceSink> {
            match self.output {
                EvidenceOutput::Stdout => Ok(EvidenceSink(EvidenceSinkKind::Stdout)),
                EvidenceOutput::ReviewRecord => {
                    let filename = self.mode.local_result_filename().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "mode has no review record")
                    })?;
                    EvidenceSink::prepare_review_record(Path::new(EVIDENCE_DIRECTORY), filename)
                }
            }
        }
    }

    pub(super) struct EvidenceSink(EvidenceSinkKind);

    enum EvidenceSinkKind {
        Stdout,
        ReviewRecord(Option<PendingRecord>),
    }

    struct PendingRecord {
        file: tempfile::NamedTempFile,
        destination: PathBuf,
    }

    impl EvidenceSink {
        fn prepare_review_record(directory: &Path, filename: &str) -> io::Result<Self> {
            let metadata = std::fs::symlink_metadata(directory)?;
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
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
            Ok(Self(EvidenceSinkKind::ReviewRecord(Some(PendingRecord {
                file,
                destination,
            }))))
        }

        pub(super) fn emit(&mut self, encoded: &[u8]) -> io::Result<()> {
            match &mut self.0 {
                EvidenceSinkKind::Stdout => {
                    let mut stdout = io::stdout().lock();
                    stdout.write_all(encoded)?;
                    stdout.flush()
                }
                EvidenceSinkKind::ReviewRecord(pending) => {
                    let mut pending = pending.take().ok_or_else(|| {
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
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn arguments(values: &[&str]) -> Vec<OsString> {
            values.iter().map(OsString::from).collect()
        }

        #[test]
        fn invocation_allows_only_fixed_review_records_and_keeps_focus_separate() {
            for mode in WINDOWS_PHYSICAL_REVIEW_MODES {
                let invocation = Invocation::parse(&arguments(&[
                    mode.argument(),
                    "--evidence-directory",
                    EVIDENCE_DIRECTORY,
                ]))
                .expect("fixed review invocation");
                assert_eq!(invocation.mode(), mode);
                assert_eq!(invocation.output, EvidenceOutput::ReviewRecord);
            }

            let stdout = Invocation::parse(&arguments(&["--ci-hidden-hwnd"]))
                .expect("stdout invocation")
                .prepare_sink()
                .expect("stdout sink");
            assert!(matches!(stdout.0, EvidenceSinkKind::Stdout));

            assert!(Invocation::parse(&arguments(&[
                "--ci-hidden-hwnd",
                "--evidence-directory",
                "arbitrary-output",
            ]))
            .is_none());
            assert!(Invocation::parse(&arguments(&[
                "--visible-focused-windows-all",
                "--evidence-directory",
                EVIDENCE_DIRECTORY,
            ]))
            .is_none());
            assert!(Invocation::parse(&arguments(&[
                "--ci-hidden-windows-all",
                "--evidence-directory",
                EVIDENCE_DIRECTORY,
            ]))
            .is_none());
            assert!(Invocation::parse(&arguments(&[
                "--visible-focused-windows-all",
                "--allow-visible-focused",
            ]))
            .is_some());
        }

        #[test]
        fn review_record_is_atomic_create_new_and_byte_exact() {
            let directory = tempfile::tempdir().expect("temporary directory");
            let mut sink = EvidenceSink::prepare_review_record(directory.path(), "evidence.jsonl")
                .expect("prepare record");
            let encoded = b"{\"protocolVersion\":1}\n";
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
    eprintln!("windows-agentic-input-probe: unsupported platform");
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
fn main() {
    windows::main();
}

#[cfg(target_os = "windows")]
mod windows {
    use zephium_agentic::{
        encode_response_line, qualify_windows_probe_evidence, CaseOutcome, InputEventKind,
        ProbeFailure, ProbeGate, ProbeReply, ProbeResponse, PROBE_PROTOCOL_VERSION,
    };

    use super::capture::{EvidenceSink, Invocation};

    pub(super) fn main() {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        let invocation = Invocation::parse(&arguments).unwrap_or_else(|| {
            fail(
                "expected one closed hidden/background mode (optionally followed by the fixed evidence-directory pair), or the focused mode followed by --allow-visible-focused",
            )
        });
        let mode = invocation.mode();
        let mut sink = invocation
            .prepare_sink()
            .unwrap_or_else(|_| fail("evidence output preflight failed"));
        let matrix = mode.matrix();
        let gate = ProbeGate::new();
        let permit = gate
            .try_start(1)
            .unwrap_or_else(|_| fail("probe permit admission failed"));
        let evidence =
            match zephium_engine::run_windows_agentic_input_matrix(1, &matrix, &permit, || {}) {
                Ok(evidence) => evidence,
                Err(failure) => {
                    if !emit_reply(&mut sink, ProbeReply::Rejected(failure)) {
                        fail("machine evidence output failed");
                    }
                    fail_probe("matrix failed", failure);
                }
            };
        let qualification = qualify_windows_probe_evidence(mode, &evidence);
        let trusted_effect_events = evidence
            .cases
            .iter()
            .flat_map(|case| &case.events)
            .filter(|event| {
                event.is_trusted
                    && !matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
            })
            .count();
        let trusted_focus_events = evidence
            .cases
            .iter()
            .flat_map(|case| &case.events)
            .filter(|event| {
                event.is_trusted
                    && matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
            })
            .count();
        let activated_rows = evidence
            .cases
            .iter()
            .filter(|case| {
                case.activation.active_before
                    || case.activation.active_during_event
                    || case.activation.active_after_event
                    || case.activation.active_after_settle
                    || case.activation.has_been_active
            })
            .count();
        let verified_rows = evidence
            .cases
            .iter()
            .filter(|case| case.outcome == CaseOutcome::Verified)
            .count();
        let unsupported_rows = evidence
            .cases
            .iter()
            .filter(|case| case.outcome == CaseOutcome::Unsupported)
            .count();
        let presentation = matrix.presentation;
        let summary = format!(
            "windows-agentic-input-probe: passed; profile=ephemeral-udf+inprivate; extensions=construction-disabled; page_bridge=absent; observation=bounded-cdp-userGesture-false; os={}; engine={}; engine_version={}; presentation={presentation:?}; backends={}; cases={}; verified_rows={verified_rows}; unsupported_rows={unsupported_rows}; trusted_effect_events={trusted_effect_events}; trusted_focus_events={trusted_focus_events}; activated_rows={activated_rows}; focus_theft=0; retained_views=0",
            evidence.runtime.os_version.as_str(),
            evidence.runtime.engine.as_str(),
            evidence.runtime.engine_version.as_str(),
            matrix.backends.len(),
            evidence.cases.len(),
        );
        if !emit_reply(&mut sink, ProbeReply::RunCompleted(evidence)) {
            fail("machine evidence output failed");
        }
        match qualification {
            Ok(_) => eprintln!("{summary}"),
            Err(_) => fail("matrix failed its exact qualification contract"),
        }
    }

    fn emit_reply(sink: &mut EvidenceSink, reply: ProbeReply) -> bool {
        let response = ProbeResponse {
            protocol_version: PROBE_PROTOCOL_VERSION,
            request_id: 1,
            reply,
        };
        let Ok(encoded) = encode_response_line(&response) else {
            return false;
        };
        sink.emit(&encoded).is_ok()
    }

    fn fail_probe(message: &str, failure: ProbeFailure) -> ! {
        eprintln!(
            "windows-agentic-input-probe: {message}; code={:?}; stage={:?}; backend={:?}; case={:?}; retryable={}",
            failure.code, failure.stage, failure.backend, failure.case, failure.retryable
        );
        std::process::exit(2);
    }

    fn fail(message: &str) -> ! {
        eprintln!("windows-agentic-input-probe: {message}");
        std::process::exit(2);
    }
}
