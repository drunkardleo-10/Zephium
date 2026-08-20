use std::collections::VecDeque;
use std::sync::Mutex;

use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
use sha2::{Digest as _, Sha256};
use zephium_core::extensions::ExtensionPackageKey;
use zephium_core::ports::extensions::{
    ExtensionAcquiredRuntimeProfile, ExtensionAcquiredRuntimeSelection,
};
use zephium_extension_package::MAX_CRX3_HEADER_BYTES;

use super::*;
use crate::authentication::StructuralTestCatalogAuthenticator;

const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
    0x03, 0x01, 0x07,
];

pub(crate) struct ExpectedFetch {
    pub(crate) url: Url,
    pub(crate) max_bytes: usize,
    pub(crate) bytes: Box<[u8]>,
}

pub(crate) struct FakeTransport {
    pub(crate) expected: Mutex<VecDeque<ExpectedFetch>>,
}

impl ArtifactTransport for FakeTransport {
    async fn fetch_bounded(
        &self,
        url: Url,
        max_bytes: usize,
    ) -> Result<Box<[u8]>, FixedOriginFetchError> {
        let expected = self.expected.lock().unwrap().pop_front().unwrap();
        assert_eq!(url, expected.url);
        assert_eq!(max_bytes, expected.max_bytes);
        Ok(expected.bytes)
    }
}

