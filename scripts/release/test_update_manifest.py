#!/usr/bin/env python3

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

import update_manifest


COMMIT = "a" * 40
OTHER_COMMIT = "b" * 40
PUBLISHED = "2026-07-14T12:34:56Z"
REPOSITORY = "zephium/browser"
SIGNING_KEY_ID = "sha256:" + "c" * 64


class UpdateManifestTests(unittest.TestCase):
    def generate(
        self,
        root: Path,
        sequence: int,
        previous: Path | None = None,
        *,
        version: str | None = None,
        commit: str = COMMIT,
        repository: str = REPOSITORY,
        signing_key_id: str = SIGNING_KEY_ID,
        published_at: str = PUBLISHED,
        suffix: str = "",
    ) -> tuple[dict, bytes, Path]:
        version = version or f"0.1.{sequence - 1}"
        identity = f"{sequence}-{version.replace('.', '_')}{suffix}"
        artifacts = root / f"artifacts-{identity}"
        artifacts.mkdir()
        (artifacts / f"Zephium-{version}-x86_64.rpm").write_bytes(
            b"rpm" + identity.encode("ascii")
        )
        output = root / f"manifest-{identity}.json"
        args = argparse.Namespace(
            artifact_dir=artifacts,
            output=output,
            repository=repository,
            tag=f"v{version}",
            commit=commit,
            sequence=sequence,
            published_at=published_at,
            signing_key_id=signing_key_id,
            previous=previous,
        )
        update_manifest.command_generate(args)
        return json.loads(output.read_text()), output.read_bytes(), output

    def record(
        self,
        path: Path,
        *,
        release_id: int,
        tag: str | None = None,
        commit: str = COMMIT,
    ) -> update_manifest.ChainRelease:
        manifest, raw = update_manifest.load_canonical_manifest(path)
        return update_manifest.ChainRelease(
            release_id=release_id,
            tag=tag or manifest["tag"],
            commit=commit,
            manifest=manifest,
            raw_manifest=raw,
        )

    def mutated_record(
        self,
        release: update_manifest.ChainRelease,
        *,
        release_id: int | None = None,
        tag: str | None = None,
        commit: str | None = None,
        mutate,
    ) -> update_manifest.ChainRelease:
        manifest = copy.deepcopy(release.manifest)
        mutate(manifest)
        raw = update_manifest.canonical_bytes(manifest)
        return update_manifest.ChainRelease(
            release_id=release_id or release.release_id,
            tag=tag or release.tag,
            commit=commit or release.commit,
            manifest=manifest,
            raw_manifest=raw,
        )

    @staticmethod
    def api_asset(repository: str, tag: str, name: str, payload: bytes, asset_id: int) -> dict:
        return {
            "browser_download_url": update_manifest.expected_download_url(
                repository, tag, name
            ),
            "digest": "sha256:" + hashlib.sha256(payload).hexdigest(),
            "id": asset_id,
            "name": name,
            "size": len(payload),
            "state": "uploaded",
        }

    def api_release(
        self,
        *,
        release_id: int,
        tag: str,
        manifest: bytes,
        bundle: bytes,
        immutable: bool = True,
        draft: bool = False,
        prerelease: bool = False,
    ) -> dict:
        return {
            "assets": [
                self.api_asset(
                    REPOSITORY,
                    tag,
                    update_manifest.MANIFEST_NAME,
                    manifest,
                    release_id * 10,
                ),
                self.api_asset(
                    REPOSITORY,
                    tag,
                    update_manifest.BUNDLE_NAME,
                    bundle,
                    release_id * 10 + 1,
                ),
            ],
            "draft": draft,
            "id": release_id,
            "immutable": immutable,
            "prerelease": prerelease,
            "tag_name": tag,
        }

    def test_manifest_is_canonical_self_bound_and_chained(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first, first_raw, first_path = self.generate(root, 1)
            second, second_raw, _ = self.generate(root, 2, first_path)
            self.assertEqual(first_raw, update_manifest.canonical_bytes(first))
            self.assertEqual(second_raw, update_manifest.canonical_bytes(second))
            self.assertEqual(first["repository"], REPOSITORY)
            self.assertEqual(first["tag"], "v0.1.0")
            self.assertEqual(first["schema_version"], 2)
            self.assertEqual(second["rollback"]["previous_sequence"], 1)
            self.assertEqual(
                second["rollback"]["previous_manifest_sha256"],
                hashlib.sha256(first_raw).hexdigest(),
            )
            artifact = first["artifacts"][0]
            self.assertEqual(
                artifact["url"],
                update_manifest.expected_download_url(
                    REPOSITORY, "v0.1.0", artifact["name"]
                ),
            )

    def test_sequence_gap_and_rollback_are_rejected_during_generation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, first_path = self.generate(root, 1)
            with self.assertRaisesRegex(update_manifest.ManifestError, "exactly one"):
                self.generate(root, 3, first_path)

    def test_noncanonical_previous_manifest_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first, _, first_path = self.generate(root, 1)
            first_path.write_text(json.dumps(first, indent=2))
            with self.assertRaisesRegex(update_manifest.ManifestError, "canonical"):
                self.generate(root, 2, first_path)

    def test_previous_manifest_must_use_same_repository_and_key(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, first_path = self.generate(root, 1)
            with self.assertRaisesRegex(update_manifest.ManifestError, "repository"):
                self.generate(root, 2, first_path, repository="zephium/other")
            with self.assertRaisesRegex(update_manifest.ManifestError, "trust root"):
                self.generate(
                    root,
                    2,
                    first_path,
                    signing_key_id="sha256:" + "d" * 64,
                    suffix="-other-key",
                )

    def test_unlabelled_architecture_is_rejected(self) -> None:
        with self.assertRaises(update_manifest.ManifestError):
            update_manifest.classify_artifact("Zephium.dmg")

    def test_semver_rejects_numeric_prerelease_leading_zero(self) -> None:
        self.assertTrue(update_manifest.is_strict_semver("1.2.3-rc.1"))
        self.assertFalse(update_manifest.is_strict_semver("1.2.3-rc.01"))
        self.assertTrue(update_manifest.is_stable_version("1.2.3"))
        self.assertFalse(update_manifest.is_stable_version("1.2.3-rc.1"))
        self.assertFalse(update_manifest.is_stable_version("1.2.3+build"))
        self.assertTrue(update_manifest.semver_is_greater("1.2.3", "1.2.3-rc.9"))
        self.assertTrue(update_manifest.semver_is_greater("1.2.3-rc.10", "1.2.3-rc.2"))
        self.assertFalse(update_manifest.semver_is_greater("1.2.3+new", "1.2.3+old"))

    def test_stable_manifest_rejects_prerelease_tag(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (artifacts / "Zephium-1.2.3-x86_64.rpm").write_bytes(b"rpm")
            args = argparse.Namespace(
                artifact_dir=artifacts,
                output=root / "manifest.json",
                repository=REPOSITORY,
                tag="v1.2.3-rc.1",
                commit=COMMIT,
                sequence=1,
                published_at=PUBLISHED,
                signing_key_id=SIGNING_KEY_ID,
                previous=None,
            )
            with self.assertRaises(update_manifest.ManifestError):
                update_manifest.command_generate(args)

    def test_rfc3339_must_represent_a_real_instant(self) -> None:
        for invalid in (
            "2026-99-99T99:99:99+99:99",
            "2026-02-29T12:00:00Z",
            "2026-07-14T12:34:60Z",
            "2026-07-14T12:34:56",
        ):
            with self.subTest(invalid=invalid), self.assertRaises(
                update_manifest.ManifestError
            ):
                update_manifest.validate_rfc3339(invalid)
        update_manifest.validate_rfc3339("2028-02-29T12:34:56.123+02:00")

    def test_manifest_rejects_unrelated_repository_or_tag_url(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first, _, _ = self.generate(root, 1)
            for url in (
                first["artifacts"][0]["url"].replace(
                    "zephium/browser", "unrelated/project"
                ),
                first["artifacts"][0]["url"].replace("v0.1.0", "v9.9.9"),
                first["artifacts"][0]["url"] + "?download=1",
            ):
                changed = copy.deepcopy(first)
                changed["artifacts"][0]["url"] = url
                with self.subTest(url=url), self.assertRaisesRegex(
                    update_manifest.ManifestError, "protected release URL"
                ):
                    update_manifest.validate_manifest_shape(changed)

    def test_generate_rejects_final_output_symlink_without_touching_target(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (artifacts / "Zephium-1.2.3-x86_64.rpm").write_bytes(b"rpm")
            outside = root / "outside.json"
            output = root / "manifest.json"
            output.symlink_to(outside)
            args = argparse.Namespace(
                artifact_dir=artifacts,
                output=output,
                repository=REPOSITORY,
                tag="v1.2.3",
                commit=COMMIT,
                sequence=1,
                published_at=PUBLISHED,
                signing_key_id=SIGNING_KEY_ID,
                previous=None,
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "replace"):
                update_manifest.command_generate(args)
            self.assertTrue(output.is_symlink())
            self.assertFalse(outside.exists())

    def test_generate_rejects_symbolic_output_parent(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            real = root / "real"
            real.mkdir()
            linked = root / "linked"
            linked.symlink_to(real, target_is_directory=True)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (artifacts / "Zephium-1.2.3-x86_64.rpm").write_bytes(b"rpm")
            args = argparse.Namespace(
                artifact_dir=artifacts,
                output=linked / "manifest.json",
                repository=REPOSITORY,
                tag="v1.2.3",
                commit=COMMIT,
                sequence=1,
                published_at=PUBLISHED,
                signing_key_id=SIGNING_KEY_ID,
                previous=None,
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "parent"):
                update_manifest.command_generate(args)
            self.assertFalse((real / "manifest.json").exists())

    def test_generate_never_replaces_an_existing_regular_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (artifacts / "Zephium-1.2.3-x86_64.rpm").write_bytes(b"rpm")
            output = root / "manifest.json"
            output.write_bytes(b"existing")
            args = argparse.Namespace(
                artifact_dir=artifacts,
                output=output,
                repository=REPOSITORY,
                tag="v1.2.3",
                commit=COMMIT,
                sequence=1,
                published_at=PUBLISHED,
                signing_key_id=SIGNING_KEY_ID,
                previous=None,
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "replace"):
                update_manifest.command_generate(args)
            self.assertEqual(output.read_bytes(), b"existing")

    def test_verify_rejects_changed_artifact_bytes_and_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, manifest_path = self.generate(root, 1)
            artifact_dir = root / "artifacts-1-0_1_0"
            artifact = next(artifact_dir.glob("*.rpm"))
            artifact.write_bytes(b"tampered")
            args = argparse.Namespace(
                manifest=manifest_path,
                artifact_dir=artifact_dir,
                expected_sequence=1,
                expected_repository=REPOSITORY,
                expected_tag="v0.1.0",
                expected_commit=COMMIT,
                expected_signing_key_id=SIGNING_KEY_ID,
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "bytes"):
                update_manifest.command_verify(args)
            args.artifact_dir = None
            args.expected_repository = "zephium/other"
            with self.assertRaisesRegex(update_manifest.ManifestError, "repository"):
                update_manifest.command_verify(args)

    def test_published_release_inventory_requires_immutable_exact_assets(self) -> None:
        manifest = b"manifest\n"
        bundle = b"bundle\n"
        valid = self.api_release(
            release_id=1, tag="v1.2.3", manifest=manifest, bundle=bundle
        )
        descriptors = update_manifest.published_releases(
            [valid], repository=REPOSITORY
        )
        self.assertEqual([release.tag for release in descriptors], ["v1.2.3"])

        mutable = copy.deepcopy(valid)
        mutable["immutable"] = False
        with self.assertRaisesRegex(update_manifest.ManifestError, "not immutable"):
            update_manifest.published_releases([mutable], repository=REPOSITORY)

        missing = copy.deepcopy(valid)
        missing["assets"] = missing["assets"][:1]
        with self.assertRaisesRegex(update_manifest.ManifestError, "exactly one"):
            update_manifest.published_releases([missing], repository=REPOSITORY)

        duplicate = copy.deepcopy(valid)
        duplicate["assets"].append(copy.deepcopy(duplicate["assets"][0]))
        with self.assertRaisesRegex(update_manifest.ManifestError, "ambiguous"):
            update_manifest.published_releases([duplicate], repository=REPOSITORY)

    def test_published_release_inventory_ignores_drafts_and_prereleases_only(self) -> None:
        ignored_draft = {"draft": True, "prerelease": False}
        ignored_prerelease = {"draft": False, "prerelease": True}
        self.assertEqual(
            update_manifest.published_releases(
                [ignored_draft, ignored_prerelease], repository=REPOSITORY
            ),
            [],
        )
        invalid_full = {
            "draft": False,
            "prerelease": False,
            "tag_name": "latest",
        }
        with self.assertRaisesRegex(update_manifest.ManifestError, "vMAJOR"):
            update_manifest.published_releases(
                [invalid_full], repository=REPOSITORY
            )

    def test_valid_complete_chain_derives_unique_next_sequence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, first_path = self.generate(root, 1)
            _, _, second_path = self.generate(root, 2, first_path)
            first = self.record(first_path, release_id=10)
            second = self.record(second_path, release_id=20)
            head = update_manifest.verify_release_chain(
                [second, first],
                repository=REPOSITORY,
                signing_key_id=SIGNING_KEY_ID,
                expected_current_version="0.1.2",
            )
            self.assertEqual(head.next_sequence, 3)
            self.assertEqual(head.release, second)

    def test_empty_chain_derives_genesis_sequence(self) -> None:
        head = update_manifest.verify_release_chain(
            [],
            repository=REPOSITORY,
            signing_key_id=SIGNING_KEY_ID,
            expected_current_version="0.1.0",
        )
        self.assertIsNone(head.release)
        self.assertEqual(head.next_sequence, 1)

    def test_chain_rejects_duplicate_sequence_fork_and_gap(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, first_path = self.generate(root, 1)
            _, _, second_path = self.generate(root, 2, first_path)
            _, _, third_path = self.generate(root, 3, second_path)
            first = self.record(first_path, release_id=10)
            second = self.record(second_path, release_id=20)
            third = self.record(third_path, release_id=30)
            fork = replace(second, release_id=30)
            with self.assertRaisesRegex(update_manifest.ManifestError, "fork"):
                update_manifest.verify_release_chain(
                    [first, second, fork],
                    repository=REPOSITORY,
                    signing_key_id=SIGNING_KEY_ID,
                )

            gap = self.mutated_record(
                second,
                mutate=lambda manifest: manifest["rollback"].update(sequence=3),
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "missing"):
                update_manifest.verify_release_chain(
                    [first, gap],
                    repository=REPOSITORY,
                    signing_key_id=SIGNING_KEY_ID,
                )

            missing_predecessor = self.mutated_record(
                third,
                mutate=lambda manifest: manifest["rollback"].update(
                    previous_sequence=1
                ),
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "predecessor"):
                update_manifest.verify_release_chain(
                    [first, second, missing_predecessor],
                    repository=REPOSITORY,
                    signing_key_id=SIGNING_KEY_ID,
                )

    def test_chain_rejects_digest_mismatch_and_nonincreasing_version(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, first_path = self.generate(root, 1)
            _, _, second_path = self.generate(root, 2, first_path)
            first = self.record(first_path, release_id=10)
            second = self.record(second_path, release_id=20)
            bad_digest = self.mutated_record(
                second,
                mutate=lambda manifest: manifest["rollback"].update(
                    previous_manifest_sha256="0" * 64
                ),
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "digest"):
                update_manifest.verify_release_chain(
                    [first, bad_digest],
                    repository=REPOSITORY,
                    signing_key_id=SIGNING_KEY_ID,
                )

            def make_nonincreasing(manifest: dict) -> None:
                manifest["version"] = "0.1.0"
                manifest["tag"] = "v0.1.0"
                for artifact in manifest["artifacts"]:
                    artifact["url"] = update_manifest.expected_download_url(
                        REPOSITORY, "v0.1.0", artifact["name"]
                    )

            nonincreasing = self.mutated_record(
                second, tag="v0.1.0", mutate=make_nonincreasing
            )
            with self.assertRaisesRegex(update_manifest.ManifestError, "do not increase"):
                update_manifest.verify_release_chain(
                    [first, nonincreasing],
                    repository=REPOSITORY,
                    signing_key_id=SIGNING_KEY_ID,
                )

    def test_chain_rejects_tag_commit_repository_and_key_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, _, first_path = self.generate(root, 1)
            first = self.record(first_path, release_id=10)
            cases = (
                (replace(first, tag="v9.9.9"), REPOSITORY, SIGNING_KEY_ID, "tag"),
                (replace(first, commit=OTHER_COMMIT), REPOSITORY, SIGNING_KEY_ID, "commit"),
                (first, "zephium/other", SIGNING_KEY_ID, "repository"),
                (first, REPOSITORY, "sha256:" + "d" * 64, "trust root"),
            )
            for release, repository, key, message in cases:
                with self.subTest(message=message), self.assertRaisesRegex(
                    update_manifest.ManifestError, message
                ):
                    update_manifest.verify_release_chain(
                        [release], repository=repository, signing_key_id=key
                    )

    def test_verify_chain_command_confirms_sequence_and_exact_api_digests(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, first_raw, first_path = self.generate(root, 1)
            bundle = b"signed bundle\n"
            release = self.api_release(
                release_id=10,
                tag="v0.1.0",
                manifest=first_raw,
                bundle=bundle,
            )
            releases_json = root / "releases.json"
            releases_json.write_text(json.dumps([release]))
            material = root / "material"
            version_dir = material / "v0.1.0"
            version_dir.mkdir(parents=True)
            (version_dir / update_manifest.MANIFEST_NAME).write_bytes(first_path.read_bytes())
            (version_dir / update_manifest.BUNDLE_NAME).write_bytes(bundle)
            (version_dir / update_manifest.COMMIT_NAME).write_text(COMMIT + "\n")
            output = root / "chain.json"
            args = argparse.Namespace(
                releases_json=releases_json,
                material_dir=material,
                repository=REPOSITORY,
                signing_key_id=SIGNING_KEY_ID,
                expected_current_tag="v0.1.1",
                expected_next_sequence=2,
                output=output,
            )
            update_manifest.command_verify_chain(args)
            result = json.loads(output.read_text())
            self.assertEqual(result["head_release_id"], 10)
            self.assertEqual(result["head_tag"], "v0.1.0")
            self.assertEqual(result["next_sequence"], 2)

            output.unlink()
            args.expected_next_sequence = 3
            with self.assertRaisesRegex(update_manifest.ManifestError, "uniquely"):
                update_manifest.command_verify_chain(args)

            release["assets"][0]["digest"] = "sha256:" + "0" * 64
            releases_json.write_text(json.dumps([release]))
            args.expected_next_sequence = 2
            with self.assertRaisesRegex(update_manifest.ManifestError, "asset metadata"):
                update_manifest.command_verify_chain(args)

    def test_release_workflow_reconstructs_and_rechecks_the_complete_chain(self) -> None:
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/build.yml").read_text()
        self.assertNotIn("/releases/latest", workflow)
        self.assertEqual(workflow.count("update_manifest.py verify-chain"), 2)
        self.assertGreaterEqual(
            workflow.count("repos/${GITHUB_REPOSITORY}/releases?per_page=100"), 2
        )
        self.assertEqual(workflow.count("${{ inputs.release_sequence }}"), 2)
        self.assertEqual(
            workflow.count("${{ steps.previous.outputs.next_sequence }}"), 2
        )
        initial = workflow.index(
            "Verify the update key and complete immutable stable chain"
        )
        signing = workflow.index("cosign verify-blob", initial)
        first_chain_check = workflow.index("update_manifest.py verify-chain", initial)
        recheck = workflow.index("recheck_releases=", initial)
        draft_proof = workflow.index("final_draft=", first_chain_check)
        publish = workflow.index(
            'gh release edit "${RELEASE_TAG}" --draft=false --latest', recheck
        )
        self.assertLess(initial, recheck)
        self.assertLess(signing, first_chain_check)
        self.assertLess(draft_proof, recheck)
        self.assertLess(recheck, publish)


if __name__ == "__main__":
    unittest.main()
