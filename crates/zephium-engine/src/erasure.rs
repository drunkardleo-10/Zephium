use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg(any(not(target_os = "macos"), test))]
use zephium_core::ids::ProfileId;
use zephium_core::ports::engine::ProfileDataErasureOutcome;

const ERASURE_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(any(not(target_os = "macos"), test))]
const REMOVE_ATTEMPTS: usize = 40;
#[cfg(any(not(target_os = "macos"), test))]
const REMOVE_RETRY_DELAY: Duration = Duration::from_millis(50);

type ErasureDone = Box<dyn FnOnce(ProfileDataErasureOutcome) + Send>;

pub(crate) fn metadata_is_direct_directory(metadata: &fs::Metadata) -> bool {
    #[cfg(target_os = "windows")]
    let is_reparse_point = {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT. Junctions and mount points are not
        // necessarily reported by FileType::is_symlink().
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(target_os = "windows"))]
    let is_reparse_point = false;

    metadata.is_dir() && !metadata.file_type().is_symlink() && !is_reparse_point
}

fn direct_directory_identity(path: &Path) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata_is_direct_directory(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "engine data path is not a direct owned directory",
        ));
    }
    #[cfg(target_os = "windows")]
    for ancestor in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(ancestor)?;
        if !metadata_is_direct_directory(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "engine data path has a redirecting/non-directory ancestor: {}",
                    ancestor.display()
                ),
            ));
        }
    }
    path.canonicalize()
}

/// Make a completed unlink/recursive removal durable on Unix before higher
/// layers clear their deletion journal. File deletion and SQLite durability
/// are separate ordering domains: without syncing the containing directory,
/// a sudden power loss could preserve the journal deletion while resurrecting
/// the directory entry that contained private data.
#[cfg(all(unix, any(not(target_os = "macos"), test)))]
fn sync_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata_is_direct_directory(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "durability barrier target is not a direct directory",
        ));
    }

    fs::File::open(path)?.sync_all()
}

#[cfg(any(not(target_os = "macos"), test))]
fn sync_removed_child(parent: &Path) -> bool {
    #[cfg(unix)]
    {
        sync_directory(parent).is_ok()
    }
    #[cfg(target_os = "windows")]
    {
        // Win32 does not document FlushFileBuffers for directory handles.
        // Absence is revalidated here, but power-loss durability remains a
        // packaged real-OS release gate instead of relying on an unsupported
        // pseudo-fsync that would fail every deletion.
        let _ = parent;
        true
    }
}

#[cfg(any(not(target_os = "macos"), test))]
fn sync_removed_root(root: &Path) -> io::Result<()> {
    let parent = root.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "engine data root has no containing directory",
        )
    })?;
    #[cfg(unix)]
    {
        sync_directory(parent)
    }
    #[cfg(target_os = "windows")]
    {
        // See `sync_removed_child`: Windows cleanup is crash-resumable and
        // exact-process-gated, but no unsupported directory flush is claimed.
        let _ = parent;
        Ok(())
    }
}

/// Exactly-once public completion plus the per-attempt admission flag kept by
/// the UI-thread host. Timing out reports once but deliberately keeps the slot
/// occupied; only a later native terminal callback releases admission, and it
/// cannot invoke the public completion a second time.
pub(crate) struct Completion {
    done: Mutex<Option<ErasureDone>>,
    active: Arc<AtomicBool>,
    settled: Mutex<bool>,
    settled_changed: Condvar,
    #[cfg(test)]
    watchdog_exited: AtomicBool,
}

impl Completion {
    pub(crate) fn start(done: ErasureDone, active: Arc<AtomicBool>) -> Arc<Self> {
        Self::start_with_timeout(done, active, ERASURE_TIMEOUT)
    }

    fn start_with_timeout(
        done: ErasureDone,
        active: Arc<AtomicBool>,
        timeout: Duration,
    ) -> Arc<Self> {
        let completion = Arc::new(Self {
            done: Mutex::new(Some(done)),
            active,
            settled: Mutex::new(false),
            settled_changed: Condvar::new(),
            #[cfg(test)]
            watchdog_exited: AtomicBool::new(false),
        });
        let watchdog = completion.clone();
        let watchdog_task = move || {
            let settled = watchdog
                .settled
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (settled, wait) = watchdog
                .settled_changed
                .wait_timeout_while(settled, timeout, |settled| !*settled)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let timed_out = !*settled && wait.timed_out();
            drop(settled);
            if timed_out {
                // Reporting a caller-visible timeout does not prove that the
                // native async operation stopped. Keep the admission flag set
                // until a real platform terminal callback calls `finish`.
                watchdog.report_unsettled(ProfileDataErasureOutcome::TimedOut);
            }
            #[cfg(test)]
            watchdog.watchdog_exited.store(true, Ordering::Release);
        };
        if let Err(error) = std::thread::Builder::new()
            .name("zephium-erasure-watchdog".into())
            .spawn(watchdog_task)
        {
            // Without the watchdog there is no bounded proof that a native
            // clear operation will ever call back. Report the same fail-closed
            // terminal condition immediately; keep `active` set until a real
            // platform completion arrives so a retry cannot fabricate absence.
            eprintln!("privacy: cannot start profile-erasure watchdog: {error}");
            completion.report_unsettled(ProfileDataErasureOutcome::TimedOut);
        }
        completion
    }

