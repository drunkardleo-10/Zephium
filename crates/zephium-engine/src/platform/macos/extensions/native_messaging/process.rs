//! Cold, bounded stdio transport for one publisher native-messaging host.
//!
//! No worker or process exists until the main-thread broker has joined a live
//! WebKit context to a sealed package and exact publisher requirement. This
//! module then discovers only fixed Chromium registration roots, validates a
//! duplicate-key-safe host manifest, verifies the executable's Apple code
//! signature, and runs one backpressured stdio actor per connected port.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use objc2_core_foundation::{CFRetained, CFString, CFURL};
use objc2_security::{
    kSecCSCheckAllArchitectures, kSecCSCheckNestedCode, kSecCSStrictValidate, SecCSFlags,
    SecRequirement, SecStaticCode,
};
use zephium_core::extensions::{
    ExtensionPublisherNativeHostRequirement, MAX_EXTENSION_NATIVE_HOST_CONNECTIONS,
};
use zephium_extension_package::{
    parse_bounded_json, BoundedJsonLimits, ChromiumExtensionId, NativeMessagingFrameLength,
    NativeMessagingHostManifest, MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES,
    MAX_NATIVE_MESSAGING_MESSAGE_BYTES,
};

const MAX_DISCOVERY_ROOTS: usize = 4;
const OUTBOUND_FRAME_QUEUE_CAPACITY: usize = 2;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CHROMIUM_ORIGIN_PREFIX: &str = "chrome-extension://";
const CHROMIUM_ORIGIN_SUFFIX: &str = "/";

/// Process-wide ceiling shared by every profile controller.
pub(crate) struct NativeHostProcessPool {
    active: AtomicUsize,
    roots: Option<NativeHostDiscoveryRoots>,
}

impl NativeHostProcessPool {
    pub(super) const fn new(roots: Option<NativeHostDiscoveryRoots>) -> Self {
        Self {
            active: AtomicUsize::new(0),
            roots,
        }
    }

    pub(super) fn roots(&self) -> Option<NativeHostDiscoveryRoots> {
        self.roots.clone()
    }

    pub(super) fn try_reserve(self: &Arc<Self>) -> Option<NativeHostProcessPermit> {
        let reserved = self
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_EXTENSION_NATIVE_HOST_CONNECTIONS).then_some(active + 1)
            })
            .ok()?;
        debug_assert!(reserved < MAX_EXTENSION_NATIVE_HOST_CONNECTIONS);
        Some(NativeHostProcessPermit {
            pool: Arc::clone(self),
        })
    }

    #[cfg(test)]
    fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

/// Linear reservation retained by the worker through process reap.
pub(super) struct NativeHostProcessPermit {
    pool: Arc<NativeHostProcessPool>,
}

impl Drop for NativeHostProcessPermit {
    fn drop(&mut self) {
        let previous = self.pool.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0);
    }
}

/// Fixed manifest search roots prepared from the current user's native home.
#[derive(Clone)]
pub(super) struct NativeHostDiscoveryRoots {
    roots: Box<[PathBuf]>,
}

impl NativeHostDiscoveryRoots {
    pub(super) fn macos_default(home: &Path) -> Option<Self> {
        if !home.is_absolute() {
            return None;
        }
        let roots = vec![
            home.join("Library/Application Support/Google/Chrome/NativeMessagingHosts"),
            home.join("Library/Application Support/Chromium/NativeMessagingHosts"),
            PathBuf::from("/Library/Google/Chrome/NativeMessagingHosts"),
            PathBuf::from("/Library/Application Support/Chromium/NativeMessagingHosts"),
        ];
        debug_assert_eq!(roots.len(), MAX_DISCOVERY_ROOTS);
        Some(Self {
            roots: roots.into_boxed_slice(),
        })
    }

    #[cfg(test)]
    fn one(root: PathBuf) -> Self {
        Self {
            roots: vec![root].into_boxed_slice(),
        }
    }
}

/// Redacted terminal reason returned to the WebKit broker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeHostProcessFailure {
    RegistrationUnavailable,
    RegistrationInvalid,
    RegistrationUnauthorized,
    ExecutableInvalid,
    SignatureInvalid,
    SpawnFailed,
    TransportFailed,
    MessageInvalid,
    QueueExceeded,
    Cancelled,
    Exited,
}

