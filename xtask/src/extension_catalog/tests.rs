use super::*;
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

const P256_ALGORITHM_IDENTIFIER: &[u8] = &[
    0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d,
    0x03, 0x01, 0x07,
];

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn p256_spki(point: &[u8]) -> Vec<u8> {
    let mut spki = vec![0x30, 0x59, 0x30, 0x13];
    spki.extend_from_slice(P256_ALGORITHM_IDENTIFIER);
    spki.extend_from_slice(&[0x03, 0x42, 0x00]);
    spki.extend_from_slice(point);
    spki
}

fn fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    write(
        &source,
        "manifest.json",
        br#"{"background":{"service_worker":"worker.js"},"manifest_version":3,"name":"Publisher fixture","version":"1.0.0"}"#,
    );
    write(&source, "worker.js", b"globalThis.ready = true;");
    let index = root.join("source-tree.json");
    fs::write(
        &index,
        extension_tree::build_tree_index(&source).unwrap().bytes,
    )
    .unwrap();

    let random = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
    let pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random).unwrap();
    let public_key = p256_spki(pair.public_key().as_ref());
    let public_key_path = root.join("public-key.der");
    fs::write(&public_key_path, &public_key).unwrap();
    let compatibility = root.join("compatibility");
    crate::macos_extension_compatibility::materialize(&source, &index, &compatibility).unwrap();
    let prepared = root.join("prepared");
    crate::extension_release::prepare_compatibility(&compatibility, &public_key_path, &prepared)
        .unwrap();
    let archive = fs::read(prepared.join("extension.zip")).unwrap();
    let request =
        zephium_extension_package::Crx3SigningRequest::new_ecdsa_p256_sha256(&archive, &public_key)
            .unwrap();
    let mut message = Vec::new();
    for part in request.signed_message_parts() {
        message.extend_from_slice(part);
    }
    let signature = pair.sign(&random, &message).unwrap();
    let crx = request.finish(signature.as_ref()).unwrap();
    let crx_path = root.join("fixture.crx3");
    fs::write(&crx_path, crx).unwrap();
    let legal = root.join("NOTICE.txt");
    fs::write(&legal, b"Publisher fixture legal notice\n").unwrap();
    (prepared, crx_path, legal)
}

fn review(root: &Path) -> PathBuf {
    let review = PublicationReview {
        schema: 1,
        catalog_revision: 1,
        created_unix: 1,
        authority_id: "11".repeat(32),
        admission_policy: ReviewedAdmissionPolicy {
            digest_sha256: "22".repeat(32),
            license_rules: vec![ReviewedLicenseRule {
                expression: "MIT".into(),
                corresponding_source_required: false,
            }],
        },
        packages: vec![ReviewedPackage {
            package_line: "publisher-fixture".into(),
            package_revision: 1,
            release_archive: "prepared".into(),
            signed_crx3: "fixture.crx3".into(),
            legal_notice: "NOTICE.txt".into(),
            source_url: "https://example.com/publisher-fixture/source".into(),
            upstream_version: "1.0.0".into(),
            upstream_revision: "aa".repeat(20),
            license_expression: "MIT".into(),
            attribution: "Publisher fixture contributors".into(),
            redistribution: "Test-only reviewed redistribution".into(),
            corresponding_source: None,
        }],
    };
    let path = root.join("review.json");
    fs::write(&path, serde_json::to_vec(&review).unwrap()).unwrap();
    path
}