    pub(crate) fn attempt_flag(&self) -> Arc<AtomicBool> {
        self.active.clone()
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn finish(&self, outcome: ProfileDataErasureOutcome) {
        // Serialize settlement with the watchdog. If native completion wins
        // this lock, the caller receives its terminal outcome; if the
        // watchdog wins, native settlement only releases admission and never
        // invokes the callback a second time.
        let done = {
            let mut done = self
                .done
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.active.store(false, Ordering::Release);
            done.take()
        };
        {
            let mut settled = self
                .settled
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *settled = true;
            self.settled_changed.notify_all();
        }
        if let Some(done) = done {
            done(outcome);
        }
    }

    /// Terminal failure paths must not wait for an arbitrary public callback
    /// before invoking the composition-root fatal action. Settle exactly once,
    /// wake the watchdog, and invoke the callback on a detached worker.
    pub(crate) fn finish_detached(&self, outcome: ProfileDataErasureOutcome) {
        let done = {
            let mut done = self
                .done
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.active.store(false, Ordering::Release);
            done.take()
        };
        {
            let mut settled = self
                .settled
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *settled = true;
            self.settled_changed.notify_all();
        }
        if let Some(done) = done {
            let _ = std::thread::Builder::new()
                .name("zephium-erasure-completion".into())
                .spawn(move || done(outcome));
        }
    }

    /// Report an outcome without claiming the platform operation settled.
    /// The attempt stays in flight and retries remain denied.
    pub(crate) fn report_unsettled(&self, outcome: ProfileDataErasureOutcome) {
        let done = self
            .done
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(done) = done {
            done(outcome);
        }
    }

    #[cfg(test)]
    fn watchdog_has_exited(&self) -> bool {
        self.watchdog_exited.load(Ordering::Acquire)
    }
}

pub(crate) fn canonical_owned_root(path: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(path)?;
    let canonical = direct_directory_identity(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(canonical)
}

#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn profile_path(root: &Path, profile: ProfileId) -> PathBuf {
    // ProfileId's Display is a canonical, separator-free ULID. Keeping path
    // derivation here prevents deletion callers from supplying arbitrary
    // relative paths.
    root.join(profile.to_string())
}

#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn prepare_profile_directory(root: &Path, profile: ProfileId) -> io::Result<PathBuf> {
    validate_existing_root(root)?;
    let path = profile_path(root, profile);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // The canonical parent already exists; create exactly one typed
            // child so a final-component symlink can never be traversed by a
            // recursive directory creator.
            if let Err(create_error) = fs::create_dir(&path) {
                if create_error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(create_error);
                }
            }
        }
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata_is_direct_directory(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "profile data path is not an owned directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    let canonical = path.canonicalize()?;
    if canonical.parent() != Some(root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "profile data escaped its owned root",
        ));
    }
    Ok(canonical)
}

#[cfg(any(not(target_os = "macos"), test))]
fn validate_existing_root(root: &Path) -> io::Result<()> {
    if direct_directory_identity(root)? != root {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "engine data root changed after startup",
        ));
    }
    Ok(())
}

#[cfg(any(not(target_os = "macos"), test))]
fn remove_one_profile_directory(root: &Path, profile: ProfileId) -> bool {
    let path = profile_path(root, profile);
    for attempt in 0..REMOVE_ATTEMPTS {
        match direct_directory_identity(root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return sync_removed_root(root).is_ok();
            }
            Err(_) => return false,
            Ok(canonical) if canonical == root => {}
            Ok(_) => return false,
        }

        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return sync_removed_child(root);
            }
            Err(_) => return false,
            // Refuse ambiguous filesystem objects rather than following or
            // deleting an attacker-controlled link target.
            Ok(metadata) if !metadata_is_direct_directory(&metadata) => {
                return false;
            }
            Ok(_) => {
                let Ok(canonical) = path.canonicalize() else {
                    if attempt + 1 < REMOVE_ATTEMPTS {
                        std::thread::sleep(REMOVE_RETRY_DELAY);
                        continue;
                    }
                    return false;
                };
                if canonical.parent() != Some(root) {
                    return false;
                }
                // Revalidate the exact object immediately before every
                // recursive delete. A stored canonical path is not proof that
                // another process did not replace it with a junction later.
                let pre_delete_identity = match direct_directory_identity(&canonical) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return true,
                    Err(_) if attempt + 1 < REMOVE_ATTEMPTS => {
                        std::thread::sleep(REMOVE_RETRY_DELAY);
                        continue;
                    }
                    Err(_) => return false,
                    Ok(identity) => identity,
                };
                if pre_delete_identity != canonical || canonical.parent() != Some(root) {
                    return false;
                }
                let _ = fs::remove_dir_all(&canonical);
            }
        }

        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return sync_removed_child(root);
            }
            _ if attempt + 1 < REMOVE_ATTEMPTS => {
                std::thread::sleep(REMOVE_RETRY_DELAY);
            }
            _ => return false,
        }
    }
    false
}

