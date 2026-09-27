//! Shared authenticated tree reads and their materialization regression tests.

pub(crate) use crate::tree_reader::{with_verified_tree_resource, TreeResourceError};

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use sha2::{Digest, Sha256};
    use std::cell::Cell;
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use zephium_extension_package::{CanonicalExtensionTreeIndex, PortableRelativePath};
    use zephium_private_fs::{ByteLimit, PrivateEntryName, SealedPrivateDirectory};

    use super::*;
    use crate::materialization::tree_writer::cleanup_tree_stage;
    use zephium_private_fs::{LockedPrivateNamespace, PrivateComponent};

    const MANIFEST: &[u8] = br#"{"manifest_version":3}"#;
    const NESTED: &[u8] = b"nested resource";

    #[test]
    fn resource_reader_drains_partial_results_and_handles_zero_and_nested_files() {
        let fixture = Fixture::new();
        let nested = PortableRelativePath::parse("nested/data.bin").unwrap();
        let observed =
            with_verified_tree_resource(fixture.root(), &fixture.index, &nested, |reader| {
                let mut first = [0_u8; 1];
                reader.read_exact(&mut first)?;
                Ok::<_, io::Error>(first[0])
            })
            .unwrap()
            .unwrap();
        assert_eq!(observed, NESTED[0]);

        let callback_error =
            with_verified_tree_resource(fixture.root(), &fixture.index, &nested, |reader| {
                let mut first = [0_u8; 1];
                reader.read_exact(&mut first).unwrap();
                Err::<(), _>("caller refused")
            })
            .unwrap();
        assert_eq!(callback_error, Err("caller refused"));

        let empty = PortableRelativePath::parse("empty.bin").unwrap();
        let length =
            with_verified_tree_resource(fixture.root(), &fixture.index, &empty, |reader| {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).map(|_| bytes.len())
            })
            .unwrap()
            .unwrap();
        assert_eq!(length, 0);

        let undeclared = PortableRelativePath::parse("not-declared.bin").unwrap();
        let callback_invoked = Cell::new(false);
        assert!(matches!(
            with_verified_tree_resource(fixture.root(), &fixture.index, &undeclared, |_reader| {
                callback_invoked.set(true);
                Ok::<_, io::Error>(())
            }),
            Err(TreeResourceError::NotDeclared)
        ));
        assert!(!callback_invoked.get());
    }

    #[test]
    fn resource_reader_rejects_missing_and_wrong_kind_leaves_before_callback() {
        for mutation in [LeafMutation::Missing, LeafMutation::Directory] {
            let fixture = Fixture::new();
            fixture.mutate_manifest(mutation);
            let callback_invoked = Cell::new(false);
            let manifest = PortableRelativePath::parse("manifest.json").unwrap();
            let result =
                with_verified_tree_resource(fixture.root(), &fixture.index, &manifest, |_reader| {
                    callback_invoked.set(true);
                    Ok::<_, io::Error>(())
                });
            assert!(matches!(
                result,
                Err(TreeResourceError::Missing | TreeResourceError::Mismatch)
            ));
            assert!(!callback_invoked.get());
        }
    }

    #[test]
    fn resource_reader_rejects_oversized_leaf_before_callback() {
        let fixture = Fixture::new();
        fixture.replace_manifest(&[MANIFEST, b"oversized"].concat());
        let callback_invoked = Cell::new(false);
        let manifest = PortableRelativePath::parse("manifest.json").unwrap();
        assert!(matches!(
            with_verified_tree_resource(fixture.root(), &fixture.index, &manifest, |_reader| {
                callback_invoked.set(true);
                Ok::<_, io::Error>(())
            },),
            Err(TreeResourceError::Mismatch)
        ));
        assert!(!callback_invoked.get());
    }

    #[test]
    fn resource_verification_outranks_callback_errors_for_short_and_hash_mismatched_bytes() {
        for replacement in [
            MANIFEST[..MANIFEST.len() - 1].to_vec(),
            vec![b'X'; MANIFEST.len()],
        ] {
            let fixture = Fixture::new();
            fixture.replace_manifest(&replacement);
            let callback_invoked = Cell::new(false);
            let manifest = PortableRelativePath::parse("manifest.json").unwrap();
            assert_eq!(
                with_verified_tree_resource(fixture.root(), &fixture.index, &manifest, |reader| {
                    callback_invoked.set(true);
                    let mut bytes = Vec::new();
                    reader.read_to_end(&mut bytes).unwrap();
                    Err::<(), _>("caller refused")
                },),
                Err(TreeResourceError::Mismatch)
            );
            assert!(callback_invoked.get());
        }
    }

    #[derive(Clone, Copy)]
    enum LeafMutation {
        Missing,
        Directory,
    }

    struct Fixture {
        _temporary: tempfile::TempDir,
        _namespace: LockedPrivateNamespace,
        trees: zephium_private_fs::PrivateDirectory,
        object: PrivateComponent,
        object_path: PathBuf,
        root: Option<Arc<SealedPrivateDirectory>>,
        index: CanonicalExtensionTreeIndex,
    }

    impl Fixture {
        fn new() -> Self {
            #[cfg(target_os = "macos")]
            let temporary = tempfile::tempdir_in("/private/tmp").unwrap();
            #[cfg(target_os = "linux")]
            let temporary = tempfile::tempdir_in("/tmp").unwrap();
            fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let namespace =
                LockedPrivateNamespace::open_or_create(temporary.path().join("repository"))
                    .unwrap();
            let trees = namespace
                .directory()
                .create_new_private_child(&PrivateComponent::new("trees").unwrap())
                .unwrap();
            let object = PrivateComponent::new("fixture.object").unwrap();
            let object_path = temporary
                .path()
                .join("repository")
                .join("trees")
                .join(object.as_str());
            let writable = trees.create_new_private_child(&object).unwrap();
            write_sealed_file(&writable, "empty.bin", b"");
            write_sealed_file(&writable, "manifest.json", MANIFEST);
            let nested_name = PrivateEntryName::new("nested").unwrap();
            let nested = writable.create_new_entry_child(&nested_name).unwrap();
            write_sealed_file(&nested, "data.bin", NESTED);
            drop(nested.seal().unwrap());
            let root = Arc::new(writable.seal().unwrap());
            let index = tree_index(&[
                ("empty.bin", b""),
                ("manifest.json", MANIFEST),
                ("nested/data.bin", NESTED),
            ]);
            Self {
                _temporary: temporary,
                _namespace: namespace,
                trees,
                object,
                object_path,
                root: Some(root),
                index,
            }
        }

        fn root(&self) -> &Arc<SealedPrivateDirectory> {
            self.root.as_ref().unwrap()
        }

        fn replace_manifest(&self, bytes: &[u8]) {
            let manifest = self.object_path.join("manifest.json");
            fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&manifest, bytes).unwrap();
            fs::set_permissions(&manifest, fs::Permissions::from_mode(0o400)).unwrap();
        }

        fn mutate_manifest(&self, mutation: LeafMutation) {
            let manifest = self.object_path.join("manifest.json");
            set_mode(&self.object_path, 0o700);
            fs::remove_file(&manifest).unwrap();
            if matches!(mutation, LeafMutation::Directory) {
                fs::create_dir(&manifest).unwrap();
                set_mode(&manifest, 0o500);
            }
            set_mode(&self.object_path, 0o500);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            drop(self.root.take());
            cleanup_tree_stage(&self.trees, &self.object).unwrap();
        }
    }

    fn write_sealed_file(
        directory: &zephium_private_fs::PrivateDirectory,
        name: &str,
        bytes: &[u8],
    ) {
        let name = PrivateEntryName::new(name).unwrap();
        directory
            .write_new_entry_synced(&name, bytes, ByteLimit::new(bytes.len().max(1)).unwrap())
            .unwrap();
        directory
            .seal_verified_entry_regular(&name)
            .unwrap()
            .unwrap();
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn tree_index(entries: &[(&str, &[u8])]) -> CanonicalExtensionTreeIndex {
        let mut encoded = String::from("{\"schema_version\":1,\"files\":[");
        for (position, (path, bytes)) in entries.iter().enumerate() {
            if position != 0 {
                encoded.push(',');
            }
            encoded.push_str(&format!(
                "{{\"path\":\"{path}\",\"length\":{},\"sha256\":\"{}\"}}",
                bytes.len(),
                lower_hex(Sha256::digest(bytes).into())
            ));
        }
        encoded.push_str("]}");
        CanonicalExtensionTreeIndex::parse_canonical(encoded.as_bytes()).unwrap()
    }

    fn lower_hex(bytes: [u8; 32]) -> String {
        use std::fmt::Write as _;

        let mut encoded = String::with_capacity(64);
        for byte in bytes {
            write!(encoded, "{byte:02x}").unwrap();
        }
        encoded
    }
}
