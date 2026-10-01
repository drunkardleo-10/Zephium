# Production release runbook

The `Production release` workflow is intentionally unusable until the
`production-release` GitHub environment is protected and all publisher
credentials are provisioned. It never publishes an unsigned fallback.

This runbook describes artifact authenticity, not overall browser readiness.
The current tree is pre-production: all gates in `docs/security-model.md` still
apply, including packaged hostile/native-erasure tests, resource endurance, and
an external native-boundary audit. Before the first stable release, rehearse
this workflow end to end with the real protected publisher credentials on each
installer host and retain the verification evidence. Signed update metadata is
not an automatic updater; the client trust root and monotonic sequence store
must land first.

Engine-floor, dependency-advisory, and native-boundary fork review procedures
live in `docs/security-maintenance.md`. A release failure caused by an expired
review or a new advisory is never resolved by weakening the gate or extending
a date without repeating that review.

## Repository and environment policy

- Keep `main` as the default branch. Require pull requests and the complete CI
  workflow on it; a production dispatch is valid only from the current head of
  protected `main`.
- Protect `v*` tags. Only release maintainers may create or delete them.
- Require an annotated `vMAJOR.MINOR.PATCH` tag whose target is the reviewed
  release commit. The workflow verifies the remote Git ref and annotated-tag
  object initially, before draft upload, and immediately before publication.
  Effective tag-ruleset bypass state is still a repository-policy/manual or
  protection-app gate; the workflow proves tag identity, not every actor's
  ability to bypass GitHub tag rules.
- Protect the `production-release` environment with at least one required
  reviewer, enable **Prevent self-review**, and configure exactly one custom
  deployment branch policy: the literal branch `main` (not every protected
  branch and not a wildcard). The workflow verifies every API-visible part of
  that policy initially and immediately before publication.
- Disable administrator bypass for `production-release`, or require an
  equivalent external/custom deployment-protection approval. GitHub's current
  GET-environment response does **not** expose the administrator-bypass state,
  so this remains a separately audited manual or protection-app gate; the
  workflow does not claim to prove it.
- Store every publisher secret and variable listed below only in
  `production-release`. Do not define the same names as repository-level or
  organization-level Actions secrets/variables shared with this repository.
  The workflow enumerates all three scopes and fails on any duplicate name.
- Enable GitHub **Release immutability** for this repository. The workflow
  queries the versioned immutable-releases API before building, again before
  uploading, and immediately before publication; a disabled or unreadable
  policy is a hard failure.
- Treat every principal with repository **Contents: write** as part of the
  publisher trust boundary. GitHub draft releases remain mutable until the
  publish operation, so release-edit authority must be restricted to the
  smallest practical maintainer set. Prefer a dedicated publisher GitHub App
  or dedicated release repository when that can remove ordinary developer
  credentials from the draft-mutation path. The workflow performs its final
  set/byte comparison after every other remote policy check and verifies every
  local asset against the immutable release attestation after publication, but
  post-publication detection cannot undo an immutable compromised release.
- Restrict Actions to pinned, approved actions. Do not allow actions to create
  or approve pull requests.
- Configure Actions artifact retention for at least 90 days.
- Treat Actions artifacts named `zephium-private-symbols-*` as confidential
  crash-analysis material. Only authenticated-encryption ciphertext and its
  HMAC are retained for 90 days; raw symbols are never uploaded or copied to
  the public GitHub release.

## Protected environment secrets

| Name | Purpose |
| --- | --- |
| `RELEASE_POLICY_TOKEN` | Repository-scoped fine-grained token with repository **Actions: read**, **Administration: read**, **Contents: read**, **Environments: read**, **Secrets: read**, and **Variables: read**, and no write permission; used only for release-policy, ref, environment, credential-scope, and prior-release reads |
| `APPLE_CERTIFICATE` | Base64 Developer ID Application certificate (`.p12`) |
| `APPLE_CERTIFICATE_PASSWORD` | Export password for that certificate |
| `APPLE_ID` | Apple notarization account |
| `APPLE_PASSWORD` | Apple app-specific notarization password |
| `WINDOWS_CERTIFICATE_BASE64` | Base64 Authenticode PFX |
| `WINDOWS_CERTIFICATE_PASSWORD` | PFX export password |
| `RPM_SIGNING_PRIVATE_KEY` | Armored RPM publisher private key |
| `RPM_SIGNING_KEY_PASSPHRASE` | RPM key passphrase |
| `SYMBOL_ENCRYPTION_KEY` | Base64 encoding of 32 random symbol-encryption bytes |
| `SYMBOL_AUTHENTICATION_KEY` | Base64 encoding of a different 32 random HMAC bytes |
| `UPDATE_SIGNING_PRIVATE_KEY` | Cosign private key for stable update metadata |
| `UPDATE_SIGNING_KEY_PASSWORD` | Update-key password |

## Protected environment variables

