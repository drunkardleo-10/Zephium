#!/usr/bin/env python3
"""Seal a private symbol archive before storing it as an Actions artifact."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import hmac
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import BinaryIO


ITERATIONS = 600_000
COPY_CHUNK = 1024 * 1024


class SealError(RuntimeError):
    pass


def secret(name: str) -> tuple[str, bytes]:
    encoded = os.environ.get(name, "").strip()
    try:
        decoded = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error) as error:
        raise SealError(f"{name} must be strict base64") from error
    if len(decoded) != 32:
        raise SealError(f"{name} must encode exactly 32 random bytes")
    return encoded, decoded


def sha256(path: Path) -> bytes:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(COPY_CHUNK), b""):
            digest.update(chunk)
    return digest.digest()


def hmac_sha256(path: Path, key: bytes) -> str:
    digest = hmac.new(key, digestmod=hashlib.sha256)
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(COPY_CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _openssl_environment(encryption_password: str) -> tuple[str, dict[str, str]]:
    path = os.environ.get("PATH", os.defpath)
    executable = shutil.which("openssl", path=path)
    if executable is None:
        raise SealError("OpenSSL is not available on PATH")
    environment = {
        "LANG": "C",
        "LC_ALL": "C",
        "PATH": path,
        "ZEPHIUM_SYMBOL_PASSWORD": encryption_password,
    }
    # CreateProcess needs the Windows directory for system DLL resolution.
    # Do not copy the rest of the parent CI environment or unrelated secrets.
    if os.name == "nt":
        for name in ("SYSTEMROOT", "WINDIR"):
            if value := os.environ.get(name):
                environment[name] = value
    return executable, environment


def run_openssl(
    arguments: list[str],
    encryption_password: str,
    source: BinaryIO,
    destination: BinaryIO,
) -> None:
    executable, environment = _openssl_environment(encryption_password)
    source.seek(0)
    destination.seek(0)
    destination.truncate(0)
    try:
        subprocess.run(
            [executable, *arguments],
            check=True,
            close_fds=True,
            env=environment,
            stdin=source,
            stdout=destination,
        )
        destination.flush()
        os.fsync(destination.fileno())
        destination.seek(0)
    except (OSError, subprocess.CalledProcessError) as error:
        raise SealError(f"OpenSSL symbol-archive operation failed: {error}") from error


def _set_descriptor_mode(descriptor: int, mode: int) -> None:
    # Windows exposes confidentiality through the containing directory's DACL;
    # its Python stat/chmod API only models the DOS read-only bit. POSIX hosts
    # additionally get exact owner-only modes on every temporary and output.
    fchmod = getattr(os, "fchmod", None)
    if fchmod is not None:
        fchmod(descriptor, mode)


def _open_private(path: Path) -> BinaryIO:
    flags = os.O_RDWR | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(path, flags, 0o600)
    _set_descriptor_mode(descriptor, 0o600)
    return os.fdopen(descriptor, "w+b")


def _source_identity(value: os.stat_result) -> tuple[int, int, int, int, int]:
    return (
        value.st_dev,
        value.st_ino,
        value.st_size,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def _snapshot_regular_file(
    source_path: Path,
    snapshot: BinaryIO,
    description: str,
    minimum_size: int,
    maximum_size: int | None = None,
) -> tuple[bytes, int]:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(source_path, flags)
    except OSError as error:
        raise SealError(f"cannot open {description} without following links: {error}") from error

    digest = hashlib.sha256()
    copied = 0
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise SealError(f"{description} must be a non-symbolic regular file")
        if before.st_size < minimum_size or (
            maximum_size is not None and before.st_size > maximum_size
        ):
            raise SealError(f"{description} has an invalid byte length")
        # On platforms without O_NOFOLLOW, prove that the opened descriptor is
        # still the exact non-link directory entry inspected after open.
        linked = source_path.lstat()
        if stat.S_ISLNK(linked.st_mode) or (
            linked.st_dev,
            linked.st_ino,
        ) != (before.st_dev, before.st_ino):
            raise SealError(f"{description} changed while it was opened")

        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = -1
            for chunk in iter(lambda: source.read(COPY_CHUNK), b""):
                copied += len(chunk)
                digest.update(chunk)
                snapshot.write(chunk)
            after = os.fstat(source.fileno())
        if _source_identity(before) != _source_identity(after) or copied != before.st_size:
            raise SealError(f"{description} changed while its immutable snapshot was created")
    finally:
        if descriptor >= 0:
            os.close(descriptor)

    snapshot.flush()
    os.fsync(snapshot.fileno())
    _set_descriptor_mode(snapshot.fileno(), 0o400)
    snapshot.seek(0)
    return digest.digest(), copied


def _private_workspace(parent: Path) -> tempfile.TemporaryDirectory[str]:
    if parent.is_symlink() or not parent.is_dir():
        raise SealError(f"output parent must be a non-symbolic directory: {parent}")
    workspace = tempfile.TemporaryDirectory(
        dir=parent, prefix=".zephium-symbols-"
    )
    os.chmod(workspace.name, 0o700)
    return workspace


def _require_output_parent(parent: Path) -> None:
    if parent.is_symlink() or not parent.is_dir():
        raise SealError(f"output parent must be a non-symbolic directory: {parent}")


def _exists_without_following(path: Path) -> bool:
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    return True


def _unlink_if_identity(path: Path, identity: tuple[int, int]) -> None:
    try:
        value = path.lstat()
    except FileNotFoundError:
        return
    if not stat.S_ISLNK(value.st_mode) and (value.st_dev, value.st_ino) == identity:
        path.unlink()


def _publish_exclusive(source: Path, destination: Path) -> tuple[int, int]:
    if _exists_without_following(destination):
        raise SealError(f"refusing to replace existing output: {destination}")
    descriptor, temporary_name = tempfile.mkstemp(
        dir=destination.parent,
        prefix=f".{destination.name}.",
        suffix=".tmp",
    )
    temporary = Path(temporary_name)
    published_identity: tuple[int, int] | None = None
    try:
        _set_descriptor_mode(descriptor, 0o600)
        with source.open("rb") as input_file, os.fdopen(
            descriptor, "wb", closefd=True
        ) as output_file:
            descriptor = -1
            shutil.copyfileobj(input_file, output_file, COPY_CHUNK)
            output_file.flush()
            os.fsync(output_file.fileno())
            if not stat.S_ISREG(os.fstat(output_file.fileno()).st_mode):
                raise SealError("private publication temporary is not a regular file")
        # A hard link is an atomic O_EXCL-style publication of the exact
        # verified inode and never follows an attacker-created destination.
        os.link(temporary, destination, follow_symlinks=False)
        expected = temporary.lstat()
        published = destination.lstat()
        published_identity = (expected.st_dev, expected.st_ino)
        if stat.S_ISLNK(published.st_mode) or not stat.S_ISREG(published.st_mode):
            raise SealError("published symbol output is not a regular file")
        if (published.st_dev, published.st_ino) != published_identity:
            raise SealError("published symbol output changed during publication")
        if os.name != "nt" and stat.S_IMODE(published.st_mode) != 0o600:
            raise SealError("published symbol output permissions are not private")
        return published_identity
    except BaseException:
        if published_identity is not None:
            _unlink_if_identity(destination, published_identity)
        raise
    finally:
        try:
            if descriptor >= 0:
                os.close(descriptor)
            temporary.unlink(missing_ok=True)
        except BaseException:
            if published_identity is not None:
                _unlink_if_identity(destination, published_identity)
            raise


def _openssl_arguments(decrypt: bool = False) -> list[str]:
    arguments = ["enc"]
    if decrypt:
        arguments.append("-d")
    arguments.extend(
        [
            "-aes-256-cbc",
            "-md",
            "sha256",
            "-pbkdf2",
            "-iter",
            str(ITERATIONS),
            "-salt",
            "-pass",
            "env:ZEPHIUM_SYMBOL_PASSWORD",
        ]
    )
    return arguments


def seal(source: Path, output: Path, mac_path: Path) -> None:
    if _exists_without_following(output) or _exists_without_following(mac_path):
        raise SealError("refusing to replace an existing sealed archive")
    encryption_password, encryption_key = secret("SYMBOL_ENCRYPTION_KEY")
    _, authentication_key = secret("SYMBOL_AUTHENTICATION_KEY")
    if hmac.compare_digest(encryption_key, authentication_key):
        raise SealError("symbol encryption and authentication keys must be independent")

    _require_output_parent(output.parent)
    _require_output_parent(mac_path.parent)
    published: list[tuple[Path, tuple[int, int]]] = []
    try:
        with _private_workspace(output.parent) as temporary_name:
            temporary = Path(temporary_name)
            source_snapshot = temporary / "source.snapshot"
            encrypted_snapshot = temporary / "symbols.enc"
            recovered_snapshot = temporary / "roundtrip.snapshot"
            tag_snapshot = temporary / "symbols.hmac-sha256"

            with _open_private(source_snapshot) as snapshot:
                source_digest, _ = _snapshot_regular_file(
                    source, snapshot, "symbol archive", 1
                )
            with source_snapshot.open("rb") as snapshot, _open_private(
                encrypted_snapshot
            ) as encrypted:
                run_openssl(
                    _openssl_arguments(), encryption_password, snapshot, encrypted
                )
                if os.fstat(encrypted.fileno()).st_size < 32:
                    raise SealError("OpenSSL emitted an invalid sealed archive")
                _set_descriptor_mode(encrypted.fileno(), 0o400)

            tag = hmac_sha256(encrypted_snapshot, authentication_key)
            with _open_private(tag_snapshot) as tag_file:
                tag_file.write((tag + "\n").encode("ascii"))
                tag_file.flush()
                os.fsync(tag_file.fileno())
                _set_descriptor_mode(tag_file.fileno(), 0o400)

            with encrypted_snapshot.open("rb") as encrypted, _open_private(
                recovered_snapshot
            ) as recovered:
                run_openssl(
                    _openssl_arguments(decrypt=True),
                    encryption_password,
                    encrypted,
                    recovered,
                )
                _set_descriptor_mode(recovered.fileno(), 0o400)
            if not hmac.compare_digest(source_digest, sha256(recovered_snapshot)):
                raise SealError("sealed symbol archive did not round-trip exactly")

            output_identity = _publish_exclusive(encrypted_snapshot, output)
            published.append((output, output_identity))
            tag_identity = _publish_exclusive(tag_snapshot, mac_path)
            published.append((mac_path, tag_identity))
    except BaseException:
        for path, identity in reversed(published):
            _unlink_if_identity(path, identity)
        raise


def unseal(source: Path, mac_path: Path, output: Path) -> None:
    if _exists_without_following(output):
        raise SealError("refusing to replace an existing recovered archive")
    encryption_password, encryption_key = secret("SYMBOL_ENCRYPTION_KEY")
    _, authentication_key = secret("SYMBOL_AUTHENTICATION_KEY")
    if hmac.compare_digest(encryption_key, authentication_key):
        raise SealError("symbol encryption and authentication keys must be independent")

    _require_output_parent(output.parent)
    published_identity: tuple[int, int] | None = None
    try:
        with _private_workspace(output.parent) as temporary_name:
            temporary = Path(temporary_name)
            encrypted_snapshot = temporary / "symbols.enc"
            tag_snapshot = temporary / "symbols.hmac-sha256"
            recovered_snapshot = temporary / "symbols.recovered"

            with _open_private(encrypted_snapshot) as snapshot:
                _snapshot_regular_file(source, snapshot, "sealed symbol archive", 32)
            with _open_private(tag_snapshot) as snapshot:
                _snapshot_regular_file(
                    mac_path,
                    snapshot,
                    "sealed symbol archive HMAC",
                    65,
                    65,
                )
            try:
                supplied_tag = tag_snapshot.read_text(encoding="ascii")
            except UnicodeError as error:
                raise SealError("sealed symbol archive HMAC is not ASCII") from error
            expected_tag = hmac_sha256(encrypted_snapshot, authentication_key) + "\n"
            if not hmac.compare_digest(supplied_tag, expected_tag):
                raise SealError("sealed symbol archive authentication failed")

            with encrypted_snapshot.open("rb") as encrypted, _open_private(
                recovered_snapshot
            ) as recovered:
                run_openssl(
                    _openssl_arguments(decrypt=True),
                    encryption_password,
                    encrypted,
                    recovered,
                )
                if os.fstat(recovered.fileno()).st_size < 1:
                    raise SealError("recovered symbol archive is empty")
                _set_descriptor_mode(recovered.fileno(), 0o400)

            published_identity = _publish_exclusive(recovered_snapshot, output)
    except BaseException:
        if published_identity is not None:
            _unlink_if_identity(output, published_identity)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    seal_parser = commands.add_parser("seal")
    seal_parser.add_argument("source", type=Path)
    seal_parser.add_argument("output", type=Path)
    seal_parser.add_argument("mac", type=Path)
    unseal_parser = commands.add_parser("unseal")
    unseal_parser.add_argument("source", type=Path)
    unseal_parser.add_argument("mac", type=Path)
    unseal_parser.add_argument("output", type=Path)
    arguments = parser.parse_args()
    try:
        if arguments.command == "seal":
            seal(arguments.source, arguments.output, arguments.mac)
        else:
            unseal(arguments.source, arguments.mac, arguments.output)
    except (OSError, SealError) as error:
        print(f"symbol sealing error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