/// At most one event is in flight per worker; the worker waits for the main
/// actor to acknowledge each callback before reading another host message.
pub(crate) enum NativeHostWorkerEvent {
    Ready,
    Message(Box<[u8]>),
    Closed(NativeHostProcessFailure),
}

/// Main-thread control for one detached, self-reaping worker.
pub(super) struct NativeHostSessionControl {
    outbound: SyncSender<Box<[u8]>>,
    wake: UnixStream,
    cancelled: Arc<AtomicBool>,
}

impl NativeHostSessionControl {
    pub(super) fn try_send(&mut self, frame: Box<[u8]>) -> Result<(), NativeHostProcessFailure> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(NativeHostProcessFailure::Cancelled);
        }
        match self.outbound.try_send(frame) {
            Ok(()) => {
                signal_worker(&mut self.wake);
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(NativeHostProcessFailure::QueueExceeded),
            Err(TrySendError::Disconnected(_)) => Err(NativeHostProcessFailure::Exited),
        }
    }

    pub(super) fn cancel(&mut self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            signal_worker(&mut self.wake);
        }
    }
}

impl Drop for NativeHostSessionControl {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Starts one cold worker after authority and capacity admission.
pub(super) fn spawn_native_host_worker(
    requirement: ExtensionPublisherNativeHostRequirement,
    roots: NativeHostDiscoveryRoots,
    permit: NativeHostProcessPermit,
    emit: impl Fn(NativeHostWorkerEvent) -> bool + Send + 'static,
) -> Result<NativeHostSessionControl, NativeHostProcessFailure> {
    let (outbound, receiver) = mpsc::sync_channel(OUTBOUND_FRAME_QUEUE_CAPACITY);
    let (wake, worker_wake) =
        UnixStream::pair().map_err(|_| NativeHostProcessFailure::SpawnFailed)?;
    wake.set_nonblocking(true)
        .map_err(|_| NativeHostProcessFailure::SpawnFailed)?;
    worker_wake
        .set_nonblocking(true)
        .map_err(|_| NativeHostProcessFailure::SpawnFailed)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    std::thread::Builder::new()
        .name("zephium-native-host".into())
        .spawn(move || {
            let _permit = permit;
            run_worker(
                &requirement,
                &roots,
                receiver,
                worker_wake,
                &worker_cancelled,
                emit,
            );
        })
        .map_err(|_| NativeHostProcessFailure::SpawnFailed)?;
    Ok(NativeHostSessionControl {
        outbound,
        wake,
        cancelled,
    })
}

fn run_worker(
    requirement: &ExtensionPublisherNativeHostRequirement,
    roots: &NativeHostDiscoveryRoots,
    receiver: Receiver<Box<[u8]>>,
    worker_wake: UnixStream,
    cancelled: &AtomicBool,
    emit: impl Fn(NativeHostWorkerEvent) -> bool,
) {
    let result = (|| {
        let executable = discover_and_verify(requirement, roots)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(NativeHostProcessFailure::Cancelled);
        }
        let mut child = spawn_host(&executable, requirement.upstream_chromium_extension_id())?;
        if !emit(NativeHostWorkerEvent::Ready) {
            terminate_and_reap(&mut child);
            return Err(NativeHostProcessFailure::Cancelled);
        }
        let outcome = run_stdio_actor(&mut child, receiver, worker_wake, cancelled, &emit);
        terminate_and_reap(&mut child);
        outcome
    })();
    let reason = match result {
        Ok(()) => NativeHostProcessFailure::Exited,
        Err(reason) => reason,
    };
    let _ = emit(NativeHostWorkerEvent::Closed(reason));
}

fn discover_and_verify(
    requirement: &ExtensionPublisherNativeHostRequirement,
    roots: &NativeHostDiscoveryRoots,
) -> Result<PathBuf, NativeHostProcessFailure> {
    let manifest = discover_manifest(requirement.host_name(), roots)?;
    if manifest.name().as_str() != requirement.host_name() {
        return Err(NativeHostProcessFailure::RegistrationInvalid);
    }
    let upstream = ChromiumExtensionId::parse(requirement.upstream_chromium_extension_id())
        .map_err(|_| NativeHostProcessFailure::RegistrationUnauthorized)?;
    if !manifest.allows_extension(&upstream) {
        return Err(NativeHostProcessFailure::RegistrationUnauthorized);
    }
    let path = PathBuf::from(manifest.path());
    if !path.is_absolute() {
        return Err(NativeHostProcessFailure::ExecutableInvalid);
    }
    let canonical =
        std::fs::canonicalize(&path).map_err(|_| NativeHostProcessFailure::ExecutableInvalid)?;
    let before = executable_identity(&canonical)?;
    verify_apple_signature(&canonical, requirement)?;
    if executable_identity(&canonical)? != before {
        return Err(NativeHostProcessFailure::ExecutableInvalid);
    }
    Ok(canonical)
}

