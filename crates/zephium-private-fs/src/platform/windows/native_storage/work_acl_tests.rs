use super::super::ANCHOR;
use super::*;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Profile {
    directory: tempfile::TempDir,
    owner: super::tests::OwnedSession,
}
impl Profile {
    fn new() -> Self {
        let owner = super::tests::OwnedSession::new();
        let directory = tempfile::tempdir_in(owner.anchor().inner.directory()).unwrap();
        Self { directory, owner }
    }
    fn path(&self) -> &std::path::Path {
        self.directory.path()
    }
    fn finish(self) {
        self.directory.close().unwrap();
        self.owner.finish();
    }
}

// Modify only disposable synthetic validation roots, never real KnownFolder
// ancestors, Product, or the user's QA session. No existing ACL is repaired.
fn grant(path: &std::path::Path, rights: &str) {
    let mut child = Command::new("icacls.exe")
        .arg(path)
        .arg("/grant")
        .arg(format!("*S-1-1-0:(OI)(CI)({rights})"))
        .creation_flags(0x0800_0000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned ACL fixture timeout");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn prepare(
    profile: &std::path::Path,
    session: &NativeSession,
) -> Result<NativeWorkStorageAnchor, PrivateFsError> {
    NativeWorkStorageAnchor::prepare_validation_fixture(
        profile,
        NativeApplication::ExtensionQa,
        Some(session),
    )
}

#[test]
fn readonly_top_is_ancestor_but_held_acl_changes_quarantine() {
    let profile = Profile::new();
    let session = NativeSession::new("readonly-top").unwrap();
    let first = prepare(profile.path(), &session).unwrap();
    grant(&profile.path().join(ANCHOR), "RX");
    assert!(first.verify().is_err());
    assert!(matches!(first.verify(), Err(PrivateFsError::Quarantined)));
    assert!(matches!(
        prepare(profile.path(), &session),
        Err(PrivateFsError::LockUnavailable)
    ));
    drop(first);
    let next = prepare(profile.path(), &session).unwrap();
    next.create_or_hold_database()
        .unwrap()
        .verify(&next)
        .unwrap();
    next.verify().unwrap();
    drop(next);
    profile.finish();
}

#[test]
fn untrusted_top_mutation_and_private_root_readonly_refuse() {
    for rights in ["W", "DC"] {
        let profile = Profile::new();
        let session = NativeSession::new("untrusted-top").unwrap();
        drop(prepare(profile.path(), &session).unwrap());
        grant(&profile.path().join(ANCHOR), rights);
        assert!(prepare(profile.path(), &session).is_err());
        profile.finish();
    }
    for root in ["application", "session"] {
        let profile = Profile::new();
        let session = NativeSession::new("private-root").unwrap();
        let first = prepare(profile.path(), &session).unwrap();
        let path = if root == "application" {
            profile
                .path()
                .join(ANCHOR)
                .join(NativeApplication::ExtensionQa.component())
        } else {
            first.database_path().parent().unwrap().to_path_buf()
        };
        drop(first);
        grant(&path, "RX");
        assert!(prepare(profile.path(), &session).is_err());
        profile.finish();
    }
}