pub(crate) struct Fixture {
    pub(crate) catalog: Box<[u8]>,
    pub(crate) crx: Box<[u8]>,
    pub(crate) legal: Box<[u8]>,
    pub(crate) package_key: ExtensionPackageKey,
    pub(crate) archive_length: usize,
    pub(crate) archive_sha256: [u8; 32],
    pub(crate) legal_sha256: [u8; 32],
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_session_fetches_content_derived_objects_and_builds_path_free_requests() {
    let fixture = fixture();
    let metadata = Url::parse("https://metadata.example/extensions/stable/").unwrap();
    let targets = Url::parse("https://objects.example/extensions/stable/").unwrap();
    let catalog_url = crate::layout::catalog_url(&metadata).unwrap();
    let legal_url = crate::layout::legal_notice_url(&targets, &fixture.legal_sha256).unwrap();
    let crx_url = crate::layout::crx3_url(
        &targets,
        fixture.package_key,
        zephium_core::extensions::ExtensionPackageRevision::INITIAL,
        zephium_core::extensions::ExtensionArchiveDigest::from_bytes(fixture.archive_sha256),
    )
    .unwrap();
    let transport = FakeTransport {
        expected: Mutex::new(VecDeque::from([
            ExpectedFetch {
                url: catalog_url.clone(),
                max_bytes: MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
                bytes: fixture.catalog.clone(),
            },
            ExpectedFetch {
                url: legal_url,
                max_bytes: fixture.legal.len(),
                bytes: fixture.legal.clone(),
            },
            ExpectedFetch {
                url: crx_url,
                max_bytes: fixture.archive_length + MAX_CRX3_HEADER_BYTES + CRX3_PREFIX_BYTES,
                bytes: fixture.crx.clone(),
            },
        ])),
    };
    let client = DistributionClient {
        transport,
        authenticator: StructuralTestCatalogAuthenticator,
        catalog_url,
        targets_base: targets,
    };
    let selection = ExtensionAcquiredRuntimeSelection::new_for_profile(
        fixture.package_key,
        ExtensionAcquiredRuntimeProfile::MacosNative,
    );
    let session = client.begin(vec![selection]).await.unwrap();
    assert_eq!(session.package_count(), 1);
    assert!(session.retained_bytes() <= crate::MAX_EXTENSION_DISTRIBUTION_SESSION_RETAINED_BYTES);

    let request = client.fetch_package(&session, 0).await.unwrap();
    let (catalog, key, profile, crx, legal) = request.into_parts();
    assert_eq!(catalog, fixture.catalog.as_ref());
    assert_eq!(key, fixture.package_key);
    assert_eq!(profile, ExtensionAcquiredRuntimeProfile::MacosNative);
    assert_eq!(crx, fixture.crx.as_ref());
    assert_eq!(legal, fixture.legal.as_ref());

    let activation = session.into_activation_request().unwrap();
    let (catalog, selections) = activation.into_parts();
    assert_eq!(catalog, fixture.catalog.as_ref());
    assert_eq!(selections, [selection]);
    assert!(client.transport.expected.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn incomplete_projection_is_rejected_before_any_package_object_is_requested() {
    let fixture = fixture();
    let metadata = Url::parse("https://metadata.example/extensions/stable/").unwrap();
    let targets = Url::parse("https://objects.example/extensions/stable/").unwrap();
    let catalog_url = crate::layout::catalog_url(&metadata).unwrap();
    let client = DistributionClient {
        transport: FakeTransport {
            expected: Mutex::new(VecDeque::from([ExpectedFetch {
                url: catalog_url.clone(),
                max_bytes: MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
                bytes: fixture.catalog,
            }])),
        },
        authenticator: StructuralTestCatalogAuthenticator,
        catalog_url,
        targets_base: targets,
    };
    assert_eq!(
        client.begin(Vec::new()).await.unwrap_err(),
        ExtensionDistributionError::InvalidSelection
    );
    assert!(client.transport.expected.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn unauthenticated_catalog_never_authorizes_a_secondary_request() {
    let mut fixture = fixture();
    fixture.catalog[0] = b'[';
    let metadata = Url::parse("https://metadata.example/extensions/stable/").unwrap();
    let targets = Url::parse("https://objects.example/extensions/stable/").unwrap();
    let catalog_url = crate::layout::catalog_url(&metadata).unwrap();
    let client = DistributionClient {
        transport: FakeTransport {
            expected: Mutex::new(VecDeque::from([ExpectedFetch {
                url: catalog_url.clone(),
                max_bytes: MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
                bytes: fixture.catalog,
            }])),
        },
        authenticator: StructuralTestCatalogAuthenticator,
        catalog_url,
        targets_base: targets,
    };
    let selection = ExtensionAcquiredRuntimeSelection::new_for_profile(
        fixture.package_key,
        ExtensionAcquiredRuntimeProfile::MacosNative,
    );
    assert_eq!(
        client.begin(vec![selection]).await.unwrap_err(),
        ExtensionDistributionError::CatalogRejected
    );
    assert!(client.transport.expected.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn legal_mismatch_is_rejected_before_downloading_the_larger_crx() {
    let fixture = fixture();
    let metadata = Url::parse("https://metadata.example/extensions/stable/").unwrap();
    let targets = Url::parse("https://objects.example/extensions/stable/").unwrap();
    let catalog_url = crate::layout::catalog_url(&metadata).unwrap();
    let legal_url = crate::layout::legal_notice_url(&targets, &fixture.legal_sha256).unwrap();
    let crx_url = crate::layout::crx3_url(
        &targets,
        fixture.package_key,
        zephium_core::extensions::ExtensionPackageRevision::INITIAL,
        zephium_core::extensions::ExtensionArchiveDigest::from_bytes(fixture.archive_sha256),
    )
    .unwrap();
    let client = DistributionClient {
        transport: FakeTransport {
            expected: Mutex::new(VecDeque::from([
                ExpectedFetch {
                    url: catalog_url.clone(),
                    max_bytes: MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
                    bytes: fixture.catalog.clone(),
                },
                ExpectedFetch {
                    url: legal_url,
                    max_bytes: fixture.legal.len(),
                    bytes: Box::from(b"y".as_slice()),
                },
                ExpectedFetch {
                    url: crx_url,
                    max_bytes: fixture.archive_length + MAX_CRX3_HEADER_BYTES + CRX3_PREFIX_BYTES,
                    bytes: fixture.crx,
                },
            ])),
        },
        authenticator: StructuralTestCatalogAuthenticator,
        catalog_url,
        targets_base: targets,
    };
    let selection = ExtensionAcquiredRuntimeSelection::new_for_profile(
        fixture.package_key,
        ExtensionAcquiredRuntimeProfile::MacosNative,
    );
    let session = client.begin(vec![selection]).await.unwrap();
    assert_eq!(
        client.fetch_package(&session, 0).await.unwrap_err(),
        ExtensionDistributionError::LegalNoticeRejected
    );
    assert_eq!(client.transport.expected.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_catalog_path_cannot_substitute_an_invalid_crx_envelope() {
    let mut fixture = fixture();
    let last = fixture.crx.len() - 1;
    fixture.crx[last] ^= 1;
    let metadata = Url::parse("https://metadata.example/extensions/stable/").unwrap();
    let targets = Url::parse("https://objects.example/extensions/stable/").unwrap();
    let catalog_url = crate::layout::catalog_url(&metadata).unwrap();
    let legal_url = crate::layout::legal_notice_url(&targets, &fixture.legal_sha256).unwrap();
    let crx_url = crate::layout::crx3_url(
        &targets,
        fixture.package_key,
        zephium_core::extensions::ExtensionPackageRevision::INITIAL,
        zephium_core::extensions::ExtensionArchiveDigest::from_bytes(fixture.archive_sha256),
    )
    .unwrap();
    let client = DistributionClient {
        transport: FakeTransport {
            expected: Mutex::new(VecDeque::from([
                ExpectedFetch {
                    url: catalog_url.clone(),
                    max_bytes: MAX_EXTENSION_ACQUIRED_CATALOG_BYTES,
                    bytes: fixture.catalog.clone(),
                },
                ExpectedFetch {
                    url: legal_url,
                    max_bytes: fixture.legal.len(),
                    bytes: fixture.legal,
                },
                ExpectedFetch {
                    url: crx_url,
                    max_bytes: fixture.archive_length + MAX_CRX3_HEADER_BYTES + CRX3_PREFIX_BYTES,
                    bytes: fixture.crx,
                },
            ])),
        },
        authenticator: StructuralTestCatalogAuthenticator,
        catalog_url,
        targets_base: targets,
    };
    let selection = ExtensionAcquiredRuntimeSelection::new_for_profile(
        fixture.package_key,
        ExtensionAcquiredRuntimeProfile::MacosNative,
    );
    let session = client.begin(vec![selection]).await.unwrap();
    assert_eq!(
        client.fetch_package(&session, 0).await.unwrap_err(),
        ExtensionDistributionError::CrxRejected
    );
    assert!(client.transport.expected.lock().unwrap().is_empty());
}

#[test]
fn client_construction_tracks_product_authority_availability() {
    let result = ExtensionDistributionClient::new(
        Url::parse("https://metadata.example/extensions/stable/").unwrap(),
        Url::parse("https://objects.example/extensions/stable/").unwrap(),
        Duration::from_secs(30),
        Duration::from_secs(10),
    );
    match zephium_extension_authority::BundledPackageAuthority::product_status() {
        zephium_extension_authority::BundledProductAuthorityStatus::Unprovisioned => assert_eq!(
            result.unwrap_err(),
            ExtensionDistributionClientError::ProductAuthorityUnavailable
        ),
        zephium_extension_authority::BundledProductAuthorityStatus::Configured => {
            assert!(result.is_ok())
        }
        zephium_extension_authority::BundledProductAuthorityStatus::InvalidProvisioning => {
            assert_eq!(
                result.unwrap_err(),
                ExtensionDistributionClientError::ProductAuthorityInvalid
            )
        }
    }
}

#[cfg(feature = "staging-extension-catalog")]
#[tokio::test(flavor = "current_thread")]
async fn staging_client_fetches_only_the_exact_embedded_catalog_objects() {
    let authority =
        zephium_extension_authority::ProductExtensionManifestAuthority::product().unwrap();
    let selections = authority
        .active_acquired_runtime_selections_for_targets(&[
            zephium_extension_authority::ProductExtensionRuntimeTarget::MacosNative,
            zephium_extension_authority::ProductExtensionRuntimeTarget::MacosNativeBrokered,
        ])
        .unwrap();
    let client = ExtensionDistributionClient::staging().unwrap();
    let session = client.begin(selections).await.unwrap();
    assert_eq!(session.package_count(), 2);
    let mut saw_native = false;
    let mut saw_brokered = false;
    for index in 0..session.package_count() {
        let selection = session.selection(index).unwrap();
        let selected_package = selection.package_key();
        let selected_profile = selection.runtime_profile();
        let expected = match selected_profile {
            ExtensionAcquiredRuntimeProfile::MacosNative => {
                saw_native = true;
                (
                    crate::staging::DARK_READER_CRX3_BYTES,
                    crate::staging::DARK_READER_LEGAL_BYTES,
                )
            }
            ExtensionAcquiredRuntimeProfile::MacosNativeBrokered => {
                saw_brokered = true;
                (
                    crate::staging::VIMIUM_CRX3_BYTES,
                    crate::staging::VIMIUM_LEGAL_BYTES,
                )
            }
            profile => panic!("unexpected staging runtime profile {profile:?}"),
        };
        let request = client.fetch_package(&session, index).await.unwrap();
        assert_eq!(request.runtime_profile(), selected_profile);
        let (catalog, package, profile, crx, legal) = request.into_parts();
        assert_eq!(catalog, crate::staging::CATALOG_BYTES);
        assert_eq!(package, selected_package);
        assert_eq!(profile, selected_profile);
        assert_eq!(crx, expected.0);
        assert_eq!(legal, expected.1);
    }
    assert!(saw_native && saw_brokered);

    let transport = EmbeddedStagingArtifactTransport;
    assert_eq!(
        transport
            .fetch_bounded(
                Url::parse("https://staging.extensions.zephium.invalid/v1/targets/other").unwrap(),
                1,
            )
            .unwrap_err(),
        FixedOriginFetchError::Boundary
    );
}

pub(crate) fn fixture() -> Fixture {
    let archive = b"PK\x03\x04zephium-extension-distribution-fixture".to_vec();
    let archive_sha256: [u8; 32] = Sha256::digest(&archive).into();
    let legal = Box::<[u8]>::from(b"x".as_slice());
    let legal_sha256: [u8; 32] = Sha256::digest(&legal).into();
    let (crx, developer_key_sha256) = signed_crx(&archive);
    let package_key = ExtensionPackageKey::from_bytes([3; 32]);
    let catalog = format!(
        concat!(
            "{{\"schema_version\":1,\"catalog_revision\":1,\"created_unix\":1,",
            "\"authority_id\":\"{authority}\",\"admission_policy_sha256\":\"{policy}\",",
            "\"packages\":[{{\"package_key\":\"{key}\",\"revision\":1,",
            "\"payload\":{{\"kind\":\"acquired_zip\",\"length\":{archive_length},",
            "\"sha256\":\"{archive_digest}\"}},\"manifest_sha256\":\"{manifest}\",",
            "\"tree_sha256\":\"{tree}\",\"tree_index_sha256\":\"{index}\",",
            "\"tree_index_length\":1,\"tree_file_count\":1,\"tree_bytes\":1,",
            "\"chromium\":{{\"manifest_key_sha256\":\"{developer}\"}},",
            "\"provenance\":{{\"source_url\":\"https://example.com/source\",",
            "\"upstream_version\":\"1\",",
            "\"upstream_revision\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",",
            "\"license_expression\":\"MPL-2.0\",\"attribution\":\"Fixture\",",
            "\"redistribution\":\"Test fixture\",\"legal_notice\":{{",
            "\"target\":\"licenses/fixture.txt\",\"kind\":\"notice_bundle\",",
            "\"length\":1,\"sha256\":\"{legal_digest}\"}},",
            "\"corresponding_source\":null}}}}]}}"
        ),
        authority = hex(&[1; 32]),
        policy = hex(&[2; 32]),
        key = hex(package_key.as_bytes()),
        archive_length = archive.len(),
        archive_digest = hex(&archive_sha256),
        manifest = hex(&[4; 32]),
        tree = hex(&[5; 32]),
        index = hex(&[6; 32]),
        developer = hex(&developer_key_sha256),
        legal_digest = hex(&legal_sha256),
    )
    .into_bytes()
    .into_boxed_slice();
    zephium_extension_package::ExtensionReleaseCatalog::parse_canonical(&catalog).unwrap();
    Fixture {
        catalog,
        crx: crx.into_boxed_slice(),
        legal,
        package_key,
        archive_length: archive.len(),
        archive_sha256,
        legal_sha256,
    }
}

fn signed_crx(archive: &[u8]) -> (Vec<u8>, [u8; 32]) {
    let random = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
    let pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random).unwrap();
    let mut public_key = vec![0x30, 0x59, 0x30, 0x13];
    public_key.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
    public_key.extend_from_slice(&[0x03, 0x42, 0x00]);
    public_key.extend_from_slice(pair.public_key().as_ref());
    let developer_key_sha256: [u8; 32] = Sha256::digest(&public_key).into();
    let mut signed_header = Vec::new();
    push_bytes_field(&mut signed_header, 1, &developer_key_sha256[..16]);
    let mut message = b"CRX3 SignedData\0".to_vec();
    message.extend_from_slice(&(signed_header.len() as u32).to_le_bytes());
    message.extend_from_slice(&signed_header);
    message.extend_from_slice(archive);
    let signature = pair.sign(&random, &message).unwrap();
    let mut proof = Vec::new();
    push_bytes_field(&mut proof, 1, &public_key);
    push_bytes_field(&mut proof, 2, signature.as_ref());
    let mut header = Vec::new();
    push_bytes_field(&mut header, 3, &proof);
    push_bytes_field(&mut header, 10_000, &signed_header);
    let mut crx = b"Cr24".to_vec();
    crx.extend_from_slice(&3_u32.to_le_bytes());
    crx.extend_from_slice(&(header.len() as u32).to_le_bytes());
    crx.extend_from_slice(&header);
    crx.extend_from_slice(archive);
    (crx, developer_key_sha256)
}

fn push_varint(bytes: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn push_bytes_field(bytes: &mut Vec<u8>, number: u64, value: &[u8]) {
    push_varint(bytes, (number << 3) | 2);
    push_varint(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
