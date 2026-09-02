//! Filesystem-shape checks for release-excluded physical evidence.

use std::fs::Metadata;

// Microsoft defines every symbolic link, junction, and volume mount point as
// a reparse point. `FileType::is_symlink` is not sufficient for all of those
// Windows path shapes.
#[cfg(any(test, target_os = "windows"))]
const WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// Returns whether metadata names one direct directory rather than a link.
///
/// On Windows this additionally rejects every reparse point, including NTFS
/// junctions and mounted folders that are not reported as symbolic links.
pub fn evidence_metadata_is_direct_directory(metadata: &Metadata) -> bool {
    metadata.file_type().is_dir()
        && !metadata.file_type().is_symlink()
        && metadata_has_no_windows_reparse_point(metadata)
}

/// Returns whether metadata names one direct regular file rather than a link.
///
/// On Windows this additionally rejects every reparse point, including files
/// controlled by an installed reparse provider.
pub fn evidence_metadata_is_direct_file(metadata: &Metadata) -> bool {
    metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && metadata_has_no_windows_reparse_point(metadata)
}

#[cfg(target_os = "windows")]
fn metadata_has_no_windows_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    windows_attributes_have_no_reparse_point(metadata.file_attributes())
}

#[cfg(not(target_os = "windows"))]
const fn metadata_has_no_windows_reparse_point(_metadata: &Metadata) -> bool {
    true
}

#[cfg(any(test, target_os = "windows"))]
const fn windows_attributes_have_no_reparse_point(attributes: u32) -> bool {
    attributes & WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "zephium-probe-evidence-path-test-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create isolated test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn direct_file_and_directory_kinds_are_distinct() {
        let directory = TestDirectory::new();
        let file = directory.path().join("evidence.jsonl");
        std::fs::write(&file, b"{}\n").expect("write evidence fixture");

        let directory_metadata =
            std::fs::symlink_metadata(directory.path()).expect("directory metadata");
        let file_metadata = std::fs::symlink_metadata(file).expect("file metadata");
        assert!(evidence_metadata_is_direct_directory(&directory_metadata));
        assert!(!evidence_metadata_is_direct_file(&directory_metadata));
        assert!(evidence_metadata_is_direct_file(&file_metadata));
        assert!(!evidence_metadata_is_direct_directory(&file_metadata));
    }

    #[test]
    fn windows_reparse_attribute_is_always_refused() {
        assert!(windows_attributes_have_no_reparse_point(0));
        assert!(windows_attributes_have_no_reparse_point(0x20));
        assert!(!windows_attributes_have_no_reparse_point(
            WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT,
        ));
        assert!(!windows_attributes_have_no_reparse_point(
            WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT | 0x10,
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_file_and_directory_metadata_are_refused() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new();
        let directory = root.path().join("direct");
        let file = root.path().join("direct.jsonl");
        std::fs::create_dir(&directory).expect("direct directory");
        std::fs::write(&file, b"{}\n").expect("direct file");
        let directory_link = root.path().join("directory-link");
        let file_link = root.path().join("file-link");
        symlink(&directory, &directory_link).expect("directory link");
        symlink(&file, &file_link).expect("file link");

        let directory_metadata =
            std::fs::symlink_metadata(directory_link).expect("directory-link metadata");
        let file_metadata = std::fs::symlink_metadata(file_link).expect("file-link metadata");
        assert!(!evidence_metadata_is_direct_directory(&directory_metadata));
        assert!(!evidence_metadata_is_direct_file(&file_metadata));
    }
}