| Name | Required value |
| --- | --- |
| `APPLE_SIGNING_IDENTITY` | Full `Developer ID Application: …` identity |
| `APPLE_TEAM_ID` | Ten-character Apple team identifier |
| `WINDOWS_CERTIFICATE_SHA256` | Uppercase 64-hex SHA-256 of the exact DER-encoded Authenticode leaf certificate |
| `WINDOWS_CERTIFICATE_SUBJECT` | Exact Authenticode certificate subject |
| `WINDOWS_TIMESTAMP_URL` | Absolute HTTPS RFC 3161 endpoint |
| `RPM_SIGNING_KEY_FINGERPRINT` | Uppercase 40-hex publisher-key fingerprint |
| `UPDATE_SIGNING_PUBLIC_KEY` | Cosign public key corresponding to the update private key |
| `UPDATE_SIGNING_KEY_ID` | `sha256:` plus SHA-256 of the normalized public-key file |

Derive the Authenticode leaf pin from certificate DER, then store the uppercase
digest. The workflow recomputes it at import and for every outer/embedded
signature; the subject string is only an additional human-readable identity.

```sh
openssl pkcs12 -in authenticode.pfx -clcerts -nokeys |
  openssl x509 -outform DER |
  sha256sum
```

The workflow normalizes `UPDATE_SIGNING_PUBLIC_KEY` by writing its value plus
one trailing newline. Compute the key ID using the same representation:

```sh
printf '%s\n' "$(cat cosign.pub)" | sha256sum
```

Recover a retained symbol archive only on an access-controlled analysis host.
After exporting both symbol keys, the helper authenticates the ciphertext
before decrypting it and refuses to overwrite an existing output:

```sh
python3 scripts/release/seal_symbols.py unseal \
  symbols.tar.gz.enc symbols.tar.gz.enc.hmac-sha256 symbols.tar.gz
```

The update public key and manifest-sequence comparison must also be implemented
in the updater client before automatic updates are enabled. A client must
verify the manifest signature with an embedded trust root and persist the
largest accepted `rollback.sequence`; a lower or repeated sequence is rejected
before any artifact is downloaded.

## Release procedure

1. Set every authoritative project version to the same semantic version.
2. Merge the reviewed commit after all required checks pass.
3. Create and push a protected annotated tag for that exact commit.
4. Dispatch `Production release` from `main`; enter the protected annotated tag
   in `release_tag` and the next update sequence. Sequence 1 is allowed only
   when no prior stable GitHub release exists; every later release must advance
   the signed prior manifest by one.
5. A required reviewer inspects the commit, tag, sequence, and publisher-key
   state before approving the `production-release` environment.

The workflow builds signed RPM, MSI/NSIS, and arm64/x86_64 DMG artifacts;
verifies native signatures and platform mitigations; extracts every installer
and proves its main executable identity; generates pinned-Syft CycloneDX SBOMs
over the Linux/macOS installed payloads and Windows extracted installer
payloads, plus locked build manifests; binds each SBOM to the installer digest
and size; signs checksums and chained update metadata; creates provenance and
SBOM attestations on the platform runner that produced the bytes; uploads a
draft release; after every final policy/tag check, downloads and byte-compares
the exact asset set immediately before publication; and only then makes the
release immutable. It verifies GitHub's release attestation, binds every local
asset back to that attestation with `gh release verify-asset`, and downloads the
published set once more to reject missing, changed, or injected assets. Windows
extraction is not a substitute for packaged clean-VM install, launch, update,
and uninstall tests; those remain a stable-release readiness gate.
The macOS entitlement allowlist is intentionally empty: every Mach-O in the
app is inspected and any explicit entitlement blob blocks publication until a
specific entitlement receives security review.

The Fedora build runs from the official Fedora 43 OCI index pinned by digest.
All native build inputs are installed from signature-enforcing stable Fedora
repositories, verified with `rpm -V`, and recorded as exact NEVRAs in the RPM
audit. Node is fixed to 24.18.0 and Syft to 1.44.0 rather than following moving
major or latest-release aliases. The separate hostile WebKitWebProcess
confinement job runs in the official Fedora 44 OCI index pinned by digest and
rejects an engine outside the reviewed WebKitGTK 2.52.x line or below 2.52.6.
That non-artifact job may explicitly consume Fedora's signed updates-testing
WebKitGTK/JSC transaction while the security update awaits promotion. The RPM
publisher remains stable-repository-only and therefore blocks rather than
shipping against 2.52.4.

If a run fails before draft creation, rerun only after fixing the cause and
re-reviewing the same tagged commit. If it fails after creating a draft, do not
silently reuse or overwrite that draft: inspect the incident, delete only the
unpublished draft under a maintainer-approved recovery procedure, then rerun
the exact tag—or cut a new version if any build input or source changed. A
published immutable release and its protected tag are never deleted, edited,
or replaced.

If publication succeeds but release-attestation, `gh release verify-asset`, or
the final published-set byte comparison fails, treat the workflow failure as a
post-publication security incident: the release is already public and
immutable. Freeze the stable channel and updater, retain all workflow/artifact
evidence, independently run `gh release verify`, `gh release verify-asset`, and
`gh attestation verify`/asset digest checks, and do not delete, edit, recreate,
or rerun that version. If authenticity cannot be established, revoke it only by
publishing a new, higher version and monotonic sequence after incident review.

Never replace assets on an existing release. Revoke a compromised release by
publishing a new, higher version and sequence. Key rotation requires a reviewed
dual-trust migration in both this workflow and the updater; the current pipeline
fails closed if the previous manifest cannot be verified by the configured key.
