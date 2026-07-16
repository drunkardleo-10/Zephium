#!/usr/bin/env python3

from __future__ import annotations

import base64
import hashlib
import hmac
import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import seal_symbols


class SealSymbolsTests(unittest.TestCase):
    def keys(self) -> dict[str, str]:
        return {
            "SYMBOL_ENCRYPTION_KEY": base64.b64encode(os.urandom(32)).decode(),
            "SYMBOL_AUTHENTICATION_KEY": base64.b64encode(os.urandom(32)).decode(),
        }

    def test_round_trip_and_tamper_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, self.keys()
        ):
            root = Path(temporary)
            source = root / "symbols.tar"
            sealed = root / "symbols.tar.enc"
            tag = root / "symbols.tar.enc.hmac-sha256"
            recovered = root / "recovered.tar"
            source.write_bytes(os.urandom(8193))
            seal_symbols.seal(source, sealed, tag)
            self.assertEqual(stat.S_IMODE(sealed.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(tag.stat().st_mode), 0o600)
            seal_symbols.unseal(sealed, tag, recovered)
            self.assertEqual(source.read_bytes(), recovered.read_bytes())
            self.assertEqual(stat.S_IMODE(recovered.stat().st_mode), 0o600)

            with sealed.open("ab") as destination:
                destination.write(b"tamper")
            tampered_output = root / "tampered.tar"
            with self.assertRaises(seal_symbols.SealError):
                seal_symbols.unseal(sealed, tag, tampered_output)
            self.assertFalse(tampered_output.exists())

    def test_keys_must_be_independent_random_bytes(self) -> None:
        encoded = base64.b64encode(os.urandom(32)).decode()
        environment = {
            "SYMBOL_ENCRYPTION_KEY": encoded,
            "SYMBOL_AUTHENTICATION_KEY": encoded,
        }
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, environment
        ):
            root = Path(temporary)
            source = root / "symbols.tar"
            source.write_bytes(b"symbols")
            with self.assertRaises(seal_symbols.SealError):
                seal_symbols.seal(source, root / "sealed", root / "tag")

    def test_failed_seal_removes_every_partial_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, self.keys()
        ):
            root = Path(temporary)
            source = root / "symbols.tar"
            sealed = root / "symbols.tar.enc"
            tag = root / "symbols.tar.enc.hmac-sha256"
            source.write_bytes(b"symbols")

            def fail_verification(
                arguments: list[str], _password: str, source_file, destination_file
            ) -> None:
                if "-d" in arguments:
                    raise seal_symbols.SealError("injected verification failure")
                self.assertEqual(source_file.read(), b"symbols")
                destination_file.write(b"x" * 32)
                destination_file.flush()

            with mock.patch.object(
                seal_symbols, "run_openssl", side_effect=fail_verification
            ):
                with self.assertRaises(seal_symbols.SealError):
                    seal_symbols.seal(source, sealed, tag)

            self.assertFalse(sealed.exists())
            self.assertFalse(tag.exists())

    def test_openssl_consumes_an_immutable_private_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, self.keys()
        ):
            root = Path(temporary)
            source = root / "symbols.tar"
            sealed = root / "symbols.tar.enc"
            tag = root / "symbols.tar.enc.hmac-sha256"
            original = b"the exact symbol archive bytes"
            source.write_bytes(original)
            encrypted_prefix = b"E" * 32
            encrypted_inputs: list[bytes] = []

            def reversible_openssl(
                arguments: list[str], _password: str, source_file, destination_file
            ) -> None:
                source_mode = stat.S_IMODE(os.fstat(source_file.fileno()).st_mode)
                destination_mode = stat.S_IMODE(
                    os.fstat(destination_file.fileno()).st_mode
                )
                self.assertEqual(source_mode, 0o400)
                self.assertEqual(destination_mode, 0o600)
                data = source_file.read()
                if "-d" in arguments:
                    destination_file.write(data[len(encrypted_prefix) :])
                else:
                    # Mutating the caller-owned path after snapshot creation must
                    # not alter the bytes handed to OpenSSL or authenticated.
                    source.write_bytes(b"changed after immutable snapshot")
                    encrypted_inputs.append(data)
                    destination_file.write(encrypted_prefix + data)
                destination_file.flush()

            with mock.patch.object(
                seal_symbols, "run_openssl", side_effect=reversible_openssl
            ):
                seal_symbols.seal(source, sealed, tag)

            self.assertEqual(encrypted_inputs, [original])
            self.assertEqual(sealed.read_bytes(), encrypted_prefix + original)
            self.assertEqual(source.read_bytes(), b"changed after immutable snapshot")

    def test_unseal_authenticates_and_decrypts_the_same_snapshot(self) -> None:
        keys = self.keys()
        authentication_key = base64.b64decode(keys["SYMBOL_AUTHENTICATION_KEY"])
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, keys
        ):
            root = Path(temporary)
            source = root / "symbols.tar.enc"
            tag = root / "symbols.tar.enc.hmac-sha256"
            output = root / "symbols.tar"
            encrypted = b"E" * 32 + b"authenticated payload"
            source.write_bytes(encrypted)
            tag.write_text(
                hmac.new(authentication_key, encrypted, hashlib.sha256).hexdigest()
                + "\n",
                encoding="ascii",
            )

            def decrypt_snapshot(
                arguments: list[str], _password: str, source_file, destination_file
            ) -> None:
                self.assertIn("-d", arguments)
                source.write_bytes(b"changed after authentication")
                destination_file.write(source_file.read()[32:])
                destination_file.flush()

            with mock.patch.object(
                seal_symbols, "run_openssl", side_effect=decrypt_snapshot
            ):
                seal_symbols.unseal(source, tag, output)

            self.assertEqual(output.read_bytes(), b"authenticated payload")

    def test_openssl_receives_only_the_required_environment_and_descriptors(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ,
            {
                "PATH": "/trusted/bin",
                "CI_JOB_TOKEN": "must-not-be-inherited",
                "SYMBOL_AUTHENTICATION_KEY": "must-not-be-inherited",
            },
        ), mock.patch.object(
            seal_symbols.shutil, "which", return_value="/trusted/bin/openssl"
        ):
            root = Path(temporary)
            source_path = root / "source"
            destination_path = root / "destination"
            source_path.write_bytes(b"input")
            captured: dict = {}

            def fake_run(command, **kwargs):
                captured["command"] = command
                captured.update(kwargs)
                kwargs["stdout"].write(b"output")

            with source_path.open("r+b") as source_file, destination_path.open(
                "w+b"
            ) as destination_file, mock.patch.object(
                seal_symbols.subprocess, "run", side_effect=fake_run
            ):
                seal_symbols.run_openssl(
                    ["enc", "-aes-256-cbc"],
                    "password",
                    source_file,
                    destination_file,
                )

            self.assertEqual(
                captured["command"],
                ["/trusted/bin/openssl", "enc", "-aes-256-cbc"],
            )
            self.assertEqual(
                captured["env"],
                {
                    "LANG": "C",
                    "LC_ALL": "C",
                    "PATH": "/trusted/bin",
                    "ZEPHIUM_SYMBOL_PASSWORD": "password",
                },
            )
            self.assertTrue(captured["check"])
            self.assertTrue(captured["close_fds"])
            self.assertNotIn("-in", captured["command"])
            self.assertNotIn("-out", captured["command"])
            self.assertEqual(destination_path.read_bytes(), b"output")

    def test_second_publication_failure_removes_the_first_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, self.keys()
        ):
            root = Path(temporary)
            source = root / "symbols.tar"
            sealed = root / "symbols.tar.enc"
            tag = root / "symbols.tar.enc.hmac-sha256"
            source.write_bytes(b"symbols")
            prefix = b"E" * 32

            def reversible_openssl(
                arguments: list[str], _password: str, source_file, destination_file
            ) -> None:
                data = source_file.read()
                destination_file.write(
                    data[len(prefix) :] if "-d" in arguments else prefix + data
                )
                destination_file.flush()

            real_publish = seal_symbols._publish_exclusive
            publications = 0

            def fail_second_publication(
                source_path: Path, destination: Path
            ) -> tuple[int, int]:
                nonlocal publications
                publications += 1
                if publications == 2:
                    raise seal_symbols.SealError("injected tag publication failure")
                return real_publish(source_path, destination)

            with mock.patch.object(
                seal_symbols, "run_openssl", side_effect=reversible_openssl
            ), mock.patch.object(
                seal_symbols,
                "_publish_exclusive",
                side_effect=fail_second_publication,
            ):
                with self.assertRaises(seal_symbols.SealError):
                    seal_symbols.seal(source, sealed, tag)

            self.assertFalse(sealed.exists())
            self.assertFalse(tag.exists())

    def test_dangling_output_symlink_is_never_followed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, self.keys()
        ):
            root = Path(temporary)
            source = root / "symbols.tar"
            source.write_bytes(b"symbols")
            sealed = root / "symbols.tar.enc"
            sealed.symlink_to(root / "missing-target")
            with self.assertRaises(seal_symbols.SealError):
                seal_symbols.seal(
                    source, sealed, root / "symbols.tar.enc.hmac-sha256"
                )
            self.assertTrue(sealed.is_symlink())
            self.assertFalse((root / "missing-target").exists())


if __name__ == "__main__":
    unittest.main()
