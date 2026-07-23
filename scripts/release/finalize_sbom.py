#!/usr/bin/env python3
"""Bind a Syft CycloneDX inventory to exact release subjects and payload bytes."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import Any


EXPECTED_SYFT_VERSION = "1.44.0"
# These release-policy anchors are deliberately independent of the staged
# fork metadata. Reading identity only from FORK.toml would let that file
# self-attest an unreviewed upstream.
EXPECTED_ADBLOCK_NAME = "adblock"
EXPECTED_ADBLOCK_VERSION = "0.13.2"
EXPECTED_ADBLOCK_LICENSE = "MPL-2.0"
EXPECTED_ADBLOCK_PURL = "pkg:cargo/adblock@0.13.2"
EXPECTED_ADBLOCK_SYFT_LOCATIONS = {
    "linux": "/build-manifests/Cargo.lock",
    "macos": "/build-manifests/Cargo.lock",
    "windows": "\\build-manifests\\Cargo.lock",
}
EXPECTED_ADBLOCK_SYFT_PROPERTIES = {
    "syft:package:foundBy": "rust-cargo-lock-cataloger",
    "syft:package:language": "rust",
    "syft:package:metadataType": "rust-cargo-lock-entry",
    "syft:package:type": "rust-crate",
}
EXPECTED_ADBLOCK_REPOSITORY = "https://github.com/brave/adblock-rust"
EXPECTED_ADBLOCK_TAG = "v0.13.2"
EXPECTED_ADBLOCK_COMMIT = "00b19a06508ddd4f3453779f8618983421ddf32b"
EXPECTED_ADBLOCK_TREE = "17f03b25091db8ce9ef88408e4be9d11f3e6f755"
EXPECTED_ADBLOCK_ARCHIVE_SHA256 = (
    "77420e48225975c472eaea1b7c767af6caebea02d910043b0af7a1271d47ec9c"
)
EXPECTED_ADBLOCK_SOURCE_MANIFEST_SHA256 = (
    "6204e3f481bfdebf263fc7c3eda963362e2353ba282919779f3e3ea8eba1b374"
)
EXPECTED_ADBLOCK_SOURCE_MANIFEST_GIT_BLOB = (
    "720abe77fa0fedfef0859f9f5ce3499cebb7aa14"
)
EXPECTED_ADBLOCK_LICENSE_SHA256 = (
    "3f3d9e0024b1921b067d6f7f88deb4a60cbe7a78e76c64e3f1d7fc3b779b9d04"
)
EXPECTED_ADBLOCK_FEATURE_GRAPHS = {
    "windows": "full-regex-handling",
    "linux": "content-blocking,full-regex-handling",
    "macos": "content-blocking,full-regex-handling",
}
EXPECTED_BLOCKER_FEATURE_GRAPHS = {
    "windows": "runtime",
    "linux": "webkit",
    "macos": "webkit",
}
EXPECTED_BLOCKER_COMPONENTS = (
    ("compiler", "zephium-blocker", "0.1.0"),
    ("service", "zephium-blocker-service", "0.1.0"),
    ("updater", "zephium-blocker-update", "0.1.0"),
    ("update-framework", "tough", "0.24.0"),
    ("transport", "reqwest", "0.13.4"),
    ("tls", "rustls", "0.23.42"),
    ("platform-verifier", "rustls-platform-verifier", "0.7.0"),
    ("crypto-provider", "aws-lc-rs", "1.17.3"),
)
ADBLOCK_MANIFEST_ROOT = Path("build-manifests/adblock")
ADBLOCK_FORK_MANIFEST = ADBLOCK_MANIFEST_ROOT / "FORK.toml"
ADBLOCK_UPSTREAM_INVENTORY = ADBLOCK_MANIFEST_ROOT / "UPSTREAM_FILES.toml"
ADBLOCK_SOURCE_MANIFEST = ADBLOCK_MANIFEST_ROOT / "Cargo.toml.orig"
ADBLOCK_ACTIVE_MANIFEST = ADBLOCK_MANIFEST_ROOT / "Cargo.toml"
ADBLOCK_STANDALONE_LOCK = ADBLOCK_MANIFEST_ROOT / "Cargo.lock"
ADBLOCK_LICENSE_FILE = ADBLOCK_MANIFEST_ROOT / "LICENSE"
ADBLOCK_PROVENANCE_MAX_BYTES = 1024 * 1024
MAX_SBOM_BYTES = 16 * 1024 * 1024
MAX_PAYLOAD_FILES = 50_000
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
SAFE_SUBJECT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,199}$")
RESERVED_PROPERTY_PREFIXES = (
    "zephium:adblock:",
    "zephium:blocker:",
    "zephium:payload:",
    "zephium:release:",
)


class SbomError(ValueError):
    """An SBOM or release subject violates a production invariant."""


def _json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for name, item in pairs:
        if name in value:
            raise SbomError(f"CycloneDX SBOM contains a duplicate JSON key: {name}")
        value[name] = item
    return value


def _invalid_json_constant(value: str) -> None:
    raise SbomError(f"CycloneDX SBOM contains a non-finite JSON number: {value}")


def strict_json_loads(encoded: bytes) -> Any:
    try:
        text = encoded.decode("utf-8")
        return json.loads(
            text,
            object_pairs_hook=_json_object,
            parse_constant=_invalid_json_constant,
        )
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise SbomError(f"CycloneDX SBOM is not valid UTF-8 JSON: {error}") from error


def _file_identity(value: os.stat_result) -> tuple[int, ...]:
    return (
        value.st_dev,
        value.st_ino,
        value.st_mode,
        value.st_nlink,
        value.st_size,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def sha256_regular_file(
    path: Path, description: str, root: Path | None = None
) -> tuple[str, int, Path, int]:
    """Hash one proven regular-file descriptor without a pathname reopen."""
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise SbomError(
            f"cannot open {description} without following links: {path}"
        ) from error

    digest = hashlib.sha256()
    size = 0
    try:
        before = os.fstat(descriptor)
        linked_before = path.lstat()
        if (
            not stat.S_ISREG(before.st_mode)
            or stat.S_ISLNK(linked_before.st_mode)
            or (before.st_dev, before.st_ino)
            != (linked_before.st_dev, linked_before.st_ino)
        ):
            raise SbomError(
                f"{description} must be a non-symbolic regular file: {path}"
            )
        resolved = path.resolve(strict=True)
        if root is not None:
            try:
                resolved.relative_to(root)
            except ValueError as error:
                raise SbomError(
                    f"{description} escapes the staged payload: {path}"
                ) from error

        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = -1
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                size += len(chunk)
                digest.update(chunk)
            after = os.fstat(source.fileno())
            linked_after = path.lstat()
            resolved_after = path.resolve(strict=True)
        if (
            _file_identity(before) != _file_identity(after)
            or _file_identity(before) != _file_identity(linked_before)
            or _file_identity(before) != _file_identity(linked_after)
            or size != before.st_size
            or resolved_after != resolved
        ):
            raise SbomError(f"{description} changed while it was hashed: {path}")
        if root is not None:
            try:
                resolved_after.relative_to(root)
            except ValueError as error:
                raise SbomError(
                    f"{description} escapes the staged payload: {path}"
                ) from error
        return digest.hexdigest(), size, resolved, before.st_mode
    except OSError as error:
        raise SbomError(f"{description} changed while it was hashed: {path}") from error
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def regular_file_bytes(
    path: Path,
    description: str,
    maximum: int,
    root: Path | None = None,
) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise SbomError(f"cannot open {description} without following links: {path}") from error
    try:
        before = os.fstat(descriptor)
        linked_before = path.lstat()
        if (
            not stat.S_ISREG(before.st_mode)
            or stat.S_ISLNK(linked_before.st_mode)
            or (before.st_dev, before.st_ino)
            != (linked_before.st_dev, linked_before.st_ino)
        ):
            raise SbomError(f"{description} must be a non-symbolic regular file: {path}")
        resolved = path.resolve(strict=True)
        if root is not None:
            try:
                resolved.relative_to(root)
            except ValueError as error:
                raise SbomError(
                    f"{description} escapes the staged payload: {path}"
                ) from error
        if before.st_size < 2 or before.st_size > maximum:
            raise SbomError(f"{description} is empty or exceeds the attestation byte limit")
        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = -1
            encoded = source.read(maximum + 1)
            after = os.fstat(source.fileno())
            linked_after = path.lstat()
            resolved_after = path.resolve(strict=True)
        if (
            len(encoded) > maximum
            or _file_identity(before) != _file_identity(after)
            or _file_identity(before) != _file_identity(linked_before)
            or _file_identity(before) != _file_identity(linked_after)
            or len(encoded) != before.st_size
            or resolved_after != resolved
        ):
            raise SbomError(f"{description} changed while it was read")
        if root is not None:
            try:
                resolved_after.relative_to(root)
            except ValueError as error:
                raise SbomError(
                    f"{description} escapes the staged payload: {path}"
                ) from error
        return encoded
    except OSError as error:
        raise SbomError(f"{description} changed while it was read: {path}") from error
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def load_sbom(path: Path) -> dict[str, Any]:
    value = strict_json_loads(
        regular_file_bytes(path, "CycloneDX SBOM", MAX_SBOM_BYTES)
    )
    if not isinstance(value, dict):
        raise SbomError("CycloneDX SBOM root must be an object")
    return value


def bom_references(value: Any) -> set[str]:
    references: set[str] = set()
    pending = [value]
    while pending:
        item = pending.pop()
        if isinstance(item, dict):
            if "bom-ref" in item:
                reference = item["bom-ref"]
                if not isinstance(reference, str) or not reference:
                    raise SbomError("every CycloneDX bom-ref must be a non-empty string")
                if reference in references:
                    raise SbomError(
                        f"CycloneDX SBOM contains a duplicate bom-ref: {reference}"
                    )
                references.add(reference)
            pending.extend(item.values())
        elif isinstance(item, list):
            pending.extend(item)
    return references


def syft_versions(value: dict[str, Any]) -> set[str]:
    metadata = value.get("metadata")
    if not isinstance(metadata, dict):
        return set()
    tools = metadata.get("tools")
    candidates: list[Any]
    if isinstance(tools, list):
        candidates = tools
    elif isinstance(tools, dict) and isinstance(tools.get("components"), list):
        candidates = tools["components"]
    else:
        candidates = []
    versions: set[str] = set()
    for tool in candidates:
        if not isinstance(tool, dict):
            continue
        name = tool.get("name")
        version = tool.get("version")
        if isinstance(name, str) and name.lower() == "syft" and isinstance(version, str):
            versions.add(version.removeprefix("v"))
    return versions


def validate_syft_document(
    value: dict[str, Any], source_name: str, version: str
) -> tuple[dict[str, Any], list[Any]]:
    if value.get("bomFormat") != "CycloneDX":
        raise SbomError("SBOM is not a CycloneDX document")
    spec = value.get("specVersion")
    if not isinstance(spec, str) or not re.fullmatch(r"1\.[4-9]", spec):
        raise SbomError("SBOM uses an unsupported CycloneDX specification version")
    versions = syft_versions(value)
    if versions != {EXPECTED_SYFT_VERSION}:
        raise SbomError(
            f"SBOM was not generated exclusively by Syft {EXPECTED_SYFT_VERSION}: {sorted(versions)}"
        )
    metadata = value.get("metadata")
    assert isinstance(metadata, dict)
    component = metadata.get("component")
    if not isinstance(component, dict):
        raise SbomError("SBOM metadata has no root component")
    if component.get("name") != source_name or component.get("version") != version:
        raise SbomError("SBOM root component does not identify the release payload")
    components = value.get("components")
    if not isinstance(components, list) or not components:
        raise SbomError("Syft discovered no dependency or binary components")
    if any(not isinstance(component, dict) for component in components):
        raise SbomError("CycloneDX components must all be objects")
    return metadata, components


def _property_array(owner: dict[str, Any], description: str) -> list[Any]:
    properties = owner.setdefault("properties", [])
    if not isinstance(properties, list):
        raise SbomError(f"{description} properties must be an array")
    if any(
        not isinstance(item, dict)
        or not isinstance(item.get("name"), str)
        or not isinstance(item.get("value"), str)
        for item in properties
    ):
        raise SbomError(
            f"{description} properties must be string name/value objects"
        )
    return properties


def reject_reserved_properties(value: Any) -> None:
    pending = [value]
    while pending:
        item = pending.pop()
        if isinstance(item, dict):
            if "properties" in item:
                properties = item["properties"]
                if not isinstance(properties, list):
                    raise SbomError("CycloneDX properties must be arrays")
                for prop in properties:
                    if (
                        not isinstance(prop, dict)
                        or not isinstance(prop.get("name"), str)
                        or not isinstance(prop.get("value"), str)
                    ):
                        raise SbomError(
                            "CycloneDX properties must be string name/value objects"
                        )
                    if prop["name"].startswith(RESERVED_PROPERTY_PREFIXES):
                        raise SbomError(
                            "Syft input uses a Zephium-reserved property: "
                            f"{prop['name']}"
                        )
            pending.extend(item.values())
        elif isinstance(item, list):
            pending.extend(item)


def _validate_adblock_license(
    component: dict[str, Any], require_bound_license: bool
) -> None:
    licenses = component.get("licenses")
    if not require_bound_license:
        if licenses is not None:
            raise SbomError(
                "raw Syft workspace-lock adblock component unexpectedly declares a license"
            )
        return
    if not isinstance(licenses, list) or len(licenses) != 1:
        raise SbomError(
            f"Syft adblock component must declare exactly {EXPECTED_ADBLOCK_LICENSE}"
        )
    license_entry = licenses[0]
    if not isinstance(license_entry, dict) or "expression" in license_entry:
        raise SbomError("Syft adblock component has a malformed license")
    license_value = license_entry.get("license")
    if (
        not isinstance(license_value, dict)
        or license_value != {"id": EXPECTED_ADBLOCK_LICENSE}
    ):
        raise SbomError(
            f"Syft adblock component must declare exactly {EXPECTED_ADBLOCK_LICENSE}"
        )


def validate_adblock_component(
    components: list[Any], platform: str, require_bound_license: bool
) -> dict[str, Any]:
    candidates = [
        component
        for component in components
        if isinstance(component, dict)
        and component.get("type") == "library"
        and (
            component.get("name") == EXPECTED_ADBLOCK_NAME
            or component.get("purl") == EXPECTED_ADBLOCK_PURL
            or (
                isinstance(component.get("purl"), str)
                and component["purl"].startswith("pkg:cargo/adblock@")
            )
        )
    ]
    if len(candidates) != 1:
        raise SbomError(
            "Syft must discover exactly one Cargo adblock component, "
            f"found {len(candidates)}"
        )
    component = candidates[0]
    if (
        component.get("name") != EXPECTED_ADBLOCK_NAME
        or component.get("version") != EXPECTED_ADBLOCK_VERSION
        or component.get("purl") != EXPECTED_ADBLOCK_PURL
    ):
        raise SbomError(
            "Syft adblock component does not identify exact Cargo "
            f"{EXPECTED_ADBLOCK_PURL}"
        )
    reference = component.get("bom-ref")
    if not isinstance(reference, str) or not re.fullmatch(
        rf"{re.escape(EXPECTED_ADBLOCK_PURL)}\?package-id=[0-9a-f]{{16}}",
        reference,
    ):
        raise SbomError("Syft adblock component has an unexpected package identity")
    properties = component.get("properties")
    if not isinstance(properties, list):
        raise SbomError("Syft adblock component has no package-discovery properties")
    discovered: dict[str, list[str]] = {}
    for prop in properties:
        if (
            not isinstance(prop, dict)
            or not isinstance(prop.get("name"), str)
            or not isinstance(prop.get("value"), str)
        ):
            raise SbomError("Syft adblock component properties are malformed")
        discovered.setdefault(prop["name"], []).append(prop["value"])
    for name, expected in EXPECTED_ADBLOCK_SYFT_PROPERTIES.items():
        if discovered.get(name) != [expected]:
            raise SbomError(
                f"Syft adblock component has an unexpected {name} property"
            )
    expected_location = EXPECTED_ADBLOCK_SYFT_LOCATIONS.get(platform)
    if expected_location is None:
        raise SbomError(f"unsupported release platform: {platform}")
    if discovered.get("syft:location:0:path") != [expected_location]:
        raise SbomError(
            "Syft adblock component was not discovered from the release workspace lock"
        )
    locations = {
        name: values
        for name, values in discovered.items()
        if name.startswith("syft:location:")
    }
    if locations != {
        "syft:location:0:path": [expected_location]
    }:
        raise SbomError("Syft adblock component has ambiguous discovery locations")
    _validate_adblock_license(component, require_bound_license)
    return component


def validate_blocker_components(
    components: list[Any], platform: str
) -> dict[str, dict[str, Any]]:
    expected_location = EXPECTED_ADBLOCK_SYFT_LOCATIONS.get(platform)
    if expected_location is None:
        raise SbomError(f"unsupported blocker release platform: {platform}")
    validated: dict[str, dict[str, Any]] = {}
    for role, name, version in EXPECTED_BLOCKER_COMPONENTS:
        purl = f"pkg:cargo/{name}@{version}"
        candidates = [
            component
            for component in components
            if isinstance(component, dict)
            and component.get("type") == "library"
            and (
                component.get("name") == name
                or component.get("purl") == purl
                or (
                    isinstance(component.get("purl"), str)
                    and component["purl"].startswith(f"pkg:cargo/{name}@")
                )
            )
        ]
        if len(candidates) != 1:
            raise SbomError(
                f"Syft must discover exactly one Cargo {name} component, "
                f"found {len(candidates)}"
            )
        component = candidates[0]
        if (
            component.get("name") != name
            or component.get("version") != version
            or component.get("purl") != purl
        ):
            raise SbomError(
                f"Syft blocker component does not identify exact Cargo {purl}"
            )
        reference = component.get("bom-ref")
        if not isinstance(reference, str) or not re.fullmatch(
            rf"{re.escape(purl)}\?package-id=[0-9a-f]{{16}}",
            reference,
        ):
            raise SbomError(
                f"Syft {name} component has an unexpected package identity"
            )
        properties = component.get("properties")
        if not isinstance(properties, list):
            raise SbomError(
                f"Syft {name} component has no package-discovery properties"
            )
        discovered: dict[str, list[str]] = {}
        for prop in properties:
            if (
                not isinstance(prop, dict)
                or not isinstance(prop.get("name"), str)
                or not isinstance(prop.get("value"), str)
            ):
                raise SbomError(f"Syft {name} component properties are malformed")
            discovered.setdefault(prop["name"], []).append(prop["value"])
        for property_name, expected in EXPECTED_ADBLOCK_SYFT_PROPERTIES.items():
            if discovered.get(property_name) != [expected]:
                raise SbomError(
                    f"Syft {name} component has an unexpected "
                    f"{property_name} property"
                )
        if discovered.get("syft:location:0:path") != [expected_location]:
            raise SbomError(
                f"Syft {name} component was not discovered from the "
                "release workspace lock"
            )
        locations = {
            property_name: values
            for property_name, values in discovered.items()
            if property_name.startswith("syft:location:")
        }
        if locations != {"syft:location:0:path": [expected_location]}:
            raise SbomError(
                f"Syft {name} component has ambiguous discovery locations"
            )
        validated[role] = component
    return validated


def blocker_component_properties(platform: str) -> list[dict[str, str]]:
    feature_graph = EXPECTED_BLOCKER_FEATURE_GRAPHS.get(platform)
    if feature_graph is None:
        raise SbomError(f"unsupported blocker release platform: {platform}")
    properties = [
        {
            "name": "zephium:blocker:compiler-feature-graph",
            "value": feature_graph,
        },
        {
            "name": "zephium:blocker:update-transport-features",
            "value": "rustls,stream,system-proxy",
        },
    ]
    properties.extend(
        {
            "name": f"zephium:blocker:component:{role}:purl",
            "value": f"pkg:cargo/{name}@{version}",
        }
        for role, name, version in EXPECTED_BLOCKER_COMPONENTS
    )
    properties.sort(key=lambda item: (item["name"], item["value"]))
    return properties


def _strict_toml(encoded: bytes, description: str) -> dict[str, Any]:
    try:
        value = tomllib.loads(encoded.decode("utf-8"))
    except (UnicodeError, tomllib.TOMLDecodeError) as error:
        raise SbomError(f"{description} is not valid UTF-8 TOML: {error}") from error
    if not isinstance(value, dict):
        raise SbomError(f"{description} root must be a TOML table")
    return value


def _table(value: dict[str, Any], name: str, description: str) -> dict[str, Any]:
    item = value.get(name)
    if not isinstance(item, dict):
        raise SbomError(f"{description} has no [{name}] table")
    return item


def _require_exact(
    value: dict[str, Any], name: str, expected: Any, description: str
) -> None:
    if value.get(name) != expected or type(value.get(name)) is not type(expected):
        raise SbomError(
            f"{description} {name} is not the reviewed value {expected!r}"
        )


def _fixed_staged_file(
    root: Path, supplied: Path, relative: Path, description: str
) -> Path:
    if root.is_symlink() or not root.is_dir():
        raise SbomError("staged payload root must be a non-symbolic directory")
    absolute_root = Path(os.path.abspath(root))
    absolute_supplied = Path(os.path.abspath(supplied))
    expected = absolute_root / relative
    if absolute_supplied != expected:
        raise SbomError(
            f"{description} must be the fixed staged payload path {relative.as_posix()}"
        )

    current = absolute_root
    for part in relative.parts[:-1]:
        current /= part
        try:
            mode = current.lstat().st_mode
        except OSError as error:
            raise SbomError(f"{description} parent is unavailable: {current}") from error
        if stat.S_ISLNK(mode) or not stat.S_ISDIR(mode):
            raise SbomError(
                f"{description} parent must be a non-symbolic directory: {current}"
            )
    resolved_root = absolute_root.resolve(strict=True)
    resolved = absolute_supplied.resolve(strict=True)
    try:
        resolved.relative_to(resolved_root)
    except ValueError as error:
        raise SbomError(f"{description} escapes the staged payload") from error
    return absolute_supplied


def _manifest_identity(
    value: dict[str, Any], description: str
) -> None:
    package = _table(value, "package", description)
    _require_exact(package, "name", EXPECTED_ADBLOCK_NAME, description)
    _require_exact(package, "version", EXPECTED_ADBLOCK_VERSION, description)
    _require_exact(package, "license", EXPECTED_ADBLOCK_LICENSE, description)
    repository = package.get("repository")
    if (
        not isinstance(repository, str)
        or repository.rstrip("/") != EXPECTED_ADBLOCK_REPOSITORY
    ):
        raise SbomError(f"{description} repository is not the reviewed upstream")


def adblock_provenance(
    root: Path,
    platform: str,
    fork_manifest_path: Path,
    upstream_inventory_path: Path,
    source_manifest_path: Path,
) -> tuple[list[dict[str, str]], dict[str, tuple[str, int]]]:
    resolved_root = root.resolve(strict=True)
    paths = {
        "fork-manifest": _fixed_staged_file(
            root,
            fork_manifest_path,
            ADBLOCK_FORK_MANIFEST,
            "adblock fork manifest",
        ),
        "upstream-inventory": _fixed_staged_file(
            root,
            upstream_inventory_path,
            ADBLOCK_UPSTREAM_INVENTORY,
            "adblock upstream inventory",
        ),
        "source-manifest": _fixed_staged_file(
            root,
            source_manifest_path,
            ADBLOCK_SOURCE_MANIFEST,
            "adblock source manifest",
        ),
        "active-manifest": _fixed_staged_file(
            root,
            root / ADBLOCK_ACTIVE_MANIFEST,
            ADBLOCK_ACTIVE_MANIFEST,
            "adblock active manifest",
        ),
        "standalone-lock": _fixed_staged_file(
            root,
            root / ADBLOCK_STANDALONE_LOCK,
            ADBLOCK_STANDALONE_LOCK,
            "adblock standalone lockfile",
        ),
        "license-file": _fixed_staged_file(
            root,
            root / ADBLOCK_LICENSE_FILE,
            ADBLOCK_LICENSE_FILE,
            "adblock license file",
        ),
    }
    encoded = {
        name: regular_file_bytes(
            path,
            f"adblock {name}",
            ADBLOCK_PROVENANCE_MAX_BYTES,
            resolved_root,
        )
        for name, path in paths.items()
    }
    digests = {
        name: hashlib.sha256(contents).hexdigest()
        for name, contents in encoded.items()
    }

    fork = _strict_toml(encoded["fork-manifest"], "adblock FORK.toml")
    _require_exact(fork, "schema", 1, "adblock FORK.toml")
    upstream = _table(fork, "upstream", "adblock FORK.toml")
    _require_exact(
        upstream, "repository", EXPECTED_ADBLOCK_REPOSITORY, "adblock FORK.toml"
    )
    _require_exact(upstream, "tag", EXPECTED_ADBLOCK_TAG, "adblock FORK.toml")
    _require_exact(
        upstream, "version", EXPECTED_ADBLOCK_VERSION, "adblock FORK.toml"
    )
    _require_exact(
        upstream, "commit", EXPECTED_ADBLOCK_COMMIT, "adblock FORK.toml"
    )
    _require_exact(upstream, "tree", EXPECTED_ADBLOCK_TREE, "adblock FORK.toml")
    _require_exact(
        upstream, "license", EXPECTED_ADBLOCK_LICENSE, "adblock FORK.toml"
    )
    import_policy = _table(fork, "import", "adblock FORK.toml")
    _require_exact(
        import_policy, "source_manifest", "Cargo.toml.orig", "adblock FORK.toml"
    )
    _require_exact(
        import_policy, "fork_manifest", "Cargo.toml", "adblock FORK.toml"
    )
    _require_exact(
        import_policy, "fork_lockfile", "Cargo.lock", "adblock FORK.toml"
    )
    _require_exact(
        import_policy,
        "upstream_file_inventory",
        "UPSTREAM_FILES.toml",
        "adblock FORK.toml",
    )

    inventory = _strict_toml(
        encoded["upstream-inventory"], "adblock UPSTREAM_FILES.toml"
    )
    _require_exact(inventory, "schema", 1, "adblock UPSTREAM_FILES.toml")
    _require_exact(
        inventory,
        "upstream_commit",
        EXPECTED_ADBLOCK_COMMIT,
        "adblock UPSTREAM_FILES.toml",
    )
    _require_exact(
        inventory,
        "upstream_tree",
        EXPECTED_ADBLOCK_TREE,
        "adblock UPSTREAM_FILES.toml",
    )
    _require_exact(
        inventory,
        "crate_archive_sha256",
        EXPECTED_ADBLOCK_ARCHIVE_SHA256,
        "adblock UPSTREAM_FILES.toml",
    )
    _require_exact(
        inventory,
        "source_manifest_sha256",
        EXPECTED_ADBLOCK_SOURCE_MANIFEST_SHA256,
        "adblock UPSTREAM_FILES.toml",
    )
    _require_exact(
        inventory,
        "source_manifest_git_blob",
        EXPECTED_ADBLOCK_SOURCE_MANIFEST_GIT_BLOB,
        "adblock UPSTREAM_FILES.toml",
    )
    if digests["source-manifest"] != EXPECTED_ADBLOCK_SOURCE_MANIFEST_SHA256:
        raise SbomError(
            "staged Cargo.toml.orig does not match the reviewed upstream manifest hash"
        )
    if digests["license-file"] != EXPECTED_ADBLOCK_LICENSE_SHA256:
        raise SbomError(
            "staged adblock LICENSE does not match the reviewed upstream license hash"
        )

    _manifest_identity(
        _strict_toml(encoded["source-manifest"], "adblock Cargo.toml.orig"),
        "adblock Cargo.toml.orig",
    )
    _manifest_identity(
        _strict_toml(encoded["active-manifest"], "adblock Cargo.toml"),
        "adblock Cargo.toml",
    )
    lock = _strict_toml(encoded["standalone-lock"], "adblock Cargo.lock")
    _require_exact(lock, "version", 4, "adblock Cargo.lock")
    packages = lock.get("package")
    if not isinstance(packages, list):
        raise SbomError("adblock Cargo.lock has no package array")
    roots = [
        package
        for package in packages
        if isinstance(package, dict) and package.get("name") == EXPECTED_ADBLOCK_NAME
    ]
    if (
        len(roots) != 1
        or roots[0].get("version") != EXPECTED_ADBLOCK_VERSION
        or "source" in roots[0]
        or "checksum" in roots[0]
    ):
        raise SbomError("adblock Cargo.lock does not identify the exact local fork root")
    feature_graph = EXPECTED_ADBLOCK_FEATURE_GRAPHS.get(platform)
    if feature_graph is None:
        raise SbomError(f"unsupported adblock release platform: {platform}")

    properties = [
        {"name": "zephium:adblock:crate:license", "value": EXPECTED_ADBLOCK_LICENSE},
        {"name": "zephium:adblock:crate:name", "value": EXPECTED_ADBLOCK_NAME},
        {"name": "zephium:adblock:crate:purl", "value": EXPECTED_ADBLOCK_PURL},
        {"name": "zephium:adblock:crate:version", "value": EXPECTED_ADBLOCK_VERSION},
        {"name": "zephium:adblock:feature-graph", "value": feature_graph},
        {
            "name": "zephium:adblock:source-manifest:git-blob",
            "value": EXPECTED_ADBLOCK_SOURCE_MANIFEST_GIT_BLOB,
        },
        {
            "name": "zephium:adblock:upstream:archive-sha256",
            "value": EXPECTED_ADBLOCK_ARCHIVE_SHA256,
        },
        {
            "name": "zephium:adblock:upstream:commit",
            "value": EXPECTED_ADBLOCK_COMMIT,
        },
        {
            "name": "zephium:adblock:upstream:repository",
            "value": EXPECTED_ADBLOCK_REPOSITORY,
        },
        {"name": "zephium:adblock:upstream:tag", "value": EXPECTED_ADBLOCK_TAG},
        {"name": "zephium:adblock:upstream:tree", "value": EXPECTED_ADBLOCK_TREE},
    ]
    payload_files: dict[str, tuple[str, int]] = {}
    for name, path in paths.items():
        relative = path.resolve(strict=True).relative_to(resolved_root).as_posix()
        digest = digests[name]
        size = len(encoded[name])
        properties.extend(
            [
                {
                    "name": f"zephium:adblock:{name}:path",
                    "value": relative,
                },
                {
                    "name": f"zephium:adblock:{name}:sha256",
                    "value": digest,
                },
                {
                    "name": f"zephium:adblock:{name}:size",
                    "value": str(size),
                },
            ]
        )
        payload_files[relative] = (digest, size)
    properties.sort(key=lambda item: (item["name"], item["value"]))
    return properties, payload_files


def cross_check_adblock_payload(
    file_components: list[dict[str, Any]],
    expected_files: dict[str, tuple[str, int]],
) -> None:
    by_name = {component.get("name"): component for component in file_components}
    for relative, (digest, size) in expected_files.items():
        component = by_name.get(relative)
        if not isinstance(component, dict):
            raise SbomError(
                f"adblock provenance file is absent from payload inventory: {relative}"
            )
        hashes = component.get("hashes")
        if hashes != [{"alg": "SHA-256", "content": digest}]:
            raise SbomError(
                f"adblock provenance payload hash disagrees for {relative}"
            )
        properties = component.get("properties")
        if not isinstance(properties, list):
            raise SbomError(
                f"adblock provenance payload metadata is absent for {relative}"
            )
        property_map = {
            item.get("name"): item.get("value")
            for item in properties
            if isinstance(item, dict)
        }
        if (
            property_map.get("zephium:payload:kind") != "regular"
            or property_map.get("zephium:payload:size") != str(size)
            or property_map.get("zephium:payload:path") != relative
        ):
            raise SbomError(
                f"adblock provenance payload metadata disagrees for {relative}"
            )


def ensure_inside(root: Path, path: Path, description: str) -> Path:
    resolved = path.resolve(strict=True)
    try:
        resolved.relative_to(root)
    except ValueError as error:
        raise SbomError(f"{description} escapes the staged payload: {path}") from error
    return resolved


def payload_components(root: Path) -> tuple[list[dict[str, Any]], str]:
    if root.is_symlink() or not root.is_dir():
        raise SbomError("staged payload root must be a non-symbolic directory")
    resolved_root = root.resolve(strict=True)
    entries: list[dict[str, Any]] = []
    inventory_lines: list[str] = []

    for directory, directory_names, file_names in os.walk(
        root, topdown=True, followlinks=False
    ):
        directory_path = Path(directory)
        directory_names.sort()
        file_names.sort()
        for name in [*directory_names, *file_names]:
            path = directory_path / name
            mode = path.lstat().st_mode
            relative = path.relative_to(root).as_posix()
            if stat.S_ISDIR(mode):
                continue
            reference = "zephium-payload:" + hashlib.sha256(
                relative.encode("utf-8")
            ).hexdigest()
            properties = [{"name": "zephium:payload:path", "value": relative}]
            permissions = format(stat.S_IMODE(mode), "04o")
            properties.append(
                {"name": "zephium:payload:mode", "value": permissions}
            )
            component: dict[str, Any] = {
                "bom-ref": reference,
                "name": relative,
                "properties": properties,
                "type": "file",
            }
            if stat.S_ISREG(mode):
                digest, size, _, verified_mode = sha256_regular_file(
                    path, "payload file", resolved_root
                )
                permissions = format(stat.S_IMODE(verified_mode), "04o")
                properties[1]["value"] = permissions
                component["hashes"] = [{"alg": "SHA-256", "content": digest}]
                properties.extend(
                    [
                        {"name": "zephium:payload:kind", "value": "regular"},
                        {"name": "zephium:payload:size", "value": str(size)},
                    ]
                )
                inventory_lines.append(
                    f"file\0{relative}\0{permissions}\0{size}\0{digest}\n"
                )
            elif stat.S_ISLNK(mode):
                target = os.readlink(path)
                ensure_inside(resolved_root, path, "payload symbolic link")
                properties.extend(
                    [
                        {"name": "zephium:payload:kind", "value": "symlink"},
                        {"name": "zephium:payload:target", "value": target},
                    ]
                )
                inventory_lines.append(
                    f"symlink\0{relative}\0{permissions}\0{target}\n"
                )
            else:
                raise SbomError(f"staged payload contains a special file: {relative}")
            entries.append(component)
            if len(entries) > MAX_PAYLOAD_FILES:
                raise SbomError("staged payload exceeds the file inventory budget")

    if not entries:
        raise SbomError("staged payload contains no files")
    inventory = "".join(inventory_lines).encode("utf-8")
    return entries, hashlib.sha256(inventory).hexdigest()


def artifact_properties(subjects: list[Path]) -> list[dict[str, str]]:
    if not subjects:
        raise SbomError("at least one release subject is required")
    names: set[str] = set()
    properties: list[dict[str, str]] = []
    for index, subject in enumerate(subjects):
        if not SAFE_SUBJECT_RE.fullmatch(subject.name) or subject.name in names:
            raise SbomError(f"release subject has an unsafe or duplicate name: {subject.name}")
        names.add(subject.name)
        digest, size, _, _ = sha256_regular_file(subject, "release subject")
        prefix = f"zephium:release:subject:{index}"
        properties.extend(
            [
                {"name": f"{prefix}:name", "value": subject.name},
                {"name": f"{prefix}:sha256", "value": digest},
                {"name": f"{prefix}:size", "value": str(size)},
            ]
        )
    return properties


def executable_properties(
    root: Path, executables: list[Path], reference: Path | None
) -> list[dict[str, str]]:
    if not executables:
        raise SbomError("at least one extracted main executable is required")
    resolved_root = root.resolve(strict=True)
    expected: tuple[str, int] | None = None
    if reference is not None:
        digest, size, _, _ = sha256_regular_file(reference, "reference executable")
        expected = (digest, size)
    properties: list[dict[str, str]] = []
    seen: set[Path] = set()
    for index, executable in enumerate(executables):
        digest, size, resolved, _ = sha256_regular_file(
            executable, "extracted main executable", resolved_root
        )
        if resolved in seen:
            raise SbomError("an extracted main executable was supplied more than once")
        seen.add(resolved)
        identity = (digest, size)
        if expected is None:
            expected = identity
        elif identity != expected:
            raise SbomError("extracted main executable differs from the signed reference")
        digest, size = identity
        prefix = f"zephium:release:executable:{index}"
        properties.extend(
            [
                {
                    "name": f"{prefix}:path",
                    "value": resolved.relative_to(resolved_root).as_posix(),
                },
                {"name": f"{prefix}:sha256", "value": digest},
                {"name": f"{prefix}:size", "value": str(size)},
            ]
        )
    return properties


def finalize(args: argparse.Namespace) -> None:
    if not COMMIT_RE.fullmatch(args.commit):
        raise SbomError("release commit must be a lowercase full Git SHA")
    value = load_sbom(args.sbom)
    existing_refs = bom_references(value)
    metadata, components = validate_syft_document(
        value, args.source_name, args.version
    )
    reject_reserved_properties(value)
    adblock_component = validate_adblock_component(
        components, args.platform, require_bound_license=False
    )
    blocker_components = validate_blocker_components(components, args.platform)
    blocker_properties = blocker_component_properties(args.platform)
    adblock_properties, adblock_files = adblock_provenance(
        args.scan_root,
        args.platform,
        args.adblock_fork_manifest,
        args.adblock_upstream_inventory,
        args.adblock_source_manifest,
    )
    file_components, payload_digest = payload_components(args.scan_root)
    cross_check_adblock_payload(file_components, adblock_files)
    if any(component["bom-ref"] in existing_refs for component in file_components):
        raise SbomError("payload file inventory collides with a Syft component reference")

    properties = _property_array(metadata, "SBOM metadata")
    properties.extend(dict(item) for item in adblock_properties)
    properties.extend(dict(item) for item in blocker_properties)
    properties.extend(
        [
            {"name": "zephium:release:commit", "value": args.commit},
            {"name": "zephium:release:platform", "value": args.platform},
            {
                "name": "zephium:release:payload-inventory-sha256",
                "value": payload_digest,
            },
            {"name": "zephium:release:version", "value": args.version},
        ]
    )
    properties.extend(artifact_properties(args.subject))
    properties.extend(
        executable_properties(
            args.scan_root, args.executable, args.reference_executable
        )
    )
    properties.sort(key=lambda item: (str(item.get("name")), str(item.get("value"))))
    component_properties = _property_array(
        adblock_component, "Syft adblock component"
    )
    component_properties.extend(dict(item) for item in adblock_properties)
    component_properties.sort(
        key=lambda item: (str(item.get("name")), str(item.get("value")))
    )
    adblock_component["licenses"] = [
        {"license": {"id": EXPECTED_ADBLOCK_LICENSE}}
    ]
    for role, component in blocker_components.items():
        component_properties = _property_array(
            component, f"Syft blocker {role} component"
        )
        component_properties.append(
            {"name": "zephium:blocker:component-role", "value": role}
        )
        component_properties.sort(
            key=lambda item: (str(item.get("name")), str(item.get("value")))
        )
    components.extend(file_components)
    components.sort(key=lambda item: str(item.get("bom-ref", "")))
    expected_references = bom_references(value)

    try:
        encoded = (
            json.dumps(
                value,
                allow_nan=False,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            )
            + "\n"
        ).encode("utf-8")
    except (TypeError, ValueError, RecursionError) as error:
        raise SbomError(f"final CycloneDX SBOM is not exactly serializable: {error}") from error
    if len(encoded) > MAX_SBOM_BYTES:
        raise SbomError("final CycloneDX SBOM exceeds the attestation byte limit")
    preflight = strict_json_loads(encoded)
    if preflight != value:
        raise SbomError("final CycloneDX SBOM changed during JSON serialization")
    if (
        json.dumps(
            preflight,
            allow_nan=False,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    ).encode("utf-8") != encoded:
        raise SbomError("final CycloneDX SBOM failed canonical JSON round-trip")
    descriptor, temporary_name = tempfile.mkstemp(
        dir=args.sbom.parent, prefix=f".{args.sbom.name}.", suffix=".tmp"
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as destination:
            destination.write(encoded)
            destination.flush()
            os.fsync(destination.fileno())
        temporary.chmod(0o644)
        os.replace(temporary, args.sbom)
    finally:
        temporary.unlink(missing_ok=True)

    # Re-read the exact bytes that the attestation step will consume. Check the
    # entire document, not selected metadata/count fields that could mask a
    # same-sized mutation elsewhere in the component graph.
    if regular_file_bytes(args.sbom, "final CycloneDX SBOM", MAX_SBOM_BYTES) != encoded:
        raise SbomError("final CycloneDX SBOM bytes changed during publication")
    verified = load_sbom(args.sbom)
    _, verified_components = validate_syft_document(
        verified, args.source_name, args.version
    )
    validate_adblock_component(
        verified_components, args.platform, require_bound_license=True
    )
    validate_blocker_components(verified_components, args.platform)
    if bom_references(verified) != expected_references or verified != value:
        raise SbomError("final CycloneDX SBOM did not round-trip exactly")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    root.add_argument("--sbom", type=Path, required=True)
    root.add_argument("--scan-root", type=Path, required=True)
    root.add_argument("--source-name", required=True)
    root.add_argument("--version", required=True)
    root.add_argument("--commit", required=True)
    root.add_argument(
        "--platform", choices=("linux", "windows", "macos"), required=True
    )
    root.add_argument("--subject", type=Path, action="append", required=True)
    root.add_argument("--executable", type=Path, action="append", required=True)
    root.add_argument("--reference-executable", type=Path)
    root.add_argument("--adblock-fork-manifest", type=Path, required=True)
    root.add_argument("--adblock-upstream-inventory", type=Path, required=True)
    root.add_argument("--adblock-source-manifest", type=Path, required=True)
    return root


def main() -> int:
    try:
        finalize(parser().parse_args())
    except (OSError, SbomError) as error:
        print(f"release SBOM error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
