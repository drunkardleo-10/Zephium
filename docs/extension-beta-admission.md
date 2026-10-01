# Beta source admission

`zephium-extension-distribution/beta-admission` is an opt-in, cross-platform
source-admission boundary. Ordinary desktop builds do not enable it. Both
product trust roots remain unprovisioned, and no public installer consumes it.

The pipeline is:

1. Authenticate the original CRX developer signature and requested Chromium ID.
2. Preflight and completely stream every ZIP member, producing the existing
   non-forgeable complete-tree receipt with original CRX/ZIP digests.
3. Require a live `AcceptedExtensionPolicy` and its exact compiled runtime
   target/version opt-in. Recommendations never supply this authority.
4. Check publisher-wide and exact-original-CRX revocations, then compare the
   authenticated numeric upstream version and bytes with supplied Store history.
5. Parse the original manifest against its exact authenticated tree, using
   compiled source rules rather than a caller-provided classification callback.
6. Return `ProductAdmittedBetaSource`, retaining original tree evidence,
   classifications, explicit limitations, and the live policy owner.

The original source package identity has a separate Beta authority domain,
channel, runtime target, and complete publisher key. Its revision is a local
Store sequence: Chromium's four 16-bit version components are not truncated or
packed into a signed SQLite revision. Store must compare both the expected
sequence and publisher high-water state again at installation commit.

## Initial compiled policy, version 1

The native targets are `macos.wkwebextension.v1` and `windows.webview2.v1`.
They can both be assessed on either build host. Source acceptance establishes
eligibility for further preparation, not completed runtime support or workflow
verification on that platform.

- The initial permission subset is `storage`, `tabs`, `scripting`, and
  `activeTab`. Required and optional permissions outside it are rejected.
- HTTP/HTTPS host patterns are assessed. `<all_urls>` carries an explicit
  local-file exclusion; explicit file-only requests are rejected.
- Isolated-world content scripts with ordinary matching are assessed. Related
  frame fallback, main-world declarations, and include/exclude globs require
  additional reviewed support and are rejected by version 1.
- Classic MV3 background workers, actions, options pages, secure extension CSP,
  and host-matched web-accessible resources are assessed. Web-accessible peer
  extension audiences are rejected.
- Module/document backgrounds, native messaging, sandbox pages, overrides,
  offscreen documents, minimum-Chromium-version declarations, commands, side
  panels, managed storage, DNR, and unknown authority are rejected by version 1.
- Storage declares unavailable Chrome-account-backed sync. macOS workers
  additionally declare the platform-managed background lifecycle. These
  limitations must be bound to installation consent; source admission grants
  no consent itself.

These rules are intentionally a closed first subset. Widening it requires
reviewed code and an explicit policy-version decision, not a remote script or
an optimistic fallback for unknown declarations. There is no claim of major
extension parity from passing this source filter.

## Verified on-device preparation

`BetaPreparationWorkspace` now consumes an admitted source and reauthenticates
the exact original CRX. Its first compiled transformation is
`beta.manifest-key.v1`: when the original manifest lacks a key, it inserts the
authenticated developer SPKI as canonical base64 and serializes recursively
sorted JSON objects while preserving array order. An existing matching key
preserves the complete original manifest bytes. Other file bytes are unchanged.
The CRX verifier exposes only the key from the proof deriving the extension ID;
an unrelated valid co-signer cannot become the inserted key.

Preparation streams the tree through descriptor-relative private filesystem
operations, recomputes the output manifest's complete declarations and
compatibility, rejects changes to the admitted execution/permission contract,
seals every file/directory, and independently re-hashes the closed output.
The original CRX, canonical output index, and deterministic transformation
evidence are retained beside the output tree. Atomic no-replace publication
moves the sealed `incoming` closure to the single `ready` slot, followed by
another complete verification before returning `PreparedBetaArtifact`.

The workspace recovers only recognized prepublication residue. A ready
artifact is reopened using newly admitted source, reauthentication of the
saved CRX, recomputation of the transform/evidence, and full file verification.
Policy refresh does not require changing immutable output bytes; newly bound
install provenance uses the current live policy. Verification, preparation,
reopen, and discard are serialized; discard or owner teardown invalidates old
receipts. An unsettled operation requires reopening. Maximum-depth package
cleanup is bounded separately from the artifact wrapper, without widening
private-fs limits.

The artifact exposes structural manifest/index/provenance, never a native path
or lease. Its provenance builder accepts provider classification only as
structural data: the install coordinator must independently prove approved
acquisition. Neither identity injection nor the original CRX signature claims
that the publisher signed the transformed tree.

## Repository custody and Store joins

`zephium-extension-repository/beta-packages` now owns a dedicated
`BetaPackageRepository`. `BetaPreparationWorkspace::open_in` retains a child
directory under the repository's original namespace lease; it does not create
an overlapping nested lock. Transfer reauthenticates the saved CRX and
recomputes the complete output in the destination. Its receipt is bound to the
repository owner, so acquisition scratch cleanup cannot invalidate it.

