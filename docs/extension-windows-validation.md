# Windows extension filesystem validation

Status: 2026-10-02. The Windows adapter has now linked and run native tests on
Windows 10.0.26200 x64, local NTFS, under an Administrators-group account with
its administrator role disabled (non-elevated). The explicit debug-validation
suite passes 38 tests. The separately invoked mandatory reparse test also
passes after the user enabled Developer Mode. A separate non-admin account,
an account name containing spaces, and a long profile path remain unqualified.
Production namespace activation remains closed: it returns
`PrimitiveUnavailable` before inspecting the requested path. The explicit
`windows-namespace-validation` feature enables the adapter only for debug
validation; optimized builds reject it at compile time.

This is a validation recipe for the Windows agent, not a completed extension
release handoff. Shared Beta installation, consent, native ownership, and update
integration remain separate work described in [Beta admission](extension-beta-admission.md).

## Implementation to review

- `zephium-private-fs/src/platform/windows.rs` owns identity, exact spelling,
  root admission, regular/directory modes, locking and native operation wiring.
- `windows/native.rs` uses held-parent, single-component NT opens, enumeration,
  atomic rename and POSIX deletion. It does not resolve child operations through
  the caller's old ambient path, copy on rename failure, or defer deletion while
  reporting a settled removal. Exact final-name checks reject case and 8.3 aliases.
- `windows/security.rs` creates protected explicit user/SYSTEM/Administrators
  DACLs, checks owner and complete ACL shape, and implements writable/sealed
  modes. Unsupported ACE forms and unrelated ancestor mutation rights fail
  closed. Administrators and malicious code already running as the same user
  remain outside this boundary, as on Unix.
- Root traversal rejects UNC/device prefixes and reparse points and requires
  a held local NTFS volume with persistent ACLs. A mapped drive does not become
  eligible merely because it has a drive letter. SUBST roots hiding additional
  ancestors are rejected. Files with multiple hard links
  are rejected. Missing ancestors are never created.
- Unix and Windows use the same bounded tree-removal algorithm: complete
  preflight, identity revalidation, top-down unsealing, bottom-up removal, then
  one final parent barrier. The existing quarantine/linear capability contract
  remains shared.
- Durability calls perform actual flushes. Read-only recovery handles can be
  reopened by held identity after unsealing; this does not change an ACL merely
  to make a flush succeed. Any unavailable barrier fails the operation. In
  particular, directory metadata barriers and reopened sealed publication need
  real Windows evidence; successful Rust compilation proves neither.

