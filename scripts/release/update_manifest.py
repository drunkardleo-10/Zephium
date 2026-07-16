#!/usr/bin/env python3
"""Create and verify Zephium's canonical, rollback-resistant update metadata.

Release automation signs the exact canonical manifest bytes emitted here.  The
chain verifier is deliberately pure: the workflow enumerates GitHub releases,
downloads each manifest, verifies its signature, resolves its protected tag,
and then supplies those immutable local inputs to this module.

An updater must still embed the update trust root and durably persist the
largest accepted sequence before it considers an artifact URL.
"""

from __future__ import annotations

import argparse
import datetime as dt
import errno
import hashlib
import json
import os
import re
import stat
import sys
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 2
CHANNEL = "stable"
MANIFEST_NAME = "zephium-update-stable.json"
BUNDLE_NAME = f"{MANIFEST_NAME}.sigstore.json"
COMMIT_NAME = "tag-commit.txt"
MAX_MANIFEST_BYTES = 1024 * 1024
MAX_SIGNATURE_BUNDLE_BYTES = 8 * 1024 * 1024
MAX_RELEASE_INDEX_BYTES = 32 * 1024 * 1024
MAX_RELEASES = 4096
MAX_ARTIFACTS = 16
MAX_ARTIFACT_BYTES = 512 * 1024 * 1024
SEMVER_RE = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)
STABLE_VERSION_RE = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$"
)
STABLE_TAG_RE = re.compile(
    r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$"
)
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
KEY_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{7,127}$")
SAFE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,199}$")
REPOSITORY_OWNER_RE = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$")
REPOSITORY_NAME_RE = re.compile(
    r"^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,98}[A-Za-z0-9._-])?$"
)
RFC3339_RE = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}"
    r"(?:\.[0-9]+)?(?:Z|[+-][0-9]{2}:[0-9]{2})$"
)


class ManifestError(ValueError):
    """A release input or manifest violates a security invariant."""


@dataclass(frozen=True)
class ReleaseAsset:
    name: str
    sha256: str
    size: int


@dataclass(frozen=True)
class PublishedRelease:
    release_id: int
    tag: str
    manifest_asset: ReleaseAsset
    bundle_asset: ReleaseAsset


@dataclass(frozen=True)
class ChainRelease:
    release_id: int
    tag: str
    commit: str
    manifest: dict[str, Any]
    raw_manifest: bytes


@dataclass(frozen=True)
class ChainHead:
    release: ChainRelease | None
    next_sequence: int


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode("utf-8")


def _same_file(left: os.stat_result, right: os.stat_result) -> bool:
    return (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino)


