# Windows extension filesystem validation

Status: 2026-09-10. The Windows private-filesystem implementation is present and
passes Windows cross-target Rust checks, including its tests. Windows linking
and native execution have **not been verified here**. Default
namespace activation still returns `PrimitiveUnavailable` before inspecting the
requested path. The explicit `windows-namespace-validation` feature enables the
adapter only for debug validation; optimized builds reject it at compile time.

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