The NT API assumptions are grounded in Microsoft's documentation for
[relative file creation](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
[volume queries](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryvolumeinformationfile),
[remote-device detection](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/616b66d5-b335-4e1c-8f87-b4a55e8d3e4a),
[reopening held objects](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-reopenfile),
and [flush access requirements](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).
These references do not replace live filesystem or crash testing.

## Run on Windows

Use a normal, non-elevated user on local NTFS with the repository's pinned Rust
toolchain and the MSVC/Windows SDK. Record Windows build, architecture, filesystem,
WebView2 runtime and command exit status. Use a fresh Cargo target directory
belonging only to this checkout. Do not share outputs with another source tree.

```powershell
cargo test --locked -p zephium-private-fs
cargo clippy --locked -p zephium-private-fs --all-targets -- -D warnings
cargo test --locked -p zephium-private-fs --features windows-namespace-validation
cargo clippy --locked -p zephium-private-fs --all-targets --features windows-namespace-validation -- -D warnings
```

The enabled suite exercises exact create and alias rejection, hard-link and ACL
rejection, held-parent redirection resistance, multi-buffer bounded enumeration,
no-replace and replacement identity, sealing/unsealing, locks, public sealed-tree
publication/reopening/removal, streaming cleanup, and quarantine at injected
post-commit failures. Unsupported primitives must fail tests; do not convert
them to success or skip the corresponding assertions.

The reparse-point test is explicitly ignored in the ordinary command because
creating its symlink fixtures requires Developer Mode symlink permission. It is
mandatory before activation and should also run non-elevated:

```powershell
cargo test --locked -p zephium-private-fs --features windows-namespace-validation platform::windows::tests::validation::reparse_file_directory_and_namespace_root_are_rejected -- --ignored --exact
```

Verify the release guard separately. The first command **must fail** specifically
with `Windows namespace validation must not enter an optimized shipping build`;
the second check must pass:

```powershell
cargo check --locked --release -p zephium-private-fs --features windows-namespace-validation
cargo check --locked --release -p zephium-private-fs
```

After the filesystem tests pass, run the real signed-policy and transformed-CRX
preparation tests against this validation adapter:

```powershell
cargo test --locked -p zephium-extension-distribution --features windows-namespace-validation
cargo test --locked -p zephium-extension-acquisition
cargo test --locked -p zephium-extension-package
```

The distribution validation feature explicitly includes portable preparation,
repository custody, crash-recovery and real Store integration tests on Windows.
Unix chmod/symlink mutation fixtures remain Unix-only; Windows ACL/reparse
coverage belongs to the Windows adapter suite above and the live cases below.
Merely enabling the private-fs dependency feature does not select the portable
distribution test module on Windows.

## Required live checks before lifting the gate

1. Run both ordinary-account and administrator-group/non-elevated account cases.
   Test an account name containing spaces and a long profile path. Resolve any
   legitimate 8.3 `%TEMP%` spelling without permitting alias-based authority.
2. Prove namespace lock exclusion across **processes**, including simultaneous
   first creation and terminated lock owners. Reopen exact canonical and staging
   lock residue; do not reinterpret arbitrary lock bytes as recoverable state.
3. Exercise NTFS junctions, mount points, case-sensitive directories, real 8.3
   aliases where enabled, unexpected ACLs, hard links, and ancestor replacement.
   Refuse remote shares/mapped drives and unsupported filesystems explicitly.
4. Terminate the writer around create, seal, rename and delete boundaries and
   reopen the namespace. Verify sealed-tree publication both immediately after
   preparation and after closing/reopening it. Test source/destination parents
   separately and confirm both barriers where applicable. An API return of
   success alone is not power-loss durability evidence.
5. Verify flush behavior under filesystem/security filters and ordinary sharing
   conflicts. No directory-sync no-op, copy/delete rename fallback, deferred
   unlink success, or permission widening may replace a missing primitive.
6. Run the existing native WebView2 qualification with actual extension workflows
   and process teardown. Private-filesystem tests do not prove install, update,
   revocation, extension behavior, or native lifetime correctness.

Do not enable the feature in shipping dependency manifests. When the evidence
supports production admission, review the gate change independently and rerun
the complete repository checks. Preserve the exact Verified authority and the
separate Beta authority throughout.

## Backend boundary

This adapter changes no public-policy JSON fields or signing contract. Live
metadata and public installation still require independently verified deployed
endpoints, exact public staging root bytes and policy identity, and approved
package-provider eligibility. No trust root, provider approval or user grant can
be inferred from successful filesystem tests.

## Local evidence

The 2026-10-02 native run used an explicitly selected disposable fixture beneath
`.codex/worktrees/windows-runtime`, with TEMP/TMP set only for the validation
process. The ambient AppData/Local/Temp ancestry on this instrumented host grants
untrusted principals mutation rights and is correctly refused. No existing ACL,
Developer Mode setting, privilege, or shipping data-root selection was changed
by the agents. The user later enabled Developer Mode explicitly; the mandatory
reparse test was then rerun non-elevated. A safe fixture does not establish
admission of the default product data root. The exact account facts are recorded
in `target/windows-private-fs-live-account-evidence.json`.

Native execution exposed and fixed three adapter defects: directory writable
reopening now uses the held local-volume file ID as a locator and independently
verifies full 128-bit identity, kind, and unchanged owned security snapshots;
directory deletion uses the exact held-parent child name rather than a linkless
by-ID handle; deletion settlement accepts an already-held zero-link regular
file only when its full identity still matches. New-file admission still requires
exactly one link, and settled deletion still requires exact-name absence.

The initial unblocked suite records 35 passed, zero failed, and one mandatory
ignored reparse test in `target/windows-private-fs-final-unblocked-validation.log`.
It exercises moved-original versus ambient-replacement reopening, sealed write
refusal, public sealed-tree publication and bounded recovery, native directory
barriers, hostile hard-link/ACL/name cases, and actual cross-process lock
exclusion and owner-exit recovery. The separate native measurements are recorded
in `target/windows-private-fs-win32-identity-reopen.log` and
`target/windows-private-fs-symlink-permission.log`; the initial file and directory
symlink attempts returned 1314. After the user enabled Developer Mode, the
existing mandatory file/directory/root reparse rejection test passed, with
one passed and zero failed in
`target/windows-private-fs-mandatory-reparse-enabled.log`. This supersedes the
symlink-permission blocker. The expanded suite subsequently passed 38 tests,
with zero failed and one mandatory ignored, in
`target/windows-private-fs-expanded-live-validation.log`; the separate mandatory
test passed again in `target/windows-private-fs-expanded-mandatory-reparse.log`.
The added cases exercise four synchronized first-create process races, forced
termination of the exact winner and independent reacquisition, canonical/staging
marker-prefix and malformed-residue rejection, and actual NTFS junction refusal
at leaf/root/ancestor positions with the outside sentinel untouched. A losing
first-create contender may conservatively report `IdentityAmbiguous` before
acquiring a lease; the test verifies the canonical full identity and marker
remain unchanged, with no staging residue and successful independent recovery.
Scoped all-targets validation Clippy also passed in
`target/windows-private-fs-expanded-clippy.log`.
These results do not complete the remaining account,
junction/mount, alias, remote-filesystem, filter, or crash-boundary campaign
above, and do not authorize production activation.


The same explicit validation graph passes 19 native Store journal/artifact tests,
including actual child-process ownership fencing, exact Claim/CAS, partial-write
and restart transaction recovery, terminal immutability, and archived artifact
reads/erasure. Its child-only test is invoked by the parent fixture. The newly
selected Windows Store callback-loss/panic fixture also passes. Evidence is in
`target/windows-work-store-native-validation.log` and
`target/windows-work-store-callback-native-validation.log`. These tests exercise
the original journal implementation against the debug adapter. They do not
promote general namespace admission. Windows Work now has a separate fixed
`NativeWorkStorageAnchor` capability and protected SQLite backend, admitted at
the native KnownFolder Profile anchor with immutable held-directory and file
identity/security proofs. This does not expose arbitrary-root namespace
activation or admit extension installation.

The restored real Shell/Store artifact subprocess fixture also passes under this
validation graph: one process publishes the result and exits; a different
process reads the durable result without restoring execution. Its captured
child output is preserved on failure. Evidence is in
`target/windows-work-app-journal-native-validation.log`. The fixture remains
selected on Windows in the ordinary graph. The protected backend subsequently
passed the default-feature Store suite (289 tests), the actual Shell artifact
subprocess, and scoped Store/App Clippy without general namespace validation.
The fresh fixed Work test session is retired only after the owning subprocess
has exited and released its permanent journal fence.

On the development Mac, the updated private-filesystem crate passed 60 unit
tests and 57 integration tests. The Windows x64 default build, explicit
validation build, and instrumented validation build passed cross-target Clippy
for all targets with warnings denied. The default Windows release check passed;
the validation-enabled release check failed with the intended compile guard.
These results include compilation of the Windows test bodies, not their execution.

The final complete `cargo xtask ci` run passed, including both native macOS
extension product variants, repository recovery and all 77 frontend tests.
[Native probe investigation and the earlier fixture timeout](extension-beta-admission.md#local-verification-observations)
are recorded separately; these successful checks do not imply Windows runtime
qualification or completed Beta installation.

The backend parser source remains
`crates/zephium-extension-package/src/public_policy.rs`, SHA-256
`daa44d8c80a650b5ac6b6dfbfa7aff390dabec1c4872d072ee7f8b03eeac907a`.

## WebView2 user-data path spelling evidence

The engine's native WebView2 boundary projects an existing canonical local
directory to ordinary Win32 spelling and prefers a strictly shorter native
GetShortPathNameW alias when available. This does not admit aliases as
filesystem authority: canonical confinement, erasure and environment proofs
remain unchanged. Both directory handles are held with directory-list access
and without delete sharing while exact canonical path, full 128-bit identity
and volume are checked. Malformed output, access/lookup errors, and binding
mismatches fail closed. Only no-shorter output or explicit unsupported native
query results fall back to the already verified ordinary spelling.

On this host, the full retained Work cookie witness previously failed its
unchanged browser-process shutdown proof at canonical native profile/Cookies
lengths of 227/243 UTF-16 units. The same deep protected fixture passed with a
verified shorter API spelling, including full semantic observations, persistent
authenticated cookie, server-authorized read, zero model calls, zero native
resource debt, exact browser-process exit and clean Shell/Store shutdown.
Evidence is in target/windows-work-shutdown-deep-short-alias-pinned.log.
Eight focused path tests pass, including rename refusal under the held handle,
unsupported-query fallback, and unrelated canonical/full-identity rejection
(target/windows-work-native-short-path-production-tests.log).

[Microsoft documents](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getshortpathnamew)
that NTFS does not guarantee short names. No aliases, ACLs, roots or OS settings
were changed. Deep paths on a volume without usable short aliases remain
unqualified; this result does not establish arbitrary-depth WebView2 support
or complete the extension filesystem activation campaign.
## Integrated Windows Work evidence

The fresh final loopback run in
`target/windows-work-all-changed-handoff-fixed.log` passes all 22 functional
checks. This includes actual GET-form Enter submission, allowed script requests
beside the denied automatic document POST, navigation and preview retirement,
persistent authenticated sessions, consent, autosave, and sign-in continuation.
Apps asks once for all three named sites and reads all three with authenticated
cookies and zero model calls; its fixture seeds each isolated Windows origin
through a real HTTP response. Entry preparation now enrolls before native
presence awaits, so already-started sibling queries join one question even when
they exceed the prior gather window; cancellation releases that enrollment.

Changed also exercises the fixture's later-handoff decline: after its sole
approval, a generic model Human request is declined only for the same execution,
attempt and Read step with a strictly newer handoff generation, an empty
decision queue and no undecided running Confirm or Ask. Its original zero-POST
and first-Approved/Failed assertions pass. The earlier isolated run remains
failed evidence in `target/windows-work-changed-followup-native.log`: the model
twice proposed unsupported Select operations against a button, which the exact
capability guard refused before native dispatch.

These are functional results, not an overall clean-run pass. The final run
exits 1 at the Shell shutdown barrier despite zero native resource ledger debt;
the parent subsequently proves `exact_session_retired=true` with
`child_success=false`. The targeted follow-up
`target/windows-work-churn-classified-recovery.log` proves why: the deliberate
churn case retains classified `RecoveryRequired` and `UNKNOWN` debt after the
original native-zero proof succeeds. Existing lifecycle policy intentionally
returns Unclean for that unresolved operation history. Store, native process
proof, private cleanup, and blocker shutdown all pass; no deadline or recovery
verdict was relaxed. The four positive writes separately pass with Clean Shell
shutdown in `target/windows-work-writes-final-integrated.log`.

The traced full repeat `target/windows-work-all-shutdown-traced.log` passes 21
assertions; SPA fails after two unsupported model Scroll proposals with zero
native actions. Its earlier passing result is preserved, and this failed run
is not relabeled a pass. Six applicable recovery tests and strict App Clippy
pass; two explicit recovery-contract tests are macOS/Linux-only and were
inspected as source rather than claimed as Windows test coverage.
This automated synthetic workflow does not establish
manual Human UI qualification or general extension namespace activation.
