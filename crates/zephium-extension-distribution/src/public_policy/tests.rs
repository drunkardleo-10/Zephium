use std::collections::BTreeMap;

use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tough::schema::{key::Key, Root, Snapshot, Targets, Timestamp};

use super::*;

pub(crate) struct Fixture {
    keys: Vec<Ed25519KeyPair>,
    ids: Vec<String>,
    root: Vec<u8>,
    files: BTreeMap<String, Vec<u8>>,
    pub(crate) policy: Value,
    time: u64,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn expires(time: u64) -> String {
    jiff::Timestamp::from_second(time as i64)
        .unwrap()
        .to_string()
}

fn signed<T: Role + DeserializeOwned>(value: Value, keys: &[(&str, &Ed25519KeyPair)]) -> Vec<u8> {
    let role: T = serde_json::from_value(value.clone()).unwrap();
    let message = role.canonical_form().unwrap();
    let signatures = keys
        .iter()
        .map(|(id, key)| json!({"keyid":id,"sig":hex(key.sign(&message).as_ref())}))
        .collect::<Vec<_>>();
    serde_json::to_vec(&json!({"signed":value,"signatures":signatures})).unwrap()
}

impl Fixture {
    pub(crate) fn new() -> Self {
        Self::with_seed(1)
    }
    fn with_seed(seed: u8) -> Self {
        let time = now().unwrap();
        let keys = (seed..seed + 6)
            .map(|seed| Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap())
            .collect::<Vec<_>>();
        let mut public = serde_json::Map::new();
        let ids = keys.iter().map(|pair| {
            let value = json!({"keytype":"ed25519","scheme":"ed25519","keyval":{"public":hex(pair.public_key().as_ref())}});
            let key: Key = serde_json::from_value(value.clone()).unwrap();
            let id = hex(key.key_id().unwrap().as_ref());
            public.insert(id.clone(), value);
            id
        }).collect::<Vec<_>>();
        let root = signed::<Root>(
            json!({
                "_type":"root", "spec_version":"1.0.0", "consistent_snapshot":true,
                "version":1, "expires":expires(time+86400*365), "keys":public,
                "roles": {
                    "root":{"keyids":ids[..3],"threshold":2},
                    "timestamp":{"keyids":[ids[3]],"threshold":1},
                    "snapshot":{"keyids":[ids[4]],"threshold":1},
                    "targets":{"keyids":[ids[5]],"threshold":1}
                }
            }),
            &[(&ids[0], &keys[0]), (&ids[1], &keys[1])],
        );
        let policy = json!({
            "schema_version":1,"policy_revision":1,"channel":"staging",
            "issued_unix":time-60,"expires_unix":time+3600,
            "beta_targets":[],"recommendations":[],"revocations":[]
        });
        let mut fixture = Self {
            keys,
            ids,
            root,
            files: BTreeMap::new(),
            policy,
            time,
        };
        fixture.publish(1);
        fixture
    }

    fn put(&mut self, directory: &str, name: &str, bytes: Vec<u8>) {
        self.files.insert(
            format!("https://extensions.zephium.app/v1/staging/{directory}/{name}"),
            bytes,
        );
    }

    pub(crate) fn publish(&mut self, version: u64) {
        let policy = serde_json::to_vec(&self.policy).unwrap();
        let hash = hex(&Sha256::digest(&policy));
        let target = signed::<Targets>(
            json!({
                "_type":"targets", "spec_version":"1.0.0", "version":version,
                "expires":expires(self.time+1800),
                "targets":{EXTENSION_PUBLIC_POLICY_TARGET:{"length":policy.len(),"hashes":{"sha256":hash}}}
            }),
            &[(&self.ids[5], &self.keys[5])],
        );
        let snapshot = signed::<Snapshot>(
            json!({
                "_type":"snapshot", "spec_version":"1.0.0", "version":version,
                "expires":expires(self.time+1800),
                "meta":{"targets.json":{"version":version,"length":target.len(),"hashes":{"sha256":hex(&Sha256::digest(&target))}}}
            }),
            &[(&self.ids[4], &self.keys[4])],
        );
        let timestamp = signed::<Timestamp>(
            json!({
                "_type":"timestamp", "spec_version":"1.0.0", "version":version,
                "expires":expires(self.time+900),
                "meta":{"snapshot.json":{"version":version,"length":snapshot.len(),"hashes":{"sha256":hex(&Sha256::digest(&snapshot))}}}
            }),
            &[(&self.ids[3], &self.keys[3])],
        );
        self.put(
            "targets",
            &format!("{hash}.{EXTENSION_PUBLIC_POLICY_TARGET}"),
            policy,
        );
        self.put("metadata", &format!("{version}.targets.json"), target);
        self.put("metadata", &format!("{version}.snapshot.json"), snapshot);
        self.put("metadata", "timestamp.json", timestamp);
    }

