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
from pathlib import Path
from typing import Any


EXPECTED_SYFT_VERSION = "1.44.0"
MAX_SBOM_BYTES = 16 * 1024 * 1024
MAX_PAYLOAD_FILES = 50_000
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
SAFE_SUBJECT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,199}$")


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


def regular_file_bytes(path: Path, description: str, maximum: int) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise SbomError(f"cannot open {description} without following links: {path}") from error
    try:
        before = os.fstat(descriptor)
        linked = path.lstat()
        if (
            not stat.S_ISREG(before.st_mode)
            or stat.S_ISLNK(linked.st_mode)
            or (before.st_dev, before.st_ino) != (linked.st_dev, linked.st_ino)
        ):
            raise SbomError(f"{description} must be a non-symbolic regular file: {path}")
        if before.st_size < 2 or before.st_size > maximum:
            raise SbomError(f"{description} is empty or exceeds the attestation byte limit")
        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = -1
            encoded = source.read(maximum + 1)
            after = os.fstat(source.fileno())
        if len(encoded) > maximum or (
            before.st_dev,
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ) or len(encoded) != before.st_size:
            raise SbomError(f"{description} changed while it was read")
        return encoded
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
    file_components, payload_digest = payload_components(args.scan_root)
    if any(component["bom-ref"] in existing_refs for component in file_components):
        raise SbomError("payload file inventory collides with a Syft component reference")

    properties = metadata.setdefault("properties", [])
    if not isinstance(properties, list):
        raise SbomError("SBOM metadata properties must be an array")
    if any(
        not isinstance(item, dict)
        or not isinstance(item.get("name"), str)
        or not isinstance(item.get("value"), str)
        for item in properties
    ):
        raise SbomError("SBOM metadata properties must be string name/value objects")
    if any(
        isinstance(item, dict)
        and isinstance(item.get("name"), str)
        and item["name"].startswith("zephium:release:")
        for item in properties
    ):
        raise SbomError("SBOM was already bound to a Zephium release")
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
    validate_syft_document(verified, args.source_name, args.version)
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