Immutable object addresses bind the package, original publisher/version/CRX/ZIP
checkpoint, output index, compatibility result and applied transform identity.
Two re-signed CRX envelopes containing the same ZIP therefore cannot collide.
Policy refresh does not rename the object. Existing output is reauthenticated
on replay and never overwritten. Unknown inventory and uncertain operations
quarantine the owner and invalidate its outstanding receipts. There is a fixed
256-slot ceiling including interrupted and orphaned slots. Startup opens slots
lazily; it does not run a repository-wide tree cleanup.

`reopen_bound` reconstructs source admission directly from the retained original
CRX using current signed policy, exact persisted provenance and a Store
publisher high-water snapshot. It needs neither an original download buffer nor
a package redownload. The matched historical Store provenance remains separate
from fresh admission provenance, so refreshing policy does not silently rewrite
an earlier install's policy identity. Historical policy fields are data for the
Store join, not a substitute for current policy authority. Source-policy or
evidence-format upgrades still require explicit requalification/migration;
immutable object contents must not be rewritten in place.

Core now owns the four structural Beta namespace labels, preserving their exact
existing digests. This is a refusal classifier, not a new source authority.
Store rejects a Beta install without provenance through both the generic
catalog-write path and the privileged provisioning path. Catalog reload,
grant/native manifest joins and core cohort construction enforce the same
requirement. Beta provenance also binds the original publisher to the package
key, the original ZIP to the payload, and the runtime target to its namespace.
Removing a provenance row cannot turn an installed Beta package into a legacy
package. The service's exclusive Store capability now exposes a bounded,
profile-local publisher-checkpoint read for admission and recovery; the broad
Store port does not gain that capability.

Tests exercise the actual repository implementation using private signed TUF
and CRX fixtures, without exporting a test root or witness constructor. One
integration test carries such a package through repository custody and the real
Store actor's atomic disabled-install/provenance/grant transaction, closes Store,
and verifies the cohort after reopening. It also exercises the pre-native
ownership protocol described below. Other tests cover scratch teardown,
owner invalidation, exact replay, all six interrupted preparation frontiers,
corruption, slot limits, saved-CRX reconstruction after policy refresh, backend
substitution and rollback refusal. This is not a native Beta installation test
or a user-facing consent flow.

## Native ownership preparation

Native journal entries now expose an explicit `ExtensionNativePackageSource`:
an active catalog, a rollback catalog, or an independent Beta object. The
immutable object digest algorithm is shared by Core, Store and the repository.
META v19 adds the separate durable Beta tag without changing existing catalog
roles or native identities. The migration rebuilds only from the program's
trusted v18 schema reference, preserves history guards, and rolls back on an
invalid source/role combination.

`BetaNativeOwnershipAdmission` joins current repository custody, authenticated
bytes, persisted provenance, enabled Store eligibility and the exact native
backend. After Store Begin, its move-only `BetaNativePackagePin` binds the exact
operation, incarnation and grant revisions. Store rechecks the object digest
against persisted provenance at Begin, MayOwn and grant rebind. Core requires
the original publisher-derived Chromium ID before a Beta row can reach MayOwn.
Beta objects cannot enter Verified catalog pin paths, compatibility backends,
or private partitions.

The signed-package integration fixture tests wrong-object, wrong-backend and
wrong-native-ID refusals, a successful Begin/MayOwn transition, and restart with
an unresolved native ownership obligation. It clears the fixture row only with
self-authored evidence that the fixture never called a native API. Migration
tests preserve an existing NativeOwned row's expected and observed IDs.

This is a path-free preparation boundary. The pin exposes neither a native
root nor an execution provider, and dropping it does not clear Store or claim
native absence. Actual native loading and lifecycle integration remain below.

## Remaining boundaries

An absent original manifest key is valid CRX input but is explicitly marked as
needing an identity transformation. The preparation path now supplies that
transformation with verified output evidence. Broader API/background adapters
still require separate compiled transformations and their verification.

Provider eligibility, user-facing consent, the executable Beta native package
lease, mixed Verified/Beta service management, and install/update
UI remain separate work. The custody API intentionally exposes no deletion or
native path: Store-bound retention/garbage collection and aggregate storage
budgeting must precede public activation. Installed-runtime offline admission
also needs its own authority; `reopen_bound` currently requires fresh policy and
must not be treated as that offline path. None of these layers may consume
this source witness as a Verified witness or infer native authority from its
structural descriptor. It must be revalidated after asynchronous boundaries;
superseding policy or closing its cache owner invalidates existing source
witnesses. Structural historical provenance remains readable for diagnostics.

