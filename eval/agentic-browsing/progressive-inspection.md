# Progressive inspection: deterministic evidence

Date: 2026-09-07. Status: first live inspection run failed safely; generic
correction is deterministic-only, **corrected live qualification pending**.

## Demonstrated problem

The preceding [retained Svelte/Luna workflow](macos-retained-open-objective.md)
mechanically completed a rational overview → `$state` → `$derived` route. Its
final answer was only partially useful: the delivered observation contained an
on-page navigation link for Destructuring, but not the actual below-fold section
body. The model caveated instead of inventing evidence. That is a content-access
gap, not a navigation failure. This implementation supplies no Svelte-specific
selector, destination, body, route, answer predicate or prompt hint.

## Generic capability

The existing `snapshot` tool now provides progressive inspection in discovery:
initial viewport, explicit region, subtree, or bounded surrounding text from an
actually acknowledged ref. A consumed observation checkpoint retires old replay
before one native capture and validates the exact successor before fresh delivery.
Ordinary read remains a projection of already acknowledged data. Both OpenAI and
Anthropic closed schemas/checkpoints are covered; the pre-existing desktop
discovery controller remains Luna/Terra-only, with Anthropic navigation refused.

Initial capture defers up to 24 rendered offscreen heading anchors until after
ordinary visible content, with at most 2 KiB additional heading text inside the
existing total limits. A generic fixture demonstrates: the actual heading anchor
is available, its below-fold sibling body is initially absent, surrounding-text
inspection reveals that body, and heading subtree does not. A TOC link is not
silently followed. Hidden and editable/private heading descendants stay excluded.

## Deterministic acceptance

- Retained Snapshot → fresh scoped delivery → Read → selected Navigate → Extract
  uses one acquisition and the same resource/lease, with exact cleanup. Old page
  marker, tool replay and refs are absent after scope replacement; navigation
  also retires the scoped body while preserving trusted current/prior URL state.
- Surrounding-text content is absent before inspection, delivered in its fresh
  scope, then available to source-backed mapping only through that acknowledgement.
- Unknown refs, wrong-role regions, unsupported frames and oversized text windows
  refuse before native dispatch; synchronous native refusal does not retry.
- Cancellation, human takeover and policy revocation during expansion reconcile
  the original retained callback. A lost callback keeps explicit unresolved debt
  rather than reporting clean native retirement.
- Registry tests reject stale/foreign lease/ref/generation authority, pending
  overlap and stale anchors after synchronous refusal. Native admission requires
  the exact previous snapshot generation. Original rendering-retirement tests
  still require all presentation, semantic callback and watchdog debt to retire
  before delivery.
- Provider-neutral checkpoint tests reject changed configuration, substituted
  predecessor, unsupported/foreign refs, older call, widened scope, wrong root and
  disconnected extra roots. Fresh scoped read rejects the old acknowledgement.
- 200 earlier offscreen headings cannot starve later visible controls/content
  under tight node or text limits. The existing initial fixture remains 2,454
  native wire bytes; one extra heading costs 97 native wire bytes in the targeted
  fixture. These are wire measurements, **not provider token claims**.

The first slice's pinned runtime was 93,187 bytes (92 KiB installation ceiling); the ceiling is
for fixed installed code, not extra page disclosure. Observation, wire, text,
provider input, model turns, account scope, deadline and navigation bounds remain
unchanged. Native presentation is still retired before provider execution.

## Verification

Deterministic gates used for this slice:

- `cargo test -p zephium-agentic --features probe-harness --lib`: 556 passed.
- `cargo test -p zephium-agent-controller --features probe-harness --lib`: 46 passed.
- `cargo test -p zephium-app --features work-execution-probe --lib`: 446 passed,
  one existing ignored test.
- `cargo test -p zephium-engine --features agentic-browser --lib host::work_resource::observation`:
  six passed (including the new current-generation admission test).
- Strict Clippy: agentic, provider-enabled controller, Work-enabled app and
  agentic-enabled engine, all targets; formatting and whitespace checks.
- `check-agent-controller-boundary`, `check-agentic-probe-boundary` (including
  runtime smoke and its negative control), and their focused mutation tests:
  14 controller, 77 probe-boundary and four retained-resource tests passed.

## Exact-composition correction

An independent exact-composition preflight after `a921e3b` found that the retained
qualification's logging task wrapper had not forwarded progressive inspection,
and its result observer rejected Snapshot. Those defects were corrected before
any live launch. Both retained and legacy discovery definition factories now
forward the capability; their actual observers accept Snapshot only alongside
the approved read-only inspection set (Read/Locate), without relaxing effects,
navigation-hop, durable-terminal or current-source checks. Exact-wrapper/observer
and architecture mutation regressions cover this join rather than only the bare
controller task. The original objective, destinations and answer schema are
unchanged.
Verification of that correction: 13 Work-composition tests with both
`retained-product-qualification,discovery-qualification` enabled, seven composition
boundary mutation tests, strict all-target Clippy and the complete agentic probe
boundary check passed. No app or provider was launched for these checks.

## First live inspection result: failed, not qualified

The exact `c4efde5` bundle ran the unchanged public objective with `gpt-5.6-luna`
and `store=true`. Preserved response and `.input_items.json` files are under
`/private/tmp/zephium-retained-svelte-inspection.YyQuHg`. The seven chronological
response IDs are:

