#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::{Cursor, Error as IoError, Read};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use zephium_private_fs::PrivateComponent;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use zephium_private_fs::{
    ByteLimit, PrivateChildKind, PrivateEntryName, StreamingFileLength, StreamingWriteError,
    MAX_IN_MEMORY_FILE_BYTES,
};
use zephium_private_fs::{LockedPrivateNamespace, PrivateFsError};

#[cfg(any(target_os = "macos", target_os = "linux"))]
const LOCK_NAME: &str = ".zephium-private-fs-lock-v1";
#[cfg(any(target_os = "macos", target_os = "linux"))]
const LOCK_STAGING_NAME: &str = ".zephium-private-fs-lock-staging-v1";
#[cfg(any(target_os = "macos", target_os = "linux"))]
const LOCK_MARKER: &[u8] = b"zephium-private-fs\nlock-format=1\n";

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn component(value: &str) -> PrivateComponent {
    PrivateComponent::new(value).unwrap()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn entry_name(value: &str) -> PrivateEntryName {
    PrivateEntryName::new(value).unwrap()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn private_parent() -> tempfile::TempDir {
    // macOS exposes `/var` through a system symlink, while this boundary
    // intentionally rejects symlinks in the complete ancestor chain.
    let parent = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    parent
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn namespace_root(parent: &Path) -> PathBuf {
    parent.join("private-boundary")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn open(root: &Path) -> LockedPrivateNamespace {
    LockedPrivateNamespace::open_or_create(root).unwrap()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn create_private_root(root: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::create_dir(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_private_file(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn establish_canonical_lock(root: &Path) {
    drop(open(root));
    assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct ErrorAtEof {
    bytes: Cursor<Vec<u8>>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl ErrorAtEof {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Cursor::new(bytes),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Read for ErrorAtEof {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes.position() < self.bytes.get_ref().len() as u64 {
            self.bytes.read(buffer)
        } else {
            Err(IoError::other("injected source validation failure"))
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rolling_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn creates_only_the_private_leaf_and_holds_a_stable_identity() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let first = namespace.directory().identity();

    assert!(root.is_dir());
    assert_eq!(first, namespace.directory().identity());
    assert!(namespace.directory().list_components(1).unwrap().is_empty());
    assert_eq!(
        namespace.directory().regular_exists(&component(LOCK_NAME)),
        Err(PrivateFsError::ReservedComponent)
    );
    assert_eq!(
        namespace
            .directory()
            .regular_exists(&component(LOCK_STAGING_NAME)),
        Err(PrivateFsError::ReservedComponent)
    );

    let missing_parent = parent.path().join("missing").join("leaf");
    assert_eq!(
        LockedPrivateNamespace::open_or_create(missing_parent)
            .err()
            .unwrap(),
        PrivateFsError::Unsafe
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn exact_lock_rejects_a_second_owner_until_drop() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let first = open(&root);
    assert_eq!(
        LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
        PrivateFsError::LockUnavailable
    );
    drop(first);
    assert!(LockedPrivateNamespace::open_or_create(&root).is_ok());
    assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn concurrent_initialization_has_one_authoritative_owner() {
    use std::sync::{Arc, Barrier};

    let parent = private_parent();
    let root = namespace_root(parent.path());
    create_private_root(&root);
    let start = Arc::new(Barrier::new(2));
    let hold_owner = Arc::new(Barrier::new(2));
    let outcomes = std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|_| {
                let start = Arc::clone(&start);
                let hold_owner = Arc::clone(&hold_owner);
                let root = root.clone();
                scope.spawn(move || {
                    start.wait();
                    let outcome = LockedPrivateNamespace::open_or_create(root);
                    hold_owner.wait();
                    outcome.map(drop)
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count() <= 1,
        "multiple initialization owners: {outcomes:?}"
    );
    assert!(outcomes.iter().all(|outcome| {
        matches!(
            outcome,
            Ok(()) | Err(PrivateFsError::LockUnavailable | PrivateFsError::SettlementUnknown)
        )
    }));
    assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
    let recovered = LockedPrivateNamespace::open_or_create(&root).unwrap();
    assert!(!root.join(LOCK_STAGING_NAME).exists());
    drop(recovered);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn canonical_lock_rejects_empty_every_strict_prefix_and_arbitrary_payload() {
    let mut hostile = (0..LOCK_MARKER.len())
        .map(|length| LOCK_MARKER[..length].to_vec())
        .collect::<Vec<_>>();
    hostile.extend([
        b"unrelated payload".to_vec(),
        [LOCK_MARKER, b"extra"].concat(),
    ]);

    for (index, payload) in hostile.into_iter().enumerate() {
        let parent = private_parent();
        let root = parent.path().join(format!("canonical-hostile-{index}"));
        create_private_root(&root);
        let lock = root.join(LOCK_NAME);
        write_private_file(&lock, &payload);

        assert_eq!(
            LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
            PrivateFsError::Unsafe,
            "admitted canonical payload {payload:?}"
        );
        assert_eq!(fs::read(lock).unwrap(), payload);
        assert!(!root.join(LOCK_STAGING_NAME).exists());
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn exact_staging_crash_residue_is_published_without_rewriting() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    create_private_root(&root);
    let staging = root.join(LOCK_STAGING_NAME);
    write_private_file(&staging, LOCK_MARKER);

    let namespace = LockedPrivateNamespace::open_or_create(&root).unwrap();
    assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
    assert!(!staging.exists());
    drop(namespace);
    assert!(LockedPrivateNamespace::open_or_create(root).is_ok());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn staging_crash_residue_recovers_empty_and_every_strict_marker_prefix() {
    for (index, payload) in (0..LOCK_MARKER.len())
        .map(|length| LOCK_MARKER[..length].to_vec())
        .enumerate()
    {
        let parent = private_parent();
        let root = parent.path().join(format!("staging-prefix-{index}"));
        create_private_root(&root);
        let staging = root.join(LOCK_STAGING_NAME);
        write_private_file(&staging, &payload);

        let namespace = LockedPrivateNamespace::open_or_create(&root).unwrap();
        assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
        assert!(!staging.exists());
        drop(namespace);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn staging_crash_residue_rejects_unknown_payloads_unchanged() {
    for (index, payload) in [
        b"unrelated payload".to_vec(),
        [LOCK_MARKER, b"extra"].concat(),
    ]
    .into_iter()
    .enumerate()
    {
        let parent = private_parent();
        let root = parent.path().join(format!("staging-hostile-{index}"));
        create_private_root(&root);
        let staging = root.join(LOCK_STAGING_NAME);
        write_private_file(&staging, &payload);

        assert_eq!(
            LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
            PrivateFsError::Unsafe
        );
        assert_eq!(fs::read(staging).unwrap(), payload);
        assert!(!root.join(LOCK_NAME).exists());
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn canonical_admission_durably_cleans_exact_and_prefix_staging_residue() {
    for (index, payload) in (0..=LOCK_MARKER.len())
        .map(|length| LOCK_MARKER[..length].to_vec())
        .enumerate()
    {
        let parent = private_parent();
        let root = parent
            .path()
            .join(format!("canonical-staging-cleanup-{index}"));
        establish_canonical_lock(&root);
        let staging = root.join(LOCK_STAGING_NAME);
        write_private_file(&staging, &payload);

        let namespace = LockedPrivateNamespace::open_or_create(&root).unwrap();
        assert!(!staging.exists());
        assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
        assert!(namespace.directory().list_components(1).unwrap().is_empty());
        drop(namespace);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn canonical_admission_rejects_unknown_staging_payload_unchanged() {
    for (index, payload) in [
        b"unrelated payload".to_vec(),
        [LOCK_MARKER, b"extra"].concat(),
    ]
    .into_iter()
    .enumerate()
    {
        let parent = private_parent();
        let root = parent
            .path()
            .join(format!("canonical-staging-hostile-{index}"));
        establish_canonical_lock(&root);
        let staging = root.join(LOCK_STAGING_NAME);
        write_private_file(&staging, &payload);

        assert_eq!(
            LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
            PrivateFsError::Unsafe
        );
        assert_eq!(fs::read(staging).unwrap(), payload);
        assert_eq!(fs::read(root.join(LOCK_NAME)).unwrap(), LOCK_MARKER);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn canonical_admission_refuses_busy_staging_then_recovers_after_release() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    establish_canonical_lock(&root);
    let staging = root.join(LOCK_STAGING_NAME);
    write_private_file(&staging, LOCK_MARKER);
    let staging_owner = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&staging)
        .unwrap();
    rustix::fs::flock(
        &staging_owner,
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    )
    .unwrap();

    assert_eq!(
        LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
        PrivateFsError::LockUnavailable
    );
    assert_eq!(fs::read(&staging).unwrap(), LOCK_MARKER);
    drop(staging_owner);

    let namespace = LockedPrivateNamespace::open_or_create(&root).unwrap();
    assert!(!staging.exists());
    drop(namespace);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn live_inventory_refuses_uninspected_staging_instead_of_hiding_it() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let staging = root.join(LOCK_STAGING_NAME);
    write_private_file(&staging, b"unknown post-admission residue");

    assert_eq!(
        namespace.directory().list_components(8),
        Err(PrivateFsError::Unsafe)
    );
    assert_eq!(
        fs::read(staging).unwrap(),
        b"unknown post-admission residue"
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn bounded_files_round_trip_replace_publish_and_remove() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let limit = ByteLimit::new(32).unwrap();
    let stage = component("stage.bin");
    let next = component("next.bin");
    let current = component("current.bin");

    let staged_identity = directory.write_new_synced(&stage, b"first", limit).unwrap();
    assert_eq!(
        directory.read_bounded_regular(&stage, limit).unwrap(),
        Some(b"first".to_vec())
    );
    assert_eq!(
        directory
            .publish_noreplace_verified_regular(&stage, &current)
            .unwrap(),
        staged_identity
    );
    assert!(!directory.regular_exists(&stage).unwrap());

    directory.write_new_synced(&next, b"second", limit).unwrap();
    assert_eq!(
        directory.publish_noreplace_verified_regular(&next, &current),
        Err(PrivateFsError::AlreadyExists)
    );
    assert!(directory.regular_exists(&next).unwrap());
    let next_identity = directory.regular_identity(&next).unwrap().unwrap();
    assert_eq!(
        directory.replace_verified_regular(&next, &current).unwrap(),
        next_identity
    );
    directory.sync().unwrap();
    assert_eq!(
        directory.read_bounded_regular(&current, limit).unwrap(),
        Some(b"second".to_vec())
    );
    assert!(directory.remove_verified_regular(&current).unwrap());
    assert!(!directory.remove_verified_regular(&current).unwrap());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn enforces_memory_and_inventory_bounds_before_allocation() {
    assert_eq!(ByteLimit::new(0), Err(PrivateFsError::BoundExceeded));
    assert_eq!(
        ByteLimit::new(MAX_IN_MEMORY_FILE_BYTES + 1),
        Err(PrivateFsError::BoundExceeded)
    );

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let file = component("value.bin");
    directory
        .write_new_synced(&file, b"1234", ByteLimit::new(4).unwrap())
        .unwrap();
    directory
        .write_new_synced(&component("another.bin"), b"x", ByteLimit::new(4).unwrap())
        .unwrap();
    assert_eq!(
        directory.read_bounded_regular(&file, ByteLimit::new(3).unwrap()),
        Err(PrivateFsError::BoundExceeded)
    );
    assert_eq!(
        directory.list_components(1),
        Err(PrivateFsError::BoundExceeded)
    );
    assert_eq!(
        directory.list_components(0),
        Err(PrivateFsError::BoundExceeded)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn creates_and_admits_private_child_directories() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let child = namespace
        .directory()
        .create_private_child(&component("objects"))
        .unwrap();
    let identity = child.identity();
    let reopened = namespace
        .directory()
        .create_private_child(&component("objects"))
        .unwrap();
    assert_eq!(identity, reopened.identity());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn control_children_distinguish_open_create_or_open_and_create_new() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let child_name = component("objects");

    assert_eq!(
        directory.open_private_child(&child_name).err(),
        Some(PrivateFsError::NotFound)
    );
    assert!(!root.join(child_name.as_str()).exists());

    let created = directory.create_new_private_child(&child_name).unwrap();
    let identity = created.identity();
    assert_eq!(
        directory
            .open_private_child(&child_name)
            .unwrap()
            .identity(),
        identity
    );
    assert_eq!(
        directory
            .create_private_child(&child_name)
            .unwrap()
            .identity(),
        identity
    );
    assert_eq!(
        directory.create_new_private_child(&child_name).err(),
        Some(PrivateFsError::AlreadyExists)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn entry_children_distinguish_open_create_modes_and_hostile_existing_nodes() {
    use std::os::unix::fs::PermissionsExt;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let child_name = entry_name("Objects With CASE");

    assert_eq!(
        directory.open_entry_child(&child_name).err(),
        Some(PrivateFsError::NotFound)
    );
    assert!(!root.join(child_name.as_str()).exists());

    let created = directory.create_new_entry_child(&child_name).unwrap();
    let identity = created.identity();
    assert_eq!(
        directory.open_entry_child(&child_name).unwrap().identity(),
        identity
    );
    assert_eq!(
        directory
            .create_entry_child(&child_name)
            .unwrap()
            .identity(),
        identity
    );
    assert_eq!(
        directory.create_new_entry_child(&child_name).err(),
        Some(PrivateFsError::AlreadyExists)
    );

    let hostile_file = entry_name("Existing File");
    write_private_file(&root.join(hostile_file.as_str()), b"not a directory");
    assert_eq!(
        directory.create_new_entry_child(&hostile_file).err(),
        Some(PrivateFsError::Unsafe)
    );

    let public_directory = entry_name("Public Directory");
    fs::create_dir(root.join(public_directory.as_str())).unwrap();
    fs::set_permissions(
        root.join(public_directory.as_str()),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert_eq!(
        directory.create_new_entry_child(&public_directory).err(),
        Some(PrivateFsError::Unsafe)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn case_preserving_entry_files_list_inspect_and_round_trip_with_bounds() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let payload_name = entry_name("Payload Files");
    let payload = directory.create_new_entry_child(&payload_name).unwrap();
    let manifest = entry_name("Manifest.JSON");
    let script = entry_name("A Script File.js");
    let limit = ByteLimit::new(32).unwrap();

    let manifest_identity = payload
        .write_new_entry_synced(&manifest, b"manifest", limit)
        .unwrap();
    payload
        .write_new_entry_synced(&script, b"script", limit)
        .unwrap();
    assert!(payload.entry_regular_exists(&manifest).unwrap());
    assert_eq!(
        payload.entry_regular_identity(&manifest).unwrap(),
        Some(manifest_identity)
    );
    assert_eq!(
        payload.inspect_entry(&manifest).unwrap(),
        Some(PrivateChildKind::RegularFile(manifest_identity))
    );
    assert_eq!(
        directory.inspect_entry(&payload_name).unwrap(),
        Some(PrivateChildKind::Directory(payload.identity()))
    );
    assert_eq!(
        payload
            .read_bounded_entry_regular(&manifest, limit)
            .unwrap(),
        Some(b"manifest".to_vec())
    );
    assert_eq!(
        payload.read_bounded_entry_regular(&manifest, ByteLimit::new(7).unwrap()),
        Err(PrivateFsError::BoundExceeded)
    );
    assert_eq!(
        payload.write_new_entry_synced(
            &entry_name("Too Large"),
            b"12345",
            ByteLimit::new(4).unwrap()
        ),
        Err(PrivateFsError::BoundExceeded)
    );
    assert!(!root.join("Payload Files/Too Large").exists());

    assert_eq!(
        payload.list_entry_names(8).unwrap(),
        vec![script.clone(), manifest.clone()]
    );
    assert_eq!(
        payload.list_entry_names(1),
        Err(PrivateFsError::BoundExceeded)
    );
    assert_eq!(
        payload.list_entry_names(0),
        Err(PrivateFsError::BoundExceeded)
    );
    assert_eq!(
        payload.list_entry_names(4_097),
        Err(PrivateFsError::BoundExceeded)
    );
    assert_eq!(directory.list_entry_names(8).unwrap(), vec![payload_name]);

    let missing = entry_name("Missing File");
    assert!(!payload.entry_regular_exists(&missing).unwrap());
    assert_eq!(payload.entry_regular_identity(&missing).unwrap(), None);
    assert_eq!(payload.inspect_entry(&missing).unwrap(), None);
    assert_eq!(
        payload.read_bounded_entry_regular(&missing, limit).unwrap(),
        None
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn streaming_entry_write_handles_zero_fixed_chunks_and_hash_style_reads() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();

    for (index, length) in [0_usize, 64 * 1024, 128 * 1024, 128 * 1024 + 1]
        .into_iter()
        .enumerate()
    {
        let name = entry_name(&format!("Payload {index}.BIN"));
        let bytes = vec![u8::try_from(index + 1).unwrap(); length];
        let mut source = Cursor::new(bytes.as_slice());
        let identity = directory
            .write_new_entry_from_reader(
                &name,
                &mut source,
                StreamingFileLength::new(u64::try_from(length).unwrap()).unwrap(),
            )
            .unwrap();
        assert_eq!(
            directory.entry_regular_identity(&name).unwrap(),
            Some(identity)
        );
        assert_eq!(
            fs::metadata(root.join(name.as_str())).unwrap().len(),
            u64::try_from(length).unwrap()
        );

        let consumed = directory
            .with_bounded_entry_regular_reader(
                &name,
                ByteLimit::new(length.max(1)).unwrap(),
                |reader| {
                    let mut buffer = [0_u8; 8 * 1024];
                    let mut count = 0_usize;
                    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
                    loop {
                        let read = reader.read(&mut buffer)?;
                        if read == 0 {
                            break;
                        }
                        count += read;
                        for byte in &buffer[..read] {
                            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
                        }
                    }
                    Ok::<_, std::io::Error>((count, hash))
                },
            )
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(consumed, (length, rolling_hash(&bytes)));
        assert!(directory.remove_verified_entry_regular(&name).unwrap());
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn streaming_source_rejections_prove_cleanup_and_leave_the_lease_reusable() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let cases: Vec<(Box<dyn Read>, u64, StreamingWriteError)> = vec![
        (
            Box::new(Cursor::new(b"abc".to_vec())),
            4,
            StreamingWriteError::SourceTooShort,
        ),
        (
            Box::new(Cursor::new(b"abcde".to_vec())),
            4,
            StreamingWriteError::SourceTooLong,
        ),
        (
            Box::new(ErrorAtEof::new(Vec::new())),
            4,
            StreamingWriteError::SourceRead,
        ),
        (
            Box::new(ErrorAtEof::new(b"ab".to_vec())),
            4,
            StreamingWriteError::SourceRead,
        ),
        (
            Box::new(ErrorAtEof::new(b"abcd".to_vec())),
            4,
            StreamingWriteError::SourceRead,
        ),
    ];

    for (index, (mut source, expected, error)) in cases.into_iter().enumerate() {
        let name = entry_name(&format!("Rejected Source {index}.bin"));
        assert_eq!(
            directory.write_new_entry_from_reader(
                &name,
                &mut source,
                StreamingFileLength::new(expected).unwrap(),
            ),
            Err(error)
        );
        assert!(!root.join(name.as_str()).exists());
        assert!(!directory.entry_regular_exists(&name).unwrap());

        let mut empty = Cursor::new([]);
        directory
            .write_new_entry_from_reader(&name, &mut empty, StreamingFileLength::ZERO)
            .unwrap();
        assert!(directory.remove_verified_entry_regular(&name).unwrap());
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn streaming_writer_refuses_existing_and_hostile_nodes_without_consuming_them() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let mut empty = Cursor::new([]);

    let regular = entry_name("Existing Regular.bin");
    write_private_file(&root.join(regular.as_str()), b"keep");
    assert_eq!(
        directory.write_new_entry_from_reader(&regular, &mut empty, StreamingFileLength::ZERO),
        Err(StreamingWriteError::Filesystem(
            PrivateFsError::AlreadyExists
        ))
    );
    assert_eq!(fs::read(root.join(regular.as_str())).unwrap(), b"keep");

    let child = entry_name("Existing Directory");
    fs::create_dir(root.join(child.as_str())).unwrap();
    fs::set_permissions(root.join(child.as_str()), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        directory.write_new_entry_from_reader(&child, &mut empty, StreamingFileLength::ZERO),
        Err(StreamingWriteError::Filesystem(PrivateFsError::Unsafe))
    );

    let linked = entry_name("Existing Link");
    symlink("missing", root.join(linked.as_str())).unwrap();
    assert_eq!(
        directory.write_new_entry_from_reader(&linked, &mut empty, StreamingFileLength::ZERO),
        Err(StreamingWriteError::Filesystem(PrivateFsError::Unsafe))
    );

    let fifo = entry_name("Existing FIFO");
    assert!(std::process::Command::new("mkfifo")
        .arg(root.join(fifo.as_str()))
        .status()
        .unwrap()
        .success());
    assert_eq!(
        directory.write_new_entry_from_reader(&fifo, &mut empty, StreamingFileLength::ZERO),
        Err(StreamingWriteError::Filesystem(PrivateFsError::Unsafe))
    );

    let exact_alias = entry_name("alias.bin");
    write_private_file(&root.join("Alias.BIN"), b"alias");
    if root.join(exact_alias.as_str()).is_file() {
        assert_eq!(
            directory.write_new_entry_from_reader(
                &exact_alias,
                &mut empty,
                StreamingFileLength::ZERO,
            ),
            Err(StreamingWriteError::Filesystem(PrivateFsError::Unsafe))
        );
        assert_eq!(fs::read(root.join("Alias.BIN")).unwrap(), b"alias");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn bounded_reader_preserves_callback_errors_and_enforces_the_bound_for_both_name_types() {
    use std::cell::Cell;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let control = component("control.bin");
    let entry = entry_name("Payload.bin");
    directory
        .write_new_synced(&control, b"control", ByteLimit::new(16).unwrap())
        .unwrap();
    directory
        .write_new_entry_synced(&entry, b"payload", ByteLimit::new(16).unwrap())
        .unwrap();

    let control_bytes = directory
        .with_bounded_regular_reader(&control, ByteLimit::new(16).unwrap(), |reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        })
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(control_bytes, b"control");

    let callback_error = directory
        .with_bounded_entry_regular_reader(&entry, ByteLimit::new(16).unwrap(), |reader| {
            let mut byte = [0_u8; 1];
            reader.read_exact(&mut byte).unwrap();
            Err::<(), _>("digest mismatch")
        })
        .unwrap()
        .unwrap();
    assert_eq!(callback_error, Err("digest mismatch"));
    assert!(directory.entry_regular_exists(&entry).unwrap());

    let called = Cell::new(false);
    assert_eq!(
        directory.with_bounded_entry_regular_reader(
            &entry,
            ByteLimit::new(3).unwrap(),
            |_reader| {
                called.set(true);
                Ok::<_, ()>(())
            },
        ),
        Err(PrivateFsError::BoundExceeded)
    );
    assert!(!called.get());

    let missing = entry_name("Missing.bin");
    assert_eq!(
        directory.with_bounded_entry_regular_reader(
            &missing,
            ByteLimit::new(16).unwrap(),
            |_reader| Ok::<_, ()>(()),
        ),
        Ok(None)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn bounded_reader_revalidates_the_node_even_when_the_callback_returns_an_error() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let entry = entry_name("Mutable Payload.bin");
    directory
        .write_new_entry_synced(&entry, b"payload", ByteLimit::new(16).unwrap())
        .unwrap();

    assert_eq!(
        directory.with_bounded_entry_regular_reader(
            &entry,
            ByteLimit::new(16).unwrap(),
            |_reader| {
                fs::remove_file(root.join(entry.as_str())).unwrap();
                Err::<(), _>("caller error")
            },
        ),
        Err(PrivateFsError::IdentityAmbiguous)
    );
    assert_eq!(
        directory.list_entry_names(8),
        Err(PrivateFsError::Quarantined)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn verified_entry_removal_is_exact_durable_and_optional() {
    use std::os::unix::fs::symlink;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let entry = entry_name("Remove This Payload.bin");
    directory
        .write_new_entry_synced(&entry, b"payload", ByteLimit::new(16).unwrap())
        .unwrap();

    assert!(directory.remove_verified_entry_regular(&entry).unwrap());
    assert!(!root.join(entry.as_str()).exists());
    assert!(!directory.remove_verified_entry_regular(&entry).unwrap());

    let linked = entry_name("Do Not Remove Link");
    symlink("missing", root.join(linked.as_str())).unwrap();
    assert_eq!(
        directory.remove_verified_entry_regular(&linked),
        Err(PrivateFsError::Unsafe)
    );
    assert!(fs::symlink_metadata(root.join(linked.as_str()))
        .unwrap()
        .file_type()
        .is_symlink());

    let alias = entry_name("alias.bin");
    write_private_file(&root.join("Alias.BIN"), b"keep");
    if root.join(alias.as_str()).is_file() {
        assert_eq!(
            directory.remove_verified_entry_regular(&alias),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(fs::read(root.join("Alias.BIN")).unwrap(), b"keep");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn entry_operations_never_admit_a_case_alias_on_case_insensitive_storage() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let exact_directory = entry_name("Case Sensitive Folder");
    let alias_directory = entry_name("case sensitive folder");
    let child = directory.create_new_entry_child(&exact_directory).unwrap();
    let exact_file = entry_name("Manifest.JSON");
    let alias_file = entry_name("manifest.json");
    let limit = ByteLimit::new(32).unwrap();
    child
        .write_new_entry_synced(&exact_file, b"manifest", limit)
        .unwrap();

    let aliases_resolve = root.join(alias_directory.as_str()).is_dir()
        && root
            .join(exact_directory.as_str())
            .join(alias_file.as_str())
            .is_file();
    if aliases_resolve {
        assert_eq!(
            directory.open_entry_child(&alias_directory).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(
            directory.create_entry_child(&alias_directory).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(
            directory.create_new_entry_child(&alias_directory).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(
            directory.inspect_entry(&alias_directory),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.entry_regular_exists(&alias_file),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.entry_regular_identity(&alias_file),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.read_bounded_entry_regular(&alias_file, limit),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.inspect_entry(&alias_file),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.write_new_entry_synced(&alias_file, b"alias", limit),
            Err(PrivateFsError::Unsafe)
        );
    } else {
        assert_eq!(
            directory.open_entry_child(&alias_directory).err(),
            Some(PrivateFsError::NotFound)
        );
        assert_eq!(directory.inspect_entry(&alias_directory).unwrap(), None);
        assert!(!child.entry_regular_exists(&alias_file).unwrap());
        assert_eq!(child.entry_regular_identity(&alias_file).unwrap(), None);
        assert_eq!(
            child
                .read_bounded_entry_regular(&alias_file, limit)
                .unwrap(),
            None
        );
        assert_eq!(child.inspect_entry(&alias_file).unwrap(), None);
    }
    assert_eq!(
        directory.list_entry_names(8).unwrap(),
        vec![exact_directory]
    );
    assert_eq!(child.list_entry_names(8).unwrap(), vec![exact_file]);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn control_operations_never_admit_a_case_alias_on_case_insensitive_storage() {
    use std::os::unix::fs::PermissionsExt;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let exact_directory = component("controlcase");
    let alias_directory = "ControlCase";
    fs::create_dir(root.join(alias_directory)).unwrap();
    fs::set_permissions(
        root.join(alias_directory),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();

    let payload = component("payload");
    let child = directory.create_new_private_child(&payload).unwrap();
    let exact_file = component("state.bin");
    let alias_file = "State.BIN";
    let limit = ByteLimit::new(32).unwrap();
    write_private_file(&root.join(payload.as_str()).join(alias_file), b"state");

    let aliases_resolve = root.join(exact_directory.as_str()).is_dir()
        && root
            .join(payload.as_str())
            .join(exact_file.as_str())
            .is_file();
    if aliases_resolve {
        assert_eq!(
            directory.open_private_child(&exact_directory).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(
            directory.create_private_child(&exact_directory).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(
            directory.create_new_private_child(&exact_directory).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.regular_exists(&exact_file),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.regular_identity(&exact_file),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.read_bounded_regular(&exact_file, limit),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            child.write_new_synced(&exact_file, b"alias", limit),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(directory.list_components(8), Err(PrivateFsError::Unsafe));
        assert_eq!(child.list_components(8), Err(PrivateFsError::Unsafe));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn rejects_symlinks_hardlinks_and_nonprivate_modes() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let limit = ByteLimit::new(32).unwrap();

    symlink("missing", root.join("linked.bin")).unwrap();
    assert_eq!(
        directory.read_bounded_regular(&component("linked.bin"), limit),
        Err(PrivateFsError::Unsafe)
    );

    fs::write(root.join("primary.bin"), b"value").unwrap();
    fs::set_permissions(root.join("primary.bin"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(root.join("primary.bin"), root.join("alias.bin")).unwrap();
    assert_eq!(
        directory.read_bounded_regular(&component("primary.bin"), limit),
        Err(PrivateFsError::Unsafe)
    );

    fs::write(root.join("public.bin"), b"value").unwrap();
    fs::set_permissions(root.join("public.bin"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        directory.read_bounded_regular(&component("public.bin"), limit),
        Err(PrivateFsError::Unsafe)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn entry_inspection_rejects_links_special_nodes_and_unsafe_modes_without_blocking() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::os::unix::net::UnixListener;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();

    let linked = entry_name("Linked Entry");
    symlink("missing", root.join(linked.as_str())).unwrap();
    assert_eq!(
        directory.inspect_entry(&linked),
        Err(PrivateFsError::Unsafe)
    );

    let primary = entry_name("Primary Entry");
    let alias = entry_name("Alias Entry");
    write_private_file(&root.join(primary.as_str()), b"value");
    fs::hard_link(root.join(primary.as_str()), root.join(alias.as_str())).unwrap();
    assert_eq!(
        directory.inspect_entry(&primary),
        Err(PrivateFsError::Unsafe)
    );
    assert_eq!(directory.inspect_entry(&alias), Err(PrivateFsError::Unsafe));

    let public_file = entry_name("Public Entry");
    fs::write(root.join(public_file.as_str()), b"value").unwrap();
    fs::set_permissions(
        root.join(public_file.as_str()),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(
        directory.inspect_entry(&public_file),
        Err(PrivateFsError::Unsafe)
    );

    let fifo = entry_name("Hostile FIFO");
    let status = std::process::Command::new("mkfifo")
        .arg(root.join(fifo.as_str()))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(directory.inspect_entry(&fifo), Err(PrivateFsError::Unsafe));

    let socket = entry_name("Hostile Socket");
    let _listener = match UnixListener::bind(root.join(socket.as_str())) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to create hostile socket fixture: {error}"),
    };
    assert_eq!(
        directory.inspect_entry(&socket),
        Err(PrivateFsError::Unsafe)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn rejects_root_permission_and_identity_substitution() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let parent = private_parent();
    let unsafe_root = parent.path().join("unsafe-root");
    fs::create_dir(&unsafe_root).unwrap();
    fs::set_permissions(&unsafe_root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(LockedPrivateNamespace::open_or_create(&unsafe_root).is_err());

    let symlink_root = parent.path().join("symlink-root");
    symlink(&unsafe_root, &symlink_root).unwrap();
    assert!(LockedPrivateNamespace::open_or_create(&symlink_root).is_err());

    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let moved = parent.path().join("moved-root");
    fs::rename(&root, &moved).unwrap();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("decoy.bin"), b"must-survive").unwrap();
    fs::set_permissions(root.join("decoy.bin"), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        namespace
            .directory()
            .regular_exists(&component("missing.bin")),
        Err(PrivateFsError::IdentityAmbiguous)
    );
    assert_eq!(
        namespace
            .directory()
            .remove_verified_regular(&component("decoy.bin")),
        Err(PrivateFsError::Quarantined)
    );
    assert_eq!(fs::read(root.join("decoy.bin")).unwrap(), b"must-survive");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn root_lock_component_is_reserved_from_every_caller_role() {
    use std::cell::Cell;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let stage = component("stage.bin");
    let limit = ByteLimit::new(16).unwrap();

    directory.write_new_synced(&stage, b"stage", limit).unwrap();
    for reserved in [component(LOCK_NAME), component(LOCK_STAGING_NAME)] {
        assert_eq!(
            directory.create_private_child(&reserved).err().unwrap(),
            PrivateFsError::ReservedComponent
        );
        assert_eq!(
            directory.write_new_synced(&reserved, b"x", limit),
            Err(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.remove_verified_regular(&reserved),
            Err(PrivateFsError::ReservedComponent)
        );
        for result in [
            directory.replace_verified_regular(&stage, &reserved),
            directory.replace_verified_regular(&reserved, &stage),
            directory.publish_noreplace_verified_regular(&stage, &reserved),
            directory.publish_noreplace_verified_regular(&reserved, &stage),
        ] {
            assert_eq!(result, Err(PrivateFsError::ReservedComponent));
        }
        assert_eq!(
            directory.read_bounded_regular(&reserved, limit),
            Err(PrivateFsError::ReservedComponent)
        );
        let callback_called = Cell::new(false);
        assert_eq!(
            directory.with_bounded_regular_reader(&reserved, limit, |_reader| {
                callback_called.set(true);
                Ok::<_, ()>(())
            }),
            Err(PrivateFsError::ReservedComponent)
        );
        assert!(!callback_called.get());
    }
    assert_eq!(directory.list_components(8).unwrap(), vec![stage]);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn lock_case_aliases_are_never_admitted_as_canonical_entries() {
    for reserved in [LOCK_NAME, LOCK_STAGING_NAME] {
        let parent = private_parent();
        let root = namespace_root(parent.path());
        create_private_root(&root);

        let alias = reserved.to_ascii_uppercase();
        let bytes = if reserved == LOCK_NAME {
            LOCK_MARKER
        } else {
            &LOCK_MARKER[..LOCK_MARKER.len() / 2]
        };
        write_private_file(&root.join(&alias), bytes);

        // This live assertion is meaningful only when the backing filesystem
        // resolves differently cased names to the same directory entry.
        if !root.join(reserved).exists() {
            continue;
        }
        assert_eq!(
            LockedPrivateNamespace::open_or_create(&root).err(),
            Some(PrivateFsError::Unsafe)
        );
        assert_eq!(fs::read(root.join(alias)).unwrap(), bytes);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn post_activation_lock_aliases_are_visible_hostile_inventory() {
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;

    for reserved in [LOCK_NAME, LOCK_STAGING_NAME] {
        let parent = private_parent();
        let root = namespace_root(parent.path());
        let namespace = open(&root);
        let alias_path = root.join(reserved.to_ascii_uppercase());
        let mut alias = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&alias_path)
        {
            Ok(alias) => alias,
            // The canonical lock already occupies every case alias on a
            // case-insensitive volume. The pre-activation test above covers
            // that live path; this test deterministically exercises distinct
            // aliases on case-sensitive storage.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("failed to create lock alias fixture: {error}"),
        };
        alias.write_all(LOCK_MARKER).unwrap();
        alias.sync_all().unwrap();
        fs::set_permissions(&alias_path, fs::Permissions::from_mode(0o600)).unwrap();

        assert_eq!(
            namespace.directory().list_components(8),
            Err(PrivateFsError::Unsafe)
        );
        assert_eq!(
            namespace.directory().list_entry_names(8),
            Err(PrivateFsError::Unsafe)
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn root_lock_names_are_reserved_from_every_entry_operation_and_listing() {
    use std::cell::Cell;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let limit = ByteLimit::new(16).unwrap();

    for reserved in [
        entry_name(LOCK_NAME),
        entry_name(&LOCK_NAME.to_ascii_uppercase()),
        entry_name(LOCK_STAGING_NAME),
        entry_name(&LOCK_STAGING_NAME.to_ascii_uppercase()),
    ] {
        assert_eq!(
            directory.open_entry_child(&reserved).err(),
            Some(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.create_entry_child(&reserved).err(),
            Some(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.create_new_entry_child(&reserved).err(),
            Some(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.entry_regular_exists(&reserved),
            Err(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.entry_regular_identity(&reserved),
            Err(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.inspect_entry(&reserved),
            Err(PrivateFsError::ReservedComponent)
        );
        assert_eq!(
            directory.read_bounded_entry_regular(&reserved, limit),
            Err(PrivateFsError::ReservedComponent)
        );
        let callback_called = Cell::new(false);
        assert_eq!(
            directory.with_bounded_entry_regular_reader(&reserved, limit, |_reader| {
                callback_called.set(true);
                Ok::<_, ()>(())
            }),
            Err(PrivateFsError::ReservedComponent)
        );
        assert!(!callback_called.get());
        assert_eq!(
            directory.write_new_entry_synced(&reserved, b"x", limit),
            Err(PrivateFsError::ReservedComponent)
        );
        let mut source = Cursor::new(b"x");
        assert_eq!(
            directory.write_new_entry_from_reader(
                &reserved,
                &mut source,
                StreamingFileLength::new(1).unwrap(),
            ),
            Err(StreamingWriteError::Filesystem(
                PrivateFsError::ReservedComponent
            ))
        );
        assert_eq!(
            source.position(),
            0,
            "reserved writes must not consume input"
        );
        assert_eq!(
            directory.remove_verified_entry_regular(&reserved),
            Err(PrivateFsError::ReservedComponent)
        );
    }
    assert!(directory.list_entry_names(8).unwrap().is_empty());

    write_private_file(
        &root.join(LOCK_STAGING_NAME),
        b"unexpected live staging residue",
    );
    assert_eq!(directory.list_entry_names(8), Err(PrivateFsError::Unsafe));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn lock_path_identity_mutation_quarantines_the_lease() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    fs::remove_file(root.join(LOCK_NAME)).unwrap();

    assert_eq!(
        namespace
            .directory()
            .regular_exists(&component("value.bin")),
        Err(PrivateFsError::IdentityAmbiguous)
    );
    assert_eq!(
        namespace.directory().list_components(8),
        Err(PrivateFsError::Quarantined)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn child_keeps_the_root_lock_lease_alive_after_namespace_drop() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let child = {
        let namespace = open(&root);
        namespace
            .directory()
            .create_private_child(&component("objects"))
            .unwrap()
    };

    child
        .write_new_synced(
            &component("value.bin"),
            b"value",
            ByteLimit::new(16).unwrap(),
        )
        .unwrap();
    assert_eq!(
        LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
        PrivateFsError::LockUnavailable
    );
    drop(child);
    if let Err(error) = LockedPrivateNamespace::open_or_create(&root) {
        panic!("root lock was not released with the last control child: {error:?}");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn entry_child_keeps_the_root_lock_lease_alive_after_namespace_drop() {
    let parent = private_parent();
    let root = namespace_root(parent.path());
    let child = {
        let namespace = open(&root);
        namespace
            .directory()
            .create_new_entry_child(&entry_name("Payload Objects"))
            .unwrap()
    };

    child
        .write_new_entry_synced(
            &entry_name("Value.BIN"),
            b"value",
            ByteLimit::new(16).unwrap(),
        )
        .unwrap();
    assert_eq!(
        LockedPrivateNamespace::open_or_create(&root).err().unwrap(),
        PrivateFsError::LockUnavailable
    );
    drop(child);
    if let Err(error) = LockedPrivateNamespace::open_or_create(&root) {
        panic!("root lock was not released with the last entry child: {error:?}");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn concurrent_callers_have_one_serial_no_replace_winner() {
    use std::sync::{Arc, Barrier};

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let directory = namespace.directory();
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for name in ["first.bin", "second.bin"] {
            let barrier = Arc::clone(&barrier);
            workers.push(scope.spawn(move || {
                let source = component(name);
                let destination = component("current.bin");
                directory
                    .write_new_synced(&source, name.as_bytes(), ByteLimit::new(32).unwrap())
                    .unwrap();
                barrier.wait();
                directory.publish_noreplace_verified_regular(&source, &destination)
            }));
        }
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(PrivateFsError::AlreadyExists))
            .count(),
        1
    );
    let inventory = directory.list_components(8).unwrap();
    let mut sorted = inventory.clone();
    sorted.sort_unstable();
    assert_eq!(inventory, sorted);
    assert!(!inventory.contains(&component(LOCK_NAME)));
    assert!(!inventory.contains(&component(LOCK_STAGING_NAME)));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn fifo_nonblocking_probe_child() {
    let Ok(root) = std::env::var("ZEPHIUM_PRIVATE_FS_FIFO_PROBE_ROOT") else {
        return;
    };
    let namespace = open(Path::new(&root));
    assert_eq!(
        namespace
            .directory()
            .read_bounded_regular(&component("hostile.fifo"), ByteLimit::new(16).unwrap()),
        Err(PrivateFsError::Unsafe)
    );
    assert_eq!(
        namespace
            .directory()
            .inspect_entry(&entry_name("hostile.fifo")),
        Err(PrivateFsError::Unsafe)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn fifo_and_socket_rejection_is_nonblocking_and_bounded() {
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};

    let parent = private_parent();
    let root = namespace_root(parent.path());
    create_private_root(&root);
    let fifo = root.join("hostile.fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("fifo_nonblocking_probe_child")
        .arg("--nocapture")
        .env("ZEPHIUM_PRIVATE_FS_FIFO_PROBE_ROOT", &root)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let child_status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("FIFO admission probe exceeded the nonblocking deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(child_status.success());

    let namespace = open(&root);
    let socket = root.join("hostile.socket");
    let _listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Some managed macOS sandboxes deny AF_UNIX bind inside the
            // checkout. Native and CI hosts execute the assertion below; the
            // FIFO branch still exercises the nonblocking-open defense.
            return;
        }
        Err(error) => panic!("failed to create hostile socket fixture: {error}"),
    };
    assert_eq!(
        namespace
            .directory()
            .read_bounded_regular(&component("hostile.socket"), ByteLimit::new(16).unwrap(),),
        Err(PrivateFsError::Unsafe)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn rejects_extended_acl_not_visible_in_mode_bits() {
    use std::process::Command;

    let parent = private_parent();
    let root = namespace_root(parent.path());
    let namespace = open(&root);
    let file = component("acl.bin");
    namespace
        .directory()
        .write_new_synced(&file, b"value", ByteLimit::new(16).unwrap())
        .unwrap();

    let status = Command::new("/bin/chmod")
        .arg("+a")
        .arg("everyone allow read")
        .arg(root.join(file.as_str()))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        namespace
            .directory()
            .read_bounded_regular(&file, ByteLimit::new(16).unwrap()),
        Err(PrivateFsError::Unsafe)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_namespace_activation_is_deterministically_unavailable() {
    // Platform admission runs before path parsing or mutation. The relative
    // sentinel therefore proves the phase-one Windows path cannot be reached.
    assert_eq!(
        LockedPrivateNamespace::open_or_create("not-consulted")
            .err()
            .unwrap(),
        PrivateFsError::PrimitiveUnavailable
    );
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
#[test]
fn unsupported_unix_namespace_activation_is_deterministically_unavailable() {
    assert_eq!(
        LockedPrivateNamespace::open_or_create("not-consulted")
            .err()
            .unwrap(),
        PrivateFsError::PrimitiveUnavailable
    );
}