def read_regular_bytes(
    path: Path,
    *,
    description: str,
    minimum: int,
    maximum: int,
) -> bytes:
    """Read one non-symbolic regular-file identity through one descriptor."""

    try:
        before = path.lstat()
    except OSError as error:
        raise ManifestError(f"cannot inspect {description} {path}: {error}") from error
    if stat.S_ISLNK(before.st_mode) or not stat.S_ISREG(before.st_mode):
        raise ManifestError(f"{description} is not a non-symbolic regular file: {path}")
    if not minimum <= before.st_size <= maximum:
        raise ManifestError(f"{description} is outside its byte budget: {path}")

    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_BINARY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ManifestError(f"cannot open {description} {path}: {error}") from error
    try:
        opened = os.fstat(descriptor)
        if not stat.S_ISREG(opened.st_mode) or not _same_file(before, opened):
            raise ManifestError(f"{description} changed identity while opening: {path}")
        chunks: list[bytes] = []
        total = 0
        while True:
            chunk = os.read(descriptor, min(1024 * 1024, maximum + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            if total > maximum:
                raise ManifestError(f"{description} exceeds its byte budget: {path}")
        after = os.fstat(descriptor)
        if (
            not _same_file(opened, after)
            or after.st_size != total
            or after.st_mtime_ns != opened.st_mtime_ns
            or not minimum <= total <= maximum
        ):
            raise ManifestError(f"{description} changed while it was being read: {path}")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def hash_regular_file(path: Path, *, maximum: int) -> tuple[str, int]:
    try:
        before = path.lstat()
    except OSError as error:
        raise ManifestError(f"cannot inspect release artifact {path}: {error}") from error
    if stat.S_ISLNK(before.st_mode) or not stat.S_ISREG(before.st_mode):
        raise ManifestError(f"release artifact is not a non-symbolic regular file: {path}")
    if not 1 <= before.st_size <= maximum:
        raise ManifestError(f"release artifact is outside its byte budget: {path}")

    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_BINARY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        if not stat.S_ISREG(opened.st_mode) or not _same_file(before, opened):
            raise ManifestError(f"release artifact changed identity while opening: {path}")
        digest = hashlib.sha256()
        size = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            size += len(chunk)
            if size > maximum:
                raise ManifestError(f"release artifact exceeds its byte budget: {path}")
            digest.update(chunk)
        after = os.fstat(descriptor)
        if (
            not _same_file(opened, after)
            or size != after.st_size
            or after.st_mtime_ns != opened.st_mtime_ns
            or size < 1
        ):
            raise ManifestError(f"release artifact changed while it was hashed: {path}")
        return digest.hexdigest(), size
    finally:
        os.close(descriptor)


def _validated_output_parent(path: Path) -> tuple[Path, os.stat_result]:
    if (
        path.name in {"", ".", ".."}
        or ".." in path.parts
        or SAFE_NAME_RE.fullmatch(path.name) is None
    ):
        raise ManifestError("release output has no safe final filename")
    parent = path.parent
    try:
        parent_lstat = parent.lstat()
    except OSError as error:
        raise ManifestError(f"cannot inspect release output parent {parent}: {error}") from error
    if stat.S_ISLNK(parent_lstat.st_mode) or not stat.S_ISDIR(parent_lstat.st_mode):
        raise ManifestError("release output parent must be a non-symbolic directory")
    resolved_parent = parent.resolve(strict=True)
    parent_stat = resolved_parent.stat()
    if not _same_file(parent_lstat, parent_stat):
        raise ManifestError("release output parent changed identity while resolving")
    return resolved_parent, parent_stat


def write_new_file(path: Path, payload: bytes, *, mode: int = 0o644) -> None:
    """Create one durable output without resolving or replacing its final path."""

    parent, parent_identity = _validated_output_parent(path)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_BINARY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    directory_descriptor: int | None = None
    descriptor: int | None = None
    created = False
    created_path = parent / path.name

    def remove_partial_output() -> None:
        if not created:
            return
        try:
            created_path.unlink(missing_ok=True)
        except OSError:
            pass

    try:
        if os.open in os.supports_dir_fd and hasattr(os, "O_DIRECTORY"):
            directory_descriptor = os.open(
                parent,
                os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_CLOEXEC", 0),
            )
            if not _same_file(parent_identity, os.fstat(directory_descriptor)):
                raise ManifestError("release output parent changed identity while opening")
            descriptor = os.open(
                path.name,
                flags,
                mode,
                dir_fd=directory_descriptor,
            )
        else:
            descriptor = os.open(created_path, flags, mode)
        created = True
        opened = os.fstat(descriptor)
        if not stat.S_ISREG(opened.st_mode) or opened.st_nlink != 1:
            raise ManifestError("release output is not one regular file")
        with os.fdopen(descriptor, "wb") as destination:
            descriptor = None
            destination.write(payload)
            destination.flush()
            os.fsync(destination.fileno())
        if directory_descriptor is not None:
            os.fsync(directory_descriptor)
    except FileExistsError as error:
        raise ManifestError(f"refusing to replace existing release output: {path}") from error
    except OSError as error:
        remove_partial_output()
        if error.errno in {errno.ELOOP, errno.ENOTDIR}:
            raise ManifestError(f"release output path is unsafe: {path}") from error
        raise
    except BaseException:
        remove_partial_output()
        raise
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if directory_descriptor is not None:
            os.close(directory_descriptor)


def is_strict_semver(value: str) -> bool:
    match = SEMVER_RE.fullmatch(value)
    if match is None:
        return False
    prerelease = match.group(4)
    if prerelease is None:
        return True
    return all(
        not (identifier.isdigit() and len(identifier) > 1 and identifier[0] == "0")
        for identifier in prerelease.split(".")
    )


def is_stable_version(value: str) -> bool:
    return STABLE_VERSION_RE.fullmatch(value) is not None


def is_stable_tag(value: str) -> bool:
    return STABLE_TAG_RE.fullmatch(value) is not None


def semver_is_greater(candidate: str, previous: str) -> bool:
    if not is_strict_semver(candidate) or not is_strict_semver(previous):
        raise ManifestError("cannot compare an invalid semantic version")
    candidate_match = SEMVER_RE.fullmatch(candidate)
    previous_match = SEMVER_RE.fullmatch(previous)
    assert candidate_match is not None and previous_match is not None
    candidate_core = tuple(int(candidate_match.group(index)) for index in range(1, 4))
    previous_core = tuple(int(previous_match.group(index)) for index in range(1, 4))
    if candidate_core != previous_core:
        return candidate_core > previous_core

    candidate_pre = candidate_match.group(4)
    previous_pre = previous_match.group(4)
    if candidate_pre is None or previous_pre is None:
        return candidate_pre is None and previous_pre is not None
    for candidate_id, previous_id in zip(
        candidate_pre.split("."), previous_pre.split(".")
    ):
        if candidate_id == previous_id:
            continue
        candidate_numeric = candidate_id.isdigit()
        previous_numeric = previous_id.isdigit()
        if candidate_numeric and previous_numeric:
            return int(candidate_id) > int(previous_id)
        if candidate_numeric != previous_numeric:
            return not candidate_numeric
        return candidate_id > previous_id
    return len(candidate_pre.split(".")) > len(previous_pre.split("."))


def validate_repository(repository: str) -> tuple[str, str]:
    parts = repository.split("/")
    if (
        len(parts) != 2
        or REPOSITORY_OWNER_RE.fullmatch(parts[0]) is None
        or REPOSITORY_NAME_RE.fullmatch(parts[1]) is None
        or parts[1] in {".", ".."}
    ):
        raise ManifestError("repository must be one safe GitHub owner/name pair")
    return parts[0], parts[1]


def validate_rfc3339(value: str) -> None:
    if RFC3339_RE.fullmatch(value) is None:
        raise ManifestError("timestamp must be RFC 3339 with an explicit zone")
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        parsed = dt.datetime.fromisoformat(normalized)
    except ValueError as error:
        raise ManifestError("timestamp is not a real RFC 3339 instant") from error
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise ManifestError("timestamp must have an explicit UTC offset")


def expected_download_url(repository: str, tag: str, name: str) -> str:
    owner, repo = validate_repository(repository)
    if not is_stable_tag(tag):
        raise ManifestError("release tag must be exactly vMAJOR.MINOR.PATCH")
    if SAFE_NAME_RE.fullmatch(name) is None:
        raise ManifestError("release asset name is unsafe")
    return (
        "https://github.com/"
        f"{urllib.parse.quote(owner, safe='')}/{urllib.parse.quote(repo, safe='')}"
        f"/releases/download/{urllib.parse.quote(tag, safe='')}/"
        f"{urllib.parse.quote(name, safe='')}"
    )


def load_canonical_manifest(path: Path) -> tuple[dict[str, Any], bytes]:
    raw = read_regular_bytes(
        path,
        description="update manifest",
        minimum=2,
        maximum=MAX_MANIFEST_BYTES,
    )
    try:
        value = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ManifestError(f"{path} is not valid UTF-8 JSON: {error}") from error
    if not isinstance(value, dict):
        raise ManifestError(f"{path} must contain a JSON object")
    if canonical_bytes(value) != raw:
        raise ManifestError(f"{path} is not in the required canonical JSON encoding")
    validate_manifest_shape(value)
    return value, raw


def classify_artifact(name: str) -> tuple[str, str, str]:
    lower = name.lower()
    if lower.endswith(".rpm"):
        return "linux", require_arch(name, ("x86_64", "amd64"), "x86_64"), "rpm"
    if lower.endswith(".msi"):
        return "windows", require_arch(name, ("x64", "x86_64", "amd64"), "x86_64"), "msi"
    if lower.endswith("-setup.exe"):
        return "windows", require_arch(name, ("x64", "x86_64", "amd64"), "x86_64"), "nsis"
    if lower.endswith(".dmg"):
        if any(token in lower for token in ("aarch64", "arm64")):
            arch = "aarch64"
        elif any(token in lower for token in ("x86_64", "x64", "amd64")):
            arch = "x86_64"
        elif "universal" in lower:
            arch = "universal"
        else:
            raise ManifestError(f"macOS artifact does not identify its architecture: {name}")
        return "macos", arch, "dmg"
    raise ManifestError(f"unsupported publishable artifact type: {name}")


def require_arch(name: str, tokens: tuple[str, ...], normalized: str) -> str:
    lower = name.lower()
    if not any(token in lower for token in tokens):
        raise ManifestError(f"artifact does not identify its architecture: {name}")
    return normalized


def validate_manifest_shape(value: dict[str, Any]) -> None:
    expected = {
        "artifacts",
        "channel",
        "commit",
        "published_at",
        "repository",
        "rollback",
        "schema_version",
        "signing_key_id",
        "tag",
        "version",
    }
    if set(value) != expected:
        raise ManifestError("manifest has missing or unexpected top-level fields")
    if value["schema_version"] != SCHEMA_VERSION or value["channel"] != CHANNEL:
        raise ManifestError("unsupported update manifest schema or channel")
    version = value["version"]
    tag = value["tag"]
    repository = value["repository"]
    if not isinstance(version, str) or not is_stable_version(version):
        raise ManifestError("stable-channel manifest version must be MAJOR.MINOR.PATCH")
    if not isinstance(tag, str) or tag != f"v{version}" or not is_stable_tag(tag):
        raise ManifestError("manifest tag must exactly identify its stable version")
    if not isinstance(repository, str):
        raise ManifestError("manifest repository is invalid")
    validate_repository(repository)
    if not isinstance(value["commit"], str) or not COMMIT_RE.fullmatch(value["commit"]):
        raise ManifestError("manifest commit must be a lowercase full Git SHA")
    if not isinstance(value["published_at"], str):
        raise ManifestError("manifest published_at is invalid")
    validate_rfc3339(value["published_at"])
    if not isinstance(value["signing_key_id"], str) or not KEY_ID_RE.fullmatch(
        value["signing_key_id"]
    ):
        raise ManifestError("manifest signing_key_id is invalid")

    rollback = value["rollback"]
    if not isinstance(rollback, dict) or set(rollback) != {
        "previous_manifest_sha256",
        "previous_sequence",
        "sequence",
    }:
        raise ManifestError("manifest rollback block is malformed")
    sequence = rollback["sequence"]
    if not isinstance(sequence, int) or isinstance(sequence, bool) or sequence < 1:
        raise ManifestError("manifest sequence must be a positive integer")
    previous_sequence = rollback["previous_sequence"]
    previous_digest = rollback["previous_manifest_sha256"]
    if previous_sequence is None:
        if previous_digest is not None or sequence != 1:
            raise ManifestError("only sequence 1 may omit the previous manifest link")
    else:
        if (
            not isinstance(previous_sequence, int)
            or isinstance(previous_sequence, bool)
            or previous_sequence < 1
            or previous_sequence >= sequence
        ):
            raise ManifestError("previous sequence must be positive and lower than sequence")
        if not isinstance(previous_digest, str) or not re.fullmatch(
            r"[0-9a-f]{64}", previous_digest
        ):
            raise ManifestError("previous manifest digest must be lowercase SHA-256")

    artifacts = value["artifacts"]
    if not isinstance(artifacts, list) or not 1 <= len(artifacts) <= MAX_ARTIFACTS:
        raise ManifestError("manifest artifact count is outside the release budget")
    names: set[str] = set()
    identities: set[tuple[str, str, str]] = set()
    for artifact in artifacts:
        validate_artifact(artifact, repository=repository, tag=tag)
        name = artifact["name"]
        identity = (artifact["platform"], artifact["arch"], artifact["kind"])
        if name in names or identity in identities:
            raise ManifestError("manifest contains a duplicate artifact name or identity")
        names.add(name)
        identities.add(identity)
    if artifacts != sorted(artifacts, key=lambda item: item["name"]):
        raise ManifestError("manifest artifacts must be sorted by name")


def validate_artifact(artifact: Any, *, repository: str, tag: str) -> None:
    expected = {"arch", "kind", "name", "platform", "sha256", "size", "url"}
    if not isinstance(artifact, dict) or set(artifact) != expected:
        raise ManifestError("manifest artifact has missing or unexpected fields")
    name = artifact["name"]
    if not isinstance(name, str) or not SAFE_NAME_RE.fullmatch(name):
        raise ManifestError("manifest artifact name is unsafe")
    platform, arch, kind = classify_artifact(name)
    if (artifact["platform"], artifact["arch"], artifact["kind"]) != (
        platform,
        arch,
        kind,
    ):
        raise ManifestError(f"artifact classification does not match its name: {name}")
    if not isinstance(artifact["sha256"], str) or not re.fullmatch(
        r"[0-9a-f]{64}", artifact["sha256"]
    ):
        raise ManifestError(f"artifact SHA-256 is invalid: {name}")
    if (
        not isinstance(artifact["size"], int)
        or isinstance(artifact["size"], bool)
        or artifact["size"] < 1
        or artifact["size"] > MAX_ARTIFACT_BYTES
    ):
        raise ManifestError(f"artifact size is invalid: {name}")
    if artifact["url"] != expected_download_url(repository, tag, name):
        raise ManifestError(f"artifact URL is not the exact protected release URL: {name}")


def require_manifest_identity(
    manifest: dict[str, Any],
    *,
    repository: str | None = None,
    tag: str | None = None,
    commit: str | None = None,
    signing_key_id: str | None = None,
) -> None:
    if repository is not None and manifest["repository"] != repository:
        raise ManifestError("manifest repository does not match the expected repository")
    if tag is not None and manifest["tag"] != tag:
        raise ManifestError("manifest tag does not match the protected release tag")
    if commit is not None and manifest["commit"] != commit:
        raise ManifestError("manifest commit does not match the protected release commit")
    if signing_key_id is not None and manifest["signing_key_id"] != signing_key_id:
        raise ManifestError("manifest signing key does not match the protected trust root")


def build_manifest(args: argparse.Namespace) -> dict[str, Any]:
    version = args.tag.removeprefix("v")
    if args.tag != f"v{version}" or not is_stable_version(version):
        raise ManifestError("stable release tag must be vMAJOR.MINOR.PATCH")
    validate_repository(args.repository)
    if not COMMIT_RE.fullmatch(args.commit):
        raise ManifestError("commit must be a lowercase full Git SHA")
    if not KEY_ID_RE.fullmatch(args.signing_key_id):
        raise ManifestError("signing key identifier is invalid")
    validate_rfc3339(args.published_at)

    previous_sequence: int | None = None
    previous_digest: str | None = None
    if args.previous is None:
        if args.sequence != 1:
            raise ManifestError("a release after sequence 1 must link a previous manifest")
    else:
        previous, previous_raw = load_canonical_manifest(args.previous)
        require_manifest_identity(
            previous,
            repository=args.repository,
            signing_key_id=args.signing_key_id,
        )
        previous_sequence = previous["rollback"]["sequence"]
        if args.sequence != previous_sequence + 1:
            raise ManifestError("release sequence must advance by exactly one")
        if not semver_is_greater(version, previous["version"]):
            raise ManifestError("release version must have higher SemVer precedence")
        previous_digest = hashlib.sha256(previous_raw).hexdigest()

    try:
        artifact_mode = args.artifact_dir.lstat().st_mode
    except OSError as error:
        raise ManifestError(f"cannot inspect artifact directory: {error}") from error
    if stat.S_ISLNK(artifact_mode) or not stat.S_ISDIR(artifact_mode):
        raise ManifestError("artifact directory must be a non-symbolic directory")
    artifacts: list[dict[str, Any]] = []
    for path in sorted(args.artifact_dir.iterdir(), key=lambda item: item.name):
        mode = path.lstat().st_mode
        if stat.S_ISLNK(mode):
            raise ManifestError(f"publish directory contains a symbolic link: {path.name}")
        if not stat.S_ISREG(mode):
            continue
        try:
            platform, arch, kind = classify_artifact(path.name)
        except ManifestError:
            continue
        if not SAFE_NAME_RE.fullmatch(path.name):
            raise ManifestError(f"unsafe artifact filename: {path.name}")
        artifact_digest, artifact_size = hash_regular_file(
            path, maximum=MAX_ARTIFACT_BYTES
        )
        artifacts.append(
            {
                "arch": arch,
                "kind": kind,
                "name": path.name,
                "platform": platform,
                "sha256": artifact_digest,
                "size": artifact_size,
                "url": expected_download_url(args.repository, args.tag, path.name),
            }
        )
    if not artifacts:
        raise ManifestError("publish directory contains no supported installers")

    manifest: dict[str, Any] = {
        "artifacts": artifacts,
        "channel": CHANNEL,
        "commit": args.commit,
        "published_at": args.published_at,
        "repository": args.repository,
        "rollback": {
            "previous_manifest_sha256": previous_digest,
            "previous_sequence": previous_sequence,
            "sequence": args.sequence,
        },
        "schema_version": SCHEMA_VERSION,
        "signing_key_id": args.signing_key_id,
        "tag": args.tag,
        "version": version,
    }
    validate_manifest_shape(manifest)
    return manifest


def load_release_index(path: Path) -> list[Any]:
    raw = read_regular_bytes(
        path,
        description="GitHub release index",
        minimum=2,
        maximum=MAX_RELEASE_INDEX_BYTES,
    )
    try:
        value = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ManifestError(f"release index is not valid UTF-8 JSON: {error}") from error
    if not isinstance(value, list) or len(value) > MAX_RELEASES:
        raise ManifestError("release index must be a bounded JSON array")
    return value


def _release_asset(value: Any, *, name: str, maximum: int) -> ReleaseAsset:
    if not isinstance(value, dict):
        raise ManifestError(f"GitHub release asset is malformed: {name}")
    if value.get("name") != name or value.get("state") != "uploaded":
        raise ManifestError(f"GitHub release asset is not completely uploaded: {name}")
    size = value.get("size")
    digest = value.get("digest")
    asset_id = value.get("id")
    if (
        not isinstance(asset_id, int)
        or isinstance(asset_id, bool)
        or asset_id < 1
        or not isinstance(size, int)
        or isinstance(size, bool)
        or not 1 <= size <= maximum
        or not isinstance(digest, str)
        or re.fullmatch(r"sha256:[0-9a-f]{64}", digest) is None
    ):
        raise ManifestError(f"GitHub release asset metadata is invalid: {name}")
    return ReleaseAsset(name=name, sha256=digest.removeprefix("sha256:"), size=size)


def published_releases(value: list[Any], *, repository: str) -> list[PublishedRelease]:
    validate_repository(repository)
    if len(value) > MAX_RELEASES:
        raise ManifestError("release index exceeds the stable release budget")
    releases: list[PublishedRelease] = []
    release_ids: set[int] = set()
    tags: set[str] = set()
    for entry in value:
        if not isinstance(entry, dict):
            raise ManifestError("GitHub release index contains a non-object entry")
        draft = entry.get("draft")
        prerelease = entry.get("prerelease")
        if not isinstance(draft, bool) or not isinstance(prerelease, bool):
            raise ManifestError("GitHub release draft/prerelease state is malformed")
        if draft or prerelease:
            continue
        tag = entry.get("tag_name")
        release_id = entry.get("id")
        if not isinstance(tag, str) or not is_stable_tag(tag):
            raise ManifestError("every published full release must use vMAJOR.MINOR.PATCH")
        if entry.get("immutable") is not True:
            raise ManifestError(f"published stable release is not immutable: {tag}")
        if (
            not isinstance(release_id, int)
            or isinstance(release_id, bool)
            or release_id < 1
            or release_id in release_ids
            or tag in tags
        ):
            raise ManifestError("published stable release has a duplicate or invalid identity")
        assets = entry.get("assets")
        if not isinstance(assets, list):
            raise ManifestError(f"published stable release has no asset inventory: {tag}")
        asset_ids: set[int] = set()
        asset_names: set[str] = set()
        for asset in assets:
            if not isinstance(asset, dict):
                raise ManifestError(f"published stable release has a malformed asset: {tag}")
            asset_id = asset.get("id")
            asset_name = asset.get("name")
            if (
                not isinstance(asset_id, int)
                or isinstance(asset_id, bool)
                or asset_id < 1
                or asset_id in asset_ids
                or not isinstance(asset_name, str)
                or SAFE_NAME_RE.fullmatch(asset_name) is None
                or asset_name in asset_names
                or asset.get("state") != "uploaded"
            ):
                raise ManifestError(
                    f"published stable release has an ambiguous asset inventory: {tag}"
                )
            asset_ids.add(asset_id)
            asset_names.add(asset_name)
        manifests = [asset for asset in assets if asset.get("name") == MANIFEST_NAME]
        bundles = [asset for asset in assets if asset.get("name") == BUNDLE_NAME]
        if len(manifests) != 1 or len(bundles) != 1:
            raise ManifestError(
                "published stable release must contain exactly one manifest and "
                f"signature bundle: {tag}"
            )
        manifest_asset = _release_asset(
            manifests[0], name=MANIFEST_NAME, maximum=MAX_MANIFEST_BYTES
        )
        bundle_asset = _release_asset(
            bundles[0], name=BUNDLE_NAME, maximum=MAX_SIGNATURE_BUNDLE_BYTES
        )
        if manifests[0].get("browser_download_url") != expected_download_url(
            repository, tag, MANIFEST_NAME
        ) or bundles[0].get("browser_download_url") != expected_download_url(
            repository, tag, BUNDLE_NAME
        ):
            raise ManifestError(f"published stable release asset URL is not canonical: {tag}")
        releases.append(
            PublishedRelease(
                release_id=release_id,
                tag=tag,
                manifest_asset=manifest_asset,
                bundle_asset=bundle_asset,
            )
        )
        release_ids.add(release_id)
        tags.add(tag)
    releases.sort(key=lambda release: tuple(int(part) for part in release.tag[1:].split(".")))
    return releases


def load_chain_material(
    releases: list[PublishedRelease], *, material_dir: Path
) -> list[ChainRelease]:
    root_mode = material_dir.lstat().st_mode
    if stat.S_ISLNK(root_mode) or not stat.S_ISDIR(root_mode):
        raise ManifestError("release material root must be a non-symbolic directory")
    records: list[ChainRelease] = []
    for release in releases:
        directory = material_dir / release.tag
        directory_mode = directory.lstat().st_mode
        if stat.S_ISLNK(directory_mode) or not stat.S_ISDIR(directory_mode):
            raise ManifestError(f"release material directory is unsafe: {release.tag}")
        manifest_path = directory / MANIFEST_NAME
        manifest, raw = load_canonical_manifest(manifest_path)
        if (
            len(raw) != release.manifest_asset.size
            or hashlib.sha256(raw).hexdigest() != release.manifest_asset.sha256
        ):
            raise ManifestError(
                f"downloaded manifest differs from GitHub asset metadata: {release.tag}"
            )
        bundle = read_regular_bytes(
            directory / BUNDLE_NAME,
            description="manifest signature bundle",
            minimum=1,
            maximum=MAX_SIGNATURE_BUNDLE_BYTES,
        )
        if (
            len(bundle) != release.bundle_asset.size
            or hashlib.sha256(bundle).hexdigest() != release.bundle_asset.sha256
        ):
            raise ManifestError(
                "downloaded signature bundle differs from GitHub asset metadata: "
                f"{release.tag}"
            )
        commit_raw = read_regular_bytes(
            directory / COMMIT_NAME,
            description="protected tag commit",
            minimum=41,
            maximum=41,
        )
        try:
            commit_text = commit_raw.decode("ascii")
        except UnicodeDecodeError as error:
            raise ManifestError(f"protected tag commit is not ASCII: {release.tag}") from error
        if not commit_text.endswith("\n") or COMMIT_RE.fullmatch(commit_text[:-1]) is None:
            raise ManifestError(f"protected tag commit is invalid: {release.tag}")
        records.append(
            ChainRelease(
                release_id=release.release_id,
                tag=release.tag,
                commit=commit_text[:-1],
                manifest=manifest,
                raw_manifest=raw,
            )
        )
    return records


def verify_release_chain(
    releases: list[ChainRelease],
    *,
    repository: str,
    signing_key_id: str,
    expected_current_version: str | None = None,
) -> ChainHead:
    validate_repository(repository)
    if KEY_ID_RE.fullmatch(signing_key_id) is None:
        raise ManifestError("expected signing key identifier is invalid")
    if len(releases) > MAX_RELEASES:
        raise ManifestError("stable release chain exceeds the release budget")
    by_sequence: dict[int, ChainRelease] = {}
    release_ids: set[int] = set()
    for release in releases:
        if release.release_id < 1 or release.release_id in release_ids:
            raise ManifestError("stable release chain contains a duplicate release identity")
        release_ids.add(release.release_id)
        if not 2 <= len(release.raw_manifest) <= MAX_MANIFEST_BYTES:
            raise ManifestError("stable release chain manifest exceeds its byte budget")
        validate_manifest_shape(release.manifest)
        if canonical_bytes(release.manifest) != release.raw_manifest:
            raise ManifestError("stable release chain contains noncanonical manifest bytes")
        require_manifest_identity(
            release.manifest,
            repository=repository,
            tag=release.tag,
            commit=release.commit,
            signing_key_id=signing_key_id,
        )
        sequence = release.manifest["rollback"]["sequence"]
        if sequence in by_sequence:
            raise ManifestError("stable release chain contains a duplicate sequence or fork")
        by_sequence[sequence] = release

    actual_sequences = sorted(by_sequence)
    expected_sequences = list(range(1, len(releases) + 1))
    if actual_sequences != expected_sequences:
        raise ManifestError("stable release chain contains a missing or non-genesis sequence")
    ordered = [by_sequence[sequence] for sequence in actual_sequences]
    for previous, current in zip(ordered, ordered[1:]):
        rollback = current.manifest["rollback"]
        previous_sequence = previous.manifest["rollback"]["sequence"]
        if rollback["previous_sequence"] != previous_sequence:
            raise ManifestError("stable release chain has a missing predecessor")
        expected_digest = hashlib.sha256(previous.raw_manifest).hexdigest()
        if rollback["previous_manifest_sha256"] != expected_digest:
            raise ManifestError("stable release chain predecessor digest does not match")
        if not semver_is_greater(current.manifest["version"], previous.manifest["version"]):
            raise ManifestError("stable release versions do not increase with sequence")

    if expected_current_version is not None:
        if not is_stable_version(expected_current_version):
            raise ManifestError("candidate release version is not stable SemVer")
        if ordered and not semver_is_greater(
            expected_current_version, ordered[-1].manifest["version"]
        ):
            raise ManifestError("candidate release version does not advance the chain head")
    head = ordered[-1] if ordered else None
    return ChainHead(release=head, next_sequence=len(ordered) + 1)


def command_generate(args: argparse.Namespace) -> None:
    manifest = build_manifest(args)
    write_new_file(args.output, canonical_bytes(manifest))


def command_verify(args: argparse.Namespace) -> None:
    manifest, raw = load_canonical_manifest(args.manifest)
    require_manifest_identity(
        manifest,
        repository=args.expected_repository,
        tag=args.expected_tag,
        commit=args.expected_commit,
        signing_key_id=args.expected_signing_key_id,
    )
    if (
        args.expected_sequence is not None
        and manifest["rollback"]["sequence"] != args.expected_sequence
    ):
        raise ManifestError("manifest sequence does not match the expected release sequence")
    if args.artifact_dir is not None:
        directory_mode = args.artifact_dir.lstat().st_mode
        if stat.S_ISLNK(directory_mode) or not stat.S_ISDIR(directory_mode):
            raise ManifestError("artifact verification root must be a non-symbolic directory")
        expected_names = {artifact["name"] for artifact in manifest["artifacts"]}
        actual_names: set[str] = set()
        for path in args.artifact_dir.iterdir():
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode):
                raise ManifestError(f"artifact verification root contains a symlink: {path.name}")
            if not stat.S_ISREG(mode):
                continue
            try:
                classify_artifact(path.name)
            except ManifestError:
                continue
            actual_names.add(path.name)
        if actual_names != expected_names:
            raise ManifestError("publishable artifact set differs from the signed manifest")
        for artifact in manifest["artifacts"]:
            path = args.artifact_dir / artifact["name"]
            digest, size = hash_regular_file(path, maximum=MAX_ARTIFACT_BYTES)
            if size != artifact["size"] or digest != artifact["sha256"]:
                raise ManifestError(f"artifact bytes differ from manifest: {path.name}")
    print(hashlib.sha256(raw).hexdigest())


def command_release_tags(args: argparse.Namespace) -> None:
    releases = published_releases(
        load_release_index(args.releases_json), repository=args.repository
    )
    for release in releases:
        print(release.tag)


def command_verify_chain(args: argparse.Namespace) -> None:
    releases = published_releases(
        load_release_index(args.releases_json), repository=args.repository
    )
    records = load_chain_material(releases, material_dir=args.material_dir)
    expected_version = args.expected_current_tag.removeprefix("v")
    if args.expected_current_tag != f"v{expected_version}" or not is_stable_version(
        expected_version
    ):
        raise ManifestError("candidate release tag must be exactly vMAJOR.MINOR.PATCH")
    head = verify_release_chain(
        records,
        repository=args.repository,
        signing_key_id=args.signing_key_id,
        expected_current_version=expected_version,
    )
    if head.next_sequence != args.expected_next_sequence:
        raise ManifestError(
            "confirmed release sequence does not equal the uniquely derived next sequence"
        )
    head_release = head.release
    result = {
        "head_release_id": (
            head_release.release_id if head_release is not None else None
        ),
        "head_manifest": (
            str(args.material_dir / head_release.tag / MANIFEST_NAME)
            if head_release is not None
            else None
        ),
        "head_manifest_sha256": (
            hashlib.sha256(head_release.raw_manifest).hexdigest()
            if head_release is not None
            else None
        ),
        "head_sequence": (
            head_release.manifest["rollback"]["sequence"]
            if head_release is not None
            else None
        ),
        "head_tag": head_release.tag if head_release is not None else None,
        "next_sequence": head.next_sequence,
    }
    write_new_file(args.output, canonical_bytes(result))


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)

    generate = commands.add_parser("generate", help="create a canonical update manifest")
    generate.add_argument("--artifact-dir", type=Path, required=True)
    generate.add_argument("--output", type=Path, required=True)
    generate.add_argument("--repository", required=True)
    generate.add_argument("--tag", required=True)
    generate.add_argument("--commit", required=True)
    generate.add_argument("--sequence", type=int, required=True)
    generate.add_argument("--published-at", required=True)
    generate.add_argument("--signing-key-id", required=True)
    generate.add_argument("--previous", type=Path)
    generate.set_defaults(handler=command_generate)

    verify = commands.add_parser("verify", help="verify canonical encoding and artifact bytes")
    verify.add_argument("--manifest", type=Path, required=True)
    verify.add_argument("--artifact-dir", type=Path)
    verify.add_argument("--expected-sequence", type=int)
    verify.add_argument("--expected-repository")
    verify.add_argument("--expected-tag")
    verify.add_argument("--expected-commit")
    verify.add_argument("--expected-signing-key-id")
    verify.set_defaults(handler=command_verify)

    release_tags = commands.add_parser(
        "release-tags", help="validate GitHub releases and print stable tags"
    )
    release_tags.add_argument("--releases-json", type=Path, required=True)
    release_tags.add_argument("--repository", required=True)
    release_tags.set_defaults(handler=command_release_tags)

    verify_chain = commands.add_parser(
        "verify-chain", help="verify the complete immutable stable release chain"
    )
    verify_chain.add_argument("--releases-json", type=Path, required=True)
    verify_chain.add_argument("--material-dir", type=Path, required=True)
    verify_chain.add_argument("--repository", required=True)
    verify_chain.add_argument("--signing-key-id", required=True)
    verify_chain.add_argument("--expected-current-tag", required=True)
    verify_chain.add_argument("--expected-next-sequence", type=int, required=True)
    verify_chain.add_argument("--output", type=Path, required=True)
    verify_chain.set_defaults(handler=command_verify_chain)
    return root


def main() -> int:
    try:
        args = parser().parse_args()
        args.handler(args)
    except (ManifestError, OSError) as error:
        print(f"release manifest error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
