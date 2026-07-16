#!/usr/bin/env python3

from __future__ import annotations

import copy
import pathlib
import re
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import verify_release_policy


def valid_documents() -> tuple[dict, dict, dict, dict, dict, dict, dict, dict]:
    environment = {
        "name": "production-release",
        "protection_rules": [
            {
                "id": 1,
                "type": "required_reviewers",
                "prevent_self_review": True,
                "reviewers": [
                    {
                        "type": "Team",
                        "reviewer": {"id": 7, "slug": "release-reviewers"},
                    }
                ],
            },
            {"id": 2, "type": "branch_policy"},
        ],
        "deployment_branch_policy": {
            "protected_branches": False,
            "custom_branch_policies": True,
        },
    }
    branch_policies = {
        "total_count": 1,
        "branch_policies": [{"id": 3, "name": "main", "type": "branch"}],
    }
    secret_names = sorted(verify_release_policy.REQUIRED_SECRETS | {"UNRELATED_SECRET"})
    variable_names = sorted(
        verify_release_policy.REQUIRED_VARIABLES | {"UNRELATED_VARIABLE"}
    )
    secrets = {
        "total_count": len(secret_names),
        "secrets": [{"name": name} for name in secret_names],
    }
    variables = {
        "total_count": len(variable_names),
        "variables": [
            {"name": name, "value": "not-inspected"} for name in variable_names
        ],
    }
    repository_secrets = {"total_count": 0, "secrets": []}
    repository_variables = {"total_count": 0, "variables": []}
    organization_secrets = {"total_count": 0, "secrets": []}
    organization_variables = {"total_count": 0, "variables": []}
    return (
        environment,
        branch_policies,
        secrets,
        variables,
        repository_secrets,
        repository_variables,
        organization_secrets,
        organization_variables,
    )