fn discover_manifest(
    host_name: &str,
    roots: &NativeHostDiscoveryRoots,
) -> Result<NativeMessagingHostManifest, NativeHostProcessFailure> {
    let file_name = format!("{host_name}.json");
    for root in roots.roots.iter() {
        let candidate = root.join(&file_name);
        match open_bounded_regular(&candidate, MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take((MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| NativeHostProcessFailure::RegistrationInvalid)?;
                if bytes.len() > MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES {
                    return Err(NativeHostProcessFailure::RegistrationInvalid);
                }
                return NativeMessagingHostManifest::parse(&bytes)
                    .map_err(|_| NativeHostProcessFailure::RegistrationInvalid);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(NativeHostProcessFailure::RegistrationInvalid),
        }
    }
    Err(NativeHostProcessFailure::RegistrationUnavailable)
}

fn open_bounded_regular(path: &Path, max_bytes: usize) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native host registration is not a bounded regular file",
        ));
    }
    Ok(file)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExecutableIdentity {
    device: u64,
    inode: u64,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

fn executable_identity(path: &Path) -> Result<ExecutableIdentity, NativeHostProcessFailure> {
    let metadata =
        std::fs::metadata(path).map_err(|_| NativeHostProcessFailure::ExecutableInvalid)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(NativeHostProcessFailure::ExecutableInvalid);
    }
    Ok(ExecutableIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
    })
}

fn verify_apple_signature(
    path: &Path,
    requirement: &ExtensionPublisherNativeHostRequirement,
) -> Result<(), NativeHostProcessFailure> {
    let url = CFURL::from_file_path(path).ok_or(NativeHostProcessFailure::SignatureInvalid)?;
    let mut code = std::ptr::null();
    let status = unsafe {
        SecStaticCode::create_with_path(&url, SecCSFlags::DefaultFlags, NonNull::from(&mut code))
    };
    if status != 0 {
        return Err(NativeHostProcessFailure::SignatureInvalid);
    }
    let code = NonNull::new(code.cast_mut()).ok_or(NativeHostProcessFailure::SignatureInvalid)?;
    let code: CFRetained<SecStaticCode> = unsafe { CFRetained::from_raw(code) };

    let publisher = requirement.macos_publisher();
    let requirement_text = format!(
        "anchor apple generic and identifier \"{}\" and certificate leaf[subject.OU] = \"{}\" and certificate leaf[field.1.2.840.113635.100.6.1.13] exists",
        publisher.signing_identifier(),
        publisher.team_identifier(),
    );
    let requirement_text = CFString::from_str(&requirement_text);
    let mut compiled = std::ptr::null_mut();
    let status = unsafe {
        SecRequirement::create_with_string(
            &requirement_text,
            SecCSFlags::DefaultFlags,
            NonNull::from(&mut compiled),
        )
    };
    if status != 0 {
        return Err(NativeHostProcessFailure::SignatureInvalid);
    }
    let compiled = NonNull::new(compiled).ok_or(NativeHostProcessFailure::SignatureInvalid)?;
    let compiled: CFRetained<SecRequirement> = unsafe { CFRetained::from_raw(compiled) };
    let flags =
        SecCSFlags(kSecCSCheckAllArchitectures | kSecCSCheckNestedCode | kSecCSStrictValidate)
            | SecCSFlags::NoNetworkAccess;
    if unsafe { code.check_validity(flags, Some(&compiled)) } != 0 {
        return Err(NativeHostProcessFailure::SignatureInvalid);
    }
    Ok(())
}