The Windows filesystem adapter now implements handle-relative operations,
protected ACL modes, sealing, and shared bounded recovery. Default admission
still fails closed; its debug-only validation feature is not a shipping opt-in.
The implementation and tests pass Windows cross-target Rust checks, while linking,
native execution and durability
remain unverified. See the [Windows validation recipe](extension-windows-validation.md).
The preparation algorithm and both native-target policies remain shared.

The tests use real fixture CRX signatures, complete archive streaming, real TUF
signatures, and private durable policy caching. Both runtime policies are tested
on the current host. Real Windows execution will be verified on Windows; these
source-policy tests do not substitute for that verification.

## Local verification observations

The initial implementation passed 34 manifest/parser tests, 45 distribution
tests with Beta enabled, the nominal-authority compile-fail check, and the
default-feature distribution tests. Windows cross-compilation on the development
Mac stopped in native crypto dependencies because Windows C headers were not
available (`assert.h`); it is not a successful Windows build.

The existing macOS compatibility fixture reported retained native objects at
its teardown deadline in the reused development build. Experimental window and
autorelease cleanup changes did not resolve that result and were reverted.
The same artifact passed on clean foundation revision
`e89f030e98b9941ebdcfbd41fa6d93e5ceb66607`, a fresh build of the current source,
and an instrumented run. The cause of the reused-build failures is unresolved;
these successful runs are not a native teardown-stability or release claim.

The complete `cargo xtask ci` gate subsequently passed using the fresh
current-source build directory, including both native compatibility background
variants, repository recovery checks, and all 77 frontend tests. No native
cleanup changes or relaxed teardown assertions were retained.

The preparation implementation subsequently passed 56 distribution tests
(including 11 preparation/recovery cases), both nominal-authority compile-fail
checks, 10 CRX tests, and the acquisition suite. The complete `cargo xtask ci`
gate passed in a fresh build directory, including native probes and all 77
frontend tests. Preparation tests cover both runtime target policies, identity
preservation, key insertion, co-signer exclusion, exact readback, all six
publication frontiers, policy refresh, runtime/revision mismatch, tampering,
unsafe files/modes, single-flight ownership, and maximum portable path depth.
These results still do not constitute native Windows execution or public
installation readiness.

The September 10 Windows-adapter validation pass also caught a native macOS
product-probe crash in AppKit's popover size/layout path. The exact same binary
passed in isolation, so that pass did not explain the crash. Inspection found
that `PopupSizeClampGuard::enter` eagerly constructed and dropped a guard on
refused re-entry, clearing the outer operation's flag. A regression test with
repeated synchronous callbacks failed before the fix and passed after guard
construction was restricted to successful admission. This proves the guard
defect and its correction; it does not establish that every AppKit crash shares
that cause. Native presentation/teardown assertions remain unchanged.

After that correction, both native product variants passed independently and
inside the final complete `cargo xtask ci` run, including popup toggle/reopen,
grant revocation, uninstall erasure, shutdown and restart cleanup. The final run
also passed workspace/repository tests and all 77 frontend tests. An earlier
attempt timed out in the unchanged agent-controller localhost provider fixture
`fresh_account_refusal_or_control_during_sampling_cannot_dispatch_an_action`;
its isolated run with the exact CI profile and the final full run passed. No
agentic test deadline or assertion was relaxed, and the timing failure is not
claimed resolved by the extension changes.

The subsequent custody/Store implementation passed a fresh complete
`cargo xtask ci` run, including the existing native product probes and all 77
frontend tests. Focused checks passed 67 distribution tests, 254 Store tests,
193 core extension tests and 31 repository compile-fail/doc tests. The core
also passed a Windows-target Rust check. Portable preparation/custody/Store
tests are now selected on Windows by the explicit
`windows-namespace-validation` feature; their Windows execution and linking
remain unverified on this Mac. The metadata backend's parser source remains
byte-identical (SHA-256
`daa44d8c80a650b5ac6b6dfbfa7aff390dabec1c4872d072ee7f8b03eeac907a`).

The September 11 native ownership preparation changes passed 360 Core tests,
257 Store tests, 67 signed-policy/distribution tests, 283 repository tests,
205 service tests, and all 18 acquired-package coordinator cases. The acquired
repository cases were also run separately with their matching catalog fixture.
The 33 repository documentation tests include the move-only Beta pins;
14 acquisition-boundary tests enforce the separate factory sites. Clippy and
format checks passed. The first combined internal test invocation incorrectly
selected ordinary catalog tests under the acquired-catalog fixture; those
catalog-length refusals disappeared with CI's separate fixture configurations.

The current full CI run is **not green**: after the source-boundary and macOS
compatibility-asset checks, it stops at the WebView2 security-review deadline
(September 10). The macOS review deadline is also September 10. Neither deadline
was extended by this work. The previous complete native probe results above
are historical evidence, not a native Beta execution result or a September 11
full CI pass. The backend parser checksum remains unchanged.
