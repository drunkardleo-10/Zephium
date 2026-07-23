#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import finalize_sbom


COMMIT = "a" * 40
REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def syft_document(source_name: str, version: str) -> dict:
    def cargo_component(name: str, component_version: str, package_id: str) -> dict:
        purl = f"pkg:cargo/{name}@{component_version}"
        return {
            "type": "library",
            "name": name,
            "version": component_version,
            "purl": purl,
            "bom-ref": f"{purl}?package-id={package_id}",
            "properties": [
                {
                    "name": property_name,
                    "value": property_value,
                }
                for property_name, property_value in (
                    *finalize_sbom.EXPECTED_ADBLOCK_SYFT_PROPERTIES.items(),
                    (
                        "syft:location:0:path",
                        finalize_sbom.EXPECTED_ADBLOCK_SYFT_LOCATIONS["linux"],
                    ),
                )
            ],
        }

    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": "urn:uuid:00000000-0000-4000-8000-000000000000",
        "version": 1,
        "metadata": {
            "component": {"type": "application", "name": source_name, "version": version},
            "tools": {
                "components": [
                    {
                        "type": "application",
                        "name": "syft",
                        "version": finalize_sbom.EXPECTED_SYFT_VERSION,
                    }
                ]
            },
        },
        "components": [
            cargo_component(
                "adblock",
                finalize_sbom.EXPECTED_ADBLOCK_VERSION,
                "0123456789abcdef",
            ),
            *[
                cargo_component(name, component_version, f"{index + 1:016x}")
                for index, (_, name, component_version) in enumerate(
                    finalize_sbom.EXPECTED_BLOCKER_COMPONENTS
                )
            ],
            {
                "type": "library",
                "name": "dependency",
                "version": "1.0.0",
                "bom-ref": "pkg:generic/dependency@1.0.0",
            }
        ],
    }