fn spawn_host(path: &Path, extension_id: &str) -> Result<Child, NativeHostProcessFailure> {
    Command::new(path)
        .arg(format!(
            "{CHROMIUM_ORIGIN_PREFIX}{extension_id}{CHROMIUM_ORIGIN_SUFFIX}"
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| NativeHostProcessFailure::SpawnFailed)
}

fn run_stdio_actor(
    child: &mut Child,
    receiver: Receiver<Box<[u8]>>,
    mut wake: UnixStream,
    cancelled: &AtomicBool,
    emit: &impl Fn(NativeHostWorkerEvent) -> bool,
) -> Result<(), NativeHostProcessFailure> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or(NativeHostProcessFailure::SpawnFailed)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(NativeHostProcessFailure::SpawnFailed)?;
    set_nonblocking(stdin.as_raw_fd())?;
    set_nonblocking(stdout.as_raw_fd())?;
    let mut reader = FrameReader::default();
    let mut outbound: Option<(Box<[u8]>, usize)> = None;

    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(NativeHostProcessFailure::Cancelled);
        }
        if child
            .try_wait()
            .map_err(|_| NativeHostProcessFailure::TransportFailed)?
            .is_some()
        {
            return Ok(());
        }
        if outbound.is_none() {
            match receiver.try_recv() {
                Ok(frame) => outbound = Some((frame, 0)),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return Err(NativeHostProcessFailure::Cancelled),
            }
        }

        let mut descriptors = [
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stdout.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stdin.as_raw_fd(),
                events: if outbound.is_some() { libc::POLLOUT } else { 0 },
                revents: 0,
            },
        ];
        let timeout = i32::try_from(WORKER_POLL_INTERVAL.as_millis()).unwrap_or(250);
        let ready = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                timeout,
            )
        };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(NativeHostProcessFailure::TransportFailed);
        }
        if descriptors[0].revents & libc::POLLIN != 0 {
            drain_wake(&mut wake)?;
        }
        if descriptors[2].revents & libc::POLLOUT != 0 {
            write_outbound(&mut stdin, &mut outbound)?;
        }
        if descriptors[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            loop {
                match reader.read(&mut stdout)? {
                    FrameRead::Pending => break,
                    FrameRead::Message(message) => {
                        if !emit(NativeHostWorkerEvent::Message(message)) {
                            return Err(NativeHostProcessFailure::Cancelled);
                        }
                    }
                    FrameRead::Eof => return Ok(()),
                }
            }
        }
        if descriptors
            .iter()
            .any(|descriptor| descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0)
        {
            return Err(NativeHostProcessFailure::TransportFailed);
        }
    }
}

fn write_outbound(
    stdin: &mut ChildStdin,
    outbound: &mut Option<(Box<[u8]>, usize)>,
) -> Result<(), NativeHostProcessFailure> {
    let Some((frame, offset)) = outbound.as_mut() else {
        return Ok(());
    };
    match stdin.write(&frame[*offset..]) {
        Ok(0) => return Err(NativeHostProcessFailure::TransportFailed),
        Ok(written) => *offset = offset.saturating_add(written),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(()),
        Err(_) => return Err(NativeHostProcessFailure::TransportFailed),
    }
    if *offset == frame.len() {
        outbound.take();
    }
    Ok(())
}

#[derive(Default)]
struct FrameReader {
    prefix: [u8; 4],
    prefix_used: usize,
    body: Vec<u8>,
    body_used: usize,
}

enum FrameRead {
    Pending,
    Message(Box<[u8]>),
    Eof,
}

