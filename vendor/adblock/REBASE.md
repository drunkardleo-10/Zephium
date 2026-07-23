# adblock-rust fork update procedure

Zephium treats this fork as a synchronous browser security component, not as a
source copy that can be refreshed by changing a version string. Every update
must review the complete upstream range and explicitly preserve, replace, or
retire every patch set in `FORK.toml`.

## 1. Freeze and verify the inputs

Work from a dedicated branch based on a clean, immutable Zephium commit.
Record:

- the current upstream tag, commit, tree, and all local patch sets;
- the proposed upstream tag, full commit SHA, and Git tree;
- upstream release notes, security notices, parser/serialization changes, and
  dependency changes;
- the reviewer and date for the parser, runtime matcher, WebKit conversion,
  and native-adapter boundaries.

Fetch `https://github.com/brave/adblock-rust` into a separate review checkout.
Verify that the proposed tag resolves to the recorded commit and inspect the
exact Git object. Do not use a moving branch, an unverified archive, or the
crates.io version alone as the source of truth. If provenance is ambiguous,
stop the update.

## 2. Preserve the complete delta

Before replacing source, retain two recursive, binary-capable diffs with the
review evidence:

1. old clean upstream versus the current `vendor/adblock` source and tests,
   excluding only Zephium provenance files and generated build output; and
2. old clean upstream versus new clean upstream, with rename detection
   disabled.

Also inventory additions and deletions in manifests, features, build scripts,
unsafe code, parsers, matchers, serialization, regex handling, domain
resolution, resource assembly, and test infrastructure. An upstream change outside the
current local patch paths is still in scope when it can change accepted syntax,
rule priority, exception behavior, allocation, locking, or artifact bytes.

Regenerate `UPSTREAM_FILES.toml` from the exact proposed Git tree. Every
selected source, bench, test, and support-file row must carry both the
upstream Git blob identity and a SHA-256 of the raw file bytes. Preserve
deterministic, path-sorted aggregate digests as a review aid, but do not
replace the per-file inventory with a single digest. The gate must be able to
name the exact missing, added, mutated, undeclared, or stale-patch path.

The upstream inventory is not a substitute for reviewing the resulting local
patch. It pins upstream bytes and the complete divergent-path union; the
landed Zephium commit pins the current patched bytes. Review that commit's
entire patch content rather than adding self-authored “expected patched
output” hashes that could conceal the implementation diff.

## 3. Decide every local patch

Create a review table with one row per `[[patch_sets]]` entry and one of:

- **reapplied** — the invariant remains local and has equivalent tests;
- **reworked** — the seam changed, with the replacement design and tests;
- **retired upstream** — upstream now enforces the same or stronger invariant,
  with exact source and tests cited;
- **removed intentionally** — Zephium no longer uses the capability, with all
  call sites and documentation removed.

“Applied cleanly” is not a security result. Review these properties even when
the textual patch has no conflict:

- transactional regex preparation and every aggregate/per-regex budget;
- zero mutation, compilation, eviction, I/O, blocking waits, or callbacks from
  a frozen request matcher;
- fail-open behavior for malformed native values and lock contention;
- source-independent request construction without source-host or party data;
- conservative interaction among generic rules, scoped exceptions,
  `$important`, `$badfilter`, protocol, request-type, and method predicates;
- token/bucket coverage for attribution-sensitive exceptions, including
  wildcard, regex, and zero-token rules;
- parser admission for redirect, scriptlet, CSP, removeparam, generic-hide,
  tag, cosmetic, and newly introduced mutation syntax;
- WebKit conversion losses, ordered exception semantics, modern resource
  types, canonical JSON, and exact coverage accounting;
- every unbounded allocation, panic, lock, global cache, and serialization
  compatibility edge reachable from a filter source or native request.

## 4. Rebuild from the new upstream tree

Begin with the exact new upstream source, then reapply the reviewed patches.
Do not selectively copy new files over the old fork. Preserve Zephium
provenance documents only after the implementation diff is understood.

Import only the selected source, tests, and benches. Do not import upstream
`data/` payloads. Re-express required parser and matcher cases as minimal
Zephium-authored rules under reserved `.invalid` domains, and use generated
corpora for deterministic scale coverage. Keep any licensed real-list
performance corpus outside the public source tree and production catalog.

After applying local changes, run the provenance gate before reviewing the
implementation diff. Its computed divergence must equal the union of
`patch_sets.paths`: every changed imported path is declared, and every
declared path still differs from upstream. A new patch, a test-only
compatibility adjustment, and a removed upstreamed patch all require an
explicit `FORK.toml` update.

Reconcile the source manifest, normalized standalone fork manifest, feature
set, root patch, and all Zephium call sites. Review both the fork-local
`Cargo.lock` and root application `Cargo.lock` diffs deliberately. No
unreviewed Git dependency, alternate registry, source replacement, build
script, default feature, resource payload, or runtime downloader may enter the
graph.

