#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use zephium_private_fs::PrivateComponent;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use zephium_private_fs::{ByteLimit, MAX_IN_MEMORY_FILE_BYTES};
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
    }
    assert_eq!(directory.list_components(8).unwrap(), vec![stage]);
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
    assert!(LockedPrivateNamespace::open_or_create(&root).is_ok());
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
