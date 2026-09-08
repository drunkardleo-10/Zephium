# Extension metadata service

Implementation contract, 2026-09-07. This service distributes Zephium metadata,
not third-party extension packages. Public installation remains disabled until
client admission, durable recovery, provider eligibility, and release evidence
are complete. Bun/Hono is an implementation option, not part of the protocol.

## Hosting contract

Use `https://extensions.zephium.app`. Reserve two independent TUF repositories:

| Channel | Metadata base | Targets base |
| --- | --- | --- |
| Production | `/v1/stable/metadata/` | `/v1/stable/targets/` |
| Integration | `/v1/staging/metadata/` | `/v1/staging/targets/` |

Each repository uses independent keys and TUF consistent snapshots:
`N.root.json`, `timestamp.json`, `N.snapshot.json`, `N.targets.json`, and
hash-prefixed target files. Use a maintained TUF publisher; do not implement
signature canonicalization in the request handler. The client already uses
Tough for the separate blocker trust domain; extension client wiring is being
implemented independently.

- Serve exact bytes with `Content-Length`, no compression, no redirects, and
  `Content-Type: application/json` for metadata and JSON targets.
- Strong `ETag` is a quoted lowercase SHA-256 of the exact response bytes:
  `"<64 lowercase hex characters>"`. Honor `If-None-Match` with 304. Do not
  generate visitor-specific ETags. Clients never replay opaque server tokens.
- Immutable versioned/hash-named objects use
  `Cache-Control: public, max-age=31536000, immutable`.
- Mutable `timestamp.json` uses `Cache-Control: public, max-age=300, must-revalidate`.
- Upload immutable targets, versioned targets metadata, and versioned snapshot
  metadata first; atomically replace `timestamp.json` last. Never overwrite an
  immutable path with different bytes. Preserve old roots for rotation.
- No login, cookies, API keys, client identifiers, per-extension request
  parameters, installed-extension inventories, or third-party tracking.
  Disable request analytics/IP retention at both server and reverse proxy.
  Aggregate response/error counters without client attribution are sufficient.
- Public serving holds no signing key. Keep the root offline and release
  signing in a protected publication job. Never expose keys to fork/PR jobs.
- Use distinct root and online role keys. Root rotation must be authorized by
  both old and new root thresholds. Use a 2-of-3 offline root; keep recovery
  copies separately. Staging keys must never be accepted in production.
- Start with 24-hour timestamp, 7-day snapshot/targets, and 1-year root
  expirations. Republish before expiry, with increasing role versions. Alert
  on publication failure and approaching expiry using aggregate service state.

## Shared target payload

Every client fetches the same `extension-policy-v1.json` TUF target. Do not
split downloads by installed extension. The parser and field limits live in
`crates/zephium-extension-package/src/public_policy.rs`; backend tests should
use the same positive and negative fixtures/contract. TUF hashes exact payload
bytes, so JSON object key order and whitespace do not need canonicalization.

Before signing, validate the exact payload with:

```sh
cargo xtask check-extension-public-policy --policy /absolute/path/to/extension-policy-v1.json
```

The JSON report explicitly says `signature_verified=false` and
`product_authority=false`: successful schema validation is not signed admission.
An intentionally expired unsigned parser fixture is available at
`crates/zephium-extension-package/fixtures/public-policy/empty-staging.json`.
Replace its times and sequence through the publisher; do not deploy the fixture.

An empty integration payload is valid and enables no Beta targets:

```json
{
  "schema_version": 1,
  "policy_revision": 1,
  "channel": "staging",
  "issued_unix": 1788825600,
  "expires_unix": 1788912000,
  "beta_targets": [],
  "recommendations": [],
  "revocations": []
}
```

The timestamps above are illustrative: publication must supply current UTC
Unix seconds. A policy has a strictly positive sequence, a positive validity
period of at most seven days, and at most 256 KiB of JSON. Increment
`policy_revision` whenever any payload byte changes. Never reuse a sequence
for different bytes. No unknown fields or duplicate JSON keys are accepted.

`beta_targets` contains at most eight `{ "target": "...", "policy_version": 1 }`
records with unique target names. These select already compiled client policy;
they cannot define APIs, native hosts, scripts, executable transforms, or new
privileges. Keep the array empty until the client explicitly supplies its
supported target/version pairs. A Beta lane is separate from the release
channel: `channel` is only `stable` or `staging`.

