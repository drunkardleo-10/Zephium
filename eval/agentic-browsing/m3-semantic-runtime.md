# M3 semantic runtime

Status: bounded semantic identity, snapshot, opaque-reference, hostile wire
decoder, progressive scopes, and deterministic multi-frame assembly
implemented; fixed isolated-world page program, native installation, diffs,
and compact model encoding pending.

This evidence records deterministic Rust contract properties only. It does not
claim that arbitrary pages have been instrumented or observed on either
platform.

## Implemented boundary

- One frame payload is capped at 256 KiB, 512 nodes, depth 32, 128 KiB total
  page-derived text, 512 bytes per accessible name, 4 KiB per visible-text
  segment, and 1 KiB per safe value summary. A complete observation can
  assemble at most sixteen supported frame snapshots.
- One complete observation is capped at 512 nodes, 256 KiB retained text, and
  sixteen frames even though individual frame decoders have independent
  ceilings. Each request selects smaller nonzero ceilings; the conservative
  initial filtered budget is 128 nodes, 16 KiB, and four frames. This is a
  safety bound, not yet a claim that compact encoding meets the 2,000-token
  target.
- Native code supplies context/document authority, frame id and generation,
  canonical origin, same/cross-origin trust class, invocation id, and expected
  snapshot generation out of band. Hostile bytes can only echo the invocation
  and generation; mismatch is rejected.
- The version-1 JSON schema denies unknown and duplicate struct fields and has
  only compact allowlisted roles, state/operation bitsets, safe value variants,
  integer geometry, parent indices, internal node keys, sensitivity, and a
  typed completeness/truncation result. It cannot carry HTML, selectors,
  attributes, scripts, event listeners, native handles, cookies, headers,
  storage, profile data, or platform errors.
- Internal node keys are nonzero, unique, private to the runtime, and absent
  from the public/model contract. Parent indices must refer backward; depth is
  derived and bounded. Unknown bits, disabled nodes with operations,
  role/value mismatches, role/operation mismatches, invalid geometry, control
  characters, bidi overrides/isolates, and zero-width format characters fail
  closed.
- Page text, canonical origins, exact geometry, and primitive values have
  redacted `Debug` implementations. Snapshot/node diagnostics contain counts,
  roles, labels, presence bits, and byte lengths but never content, profile
  identity, URLs, coordinates, or internal node values.
- Password values are always redacted even when the payload claims public
  sensitivity. Credential-labelled form controls and recognized token forms
  are upgraded to `Secret`; secret visible text and text values become a fixed
  redaction marker. Raw secret byte lengths count against input budgets but are
  not retained in the public snapshot byte count.
- Every retained node receives one model reference with canonical spelling
  (`@aN`) so non-interactive nodes can anchor progressive observation without
  becoming action-authorized. References cannot express CSS, XPath, or
  property paths. Each capability binds exact context/run/profile,
  context generation, navigation epoch, frame id/generation, cancellation
  generation, canonical origin/trust boundary, snapshot generation, private
  node key, and closed operation inventory. Resolution fails on an unknown id,
  any stale frame/snapshot join, or an operation not issued for that node.
- The initial scope is a fixed filtered view of the viewport, dialogs, active
  element, meaningful landmarks, and interactive controls. Expansion is a
  closed region, subtree, table, frame, or bounded surrounding-text request
  anchored by a capability from the exact preceding observation. Chains are
  capped at eight expansions, cannot reuse any request identity in the chain,
  and require the anchored frame snapshot generation to advance exactly once.
  Surrounding text is capped at 8 KiB before any smaller request budget.
- Frame callbacks may arrive in any order, but final snapshots and global
  `@aN` references are deterministically rebased in document-boundary preorder.
  Each child must share the exact context/document/cancellation join, use a
  unique invocation and frame identity, and carry same/cross-origin trust that
  agrees with canonical parent/child origins. Every retained boundary must be
  explicitly observed, deferred for a typed budget/scope reason, or marked
  unsupported for a typed safe-runtime reason; omission is a failure.

The crate remains a zero-idle-cost functional core: decoding happens only for
an admitted observation and creates no timer, thread, page, queue, or worker.

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo test --locked -p zephium-agentic --features probe-harness
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
cargo clippy --locked -p zephium-agentic --all-targets --features probe-harness -- -D warnings
cargo xtask check-agentic-probe-boundary
```

Tests cover canonical/redacted origins and page text, main/child-frame joins,
unsupported frames, canonical non-selector references, exact reference
resolution, duplicate/unknown operation and state inventories, geometry and
text limits, valid deterministic decoding, unknown fields, invocation and
generation substitution, password/token redaction, duplicate internal ids,
forward/self parents, depth, role/state/value/operation contradictions, node
and wire limits, hostile debug strings, aggregate observation budgets,
out-of-order sibling-frame completion, global reference rebasing, mandatory
frame dispositions, frame trust/invocation refusal without partial mutation,
exact progressive chains, stale anchor generations, missing anchor frames,
scope/role compatibility, and the absence of page content from diagnostics.

## Remaining M3 work

1. add deterministic compact model encoding and measure actual token counts;
2. add snapshot acknowledgement, stable-id diffing, and fresh-snapshot fallback
   when a confident diff cannot be formed;
3. implement and freeze the immutable isolated-world runtime plus closed
   invocation vocabulary after the native M2 adapter provides exact world,
   frame, navigation, cancellation, and teardown joins;
4. add fixture/runtime hostile tests for page-world bridge access, spoofing,
   open/closed shadow roots, frame replacement, stale nodes, collisions,
   mutation pressure, redaction, and release-build exclusion.
