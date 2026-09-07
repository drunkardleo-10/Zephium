# Progressive inspection: deterministic evidence

Date: 2026-09-07. Status: deterministic implementation; **live qualification pending**.

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

The pinned runtime is 93,187 bytes (92 KiB installation ceiling); the ceiling is
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

## Remaining proof

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

After deterministic review, run the unchanged open Svelte objective with Luna
and diagnostic `store=true`, without revealing the answer or choosing its route.
Review whether the model obtains useful evidence and whether the result actually
answers the objective separately from mechanical success. Record actual tools,
source provenance, tokens/latency and exact native/durable closure. Do not treat
this fixture evidence as that live proof. General hidden/virtualized content,
cross-page evidence synthesis, signed-in actions and Windows remain outside it.