class FinalizeSbomTests(unittest.TestCase):
    def arguments(self, root: Path) -> argparse.Namespace:
        payload = root / "payload"
        payload.mkdir()
        (payload / "bin").mkdir()
        executable = payload / "bin" / "zephium"
        executable.write_bytes(b"signed executable")
        (payload / "asset.txt").write_text("asset", encoding="utf-8")
        (payload / "asset-link").symlink_to("asset.txt")
        adblock = payload / finalize_sbom.ADBLOCK_MANIFEST_ROOT
        adblock.mkdir(parents=True)
        for name in (
            "Cargo.toml",
            "Cargo.lock",
            "Cargo.toml.orig",
            "FORK.toml",
            "LICENSE",
            "UPSTREAM_FILES.toml",
        ):
            shutil.copyfile(
                REPOSITORY_ROOT / "vendor" / "adblock" / name,
                adblock / name,
            )
        subject = root / "Zephium-1.2.3-linux-x86_64.rpm"
        subject.write_bytes(b"signed installer")
        sbom = root / "sbom.cdx.json"
        sbom.write_text(
            json.dumps(syft_document("Zephium Linux payload", "1.2.3")),
            encoding="utf-8",
        )
        return argparse.Namespace(
            sbom=sbom,
            scan_root=payload,
            source_name="Zephium Linux payload",
            version="1.2.3",
            commit=COMMIT,
            platform="linux",
            subject=[subject],
            executable=[executable],
            reference_executable=executable,
            adblock_fork_manifest=payload / finalize_sbom.ADBLOCK_FORK_MANIFEST,
            adblock_upstream_inventory=(
                payload / finalize_sbom.ADBLOCK_UPSTREAM_INVENTORY
            ),
            adblock_source_manifest=payload / finalize_sbom.ADBLOCK_SOURCE_MANIFEST,
        )

    def test_binds_exact_subject_payload_and_executable_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            finalize_sbom.finalize(args)
            value = json.loads(args.sbom.read_text(encoding="utf-8"))
            properties = {
                item["name"]: item["value"]
                for item in value["metadata"]["properties"]
            }
            subject_digest = hashlib.sha256(args.subject[0].read_bytes()).hexdigest()
            executable_digest = hashlib.sha256(
                args.executable[0].read_bytes()
            ).hexdigest()
            self.assertEqual(
                properties["zephium:release:subject:0:sha256"], subject_digest
            )
            self.assertEqual(
                properties["zephium:release:subject:0:name"], args.subject[0].name
            )
            self.assertEqual(
                properties["zephium:release:subject:0:size"],
                str(args.subject[0].stat().st_size),
            )
            self.assertEqual(
                properties["zephium:release:executable:0:sha256"], executable_digest
            )
            self.assertEqual(
                properties["zephium:release:executable:0:path"],
                "bin/zephium",
            )
            payload_files = {
                component["name"]: component
                for component in value["components"]
                if str(component.get("bom-ref", "")).startswith("zephium-payload:")
            }
            self.assertEqual(
                payload_files["asset.txt"]["hashes"][0]["content"],
                hashlib.sha256(b"asset").hexdigest(),
            )
            self.assertIn("asset-link", payload_files)
            asset_properties = {
                item["name"]: item["value"]
                for item in payload_files["asset.txt"]["properties"]
            }
            self.assertRegex(asset_properties["zephium:payload:mode"], r"^[0-7]{4}$")
            self.assertEqual(
                properties["zephium:adblock:crate:purl"],
                finalize_sbom.EXPECTED_ADBLOCK_PURL,
            )
            self.assertEqual(
                properties["zephium:adblock:feature-graph"],
                finalize_sbom.EXPECTED_ADBLOCK_FEATURE_GRAPHS[args.platform],
            )
            self.assertEqual(
                properties["zephium:blocker:compiler-feature-graph"],
                finalize_sbom.EXPECTED_BLOCKER_FEATURE_GRAPHS[args.platform],
            )
            for role, name, component_version in (
                finalize_sbom.EXPECTED_BLOCKER_COMPONENTS
            ):
                self.assertEqual(
                    properties[f"zephium:blocker:component:{role}:purl"],
                    f"pkg:cargo/{name}@{component_version}",
                )
                component = next(
                    component
                    for component in value["components"]
                    if component.get("purl")
                    == f"pkg:cargo/{name}@{component_version}"
                )
                component_properties = {
                    item["name"]: item["value"]
                    for item in component["properties"]
                }
                self.assertEqual(
                    component_properties["zephium:blocker:component-role"],
                    role,
                )
            source_manifest = args.adblock_source_manifest
            source_digest = hashlib.sha256(source_manifest.read_bytes()).hexdigest()
            self.assertEqual(
                properties["zephium:adblock:source-manifest:sha256"],
                source_digest,
            )
            adblock_component = next(
                component
                for component in value["components"]
                if component.get("purl") == finalize_sbom.EXPECTED_ADBLOCK_PURL
            )
            self.assertEqual(
                adblock_component["licenses"],
                [
                    {
                        "license": {
                            "id": finalize_sbom.EXPECTED_ADBLOCK_LICENSE,
                        }
                    }
                ],
            )
            component_properties = {
                item["name"]: item["value"]
                for item in adblock_component["properties"]
            }
            self.assertEqual(
                component_properties["zephium:adblock:upstream:commit"],
                finalize_sbom.EXPECTED_ADBLOCK_COMMIT,
            )
            source_payload = payload_files[
                finalize_sbom.ADBLOCK_SOURCE_MANIFEST.as_posix()
            ]
            self.assertEqual(
                source_payload["hashes"],
                [{"alg": "SHA-256", "content": source_digest}],
            )

    def test_rejects_executable_that_differs_from_signed_reference(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            reference = Path(temporary) / "reference"
            reference.write_bytes(b"different")
            args.reference_executable = reference
            with self.assertRaises(finalize_sbom.SbomError):
                finalize_sbom.finalize(args)

    def test_rejects_release_subject_swapped_after_descriptor_open(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.arguments(root)
            subject = args.subject[0]
            displaced = root / "displaced.rpm"
            real_open = os.open
            swapped = False

            def open_then_swap(path, flags, *positional, **keywords):
                nonlocal swapped
                descriptor = real_open(path, flags, *positional, **keywords)
                if not swapped and Path(path) == subject:
                    swapped = True
                    subject.rename(displaced)
                    subject.write_bytes(b"attacker replacement")
                return descriptor

            with mock.patch.object(
                finalize_sbom.os, "open", side_effect=open_then_swap
            ), self.assertRaisesRegex(finalize_sbom.SbomError, "regular file|changed"):
                finalize_sbom.finalize(args)

            self.assertTrue(swapped)


@unittest.skipUnless(sys.platform.startswith("linux"), "RPM helper targets GNU/Linux")
class SignRpmTests(unittest.TestCase):
    fingerprint = "A" * 40

    def write_tool(self, directory: Path, name: str, source: str) -> None:
        path = directory / name
        path.write_text(source, encoding="utf-8")
        path.chmod(0o755)

    def fake_tools(self, root: Path) -> tuple[Path, Path]:
        tools = root / "tools"
        tools.mkdir()
        log = root / "tool.log"
        common = r'''
if [[ -n "${RPM_SIGNING_PRIVATE_KEY:-}" || -n "${RPM_SIGNING_KEY_PASSPHRASE:-}" ]]; then
  echo "signing secret leaked into child environment" >&2
  exit 90
fi
printf '%s\n' "$0 $*" >> "${FAKE_TOOL_LOG}"
'''
        self.write_tool(
            tools,
            "gpg",
            "#!/usr/bin/env bash\nset -euo pipefail\n"
            + common
            + r'''
contains() {
  local wanted="$1"
  shift
  local value
  for value in "$@"; do
    [[ "${value}" == "${wanted}" ]] && return 0
  done
  return 1
}
if contains --list-secret-keys "$@"; then
  validity="${FAKE_GPG_VALIDITY:-u}"
  expiry="${FAKE_GPG_EXPIRY:-0}"
  printf 'sec:%s:2048:1:0123456789ABCDEF:1700000000:%s:::::scESC:\n' "${validity}" "${expiry}"
  printf 'fpr:::::::::%s:\n' "${FAKE_FINGERPRINT}"
  printf 'ssb:%s:2048:1:FEDCBA9876543210:1700000000:%s:::::s:\n' "${validity}" "${expiry}"
  exit 0
fi
if contains --show-keys "$@"; then
  printf 'pub:u:2048:1:0123456789ABCDEF:1700000000:0:::::scESC:\n'
  printf 'fpr:::::::::%s:\n' "${FAKE_FINGERPRINT}"
  exit 0
fi
output=""
previous=""
for value in "$@"; do
  if [[ "${previous}" == "--output" ]]; then output="${value}"; fi
  previous="${value}"
done
if contains --detach-sign "$@"; then
  [[ -n "${output}" ]]
  printf 'signature\n' > "${output}"
  exit 0
fi
if contains --verify "$@" || contains --import "$@"; then
  exit 0
fi
if contains --export "$@"; then
  [[ -n "${output}" ]]
  printf 'PUBLIC KEY\n' > "${output}"
  exit 0
fi
echo "unexpected fake gpg invocation" >&2
exit 91
''',
        )
        self.write_tool(
            tools,
            "rpmsign",
            "#!/usr/bin/env bash\nset -euo pipefail\n"
            + common
            + r'''
target="${!#}"
if [[ "${target}" == "${FAKE_ORIGINAL_RPM}" || "$(stat -c '%a' -- "${target}")" != "600" ]]; then
  echo "rpmsign did not receive an isolated private copy" >&2
  exit 92
fi
if [[ "${FAKE_RPMSIGN_FAIL:-0}" == "1" ]]; then exit 93; fi
printf 'SIGNED' >> "${target}"
''',
        )
        self.write_tool(
            tools,
            "rpmkeys",
            "#!/usr/bin/env bash\nset -euo pipefail\n"
            + common
            + r'''
for value in "$@"; do
  [[ "${value}" == "--import" ]] && exit 0
done
target="${!#}"
if [[ "${FAKE_RPMKEYS_FAIL:-0}" == "1" ]] || ! grep -q 'SIGNED$' "${target}"; then
  printf '%s: digests signatures NOT OK\n' "${target}"
else
  printf '%s: digests signatures OK\n' "${target}"
fi
''',
        )
        return tools, log

    def run_signer(
        self,
        root: Path,
        rpm: Path,
        public_key: Path,
        **extra_environment: str,
    ) -> subprocess.CompletedProcess[str]:
        tools, log = self.fake_tools(root)
        environment = os.environ.copy()
        environment.update(
            {
                "FAKE_FINGERPRINT": self.fingerprint,
                "FAKE_ORIGINAL_RPM": str(rpm.resolve()),
                "FAKE_TOOL_LOG": str(log),
                "PATH": f"{tools}:/usr/bin:/bin",
                "RPM_SIGNING_KEY_FINGERPRINT": self.fingerprint,
                "RPM_SIGNING_KEY_PASSPHRASE": "exact passphrase",
                "RPM_SIGNING_PRIVATE_KEY": "private key material",
            }
        )
        environment.update(extra_environment)
        return subprocess.run(
            [
                "bash",
                str(Path(__file__).with_name("sign_rpm.sh")),
                str(rpm),
                str(public_key),
            ],
            check=False,
            capture_output=True,
            env=environment,
            text=True,
        )

    def assert_no_private_temporaries(self, root: Path) -> None:
        self.assertEqual(list(root.glob(".zephium-rpm-sign.*")), [])
        self.assertEqual(list(root.glob(".zephium-rpm-key.*")), [])

    def test_signs_verified_copy_then_atomically_replaces_original(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "Zephium.rpm"
            public_key = root / "publisher.asc"
            rpm.write_bytes(b"unsigned rpm")
            rpm.chmod(0o640)

            result = self.run_signer(root, rpm, public_key)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(rpm.read_bytes(), b"unsigned rpmSIGNED")
            self.assertEqual(stat.S_IMODE(rpm.stat().st_mode), 0o640)
            self.assertEqual(public_key.read_bytes(), b"PUBLIC KEY\n")
            self.assertEqual(stat.S_IMODE(public_key.stat().st_mode), 0o644)
            self.assertFalse(public_key.is_symlink())
            self.assert_no_private_temporaries(root)

    def test_rpmsign_failure_preserves_original_and_publishes_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "Zephium.rpm"
            public_key = root / "publisher.asc"
            rpm.write_bytes(b"unsigned rpm")
            rpm.chmod(0o640)

            result = self.run_signer(root, rpm, public_key, FAKE_RPMSIGN_FAIL="1")

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(rpm.read_bytes(), b"unsigned rpm")
            self.assertEqual(stat.S_IMODE(rpm.stat().st_mode), 0o640)
            self.assertFalse(public_key.exists())
            self.assert_no_private_temporaries(root)

    def test_signature_verification_failure_is_transactional(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "Zephium.rpm"
            public_key = root / "publisher.asc"
            rpm.write_bytes(b"unsigned rpm")

            result = self.run_signer(root, rpm, public_key, FAKE_RPMKEYS_FAIL="1")

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(rpm.read_bytes(), b"unsigned rpm")
            self.assertFalse(public_key.exists())
            self.assert_no_private_temporaries(root)

    def test_rejects_revoked_key_before_signing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "Zephium.rpm"
            public_key = root / "publisher.asc"
            rpm.write_bytes(b"unsigned rpm")

            result = self.run_signer(root, rpm, public_key, FAKE_GPG_VALIDITY="r")

            self.assertEqual(result.returncode, 66, result.stderr)
            self.assertEqual(rpm.read_bytes(), b"unsigned rpm")
            self.assertFalse(public_key.exists())
            self.assert_no_private_temporaries(root)

    def test_rejects_expired_key_before_signing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "Zephium.rpm"
            public_key = root / "publisher.asc"
            rpm.write_bytes(b"unsigned rpm")

            result = self.run_signer(root, rpm, public_key, FAKE_GPG_EXPIRY="1")

            self.assertEqual(result.returncode, 66, result.stderr)
            self.assertEqual(rpm.read_bytes(), b"unsigned rpm")
            self.assertFalse(public_key.exists())
            self.assert_no_private_temporaries(root)

    def test_never_follows_existing_public_key_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rpm = root / "Zephium.rpm"
            public_key = root / "publisher.asc"
            target = root / "missing"
            rpm.write_bytes(b"unsigned rpm")
            public_key.symlink_to(target)

            result = self.run_signer(root, rpm, public_key)

            self.assertEqual(result.returncode, 65, result.stderr)
            self.assertTrue(public_key.is_symlink())
            self.assertFalse(target.exists())
            self.assertEqual(rpm.read_bytes(), b"unsigned rpm")

class AdditionalFinalizeSbomTests(unittest.TestCase):
    arguments = FinalizeSbomTests.arguments

    def test_rejects_missing_duplicate_or_inexact_adblock_component(self) -> None:
        mutations = {
            "missing": lambda components: components.pop(0),
            "duplicate": lambda components: components.append(
                json.loads(json.dumps(components[0]))
            ),
            "version": lambda components: components[0].__setitem__(
                "version", "0.13.1"
            ),
            "purl": lambda components: components[0].__setitem__(
                "purl", "pkg:cargo/adblock@0.13.1"
            ),
            "license": lambda components: components[0].__setitem__(
                "licenses", [{"license": {"id": "MIT"}}]
            ),
            "source": lambda components: next(
                prop
                for prop in components[0]["properties"]
                if prop["name"] == "syft:location:0:path"
            ).__setitem__("value", "/build-manifests/adblock/Cargo.lock"),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                args = self.arguments(Path(temporary))
                value = json.loads(args.sbom.read_text(encoding="utf-8"))
                mutate(value["components"])
                args.sbom.write_text(json.dumps(value), encoding="utf-8")
                with self.assertRaises(finalize_sbom.SbomError):
                    finalize_sbom.finalize(args)

    def test_rejects_missing_duplicate_or_inexact_blocker_supply_components(
        self,
    ) -> None:
        mutations = {
            "missing": lambda components: components.pop(3),
            "duplicate": lambda components: components.append(
                {
                    **json.loads(json.dumps(components[3])),
                    "bom-ref": (
                        f"{components[3]['purl']}?package-id=fedcba9876543210"
                    ),
                }
            ),
            "version": lambda components: components[3].__setitem__(
                "version", "0.0.1"
            ),
            "purl": lambda components: components[3].__setitem__(
                "purl", "pkg:cargo/zephium-blocker-update@0.0.1"
            ),
            "source": lambda components: next(
                prop
                for prop in components[3]["properties"]
                if prop["name"] == "syft:location:0:path"
            ).__setitem__("value", "/build-manifests/unreviewed/Cargo.lock"),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                args = self.arguments(Path(temporary))
                value = json.loads(args.sbom.read_text(encoding="utf-8"))
                mutate(value["components"])
                args.sbom.write_text(json.dumps(value), encoding="utf-8")
                with self.assertRaises(finalize_sbom.SbomError):
                    finalize_sbom.finalize(args)

    def test_rejects_reserved_properties_anywhere_in_syft_input(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            value = json.loads(args.sbom.read_text(encoding="utf-8"))
            value["components"][0]["properties"] = [
                {
                    "name": "zephium:adblock:upstream:commit",
                    "value": finalize_sbom.EXPECTED_ADBLOCK_COMMIT,
                }
            ]
            args.sbom.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(
                finalize_sbom.SbomError, "Zephium-reserved property"
            ):
                finalize_sbom.finalize(args)

    def test_rejects_noncanonical_provenance_argument(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.arguments(root)
            alternate = args.scan_root / "alternate-FORK.toml"
            shutil.copyfile(args.adblock_fork_manifest, alternate)
            args.adblock_fork_manifest = alternate
            with self.assertRaisesRegex(finalize_sbom.SbomError, "fixed staged"):
                finalize_sbom.finalize(args)

    def test_rejects_symlinked_provenance_parent(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.arguments(root)
            manifest_root = args.scan_root / "build-manifests"
            adblock = manifest_root / "adblock"
            displaced = manifest_root / "displaced-adblock"
            adblock.rename(displaced)
            adblock.symlink_to(displaced.name)
            with self.assertRaisesRegex(
                finalize_sbom.SbomError, "non-symbolic directory"
            ):
                finalize_sbom.finalize(args)

    def test_rejects_source_manifest_that_disagrees_with_inventory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            args.adblock_source_manifest.write_bytes(
                args.adblock_source_manifest.read_bytes() + b"\n"
            )
            with self.assertRaisesRegex(
                finalize_sbom.SbomError, "reviewed upstream manifest hash"
            ):
                finalize_sbom.finalize(args)

    def test_rejects_mutated_license_and_inexact_standalone_lock(self) -> None:
        mutations = {
            "license": lambda args: (
                args.scan_root / finalize_sbom.ADBLOCK_LICENSE_FILE
            ).write_bytes(b"not the reviewed license"),
            "lock": lambda args: (
                args.scan_root / finalize_sbom.ADBLOCK_STANDALONE_LOCK
            ).write_text("version = 3\n", encoding="utf-8"),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                args = self.arguments(Path(temporary))
                mutate(args)
                with self.assertRaises(finalize_sbom.SbomError):
                    finalize_sbom.finalize(args)

    def test_rejects_unreviewed_fork_upstream_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            encoded = args.adblock_fork_manifest.read_text(encoding="utf-8")
            encoded = encoded.replace(
                finalize_sbom.EXPECTED_ADBLOCK_COMMIT,
                "b" * 40,
                1,
            )
            args.adblock_fork_manifest.write_text(encoded, encoding="utf-8")
            with self.assertRaisesRegex(finalize_sbom.SbomError, "commit"):
                finalize_sbom.finalize(args)

    def test_cross_checks_provenance_against_payload_file_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            real_payload_components = finalize_sbom.payload_components

            def mutate_then_inventory(root: Path):
                args.adblock_fork_manifest.write_bytes(
                    args.adblock_fork_manifest.read_bytes() + b"\n"
                )
                return real_payload_components(root)

            with mock.patch.object(
                finalize_sbom,
                "payload_components",
                side_effect=mutate_then_inventory,
            ), self.assertRaisesRegex(
                finalize_sbom.SbomError, "payload hash disagrees"
            ):
                finalize_sbom.finalize(args)

    def test_release_workflow_stages_and_binds_adblock_provenance_everywhere(
        self,
    ) -> None:
        workflow = (
            REPOSITORY_ROOT / ".github" / "workflows" / "build.yml"
        ).read_text(encoding="utf-8")
        staged_sources = (
            "vendor/adblock/Cargo.lock",
            ("vendor/adblock/FORK.toml", "--adblock-fork-manifest"),
            "vendor/adblock/LICENSE",
            (
                "vendor/adblock/UPSTREAM_FILES.toml",
                "--adblock-upstream-inventory",
            ),
            ("vendor/adblock/Cargo.toml.orig", "--adblock-source-manifest"),
        )
        bindings = tuple(
            item for item in staged_sources if isinstance(item, tuple)
        )
        sources = tuple(
            item[0] if isinstance(item, tuple) else item for item in staged_sources
        )
        for source in sources:
            self.assertEqual(workflow.count(source), 3)
        for source, argument in bindings:
            self.assertEqual(workflow.count(argument), 3)
        for step_name in (
            "Generate and bind the Linux installed-payload SBOM",
            "Generate and bind the Windows extracted-installer-payload SBOM",
            "Generate and bind the macOS installed-payload SBOM",
        ):
            start = workflow.index(f"      - name: {step_name}")
            end = workflow.find("\n      - name:", start + 1)
            section = workflow[start:] if end < 0 else workflow[start:end]
            for source in sources:
                self.assertEqual(section.count(source), 1, step_name)
                self.assertGreater(
                    section.index(source),
                    section.index("syft scan"),
                    f"{source} must be staged after the authoritative graph scan",
                )
            for _, argument in bindings:
                self.assertEqual(section.count(argument), 1, step_name)
        ordinary_ci = (
            REPOSITORY_ROOT / ".github" / "workflows" / "ci.yml"
        ).read_text(encoding="utf-8")
        self.assertEqual(
            ordinary_ci.count("python3 scripts/release/test_finalize_sbom.py"),
            1,
        )

    def test_rejects_unpinned_syft_version(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            value = json.loads(args.sbom.read_text(encoding="utf-8"))
            value["metadata"]["tools"]["components"][0]["version"] = "1.45.0"
            args.sbom.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(finalize_sbom.SbomError):
                finalize_sbom.finalize(args)

    def test_rejects_real_syft_duplicate_from_standalone_fork_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            value = json.loads(args.sbom.read_text(encoding="utf-8"))
            duplicate = json.loads(json.dumps(value["components"][0]))
            duplicate["bom-ref"] = (
                f"{finalize_sbom.EXPECTED_ADBLOCK_PURL}"
                "?package-id=fedcba9876543210"
            )
            next(
                prop
                for prop in duplicate["properties"]
                if prop["name"] == "syft:location:0:path"
            )["value"] = "/build-manifests/adblock/Cargo.lock"
            value["components"].append(duplicate)
            args.sbom.write_text(json.dumps(value), encoding="utf-8")

            with self.assertRaisesRegex(
                finalize_sbom.SbomError,
                "exactly one Cargo adblock component, found 2",
            ):
                finalize_sbom.finalize(args)

    def test_syft_workspace_lock_location_is_exact_for_release_platform(self) -> None:
        for platform, expected in finalize_sbom.EXPECTED_ADBLOCK_SYFT_LOCATIONS.items():
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as temporary:
                args = self.arguments(Path(temporary))
                args.platform = platform
                value = json.loads(args.sbom.read_text(encoding="utf-8"))
                cargo_components = [
                    component
                    for component in value["components"]
                    if str(component.get("purl", "")).startswith("pkg:cargo/")
                ]
                for component in cargo_components:
                    location = next(
                        prop
                        for prop in component["properties"]
                        if prop["name"] == "syft:location:0:path"
                    )
                    location["value"] = expected
                args.sbom.write_text(json.dumps(value), encoding="utf-8")
                finalize_sbom.finalize(args)

                wrong = (
                    finalize_sbom.EXPECTED_ADBLOCK_SYFT_LOCATIONS["windows"]
                    if platform != "windows"
                    else finalize_sbom.EXPECTED_ADBLOCK_SYFT_LOCATIONS["linux"]
                )
                value = syft_document("Zephium Linux payload", "1.2.3")
                for component in value["components"]:
                    if not str(component.get("purl", "")).startswith("pkg:cargo/"):
                        continue
                    location = next(
                        prop
                        for prop in component["properties"]
                        if prop["name"] == "syft:location:0:path"
                    )
                    location["value"] = wrong
                args.sbom.write_text(json.dumps(value), encoding="utf-8")
                with self.assertRaisesRegex(
                    finalize_sbom.SbomError,
                    "release workspace lock",
                ):
                    finalize_sbom.finalize(args)

    def test_rejects_payload_symlink_that_escapes_staging(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            args = self.arguments(root)
            outside = root / "outside"
            outside.write_text("outside", encoding="utf-8")
            link = args.scan_root / "asset-link"
            link.unlink()
            link.symlink_to(outside)
            with self.assertRaises(finalize_sbom.SbomError):
                finalize_sbom.finalize(args)

    def test_rejects_non_object_cyclonedx_component(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            value = json.loads(args.sbom.read_text(encoding="utf-8"))
            value["components"].append("not-a-component")
            args.sbom.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(finalize_sbom.SbomError):
                finalize_sbom.finalize(args)

    def test_rejects_duplicate_bom_references_anywhere_in_document(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            value = json.loads(args.sbom.read_text(encoding="utf-8"))
            duplicate = value["components"][0]["bom-ref"]
            value["metadata"]["component"]["bom-ref"] = duplicate
            args.sbom.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(finalize_sbom.SbomError, "duplicate bom-ref"):
                finalize_sbom.finalize(args)

    def test_rejects_duplicate_json_object_keys(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            encoded = args.sbom.read_text(encoding="utf-8")
            encoded = encoded.replace(
                '{"bomFormat": "CycloneDX",',
                '{"bomFormat": "CycloneDX", "bomFormat": "CycloneDX",',
                1,
            )
            args.sbom.write_text(encoded, encoding="utf-8")
            with self.assertRaisesRegex(finalize_sbom.SbomError, "duplicate JSON key"):
                finalize_sbom.finalize(args)

    def test_rejects_non_finite_json_numbers(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            value = args.sbom.read_text(encoding="utf-8")
            value = value.replace('"version": 1', '"version": NaN', 1)
            args.sbom.write_text(value, encoding="utf-8")
            with self.assertRaisesRegex(finalize_sbom.SbomError, "non-finite"):
                finalize_sbom.finalize(args)

    def test_detects_same_shape_mutation_after_atomic_publication(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            args = self.arguments(Path(temporary))
            real_replace = os.replace

            def replace_then_mutate(source: Path, destination: Path) -> None:
                real_replace(source, destination)
                value = json.loads(destination.read_text(encoding="utf-8"))
                value["components"][0]["name"] = "tamperedxx"
                destination.write_text(
                    json.dumps(
                        value,
                        ensure_ascii=False,
                        sort_keys=True,
                        separators=(",", ":"),
                    )
                    + "\n",
                    encoding="utf-8",
                )

            with mock.patch.object(
                finalize_sbom.os, "replace", side_effect=replace_then_mutate
            ), self.assertRaisesRegex(
                finalize_sbom.SbomError, "bytes changed during publication"
            ):
                finalize_sbom.finalize(args)


if __name__ == "__main__":
    unittest.main()