1. `resp_0380dd76ddf81a31006a9f1cf33d2887d2b6b8007bfe92f9ca`
2. `resp_057a47c1a69d8077006a9f1cf5d27c87d2bd4a2edf75d3cfee`
3. `resp_08bba544c0e27a86006a9f1cf878cc87d2ab203d7a651d9386`
4. `resp_0d7f34316afc2fcd006a9f1cfb1a6887d29b1d1379b06213d0`
5. `resp_010f0fb6bcea3cef006a9f1cfe65cc87d2920ea8c06d39502d`
6. `resp_09e21a81efe0f51c006a9f1d00b90887d28654d2b439beb782`
7. `resp_07e2db9adcaa258c006a9f1d03a17087d2b5c6b6cb98298682`

The model chose Navigate(`$state`), then alternated Region(`@a21`) and Initial
three times. It exhausted the original turn limit after one navigation; no
answer was produced. The response usage sums to 35,090 input and 436 output
tokens (including reasoning). The launch review recorded 7,744 microUSD and
about 19.2 seconds, with retained-resource and shutdown cleanup satisfied.
These are failed-workflow costs, not successful-task performance.

Two concrete causes are visible in the actual input items:

- The initial `$state` capture had 114 nodes, including the main-region anchor
  and actual offscreen headings. Expanding that main region used depth-first
  traversal and consumed all 128 nodes inside its nested documentation index,
  before reaching the parent region's own article prose.
- Every fresh delivery contained the objective, new semantic snapshot and
  navigation checkpoint, but no inspection progress. Returning to Initial
  recreated the same decision context. Retiring old page evidence was correct;
  forgetting the content-free fact of prior inspections was not useful.

This is not evidence of a navigation-policy or native-lifecycle failure, nor
proof that Luna cannot perform the task. It also is not a successful inspection
qualification merely because the native operations and cleanup were sound.

## Generic correction and deterministic evidence

Region now captures one structural level: its own content plus nested
landmark/document roots as current expandable anchors. Explicit Subtree keeps
recursive semantics. Intentional omissions report `scope_boundary`; a later
actual budget exhaustion takes precedence. No URL, site selector, destination,
section name or answer was added to runtime or objective.

A generic fixture places a 180-link index before parent prose and another
article. Region returns the parent evidence and both expandable roots in five
nodes / 538 native wire bytes. The child region can then be inspected with its
fresh ref. Subtree still reaches its 128-node cap before that prose, and a
three-node Region limit reports `node_limit` rather than hiding exhaustion
behind an intentional scope boundary. Existing initial wire cost remains
2,454 bytes and the targeted heading addition remains 97 bytes; these are
fixture wire measurements, not expected live token savings.

The existing discovery checkpoint now keeps at most eight content-free capture
records within its original 14-KiB bound: scope kind, bounded text-window sizes,
generation, node count and incomplete status. Private node identities are only
lookup hints. A target appears in model-visible history only if matched afresh
to a ref in the current exact observation; otherwise it is null. Prior text,
labels, tool calls, model reasoning and old refs are not replayed. Same-document
Read preserves this history; successful navigation clears it. Original lease,
account, manifest, deadline, model-call and token budgets remain authoritative.

All history bytes are serialized before whole-input measurement and reservation
on the existing structured discovery-provider path. That delivery path currently
supports OpenAI; unsupported provider/accounting combinations explicitly refuse,
matching the pre-existing navigation restriction. This does not claim complete
Anthropic discovery parity. Non-navigation provider-neutral inspection keeps its
existing contract without silently adopting an uncounted host message.

Core regressions check generation mismatch, changed current refs, absent targets,
no old page content and the hard eight-record bound. An application/controller
loopback test reproduces Region → Initial → Region → Initial, then selected
navigation and extraction, proving the history survives each replacement and
disappears at the document transition while one retained acquisition and exact
cleanup are preserved. It is a scripted integration test, **not a claim that a
live model now chooses a better route**. Architecture mutation checks guard
current-ref remapping, history bounds and serialize → measure → reserve order.

The corrected pinned runtime is 93,863 bytes with SHA-256
`2a065d2c21e45eb25a50f9b036807a0cef2f22fc0ae499d903c68bed3e2d9b6e`;
the installation ceiling and all observation/resource limits are unchanged.
Many nested roots or large unstructured prose can still exhaust a scope; this
is hierarchical inspection, not general pagination or hidden-content access.

Corrective-slice verification:

- Core: 558 tests passed with `probe-harness` (fixed local fixture servers require
  loopback permission; the sandbox-only attempt could not bind those sockets).
- Application: 447 passed, one existing ignored, with `work-execution-probe`;
  this includes all 17 retained/navigation regressions.
- Controller: 46 passed with `probe-harness` through local-loopback transport.
- Exact retained/legacy discovery composition: 13 passed with both qualification
  features enabled.
- Architecture mutation tests: 14 controller, 77 probe-boundary and seven
  composition-boundary tests passed.
- Strict all-target Clippy for the agentic/controller/application/composition
  feature graph; runtime smoke, pinned source hash, formatting and whitespace.
- Complete `check-agent-controller-boundary` and `check-agentic-probe-boundary`,
  including the semantic smoke's negative control.

## Remaining live proof

After deterministic review, run the unchanged open Svelte objective with Luna
and diagnostic `store=true`, without revealing the answer or choosing its route.
Review whether the model obtains useful evidence and whether the result actually
answers the objective separately from mechanical success. Record actual tools,
source provenance, tokens/latency and exact native/durable closure. Do not treat
this fixture evidence as that live proof. General hidden/virtualized content,
cross-page evidence synthesis, signed-in actions and Windows remain outside it.