#[test]
fn publication_is_deterministic_content_addressed_and_non_authorizing() {
    let temporary = tempfile::tempdir().unwrap();
    fixture(temporary.path());
    let review = review(temporary.path());
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    publish(&review, &first).unwrap();
    publish(&review, &second).unwrap();

    for relative in [
        CATALOG_TARGET,
        PRODUCT_ANCHORS_TARGET,
        MANIFEST_INPUTS_TARGET,
        PUBLICATION_EVIDENCE_TARGET,
    ] {
        assert_eq!(
            fs::read(first.join(relative)).unwrap(),
            fs::read(second.join(relative)).unwrap()
        );
    }
    let catalog_bytes = fs::read(first.join(CATALOG_TARGET)).unwrap();
    let catalog = ExtensionReleaseCatalog::parse_canonical(&catalog_bytes).unwrap();
    assert_eq!(catalog.packages().len(), 1);
    let package = &catalog.packages()[0];
    let (_, archive_sha256) = package.payload().acquired_zip_evidence().unwrap();
    let crx_target = format!(
        "targets/crx3/{}/1/{}.crx3",
        lower_hex(package.identity().key().as_bytes()),
        lower_hex(archive_sha256.as_bytes())
    );
    assert_eq!(
        fs::read(first.join(&crx_target)).unwrap(),
        fs::read(temporary.path().join("fixture.crx3")).unwrap()
    );
    let legal_target = format!(
        "targets/legal/{}.notice",
        lower_hex(&package.provenance().legal_notice().sha256())
    );
    assert_eq!(
        fs::read(first.join(legal_target)).unwrap(),
        b"Publisher fixture legal notice\n"
    );
    let mut manifest_inputs: serde_json::Value =
        serde_json::from_slice(&fs::read(first.join(MANIFEST_INPUTS_TARGET)).unwrap()).unwrap();
    assert_eq!(manifest_inputs["product_authority"], false);
    assert_eq!(manifest_inputs["classification_settled"], false);
    let profile = &manifest_inputs["profiles"][0];
    let manifest_target = profile["manifest_target"].as_str().unwrap();
    let tree_index_target = profile["tree_index_target"].as_str().unwrap();
    assert_eq!(
        lower_hex(&Sha256::digest(
            fs::read(first.join(manifest_target)).unwrap()
        )),
        profile["manifest_sha256"].as_str().unwrap()
    );
    assert_eq!(
        lower_hex(&Sha256::digest(
            fs::read(first.join(tree_index_target)).unwrap()
        )),
        profile["tree_index_sha256"].as_str().unwrap()
    );
    assert_eq!(profile["declarations"][0]["level"], "unassessed");
    assert!(profile["declarations"]
        .as_array()
        .is_some_and(|rows| rows.len() >= 2));
    manifest_inputs["kind"] = serde_json::Value::String(MANIFEST_REVIEW_KIND.into());
    manifest_inputs["classification_settled"] = serde_json::Value::Bool(true);
    for profile in manifest_inputs["profiles"].as_array_mut().unwrap() {
        for row in profile["declarations"].as_array_mut().unwrap() {
            row["level"] = serde_json::Value::String("compatible".into());
        }
    }
    let profile_review = temporary.path().join("profile-review.json");
    fs::write(
        &profile_review,
        serde_json::to_vec(&manifest_inputs).unwrap(),
    )
    .unwrap();
    let classified_first = temporary.path().join("classified-first");
    let classified_second = temporary.path().join("classified-second");
    finalize_manifest_profiles(&first, &profile_review, &classified_first).unwrap();
    finalize_manifest_profiles(&first, &profile_review, &classified_second).unwrap();
    for relative in [
        "classified-manifest-profiles-v1.json",
        "review-evidence-v1.json",
    ] {
        assert_eq!(
            fs::read(classified_first.join(relative)).unwrap(),
            fs::read(classified_second.join(relative)).unwrap()
        );
    }
    let classified: serde_json::Value = serde_json::from_slice(
        &fs::read(classified_first.join("classified-manifest-profiles-v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(classified["product_authority"], false);
    assert_eq!(classified["all_activatable"], true);
    assert_eq!(classified["profiles"][0]["activatable"], true);
    assert!(!first.join(INCOMPLETE_MARKER).exists());
}

#[test]
fn publication_rejects_noncanonical_reviews_and_mismatched_crx_archives() {
    let temporary = tempfile::tempdir().unwrap();
    fixture(temporary.path());
    let review_path = review(temporary.path());
    let bytes = fs::read(&review_path).unwrap();
    let mut spaced = Vec::with_capacity(bytes.len() + 1);
    spaced.extend_from_slice(&bytes[..1]);
    spaced.push(b' ');
    spaced.extend_from_slice(&bytes[1..]);
    fs::write(&review_path, &spaced).unwrap();
    assert!(publish(&review_path, &temporary.path().join("noncanonical")).is_err());

    let mut policy_drift: PublicationReview = serde_json::from_slice(&bytes).unwrap();
    policy_drift.admission_policy.license_rules[0].expression = "Apache-2.0".into();
    fs::write(&review_path, serde_json::to_vec(&policy_drift).unwrap()).unwrap();
    assert!(publish(&review_path, &temporary.path().join("policy-drift")).is_err());

    let other = tempfile::tempdir().unwrap();
    fixture(other.path());
    let review_path = review(temporary.path());
    fs::copy(
        other.path().join("fixture.crx3"),
        temporary.path().join("fixture.crx3"),
    )
    .unwrap();
    assert!(publish(&review_path, &temporary.path().join("mismatch")).is_err());
}

#[test]
fn manifest_profile_finalization_rejects_unassessed_and_identity_drift() {
    let temporary = tempfile::tempdir().unwrap();
    fixture(temporary.path());
    let review_path = review(temporary.path());
    let publication = temporary.path().join("publication");
    publish(&review_path, &publication).unwrap();
    let mut review: serde_json::Value =
        serde_json::from_slice(&fs::read(publication.join(MANIFEST_INPUTS_TARGET)).unwrap())
            .unwrap();
    review["kind"] = serde_json::Value::String(MANIFEST_REVIEW_KIND.into());
    review["classification_settled"] = serde_json::Value::Bool(true);
    let profile_review = temporary.path().join("profile-review.json");
    fs::write(&profile_review, serde_json::to_vec(&review).unwrap()).unwrap();
    assert!(finalize_manifest_profiles(
        &publication,
        &profile_review,
        &temporary.path().join("unassessed")
    )
    .is_err());

    for profile in review["profiles"].as_array_mut().unwrap() {
        for row in profile["declarations"].as_array_mut().unwrap() {
            row["level"] = serde_json::Value::String("compatible".into());
        }
    }
    review["profiles"][0]["manifest_sha256"] = serde_json::Value::String("ff".repeat(32));
    fs::write(&profile_review, serde_json::to_vec(&review).unwrap()).unwrap();
    assert!(finalize_manifest_profiles(
        &publication,
        &profile_review,
        &temporary.path().join("identity-drift")
    )
    .is_err());
}