    pub(crate) fn client(&self) -> ExtensionPolicyClient {
        let (metadata, targets) = transport::bases(ExtensionPolicyChannel::Staging).unwrap();
        ExtensionPolicyClient {
            root: Arc::from(self.root.clone()),
            channel: ExtensionPolicyChannel::Staging,
            transport: PolicyTransport::new(
                metadata,
                targets,
                Source::Fixture(Arc::new(self.files.clone())),
            ),
            in_flight: tokio::sync::Semaphore::new(1),
        }
    }
}

#[test]
fn production_trust_is_absent_in_both_channels() {
    for channel in [
        ExtensionPolicyChannel::Stable,
        ExtensionPolicyChannel::Staging,
    ] {
        assert_eq!(
            ExtensionPolicyClient::product(channel).unwrap_err(),
            ExtensionPolicyError::TrustUnavailable
        );
    }
}

#[tokio::test]
async fn refresh_is_single_flight_and_a_released_owner_can_retry() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let held = client.in_flight.try_acquire().unwrap();
    assert_eq!(
        client.refresh(None).await.unwrap_err(),
        ExtensionPolicyError::Busy
    );
    drop(held);
    assert!(client.refresh(None).await.is_ok());
}

#[tokio::test]
async fn real_signed_policy_is_exact_fresh_and_checkpoint_round_trips() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let candidate = client.refresh(None).await.unwrap();
    assert_eq!(client.transport.cached_entries(), 4);
    assert_eq!(
        candidate.bytes(),
        serde_json::to_vec(&fixture.policy).unwrap()
    );
    assert!(candidate.policy_at(fixture.time + 899).is_ok());
    // Timestamp expires sooner than the policy. A cached payload cannot extend it.
    assert_eq!(
        candidate.policy_at(fixture.time + 900).unwrap_err(),
        ExtensionPolicyError::Time
    );
    assert_eq!(
        candidate.policy_at(fixture.time - 1).unwrap_err(),
        ExtensionPolicyError::Time
    );
    let restored =
        ExtensionPolicyCheckpoint::decode(&candidate.checkpoint().encode().unwrap()).unwrap();
    assert_eq!(&restored, candidate.checkpoint());
    // The same client now gets simulated conditional 304 responses, which
    // must still complete the entire verification path.
    let repeated = client.refresh(Some(&restored)).await.unwrap();
    assert_eq!(repeated.previous_checkpoint(), Some(&restored));
}