Resolve the standalone lock after the application lock is frozen. Every
registry package reachable from the union of Zephium's shipped adblock feature
graphs must have the same version, source, and checksum in both locks.
Test-only packages may exist only in the standalone graph and must remain
pinned. Do not hide a production mismatch by validating only the fork's
default features.

Any change to filter bucket layout, FlatBuffers schema, hashing, parsing
semantics, or serialized bytes requires an explicit compatibility decision.
Increment the fork data version for an incompatible format, prove the previous
version is rejected, and regenerate expected hashes only after reviewing why
every value changed. Never reinterpret an old artifact under new semantics.

Update `FORK.toml`, `UPSTREAM.md`, and this procedure only after the code review
is complete.

## 5. Validate independently and through Zephium

At minimum, run:

```sh
cargo xtask check-security-fork-locks
cargo fmt --manifest-path vendor/adblock/Cargo.toml -- --check
cargo check --manifest-path vendor/adblock/Cargo.toml --locked --lib --no-default-features --features full-regex-handling
cargo clippy --manifest-path vendor/adblock/Cargo.toml --locked --lib --tests --no-default-features --features full-regex-handling -- -D warnings
cargo test --manifest-path vendor/adblock/Cargo.toml --locked --lib --test fork_contract --no-default-features --features full-regex-handling
cargo check --manifest-path vendor/adblock/Cargo.toml --locked --lib --no-default-features --features full-regex-handling,content-blocking
cargo clippy --manifest-path vendor/adblock/Cargo.toml --locked --lib --tests --no-default-features --features full-regex-handling,content-blocking -- -D warnings
cargo test --manifest-path vendor/adblock/Cargo.toml --locked --lib --test fork_contract --no-default-features --features full-regex-handling,content-blocking
cargo check --manifest-path vendor/adblock/Cargo.toml --locked --lib --no-default-features --features full-regex-handling,embedded-domain-resolver
cargo clippy --manifest-path vendor/adblock/Cargo.toml --locked --lib --tests --no-default-features --features full-regex-handling,embedded-domain-resolver -- -D warnings
cargo test --manifest-path vendor/adblock/Cargo.toml --locked --lib --test fork_contract --no-default-features --features full-regex-handling,embedded-domain-resolver
cargo test --manifest-path vendor/adblock/Cargo.toml --locked --lib --test fork_contract
cargo deny --manifest-path vendor/adblock/Cargo.toml --config deny.toml --locked check -A advisory-not-detected
cargo xtask check-blocker-security-fork
cargo fmt --all -- --check
cargo check --workspace --locked --all-targets
```

The first and second fork graphs above are the exact shipped Windows and
WebKit graphs. They intentionally omit default features and `single-thread`;
their full library tests are mandatory, not substitutes for one default
feature test. Check, test, and run test-target Clippy for `zephium-blocker` in
runtime-only, runtime-exact, WebKit-only, runtime+WebKit, and
runtime-exact+WebKit feature configurations. The explicit `fork_contract`
target provides an ordinary-CI sentinel over provenance-locked,
self-authored exact-attribution vectors and Zephium's unknown-attribution
policy. During each rebase, separately run exact-attribution behavior against
the clean old and proposed upstream trees and retain that differential result
with the review evidence; ordinary CI must not fetch a moving external tree.
Cover:

- positive and negative domains and first/third-party predicates;
- ordinary exceptions and `$important`;
- HTTP/HTTPS, methods, and every admitted native request type;
- wildcard, optimized, full-regex, and zero-token buckets;
- `$badfilter` before and after serialization;
- malformed, oversized, and contended matcher inputs;
- serialization round trips and rejection of all prior incompatible versions.

Use representative licensed test lists to record compilation latency, peak
RSS, retained matcher/JSON size, and request-match distributions. Treat
regressions in the synchronous callback tail, allocation count, or lock
contention as release blockers.

Source and unit tests do not prove native enforcement. Before an enabled
release, install the packaged candidate on real supported Windows, macOS, and
Fedora hosts. Verify blocking, exceptions, redirects/navigation overlap,
workers, service workers, iframes, policy replacement, profile separation,
startup recovery, cache corruption, shutdown, and the documented fail-open
paths. Repeat the endurance and battery measurements.

## 6. Review and recovery

At least two maintainers review the complete update. One must focus on filter
semantics and unknown attribution; another must focus on native installation,
resource bounds, and supply-chain changes. Stable enablement additionally
requires the source-package, native-cache, packaged-test, and performance
gates in `FORK.toml`.

Land an update only from a fully green immutable commit. Attach the upstream
range, patch-disposition table, dependency review, differential results, and
native evidence to the pull request. If an invariant, license, serialization
boundary, or native result is ambiguous, keep the previous reviewed fork.
Rollback restores the complete previous Zephium commit together with both the
fork-local and root application lockfiles; do not mix old matcher source with
new serialized artifacts or dependencies.
