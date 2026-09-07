# macOS sealed-tree publication compatibility

The macOS 15 ARM64 hosted native pre-gate in CI run `34084186909` failed at
`rename_noreplace`, errno 13 (`EACCES`), after the exact nested source was
sealed. The same gate passed locally on macOS 27.0 (26A5425a); that local success
did not establish compatibility with macOS 15.

Apple's [XNU 11215.81.4 rename authorization](https://github.com/apple-oss-distributions/xnu/blob/xnu-11215.81.4/bsd/vfs/vfs_subr.c#L8639)
unconditionally requests `KAUTH_VNODE_ADD_SUBDIRECTORY` for a source directory
in the non-swap path, including same-parent rename. That right maps to write
permission. Apple's [rename manual](https://github.com/apple-oss-distributions/xnu/blob/xnu-11215.81.4/bsd/man/man2/rename.2)
also documents the generalized write-disabled-directory restriction. The old
same-parent-only comment therefore misstated the supported kernel contract.

The corrected private-fs transition consumes the existing sealed capability and
holds the original namespace operation lock and no-path-pins admission. Only
the exact original root descriptor becomes owner-writable (`0700`); children
stay at `0500`/`0400`. One native descriptor-relative NOREPLACE rename follows.
The original descriptor is then resealed and synced regardless of rename
success/refusal. No capability leaves until fresh name/identity/mode/parent and
namespace verification succeeds. Post-commit ambiguity is terminal and sticky.
A pre-commit native refusal may return only the freshly verified sealed source.
Quarantine is published before releasing the operation mutex. The transition
grants no group/other access, invokes no external callback, and cannot start
while a namespace-wide verified-path pin is held.

The native pre-nextest diagnostic still exercises the sealed nested fixture and
the same NOREPLACE syscall seam. It now exercises the required root transition
and reports operation plus raw errno for its frontiers, checks every final mode,
and separately proves an existing nonempty destination is unchanged. There is
no retry, fallback or relaxed success assertion. Private-fs fault tests cover
root-write commitment, rename commitment reported as failure, reseal commitment,
reseal sync, native refusal and reseal refusal. Reopening a simulated `0700`
residue cannot produce sealed authority. Existing path-pin, collision, identity
substitution, exact parent-sync and repository rehash tests remain in force.

The higher repository protocol still writes its package record last and freshly
rehashes final trees. Incomplete objects cannot pass sealed admission; a crash
does not imply a receipt. No second journal or automatic unseal/retry was added.

Hosted macOS 15 success for this correction remains pending. Offline local
results must not be reported as that hosted proof.