class VerifyReleasePolicyTests(unittest.TestCase):
    def validate(
        self, documents: tuple[dict, dict, dict, dict, dict, dict, dict, dict]
    ) -> None:
        verify_release_policy.validate_policy(
            *documents,
            expected_environment="production-release",
            expected_branch="main",
        )

    def test_accepts_narrow_reviewed_environment(self) -> None:
        self.validate(valid_documents())

    def test_inventory_matches_every_release_workflow_credential_reference(self) -> None:
        workflow = self.release_workflow()
        referenced_secrets = frozenset(
            re.findall(r"\bsecrets\.([A-Z][A-Z0-9_]*)", workflow)
        )
        referenced_variables = frozenset(
            re.findall(r"\bvars\.([A-Z][A-Z0-9_]*)", workflow)
        )
        self.assertEqual(referenced_secrets, verify_release_policy.REQUIRED_SECRETS)
        self.assertEqual(referenced_variables, verify_release_policy.REQUIRED_VARIABLES)

    def test_release_publish_order_keeps_final_draft_proof_adjacent(self) -> None:
        workflow = self.release_workflow()
        start = workflow.index("      - name: Publish and bind the verified release")
        publish = workflow[start:]

        policy = publish.index("python3 scripts/release/verify_release_policy.py")
        tag = publish.index("Remote release tag changed::Refusing publication")
        final_download = publish.index(
            'gh release download "${RELEASE_TAG}" --dir "${final_draft}"'
        )
        exact_set = publish.index('<(find "${final_draft}" -maxdepth 1 -type f')
        exact_bytes = publish.index('cmp --silent "${source}" "${final_draft}/')
        publication = publish.index(
            'gh release edit "${RELEASE_TAG}" --draft=false --latest'
        )

        self.assertLess(policy, tag)
        self.assertLess(tag, final_download)
        self.assertLess(final_download, exact_set)
        self.assertLess(exact_set, exact_bytes)
        self.assertLess(exact_bytes, publication)
        self.assertNotIn(
            "      - name: Download and byte-verify the draft before publication",
            workflow,
        )

    def test_published_release_is_bound_back_to_every_local_asset(self) -> None:
        workflow = self.release_workflow()
        start = workflow.index("      - name: Publish and bind the verified release")
        publish = workflow[start:]
        publication = publish.index(
            'gh release edit "${RELEASE_TAG}" --draft=false --latest'
        )
        release_attestation = publish.index('gh release verify "${RELEASE_TAG}"', publication)
        local_asset_attestation = publish.index(
            'gh release verify-asset "${RELEASE_TAG}" "${source}"',
            release_attestation,
        )
        published_download = publish.index(
            'gh release download "${RELEASE_TAG}" --dir "${published}"',
            local_asset_attestation,
        )
        immutable_incident = publish.index(
            "Published immutable asset mismatch", published_download
        )

        self.assertLess(publication, release_attestation)
        self.assertLess(release_attestation, local_asset_attestation)
        self.assertLess(local_asset_attestation, published_download)
        self.assertLess(published_download, immutable_incident)

    def test_rejects_missing_or_self_approvable_reviewers(self) -> None:
        documents = list(copy.deepcopy(valid_documents()))
        documents[0]["protection_rules"][0]["reviewers"] = []
        with self.assertRaisesRegex(verify_release_policy.PolicyError, "reviewer"):
            self.validate(tuple(documents))

        documents = list(copy.deepcopy(valid_documents()))
        documents[0]["protection_rules"][0]["prevent_self_review"] = False
        with self.assertRaisesRegex(verify_release_policy.PolicyError, "self-review"):
            self.validate(tuple(documents))

    def test_rejects_missing_environment_credential(self) -> None:
        documents = list(copy.deepcopy(valid_documents()))
        documents[2]["secrets"] = [
            item
            for item in documents[2]["secrets"]
            if item["name"] != "UPDATE_SIGNING_PRIVATE_KEY"
        ]
        documents[2]["total_count"] -= 1
        with self.assertRaisesRegex(
            verify_release_policy.PolicyError, "UPDATE_SIGNING_PRIVATE_KEY"
        ):
            self.validate(tuple(documents))

    def test_rejects_broad_or_wrong_deployment_policy(self) -> None:
        documents = list(copy.deepcopy(valid_documents()))
        documents[0]["deployment_branch_policy"]["protected_branches"] = True
        documents[0]["deployment_branch_policy"]["custom_branch_policies"] = False
        with self.assertRaisesRegex(verify_release_policy.PolicyError, "protected branch"):
            self.validate(tuple(documents))

        documents = list(copy.deepcopy(valid_documents()))
        documents[1]["branch_policies"][0]["name"] = "release/*"
        with self.assertRaisesRegex(verify_release_policy.PolicyError, "only branch"):
            self.validate(tuple(documents))

    def test_rejects_incomplete_inventory_page(self) -> None:
        documents = list(copy.deepcopy(valid_documents()))
        documents[3]["total_count"] += 1
        with self.assertRaisesRegex(verify_release_policy.PolicyError, "incomplete"):
            self.validate(tuple(documents))

    def test_rejects_publisher_credential_available_outside_environment(self) -> None:
        documents = list(copy.deepcopy(valid_documents()))
        documents[4] = {
            "total_count": 1,
            "secrets": [{"name": "WINDOWS_CERTIFICATE_BASE64"}],
        }
        with self.assertRaisesRegex(
            verify_release_policy.PolicyError, "only in the protected environment"
        ):
            self.validate(tuple(documents))

        documents = list(copy.deepcopy(valid_documents()))
        documents[7] = {
            "total_count": 1,
            "variables": [{"name": "WINDOWS_CERTIFICATE_SHA256"}],
        }
        with self.assertRaisesRegex(
            verify_release_policy.PolicyError, "only in the protected environment"
        ):
            self.validate(tuple(documents))

        documents = list(copy.deepcopy(valid_documents()))
        documents[6] = {
            "total_count": 1,
            "secrets": [{"name": "APPLE_CERTIFICATE"}],
        }
        with self.assertRaisesRegex(
            verify_release_policy.PolicyError, "only in the protected environment"
        ):
            self.validate(tuple(documents))

    @staticmethod
    def release_workflow() -> str:
        return (
            pathlib.Path(__file__).resolve().parents[2]
            / ".github"
            / "workflows"
            / "build.yml"
        ).read_text(encoding="utf-8")


if __name__ == "__main__":
    unittest.main()