Each recommendation has these fields:

```text
extension_id: canonical 32-character Chromium id
developer_key_sha256: lowercase SHA-256 of the publisher's CRX public SPKI
tested_versions: 1..16 exact evidence records
```

The key digest must derive the declared Chromium id. Recommendation ids must
be unique. Each test record contains:

```text
upstream_version: numeric Chromium version (one to four 0..65535 components)
original_crx_sha256: exact original package digest
runtime_target: exact client runtime target token
transform_id: exact compiled transform token, or identity.v1
transformed_tree_sha256: exact tested native resource-tree digest
zephium_version: tested build label, at most 128 UTF-8 bytes
platform_runtime: tested OS/engine label, at most 128 UTF-8 bytes
tested_unix: positive completed-test time, no later than issued_unix
workflows: 1..32 plain-text labels, at most 128 bytes each
limitations: 0..32 plain-text statements, at most 512 bytes each
```

Text fields are nonempty and contain no control characters. Hashes are exactly
64 lowercase hex characters. Test records are unique by original CRX, runtime
target, transform, Zephium version, and platform/runtime. The recommendation
can survive an upstream update; its exact test evidence cannot. Recommendations
never mint the existing exact Verified manifest witness.

Each revocation contains `extension_id`, `developer_key_sha256`,
`original_crx_sha256` (a digest or explicit `null` for every version under that
key), and `reason` (nonempty plain text, at most 512 bytes). Up to 256 unique
id/digest pairs are accepted. Revocations take precedence over recommendations.
Retain publisher-wide revocations in later snapshots until an explicitly
reviewed reversal; replacing a snapshot must not accidentally erase an incident.

## Client and publication responsibilities

The client verifies TUF, exact payload identity, channel, signed expiry,
monotonic policy revision, and durable known-time state. A 304 response changes
none of those facts and never refreshes signed expiry. New Beta admissions
require fresh policy. Updates use bounded local jitter/backoff; the server
does not send personalized schedules. No installed-extension set goes to Zephium.

Original CRX bytes come from an approved store or publisher provider directly
to the user device. The client separately verifies the publisher key/id and
preserves original and transformed identities. Provider fallback must be an
explicit approved source, never an arbitrary manifest/DOM URL.

The initial backend deliverable is DNS/TLS, the two static repositories,
protected publication, and a signed **empty staging policy**. Return the exact
staging root JSON and public-key fingerprints to the browser implementation.
Never send private keys. A served file or recommendation is not authorization
to enable the public browser feature.

Future dynamic blocker updates may reuse the hosting/publication machinery,
but use a separate trust domain, roots, target schemas, and client lifecycle.

## Source-provider validation status

A bounded read-only check on 2026-09-07 exercised Google's
[documented CRX update endpoint](https://developer.chrome.com/docs/extensions/how-to/distribute/install-extensions).
For Vimium (`dbepggeogbaibhgnhhndojpepiihcmeb`), a request without
`prodversion` returned 204. Supplying `prodversion=131.0.0.0` as a diagnostic
negotiation value returned a 302 to an HTTPS `clients2.googleusercontent.com`
CRX blob named version 2.4.2. That negotiation value is not a Zephium engine
version or a shipping compatibility decision.

The original download passed the new upstream acquisition path through
`cargo xtask check-crx3`: three valid proofs, publisher key SHA-256
`314f664e6108176d77d3e9f4f8872c41589a2d2e6b442f1787802fa42446b892`,
CRX SHA-256 `3198c26aa719be462dea585050fbed9b8b80628d57ea88a113babf5334c5517c`,
208,153 ZIP bytes, 79 files, and 558,837 expanded bytes. No package was installed
or added to product authority; downloaded bytes remain outside the repository.
This proves one delivery/authentication path, not general provider readiness.

The [Chrome Web Store developer agreement](https://developer.chrome.com/docs/webstore/program-policies/terms)
describes permitted access interfaces in section 4.4.2 and a default end-user
license tied to Chrome, with a separate publisher EULA option, in section 5.2.
That developer agreement does not by itself establish Zephium's rights as a
third-party browser. The [general Google terms](https://policies.google.com/terms)
also address automated access and protective measures. Provider enablement
still requires an applicable-terms determination and package/publisher license
review; neither Orion precedent nor a successful download supplies that result.
Retain approved publisher-hosted CRX acquisition as a separate fallback.
