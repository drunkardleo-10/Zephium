//! Offline, content-free review of the four required physical Windows results.

use std::ffi::OsString;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use zephium_agentic::{
    decode_response_line, qualify_windows_probe_evidence, Platform, ProbeReply, RuntimeFingerprint,
    WindowsProbeAggregate, MAX_PROTOCOL_OUTPUT_BYTES, WINDOWS_PHYSICAL_REVIEW_MODES,
};

const REVIEW_SCHEMA_VERSION: u16 = 1;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct WindowsPhysicalReview {
    schema_version: u16,
    runtime: RuntimeFingerprint,
    modes: Vec<WindowsProbeAggregate>,
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
                "expected exactly --directory followed by the ignored result directory"
            }
            Self::Directory => "result directory must be one real non-symlink directory",
            Self::MissingRecord => "one required Windows result record is missing",
            Self::RecordType => "one Windows result must be a real non-symlink file",
            Self::RecordSize => "one Windows result violates the JSONL byte ceiling",
            Self::RecordRead => "one Windows result could not be read",
            Self::RecordProtocol => "one Windows result violates the closed response protocol",
            Self::RecordRejected => {
                "one Windows run returned a typed rejection instead of evidence"
            }
            Self::RecordIdentity => "one Windows result does not use the one-shot runner identity",
            Self::Qualification => "one Windows result failed its exact mode qualification",
            Self::RuntimeMismatch => "Windows result runtime fingerprints do not all match",
            Self::Output => "content-free Windows review output could not be encoded",
        }
    }
}

fn main() {
    if let Err(error) = run(std::env::args_os().skip(1).collect()) {
        eprintln!("windows-agentic-input-evidence-review: {}", error.message());
        std::process::exit(2);
    }
}

fn run(arguments: Vec<OsString>) -> Result<(), ReviewError> {
    let [flag, directory] = arguments.as_slice() else {
        return Err(ReviewError::Arguments);
    };
    if flag != "--directory" {
        return Err(ReviewError::Arguments);
    }
    let directory = PathBuf::from(directory);
    let metadata = std::fs::symlink_metadata(&directory).map_err(|_| ReviewError::Directory)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(ReviewError::Directory);
    }

    let mut runtime = None;
    let mut modes = Vec::with_capacity(WINDOWS_PHYSICAL_REVIEW_MODES.len());
    for mode in WINDOWS_PHYSICAL_REVIEW_MODES {
        let filename = mode
            .local_result_filename()
            .ok_or(ReviewError::MissingRecord)?;
        let bytes = read_record(&directory, filename)?;
        let response = decode_response_line(&bytes).map_err(|_| ReviewError::RecordProtocol)?;
        if response.request_id != 1 {
            return Err(ReviewError::RecordIdentity);
        }
        let ProbeReply::RunCompleted(evidence) = response.reply else {
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
            qualify_windows_probe_evidence(mode, &evidence)
                .map_err(|_| ReviewError::Qualification)?,
        );
    }

    let review = WindowsPhysicalReview {
        schema_version: REVIEW_SCHEMA_VERSION,
        runtime: runtime.ok_or(ReviewError::MissingRecord)?,
        modes,
    };
    let mut output = serde_json::to_vec(&review).map_err(|_| ReviewError::Output)?;
    output.push(b'\n');
    if output.len() > MAX_PROTOCOL_OUTPUT_BYTES {
        return Err(ReviewError::Output);
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(&output).map_err(|_| ReviewError::Output)?;
    stdout.flush().map_err(|_| ReviewError::Output)
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
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ReviewError::RecordType);
    }
    if metadata.len() == 0 || metadata.len() > MAX_PROTOCOL_OUTPUT_BYTES as u64 {
        return Err(ReviewError::RecordSize);
    }
    let file = std::fs::File::open(path).map_err(|_| ReviewError::RecordRead)?;
    let capacity = usize::try_from(metadata.len()).map_err(|_| ReviewError::RecordSize)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ReviewError::RecordRead)?;
    file.take((MAX_PROTOCOL_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ReviewError::RecordRead)?;
    if bytes.is_empty() || bytes.len() > MAX_PROTOCOL_OUTPUT_BYTES {
        return Err(ReviewError::RecordSize);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "zephium-windows-evidence-review-test-{}-{sequence}",
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

        let empty = directory.0.join("empty.jsonl");
        std::fs::write(&empty, []).expect("empty record");
        assert_eq!(
            read_record(&directory.0, "empty.jsonl"),
            Err(ReviewError::RecordSize)
        );

        let oversized = directory.0.join("oversized.jsonl");
        std::fs::write(&oversized, vec![b' '; MAX_PROTOCOL_OUTPUT_BYTES + 1])
            .expect("oversized record");
        assert_eq!(
            read_record(&directory.0, "oversized.jsonl"),
            Err(ReviewError::RecordSize)
        );

        let valid = directory.0.join("valid.jsonl");
        std::fs::write(&valid, b"{}\n").expect("bounded record");
        assert_eq!(
            read_record(&directory.0, "valid.jsonl"),
            Ok(b"{}\n".to_vec())
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
