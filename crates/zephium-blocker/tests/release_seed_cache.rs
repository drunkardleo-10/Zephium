//! Exercise the shipped corpus through the public worker/cache boundary.

#![cfg(any(
    all(target_os = "windows", feature = "runtime"),
    all(not(target_os = "windows"), feature = "webkit")
))]

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use zephium_blocker::{
    BlockerCompileFailure, CompiledArtifactCacheConfig, PolicyCatalog, PolicySource, SourceFormat,
    SourceId, StaticPolicyCatalog, WorkerBlocker,
};
use zephium_core::blocker::{BlockerConfig, ContentPolicyGeneration, ContentRules};
use zephium_core::ids::ProfileId;
use zephium_core::ports::blocker::{
    BlockerCompileOutcome, BlockerCompiler, BlockerDispatch, BlockerShutdownOutcome,
};

const MANIFEST: &[u8] = include_bytes!("../../../assets/blocker-seed/v1/catalog.json");
const SOURCES: [(&str, &[u8]); 2] = [
    (
        "easylist",
        include_bytes!("../../../assets/blocker-seed/v1/easylist.txt.gz"),
    ),
    (
        "easyprivacy",
        include_bytes!("../../../assets/blocker-seed/v1/easyprivacy.txt.gz"),
    ),
];

fn compile(worker: &WorkerBlocker, profile: u128) -> Arc<ContentRules> {
    let (send, receive) = mpsc::sync_channel(1);
    assert_eq!(
        worker.compile(
            ProfileId::from(profile),
            ContentPolicyGeneration::new(1).unwrap(),
            BlockerConfig { enabled: true },
            Box::new(move |outcome| send.send(outcome).unwrap()),
        ),
        BlockerDispatch::Scheduled
    );
    let BlockerCompileOutcome::Compiled(rules) =
        receive.recv_timeout(Duration::from_secs(60)).unwrap()
    else {
        panic!("the release artifact must be recoverable without its source loader");
    };
    assert!(rules.enabled());
    assert!(rules.coverage().has_blocking_entries());
    rules
}

#[test]
fn release_seed_recovers_after_byte_release_and_restart_without_source_loading() {
    // Avoid OS-owned /var -> /private/var symlinks: production intentionally
    // refuses symlinked cache path components.
    let root = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity: [u8; 32] = Sha256::digest(MANIFEST).into();
    let loads = Arc::new(AtomicUsize::new(0));
    let observed = loads.clone();
    let cold_catalog = PolicyCatalog::deferred_reloadable(identity, move || {
        // No second inflate can mask a cache miss in this process.
        if observed.fetch_add(1, Ordering::SeqCst) != 0 {
            return Err(BlockerCompileFailure::SourceUnavailable);
        }
        let sources = SOURCES
            .iter()
            .map(|(id, gzip)| {
                let mut text = String::new();
                flate2::read::GzDecoder::new(*gzip)
                    .read_to_string(&mut text)
                    .unwrap();
                PolicySource::new(
                    SourceId::new(*id).unwrap(),
                    SourceFormat::Standard,
                    text.into(),
                )
            })
            .collect();
        Ok(StaticPolicyCatalog::new(sources).unwrap())
    });
    let config = CompiledArtifactCacheConfig::new(root.path()).unwrap();
    let cold = WorkerBlocker::start_with_policy_catalog(cold_catalog, config.clone()).unwrap();
    let first = compile(&cold, 1);
    let digest = first.digest();
    let coverage = first.coverage();
    drop(first);
    let recovered = compile(&cold, 2);
    assert_eq!(recovered.digest(), digest);
    assert_eq!(recovered.coverage(), coverage);
    drop(recovered);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(
        cold.shutdown_until(Instant::now() + Duration::from_secs(5)),
        BlockerShutdownOutcome::Clean
    );
    drop(cold);

    let observed = loads.clone();
    let warm_catalog = PolicyCatalog::deferred_reloadable(identity, move || {
        observed.fetch_add(1, Ordering::SeqCst);
        Err(BlockerCompileFailure::SourceUnavailable)
    });
    let warm = WorkerBlocker::start_with_policy_catalog(warm_catalog, config).unwrap();
    let recovered = compile(&warm, 3);
    assert_eq!(recovered.digest(), digest);
    assert_eq!(recovered.coverage(), coverage);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(
        warm.shutdown_until(Instant::now() + Duration::from_secs(5)),
        BlockerShutdownOutcome::Clean
    );
}
