#!/usr/bin/env python3
"""Fail closed unless the production release environment is narrowly protected.

GitHub creates an environment implicitly when a workflow first references a
missing name.  Merely writing ``environment: production-release`` therefore
does not prove that publisher credentials are reviewer-gated or scoped to the
intended branch.  This helper validates the read-only API snapshots fetched by
the release workflow before any artifact job is admitted.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import stat
import sys
from collections.abc import Mapping, Sequence
from typing import Any


MAX_POLICY_BYTES = 2 * 1024 * 1024
NAME_RE = re.compile(r"^[A-Z][A-Z0-9_]{0,254}$")

REQUIRED_SECRETS = frozenset(
    {
        "APPLE_CERTIFICATE",
        "APPLE_CERTIFICATE_PASSWORD",
        "APPLE_ID",
        "APPLE_PASSWORD",
        "RELEASE_POLICY_TOKEN",
        "RPM_SIGNING_KEY_PASSPHRASE",
        "RPM_SIGNING_PRIVATE_KEY",
        "SYMBOL_AUTHENTICATION_KEY",
        "SYMBOL_ENCRYPTION_KEY",
        "UPDATE_SIGNING_KEY_PASSWORD",
        "UPDATE_SIGNING_PRIVATE_KEY",
        "WINDOWS_CERTIFICATE_BASE64",
        "WINDOWS_CERTIFICATE_PASSWORD",
    }
)

REQUIRED_VARIABLES = frozenset(
    {
        "APPLE_SIGNING_IDENTITY",
        "APPLE_TEAM_ID",
        "RPM_SIGNING_KEY_FINGERPRINT",
        "UPDATE_SIGNING_KEY_ID",
        "UPDATE_SIGNING_PUBLIC_KEY",
        "WINDOWS_CERTIFICATE_SHA256",
        "WINDOWS_CERTIFICATE_SUBJECT",
        "WINDOWS_TIMESTAMP_URL",
    }
)


class PolicyError(ValueError):
    """The supplied GitHub policy snapshot is not release-safe."""


def _object(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise PolicyError(f"{label} must be a JSON object")
    return value


def _array(value: Any, label: str) -> Sequence[Any]:
    if not isinstance(value, list):
        raise PolicyError(f"{label} must be a JSON array")
    return value


def load_bounded_json(path: pathlib.Path) -> Mapping[str, Any]:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise PolicyError(f"cannot inspect {path}: {error}") from error
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise PolicyError(f"policy input is not a regular file: {path}")
    if metadata.st_size <= 0 or metadata.st_size > MAX_POLICY_BYTES:
        raise PolicyError(
            f"policy input size is outside 1..{MAX_POLICY_BYTES} bytes: {path}"
        )
    try:
        raw = path.read_bytes()
        document = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PolicyError(f"cannot decode policy input {path}: {error}") from error
    return _object(document, str(path))


def _named_inventory(
    document: Mapping[str, Any],
    *,
    collection_key: str,
    label: str,
) -> frozenset[str]:
    total = document.get("total_count")
    if isinstance(total, bool) or not isinstance(total, int) or total < 0:
        raise PolicyError(f"{label}.total_count must be a non-negative integer")
    entries = _array(document.get(collection_key), f"{label}.{collection_key}")
    if total != len(entries):
        raise PolicyError(
            f"{label} response is incomplete: total_count={total}, entries={len(entries)}"
        )

    names: list[str] = []
    for index, entry in enumerate(entries):
        item = _object(entry, f"{label}.{collection_key}[{index}]")
        name = item.get("name")
        if not isinstance(name, str) or not NAME_RE.fullmatch(name):
            raise PolicyError(f"{label} contains an invalid credential name")
        names.append(name)
    if len(names) != len(set(names)):
        raise PolicyError(f"{label} contains duplicate credential names")
    return frozenset(names)


def _require_inventory(
    document: Mapping[str, Any],
    *,
    collection_key: str,
    required: frozenset[str],
    label: str,
) -> None:
    actual = _named_inventory(document, collection_key=collection_key, label=label)
    missing = sorted(required - actual)
    if missing:
        raise PolicyError(f"{label} is missing: {', '.join(missing)}")


def validate_policy(
    environment: Mapping[str, Any],
    branch_policies: Mapping[str, Any],
    secrets: Mapping[str, Any],
    variables: Mapping[str, Any],
    repository_secrets: Mapping[str, Any],
    repository_variables: Mapping[str, Any],
    organization_secrets: Mapping[str, Any],
    organization_variables: Mapping[str, Any],
    *,
    expected_environment: str,
    expected_branch: str,
) -> None:
    if environment.get("name") != expected_environment:
        raise PolicyError("GitHub returned a different release environment")

    rules = _array(environment.get("protection_rules"), "protection_rules")
    reviewer_rules = [
        _object(rule, "required-reviewer protection rule")
        for rule in rules
        if isinstance(rule, dict) and rule.get("type") == "required_reviewers"
    ]
    if len(reviewer_rules) != 1:
        raise PolicyError("exactly one required-reviewers rule is mandatory")
    reviewer_rule = reviewer_rules[0]
    if reviewer_rule.get("prevent_self_review") is not True:
        raise PolicyError("release requesters must be prevented from self-reviewing")
    reviewers = _array(reviewer_rule.get("reviewers"), "required reviewers")
    if not reviewers:
        raise PolicyError("at least one release environment reviewer is mandatory")
    for index, reviewer in enumerate(reviewers):
        item = _object(reviewer, f"required reviewers[{index}]")
        if item.get("type") not in {"User", "Team"} or not isinstance(
            item.get("reviewer"), dict
        ):
            raise PolicyError("required reviewer entry is malformed")

    if not any(
        isinstance(rule, dict) and rule.get("type") == "branch_policy"
        for rule in rules
    ):
        raise PolicyError("the environment has no deployment branch-policy rule")
    deployment_policy = _object(
        environment.get("deployment_branch_policy"), "deployment_branch_policy"
    )
    if deployment_policy.get("protected_branches") is not False:
        raise PolicyError("release deployment must not admit every protected branch")
    if deployment_policy.get("custom_branch_policies") is not True:
        raise PolicyError("release deployment must use a custom exact-branch policy")

    policies = _array(
        branch_policies.get("branch_policies"), "branch_policies.branch_policies"
    )
    total = branch_policies.get("total_count")
    if isinstance(total, bool) or not isinstance(total, int) or total != len(policies):
        raise PolicyError("deployment branch-policy response is incomplete")
    if len(policies) != 1:
        raise PolicyError("exactly one production deployment branch policy is required")
    policy = _object(policies[0], "branch_policies.branch_policies[0]")
    if policy.get("name") != expected_branch:
        raise PolicyError(
            f"production deployment must admit only branch {expected_branch!r}"
        )
    # Older GitHub API responses omit `type`; when present, reject a tag rule
    # with the same display name.
    if "type" in policy and policy.get("type") != "branch":
        raise PolicyError("production deployment policy must be a branch rule")

    _require_inventory(
        secrets,
        collection_key="secrets",
        required=REQUIRED_SECRETS,
        label="production environment secrets",
    )
    _require_inventory(
        variables,
        collection_key="variables",
        required=REQUIRED_VARIABLES,
        label="production environment variables",
    )
    repository_secret_names = _named_inventory(
        repository_secrets,
        collection_key="secrets",
        label="repository secrets",
    )
    organization_secret_names = _named_inventory(
        organization_secrets,
        collection_key="secrets",
        label="organization secrets shared with the repository",
    )
    broadly_available_secrets = repository_secret_names | organization_secret_names
    leaked_secret_scope = sorted(REQUIRED_SECRETS & broadly_available_secrets)
    if leaked_secret_scope:
        raise PolicyError(
            "publisher secrets must exist only in the protected environment, but "
            f"these names are also repository-available: {', '.join(leaked_secret_scope)}"
        )
    repository_variable_names = _named_inventory(
        repository_variables,
        collection_key="variables",
        label="repository variables",
    )
    organization_variable_names = _named_inventory(
        organization_variables,
        collection_key="variables",
        label="organization variables shared with the repository",
    )
    broadly_available_variables = (
        repository_variable_names | organization_variable_names
    )
    leaked_variable_scope = sorted(REQUIRED_VARIABLES & broadly_available_variables)
    if leaked_variable_scope:
        raise PolicyError(
            "publisher variables must exist only in the protected environment, but "
            f"these names are also repository-available: {', '.join(leaked_variable_scope)}"
        )


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--environment-json", required=True, type=pathlib.Path)
    parser.add_argument("--branch-policies-json", required=True, type=pathlib.Path)
    parser.add_argument("--secrets-json", required=True, type=pathlib.Path)
    parser.add_argument("--variables-json", required=True, type=pathlib.Path)
    parser.add_argument("--repository-secrets-json", required=True, type=pathlib.Path)
    parser.add_argument("--repository-variables-json", required=True, type=pathlib.Path)
    parser.add_argument(
        "--organization-secrets-json", required=True, type=pathlib.Path
    )
    parser.add_argument(
        "--organization-variables-json", required=True, type=pathlib.Path
    )
    parser.add_argument("--environment", required=True)
    parser.add_argument("--branch", required=True)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if not args.environment or not args.branch:
            raise PolicyError("expected environment and branch must be non-empty")
        validate_policy(
            load_bounded_json(args.environment_json),
            load_bounded_json(args.branch_policies_json),
            load_bounded_json(args.secrets_json),
            load_bounded_json(args.variables_json),
            load_bounded_json(args.repository_secrets_json),
            load_bounded_json(args.repository_variables_json),
            load_bounded_json(args.organization_secrets_json),
            load_bounded_json(args.organization_variables_json),
            expected_environment=args.environment,
            expected_branch=args.branch,
        )
    except PolicyError as error:
        print(f"release policy rejected: {error}", file=sys.stderr)
        return os.EX_CONFIG
    print(
        f"release policy verified: environment={args.environment}, branch={args.branch}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