#[tokio::test]
async fn policy_and_role_rollback_and_equivocation_survive_client_recreation() {
    let mut fixture = Fixture::new();
    let old = fixture.client().refresh(None).await.unwrap();
    fixture.policy["policy_revision"] = json!(2);
    fixture.publish(2);
    let new = fixture
        .client()
        .refresh(Some(old.checkpoint()))
        .await
        .unwrap();
    fixture.publish(1);
    assert_eq!(
        fixture
            .client()
            .refresh(Some(new.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Rollback
    );
    fixture.policy["policy_revision"] = json!(1);
    fixture.publish(3);
    assert_eq!(
        fixture
            .client()
            .refresh(Some(new.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Rollback
    );
    fixture.policy["policy_revision"] = json!(2);
    fixture.policy["beta_targets"] =
        json!([{"target":"macos.wkwebextension.v1","policy_version":1}]);
    fixture.publish(3);
    assert_eq!(
        fixture
            .client()
            .refresh(Some(new.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Rollback
    );
    // Same role version with changed signed content is rejected even when
    // policy_revision advances and all signatures are valid.
    fixture.policy["policy_revision"] = json!(3);
    fixture.publish(2);
    assert_eq!(
        fixture
            .client()
            .refresh(Some(new.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Rollback
    );
}

#[tokio::test]
async fn wrong_channel_or_trust_domain_never_reuses_a_checkpoint() {
    let fixture = Fixture::new();
    let checkpoint = fixture.client().refresh(None).await.unwrap();
    assert_eq!(
        Fixture::with_seed(20)
            .client()
            .refresh(Some(checkpoint.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Checkpoint
    );
    let mut client = fixture.client();
    client.channel = ExtensionPolicyChannel::Stable;
    assert_eq!(
        client.refresh(None).await.unwrap_err(),
        ExtensionPolicyError::Authentication
    );
    assert_eq!(
        client
            .refresh(Some(checkpoint.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Checkpoint
    );
}

#[tokio::test]
async fn tampering_duplicate_signatures_and_duplicate_json_are_rejected() {
    for mode in 0..4 {
        let mut fixture = Fixture::new();
        let name = fixture
            .files
            .keys()
            .find(|name| {
                if mode == 0 {
                    name.contains("/targets/")
                } else {
                    name.ends_with("timestamp.json")
                }
            })
            .unwrap()
            .clone();
        let bytes = fixture.files.get_mut(&name).unwrap();
        match mode {
            0 => bytes.push(b' '),
            1 => {
                let mut value: Value = serde_json::from_slice(bytes).unwrap();
                value["signatures"][0]["sig"] = json!("00".repeat(64));
                *bytes = serde_json::to_vec(&value).unwrap();
            }
            2 => {
                let mut value: Value = serde_json::from_slice(bytes).unwrap();
                let duplicate = value["signatures"][0].clone();
                value["signatures"].as_array_mut().unwrap().push(duplicate);
                *bytes = serde_json::to_vec(&value).unwrap();
            }
            _ => {
                bytes.splice(1..1, b"\"signatures\":[],".iter().copied());
            }
        }
        let client = fixture.client();
        assert!(client.refresh(None).await.is_err(), "mode {mode}");
        assert_eq!(
            client.transport.cached_entries(),
            0,
            "unsigned bytes cannot seed tracking hashes"
        );
    }
}

#[tokio::test]
async fn expired_future_policy_and_clock_rollback_fail_closed() {
    let mut fixture = Fixture::new();
    let candidate = fixture.client().refresh(None).await.unwrap();
    let mut value: Value =
        serde_json::from_slice(&candidate.checkpoint().encode().unwrap()).unwrap();
    value["trusted_unix"] = json!(fixture.time + 100);
    let future = ExtensionPolicyCheckpoint::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        fixture.client().refresh(Some(&future)).await.unwrap_err(),
        ExtensionPolicyError::Time
    );
    for (issued, expires) in [
        (fixture.time - 100, fixture.time - 1),
        (fixture.time + 100, fixture.time + 200),
    ] {
        fixture.policy["issued_unix"] = json!(issued);
        fixture.policy["expires_unix"] = json!(expires);
        fixture.publish(1);
        assert_eq!(
            fixture.client().refresh(None).await.unwrap_err(),
            ExtensionPolicyError::Time
        );
    }
}

#[tokio::test]
async fn publisher_revocation_cannot_disappear_in_a_new_signed_policy() {
    let mut fixture = Fixture::new();
    fixture.policy["revocations"] = json!([{
        "extension_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "developer_key_sha256":"00".repeat(32),
        "original_crx_sha256":null, "reason":"Publisher compromise"
    }]);
    fixture.publish(1);
    let old = fixture.client().refresh(None).await.unwrap();
    fixture.policy["policy_revision"] = json!(2);
    fixture.policy["revocations"] = json!([]);
    fixture.publish(2);
    assert_eq!(
        fixture
            .client()
            .refresh(Some(old.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::RevocationRemoved
    );
}

#[tokio::test]
async fn root_rotation_requires_old_and_new_thresholds_and_preserves_high_water() {
    let original = Fixture::new();
    let old = original.client().refresh(None).await.unwrap();
    let mut rotated = Fixture::with_seed(20);
    let mut document: Value = serde_json::from_slice(&rotated.root).unwrap();
    document["signed"]["version"] = json!(2);
    let root = signed::<Root>(
        document["signed"].clone(),
        &[
            (&original.ids[0], &original.keys[0]),
            (&original.ids[1], &original.keys[1]),
            (&rotated.ids[0], &rotated.keys[0]),
            (&rotated.ids[1], &rotated.keys[1]),
        ],
    );
    rotated.root = original.root;
    rotated.put("metadata", "2.root.json", root.clone());
    rotated.policy["policy_revision"] = json!(2);
    rotated.publish(2);
    let new = rotated
        .client()
        .refresh(Some(old.checkpoint()))
        .await
        .unwrap();
    assert_eq!(new.policy_at(now().unwrap()).unwrap().revision(), 2);
    rotated.publish(1);
    assert_eq!(
        rotated
            .client()
            .refresh(Some(new.checkpoint()))
            .await
            .unwrap_err(),
        ExtensionPolicyError::Rollback
    );
    // A root signed only by its own new keys cannot rotate the bootstrap trust.
    let mut invalid: Value = serde_json::from_slice(&root).unwrap();
    invalid["signatures"].as_array_mut().unwrap().drain(..2);
    rotated.put(
        "metadata",
        "2.root.json",
        serde_json::to_vec(&invalid).unwrap(),
    );
    assert!(rotated.client().refresh(None).await.is_err());
}

#[test]
fn malformed_checkpoint_is_bounded_and_not_silently_reset() {
    assert!(ExtensionPolicyCheckpoint::decode(&vec![b' '; 21 * 1024]).is_err());
    assert!(ExtensionPolicyCheckpoint::decode(b"{}").is_err());
    assert!(ExtensionPolicyCheckpoint::decode(b"{\"schema\":1,\"schema\":1}").is_err());
}

#[test]
fn bootstrap_version_is_bounded_before_tuf_can_increment_it() {
    let fixture = Fixture::new();
    let mut root: Value = serde_json::from_slice(&fixture.root).unwrap();
    root["signed"]["version"] = json!(u64::MAX);
    assert_eq!(
        transport::preflight(&serde_json::to_vec(&root).unwrap(), transport::Object::Root),
        Err(ExtensionPolicyError::Authentication)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod durable {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use zephium_private_fs::LockedPrivateNamespace;

    fn directory() -> tempfile::TempDir {
        #[cfg(target_os = "macos")]
        let directory = tempfile::tempdir_in("/private/tmp").unwrap();
        #[cfg(target_os = "linux")]
        let directory = tempfile::tempdir_in("/tmp").unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }
    fn open(directory: &tempfile::TempDir, client: &ExtensionPolicyClient) -> ExtensionPolicyCache {
        ExtensionPolicyCache::open(
            LockedPrivateNamespace::open_or_create(directory.path().join("policy")).unwrap(),
            client,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn durable_restart_retains_high_water_and_invalidates_superseded_receipts() {
        let mut fixture = Fixture::new();
        let old_client = fixture.client();
        let directory = directory();
        let mut cache = open(&directory, &old_client);
        let old = cache.refresh(&old_client).await.unwrap();
        assert_eq!(old.policy().unwrap().revision(), 1);
        fixture.policy["policy_revision"] = json!(2);
        fixture.publish(2);
        let new_client = fixture.client();
        let new = cache.refresh(&new_client).await.unwrap();
        assert_eq!(
            old.policy().unwrap_err(),
            ExtensionPolicyError::StaleCandidate
        );
        assert_eq!(new.policy().unwrap().revision(), 2);
        let checkpoint = new.checkpoint().clone();
        drop(cache);
        assert_eq!(
            new.policy().unwrap_err(),
            ExtensionPolicyError::StaleCandidate
        );
        let mut cache = open(&directory, &new_client);
        assert_eq!(cache.checkpoint(), Some(&checkpoint));
        assert_eq!(
            cache.refresh(&old_client).await.unwrap_err(),
            ExtensionPolicyError::Rollback
        );
        assert_eq!(
            cache
                .refresh(&new_client)
                .await
                .unwrap()
                .policy()
                .unwrap()
                .revision(),
            2
        );
    }

    #[tokio::test]
    async fn stale_parallel_candidate_cannot_overwrite_an_accepted_update() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let directory = directory();
        let mut cache = open(&directory, &client);
        let first = client.refresh(None).await.unwrap();
        let second = client.refresh(None).await.unwrap();
        let accepted = cache.accept(first).unwrap();
        assert_eq!(
            cache.accept(second).unwrap_err(),
            ExtensionPolicyError::StaleCandidate
        );
        assert!(accepted.policy().is_ok());
        assert_eq!(cache.checkpoint(), Some(accepted.checkpoint()));
    }

    #[tokio::test]
    async fn interrupted_staging_never_promotes_bytes_and_reopen_recovers() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let directory = directory();
        let mut cache = open(&directory, &client);
        let accepted = cache.refresh(&client).await.unwrap();
        let checkpoint = accepted.checkpoint().clone();
        let current = directory.path().join("policy/staging/current");
        let before = fs::read(&current).unwrap();
        let staged = directory.path().join("policy/staging/staged");
        fs::write(&staged, b"interrupted uncommitted bytes").unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            cache.refresh(&client).await.unwrap_err(),
            ExtensionPolicyError::Storage
        );
        assert_eq!(fs::read(&current).unwrap(), before);
        assert_eq!(
            accepted.policy().unwrap_err(),
            ExtensionPolicyError::StaleCandidate
        );
        drop(cache);
        let mut recovered = open(&directory, &client);
        assert_eq!(recovered.checkpoint(), Some(&checkpoint));
        assert!(!staged.exists());
        assert!(recovered.refresh(&client).await.unwrap().policy().is_ok());
    }

    #[tokio::test]
    async fn corrupt_or_redirected_current_state_is_never_reset_to_empty() {
        for redirected in [false, true] {
            let fixture = Fixture::new();
            let client = fixture.client();
            let directory = directory();
            let mut cache = open(&directory, &client);
            cache.refresh(&client).await.unwrap();
            drop(cache);
            let current = directory.path().join("policy/staging/current");
            if redirected {
                let external = directory.path().join("outside");
                fs::rename(&current, &external).unwrap();
                std::os::unix::fs::symlink(external, &current).unwrap();
            } else {
                let mut bytes = fs::read(&current).unwrap();
                bytes.push(b' '); // Valid JSON, but the exact policy digest changed.
                fs::write(&current, bytes).unwrap();
            }
            let namespace =
                LockedPrivateNamespace::open_or_create(directory.path().join("policy")).unwrap();
            assert!(ExtensionPolicyCache::open(namespace, &client).is_err());
            assert!(current.symlink_metadata().is_ok());
        }
    }

    #[tokio::test]
    async fn elapsed_monotonic_deadline_cannot_be_extended_by_wall_clock() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let directory = directory();
        let mut cache = open(&directory, &client);
        let mut candidate = client.refresh(None).await.unwrap();
        candidate.fresh_until = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            cache.accept(candidate).unwrap_err(),
            ExtensionPolicyError::Time
        );
        assert!(cache.checkpoint().is_none());
        assert!(!directory.path().join("policy/staging/current").exists());
    }
}

#[tokio::test]
async fn largest_publisher_revocation_cohort_fits_the_durable_checkpoint() {
    let mut fixture = Fixture::new();
    fixture.policy["revocations"] = Value::Array((0u8..=255).map(|index| {
        let key = [index; 32];
        let id = format!("{}{}", char::from(b'a' + (index >> 4)), char::from(b'a' + (index & 15))).repeat(16);
        json!({"extension_id":id,"developer_key_sha256":hex(&key),"original_crx_sha256":null,"reason":"Fixture revocation"})
    }).collect());
    fixture.publish(1);
    let candidate = fixture.client().refresh(None).await.unwrap();
    assert_eq!(
        candidate
            .policy_at(now().unwrap())
            .unwrap()
            .revocations()
            .len(),
        256
    );
    let checkpoint =
        ExtensionPolicyCheckpoint::decode(&candidate.checkpoint().encode().unwrap()).unwrap();
    assert_eq!(&checkpoint, candidate.checkpoint());
}