/// Exercise exact root-removal identity and durability behavior independently
/// of a platform runtime manager. Production profile erasure deletes only
/// typed profile children; Windows runtime-generation roots are now owned by
/// `zephium_core::webview2::RuntimeGeneration` and use its provenance gate.
#[cfg(test)]
pub(crate) fn remove_owned_root_once(root: &Path) -> io::Result<bool> {
    match direct_directory_identity(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            sync_removed_root(root)?;
            return Ok(true);
        }
        Err(error) => return Err(error),
        Ok(canonical) if canonical == root => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "engine data root identity changed before deletion",
            ));
        }
    }
    // A second metadata read keeps the safety check adjacent to the destructive
    // call. Fully race-free recursive deletion still ultimately depends on OS
    // handle-based APIs, but no known reparse point is ever followed here.
    if direct_directory_identity(root)? != root {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "engine data root changed immediately before deletion",
        ));
    }
    match fs::remove_dir_all(root) {
        Ok(()) => {
            sync_removed_root(root)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            sync_removed_root(root)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn remove_profile_directories_verified(roots: &[PathBuf], profile: ProfileId) -> bool {
    // Do not short-circuit: a failure in one engine root must not prevent a
    // best-effort removal and verification of the other independent root.
    let mut verified = true;
    for root in roots {
        verified = remove_one_profile_directory(root, profile) && verified;
    }
    verified
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn completion_is_exactly_once_and_releases_attempt() {
        let (tx, rx) = mpsc::channel();
        let completion = Completion::start_with_timeout(
            Box::new(move |outcome| tx.send(outcome).unwrap()),
            Arc::new(AtomicBool::new(true)),
            Duration::from_secs(1),
        );
        let active = completion.attempt_flag();
        completion.finish(ProfileDataErasureOutcome::Verified);
        completion.finish(ProfileDataErasureOutcome::Failed);
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            ProfileDataErasureOutcome::Verified
        );
        assert!(rx.recv_timeout(Duration::from_millis(25)).is_err());
        assert!(!active.load(Ordering::Acquire));
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        while !completion.watchdog_has_exited() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            completion.watchdog_has_exited(),
            "terminal completion must cancel and wake its watchdog"
        );
    }

    #[test]
    fn timeout_reports_once_but_keeps_native_attempt_in_flight() {
        let (tx, rx) = mpsc::channel();
        let completion = Completion::start_with_timeout(
            Box::new(move |outcome| tx.send(outcome).unwrap()),
            Arc::new(AtomicBool::new(true)),
            Duration::from_millis(10),
        );
        let active = completion.attempt_flag();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(200)).unwrap(),
            ProfileDataErasureOutcome::TimedOut
        );
        assert!(active.load(Ordering::Acquire));
        completion.finish(ProfileDataErasureOutcome::Verified);
        assert!(!active.load(Ordering::Acquire));
        assert!(rx.recv_timeout(Duration::from_millis(25)).is_err());
    }

    #[test]
    fn owned_profile_directory_is_removed_and_absence_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let root = canonical_owned_root(&temp.path().join("profiles")).unwrap();
        let profile = ProfileId::from(7);
        let path = prepare_profile_directory(&root, profile).unwrap();
        fs::create_dir(path.join("nested")).unwrap();
        fs::write(path.join("nested/data"), b"private").unwrap();

        assert!(remove_profile_directories_verified(
            std::slice::from_ref(&root),
            profile
        ));
        assert!(!profile_path(&root, profile).exists());
        assert!(remove_profile_directories_verified(&[root], profile));
    }

    #[test]
    fn owned_root_removal_revalidates_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let root = canonical_owned_root(&temp.path().join("private-runtime")).unwrap();
        fs::write(root.join("runtime-state"), b"private").unwrap();

        assert!(remove_owned_root_once(&root).unwrap());
        assert!(!root.exists());
        assert!(remove_owned_root_once(&root).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_profile_is_never_followed_or_reported_erased() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = canonical_owned_root(&temp.path().join("profiles")).unwrap();
        let target = temp.path().join("outside");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("secret"), b"keep").unwrap();
        let profile = ProfileId::from(9);
        symlink(&target, profile_path(&root, profile)).unwrap();

        assert!(!remove_profile_directories_verified(&[root], profile));
        assert_eq!(fs::read(target.join("secret")).unwrap(), b"keep");
    }
}