impl FrameReader {
    fn read(&mut self, stdout: &mut impl Read) -> Result<FrameRead, NativeHostProcessFailure> {
        if self.prefix_used < self.prefix.len() {
            match read_nonblocking(stdout, &mut self.prefix[self.prefix_used..])? {
                NonblockingRead::Read(bytes) => self.prefix_used += bytes,
                NonblockingRead::Pending => return Ok(FrameRead::Pending),
                NonblockingRead::Eof if self.prefix_used == 0 => return Ok(FrameRead::Eof),
                NonblockingRead::Eof => return Err(NativeHostProcessFailure::MessageInvalid),
            }
            if self.prefix_used < self.prefix.len() {
                return Ok(FrameRead::Pending);
            }
            let length = NativeMessagingFrameLength::decode(self.prefix)
                .map_err(|_| NativeHostProcessFailure::MessageInvalid)?;
            self.body = vec![0; length.get()];
            self.body_used = 0;
        }
        if self.body_used < self.body.len() {
            match read_nonblocking(stdout, &mut self.body[self.body_used..])? {
                NonblockingRead::Read(bytes) => self.body_used += bytes,
                NonblockingRead::Pending => return Ok(FrameRead::Pending),
                NonblockingRead::Eof => return Err(NativeHostProcessFailure::MessageInvalid),
            }
            if self.body_used < self.body.len() {
                return Ok(FrameRead::Pending);
            }
        }
        let parsed = parse_bounded_json(&self.body, BoundedJsonLimits::native_messaging_message())
            .map_err(|_| NativeHostProcessFailure::MessageInvalid)?;
        let encoded = serde_json::to_vec(parsed.as_value())
            .map_err(|_| NativeHostProcessFailure::MessageInvalid)?;
        if encoded.is_empty() || encoded.len() > MAX_NATIVE_MESSAGING_MESSAGE_BYTES {
            return Err(NativeHostProcessFailure::MessageInvalid);
        }
        self.prefix_used = 0;
        self.body.clear();
        self.body_used = 0;
        Ok(FrameRead::Message(encoded.into_boxed_slice()))
    }
}

enum NonblockingRead {
    Read(usize),
    Pending,
    Eof,
}

fn read_nonblocking(
    reader: &mut impl Read,
    destination: &mut [u8],
) -> Result<NonblockingRead, NativeHostProcessFailure> {
    match reader.read(destination) {
        Ok(0) => Ok(NonblockingRead::Eof),
        Ok(bytes) => Ok(NonblockingRead::Read(bytes)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(NonblockingRead::Pending),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(NonblockingRead::Pending),
        Err(_) => Err(NativeHostProcessFailure::TransportFailed),
    }
}

fn set_nonblocking(descriptor: libc::c_int) -> Result<(), NativeHostProcessFailure> {
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(NativeHostProcessFailure::TransportFailed);
    }
    Ok(())
}

fn drain_wake(wake: &mut UnixStream) -> Result<(), NativeHostProcessFailure> {
    let mut bytes = [0_u8; 64];
    loop {
        match wake.read(&mut bytes) {
            Ok(0) => return Err(NativeHostProcessFailure::Cancelled),
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(NativeHostProcessFailure::TransportFailed),
        }
    }
}

fn signal_worker(wake: &mut UnixStream) {
    match wake.write(&[1]) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(_) => {}
    }
}

