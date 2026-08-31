# M3 semantic runtime

Status: bounded semantic identity, snapshot, opaque-reference, and hostile wire
decoder implemented; fixed isolated-world page program, native installation,
multi-frame assembly, progressive scopes, diffs, and compact model encoding
pending.

This evidence records deterministic Rust contract properties only. It does not
claim that arbitrary pages have been instrumented or observed on either
platform.

## Implemented boundary

- One frame payload is capped at 256 KiB, 512 nodes, depth 32, 128 KiB total
  page-derived text, 512 bytes per accessible name, 4 KiB per visible-text
  segment, and 1 KiB per safe value summary. A complete observation can later
  assemble at most sixteen supported frame snapshots.
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
and wire limits, hostile debug strings, and the absence of page content from
diagnostics.

## Remaining M3 work

1. add bounded progressive observation request/scope contracts and multi-frame
   assembly with explicit unsupported boundaries;
2. add deterministic compact model encoding and measure actual token counts;
3. add snapshot acknowledgement, stable-id diffing, and fresh-snapshot fallback
   when a confident diff cannot be formed;
4. implement and freeze the immutable isolated-world runtime plus closed
   invocation vocabulary after the native M2 adapter provides exact world,
   frame, navigation, cancellation, and teardown joins;
5. add fixture/runtime hostile tests for page-world bridge access, spoofing,
   open/closed shadow roots, frame replacement, stale nodes, collisions,
   mutation pressure, redaction, and release-build exclusion.