fn terminate_and_reap(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use zephium_core::extensions::{
        ExtensionAuthorityId, ExtensionMacosPublisherIdentity, ExtensionManifestDigest,
        ExtensionPackageIdentity, ExtensionPackageKey, ExtensionPackagePayloadIdentity,
        ExtensionPackageRevision, ExtensionTreeDigest,
    };

    fn publisher_requirement() -> ExtensionPublisherNativeHostRequirement {
        ExtensionPublisherNativeHostRequirement::new(
            ExtensionPackageIdentity::new(
                ExtensionAuthorityId::from_bytes([1; 32]),
                ExtensionPackageKey::from_bytes([2; 32]),
                ExtensionPackageRevision::new(3).unwrap(),
                ExtensionPackagePayloadIdentity::BundledTree,
                ExtensionManifestDigest::from_bytes([4; 32]),
                ExtensionTreeDigest::from_bytes([5; 32]),
            ),
            "com.example.host",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ExtensionMacosPublisherIdentity::new("A1B2C3D4E5", "com.example.host").unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn process_pool_is_exact_and_releases_only_with_linear_permit() {
        let pool = Arc::new(NativeHostProcessPool::new(None));
        let permits = (0..MAX_EXTENSION_NATIVE_HOST_CONNECTIONS)
            .map(|_| pool.try_reserve().unwrap())
            .collect::<Vec<_>>();
        assert!(pool.try_reserve().is_none());
        assert_eq!(pool.active(), MAX_EXTENSION_NATIVE_HOST_CONNECTIONS);
        drop(permits);
        assert_eq!(pool.active(), 0);
    }

    #[test]
    fn discovery_uses_first_present_registration_and_never_falls_through_invalid() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let name = "com.example.host";
        std::fs::write(first.join(format!("{name}.json")), b"{}").unwrap();
        std::fs::write(
            second.join(format!("{name}.json")),
            br#"{"name":"com.example.host","description":"Host","path":"/bin/echo","type":"stdio","allowed_origins":["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"]}"#,
        )
        .unwrap();
        let roots = NativeHostDiscoveryRoots {
            roots: vec![first, second].into_boxed_slice(),
        };
        assert_eq!(
            discover_manifest(name, &roots),
            Err(NativeHostProcessFailure::RegistrationInvalid)
        );
    }

    #[test]
    fn discovery_rejects_symlink_and_oversized_registration() {
        let temp = tempfile::tempdir().unwrap();
        let roots = NativeHostDiscoveryRoots::one(temp.path().to_path_buf());
        let target = temp.path().join("target.json");
        std::fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, temp.path().join("com.example.host.json")).unwrap();
        assert_eq!(
            discover_manifest("com.example.host", &roots),
            Err(NativeHostProcessFailure::RegistrationInvalid)
        );
        std::fs::remove_file(temp.path().join("com.example.host.json")).unwrap();
        std::fs::write(
            temp.path().join("com.example.host.json"),
            vec![b'a'; MAX_NATIVE_MESSAGING_HOST_MANIFEST_BYTES + 1],
        )
        .unwrap();
        assert_eq!(
            discover_manifest("com.example.host", &roots),
            Err(NativeHostProcessFailure::RegistrationInvalid)
        );
    }

    #[test]
    fn executable_identity_requires_a_regular_executable_and_detects_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("host");
        std::fs::write(&path, b"first").unwrap();
        assert_eq!(
            executable_identity(&path),
            Err(NativeHostProcessFailure::ExecutableInvalid)
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let first = executable_identity(&path).unwrap();
        std::fs::write(&path, b"second-longer").unwrap();
        assert_ne!(first, executable_identity(&path).unwrap());
    }

    #[test]
    fn apple_signature_boundary_rejects_a_valid_but_wrong_publisher() {
        assert_eq!(
            verify_apple_signature(Path::new("/usr/bin/true"), &publisher_requirement()),
            Err(NativeHostProcessFailure::SignatureInvalid)
        );
    }

    #[test]
    fn frame_reader_accepts_partial_json_and_rejects_oversized_prefix() {
        let value = serde_json::json!({"kind": "ready"});
        let frame = zephium_extension_package::encode_native_messaging_frame(&value).unwrap();
        let mut reader = FrameReader::default();
        let mut bytes = io::Cursor::new(frame);
        let message = loop {
            match reader.read(&mut bytes).unwrap() {
                FrameRead::Message(message) => break message,
                FrameRead::Pending => continue,
                FrameRead::Eof => panic!("message unexpectedly ended"),
            }
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&message).unwrap(),
            value
        );

        let mut reader = FrameReader::default();
        let mut invalid = io::Cursor::new(
            u32::try_from(MAX_NATIVE_MESSAGING_MESSAGE_BYTES + 1)
                .unwrap()
                .to_ne_bytes(),
        );
        assert!(matches!(
            reader.read(&mut invalid),
            Err(NativeHostProcessFailure::MessageInvalid)
        ));
    }

    #[test]
    fn stdio_actor_round_trips_one_bounded_frame_and_reaps_on_cancel() {
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let (sender, receiver) = mpsc::sync_channel(OUTBOUND_FRAME_QUEUE_CAPACITY);
        let (mut wake, worker_wake) = UnixStream::pair().unwrap();
        wake.set_nonblocking(true).unwrap();
        worker_wake.set_nonblocking(true).unwrap();
        let expected = serde_json::json!({"kind": "round-trip", "sequence": 1});
        sender
            .send(
                zephium_extension_package::encode_native_messaging_frame(&expected)
                    .unwrap()
                    .into_boxed_slice(),
            )
            .unwrap();
        signal_worker(&mut wake);
        let cancelled = AtomicBool::new(false);
        let observed = RefCell::new(None);
        let outcome = run_stdio_actor(&mut child, receiver, worker_wake, &cancelled, &|event| {
            if let NativeHostWorkerEvent::Message(message) = event {
                *observed.borrow_mut() = serde_json::from_slice::<serde_json::Value>(&message).ok();
                cancelled.store(true, Ordering::Release);
            }
            true
        });
        terminate_and_reap(&mut child);
        assert_eq!(outcome, Err(NativeHostProcessFailure::Cancelled));
        assert_eq!(*observed.borrow(), Some(expected));
    }
}
