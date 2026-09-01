# M3 semantic runtime

Status: bounded semantic identity, snapshot, opaque-reference, hostile wire
decoder, progressive scopes, deterministic multi-frame assembly, compact model
encoding/token-admission ports, acknowledged stable-id diffs, compact diff
token admission with exact baseline/current delivery proof, a provider-neutral
one-shot diff-continuation authority seam, and the immutable bounded
page-projection program with its fixed reply-channel pull protocol implemented.
The macOS owned-context adapter now installs and lifecycle-binds that program
through a fixed content-world reply handler and exposes its single-flight result
through the bounded native context port. Provider-specific stateless transcript
encoders remain disabled. A mechanically release-excluded
hidden/ephemeral/loopback-only native qualifier is implemented but has not been
executed; its named-device result and actual pinned provider token measurements
remain pending.

This evidence records deterministic Rust contracts plus static and synthetic
DOM execution of the fixed program. It does not claim that arbitrary pages
have been instrumented or observed on either platform.

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
  typed 1–6 heading levels, integer geometry, parent indices, internal node
  keys, sensitivity, and a typed completeness/truncation result. It cannot
  carry HTML, selectors,
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
- Deterministic `ZSEM1` output uses one bounded writer and observation-global
  references, frame aliases, parent relationships, roles, state/operation
  inventories, trust/sensitivity/freshness labels, typed frame dispositions,
  optional action geometry, and quoted semantic strings. The header marks all
  page content untrusted; quoting escapes delimiters, backslashes, controls,
  and Unicode line separators. The format contains no internal node key,
  context/profile identity, selector, HTML, script, attribute, native handle,
  cookie, storage value, or platform error.
- Encoded content remains private and has redacted `Debug` output until an
  explicit trusted tokenizer port returns a bounded measurement for the exact
  already-selected tokenizer revision. Exact budgets reject provider estimates
  and conservative bounds as well as revision mismatches, unavailable counters,
  and over-limit counts before a model payload can be obtained. Separate exact
  and provider-estimate initial budgets both cap the encoding at 32 KiB and
  2,000 measured tokens; the byte cap is not represented as a provider-neutral
  token estimate.
- The port distinction follows current primary contracts. The
  [OpenAI Responses input-token endpoint](https://developers.openai.com/api/reference/typescript/resources/responses/subresources/input_tokens/methods/count)
  accepts a model and the structured response inputs and returns
  `input_tokens`. The
  [Anthropic token-counting endpoint](https://platform.claude.com/docs/en/build-with-claude/token-counting)
  accepts the same structured message inputs, but Anthropic explicitly states
  that its result is an estimate and actual message usage may differ slightly.
  Therefore Anthropic preflight is typed `ProviderEstimate`; it cannot satisfy
  an exact qualification gate, which must use a pinned exact local counter or
  observed provider usage from the eventual model call.
- The counter trait itself grants no network or credential access. A
  provider-backed counter must enter through the same selected-provider,
  secret-injection, page-data disclosure, cancellation, and deadline authority
  as the eventual model request; semantic encoding alone cannot send content.
- A semantic diff baseline can be minted only by consuming a token-admitted
  payload with a terminal `Committed` model-delivery settlement. Refused and
  cancelled transports consume the payload without creating baseline
  authority. The acknowledgement retains exact observation/context coordinates
  and a SHA-256 digest over the complete bounded semantics, internal stable
  keys, frame joins, generations, scope, and request metadata; digest and page
  content remain private and diagnostics-redacted.
- Diffs are capped at 512 entries process-wide and 64 entries by the default
  action budget. They require the exact acknowledged baseline, unchanged
  context and logical scope, complete snapshots, an identical ordered frame
  cohort, distinct invocations, exactly consecutive per-frame snapshot
  generations, and unchanged stable-key-addressed frame-boundary dispositions.
  Stable-key role reuse, total stable-identity loss, request-id reuse, or any
  failed premise returns a typed fresh-snapshot reason instead of a partial
  delta.
- Removed references are a separate non-actionable type with an explicit
  `old:@aN` model spelling and no parser or resolver. Current references are
  taken only from the new observation. Complete deltas report added, removed,
  changed, moved, and changed-and-moved nodes; changed fields use a closed
  bitset, movements carry old/new parent and sibling positions, and every
  current node carries its current parent/ordinal plus bounded current
  semantics. Results are deterministic in removed document order followed by
  current frame/node order and have content-free aggregate metrics. Movement
  detection compares relative order among shared stable siblings, so a simple
  insertion or removal does not create a cascade of false move entries.
- Snapshot-local references for otherwise unchanged nodes are never silently
  left stale. A bounded old-to-current rebase record is emitted whenever an
  unchanged stable node's `@aN` shifts, and those records count against the
  same delta ceiling. Excessive rebase fan-out therefore causes a typed fresh
  snapshot rather than an incomplete reference map.
- Deterministic `ZDIFF1` output carries the untrusted-content label, exact
  previous/current observation generations, frame-boundary preorder with
  canonical origin/trust and consecutive snapshot freshness, changed-field
  values, tree movement, add/remove records, and required unchanged-reference
  rebases. It contains no internal stable key, selector, HTML, script, native
  handle, profile identity, or provider secret. Optional-field removal and
  empty state/operation inventories have explicit unambiguous spellings.
- Diff bytes remain private until the selected tokenizer revision admits them
  under the same exact/provider-estimate quality rules as full observations.
  Normal exact and provider-estimate action budgets cap encoded bytes at 16 KiB
  and measured tokens at 200. Each admitted payload and committed-delivery
  receipt is bound to a domain-separated digest of both the exact acknowledged
  baseline fingerprint and exact current fingerprint; a different baseline
  with reused public observation/generation coordinates cannot substitute its
  payload or receipt. Committed delivery acknowledges the exact current
  observation for the next baseline; refusal or cancellation cannot. Digests
  and page content remain private and diagnostics-redacted. This is the
  production admission seam, not an actual provider/tokenizer measurement
  claim.
- Committing a full-observation provider request now mints one move-only,
  memory-only continuation seed beside the immutable request and usage
  authority. Bounded reads do not. The transport destroys this seed for every
  pre/post-dispatch cancellation, transport or protocol failure, terminal call
  mismatch, non-tool or mixed text/reasoning output, or tool count other than
  exactly one. Policy and trusted-pricing settlement remain independently
  mandatory even when the caller elects to retain the seed.
- Each decoded provider tool call now carries its exact immutable model-call
  lineage plus the provider's bounded tool-call correlation, closed tool kind,
  and already-bounded original arguments. Only a tool-only assistant turn is
  eligible; text or reasoning alongside the tool requires additional replay
  state and therefore falls back. Arguments are moved into the call rather
  than copied. All raw fields remain private, move-only, and
  diagnostics-redacted. A seed can join only the matching completed one-tool
  terminal from that same call and provider-specific shape; equal-length tool
  output from another response cannot substitute.
- The resulting continuation binds once to a strictly newer call in the same
  manifest, plan lease, and node; identical provider/model/tokenizer/pricing
  and stream bounds; the exact acknowledged baseline digest and coordinates;
  and the exact token-admitted semantic-diff payload. It grants neither
  browser-operation nor provider-call authority and exposes no generic JSON,
  DOM, selector, script, or transport payload.
- Provider-specific transcript encoding is deliberately still absent. OpenAI
  stateless `store: false` continuation requires replaying the prior response
  output items and matching `function_call_output` correlation, including any
  encrypted reasoning items required by the selected reasoning model
  ([conversation state](https://developers.openai.com/api/docs/guides/conversation-state),
  [function calling](https://developers.openai.com/api/docs/guides/function-calling)).
  Anthropic requires the prior assistant `tool_use` block followed immediately
  by a user `tool_result` block with matching id
  ([tool-use flow](https://platform.claude.com/docs/en/agents-and-tools/tool-use/how-tool-use-works),
  [tool-result handling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls)).
  Zephium therefore keeps remote storage disabled and continues to require a
  fresh full observation until fixed provider codecs consume the bound value;
  no raw provider response is logged or persisted.
- The native-to-isolated-world request is a separate 2 KiB maximum closed JSON
  grammar containing only protocol/invocation/snapshot generations, one fixed
  initial/region/subtree/table/frame/surrounding-text scope, an internal stable
  anchor where required, and hard per-frame budgets. It has no arbitrary
  string, URL, selector, script, property path, model value, page text, native
  handle, or provider field; its `Debug` output redacts the complete payload.
- Per-frame runtime budgets are bounded by both process-wide ceilings and the
  aggregate observation request: at most 512 retained nodes, 128 KiB retained
  text, 256 KiB response wire bytes, and 32,768 inspected DOM/open-shadow
  nodes. The conservative initial invocation requests 128 nodes, 16 KiB text,
  64 KiB wire, and 16,384 inspected nodes. Anchored requests must target the
  exact anchor frame/document and advance exactly one snapshot generation.
- Runtime results decode only through the invocation's out-of-band frame,
  invocation, generation, and smaller response-wire budget. The only non-wire
  responses are seven fixed detail-free `E1` fault codes (invalid request,
  busy, missing anchor, identity exhaustion, unsupported scope, output limit,
  or internal invariant); unknown/page-supplied fault detail is rejected.
- The production document-start asset is 51,219 ASCII bytes under a 64 KiB hard
  source ceiling. Rust owns its exact bytes, sole private-world global name,
  and pinned SHA-256 digest
  `f8adcdd636063dcac5e7cc9dcd6bbacc30df6d3dfbb1075b8ce6b1114951bb69`.
  The adapter receives an opaque program object whose diagnostics redact both
  source and digest; there is no source concatenation or dynamic script input.
- The program installs one frozen, non-enumerable, non-writable,
  non-configurable object with one `invoke` method. When its exact native
  handler exists in the same isolated world, it retains one dormant Promise
  waiting for a request; absent that handler it performs no transport work.
  It creates no observer, timer, event listener, worker, network/storage
  channel, page-world message channel, or DOM mutation path.
  A source gate rejects generic evaluation, selectors, HTML serialization,
  cookies/storage, network APIs, event synthesis, focus/click methods, and
  timer/animation/task scheduling surfaces; only the native reply Promise can
  resume its bounded pull loop.
- The only native transport spelling is the fixed
  `webkit.messageHandlers.zephiumSemanticRuntimeV1.postMessage` function that
  WebKit installs in the selected content world. One exact `P1` pull may wait
  without polling; Rust can reply only with the already-closed 2 KiB request
  grammar. One `R1:` result carries at most the invocation's 256 KiB wire
  ceiling and must receive exact `A1` acknowledgement. `S1` retires a document
  and `X1` reports exhaustion after 4,096 completed invocations. There is no
  arbitrary handler name, native method, page-provided request, or eval route.
  Apple's public contract says the handler function is scoped to its named
  `WKContentWorld` and its reply returns a Promise; current WebKit implements
  the reply as an asynchronous completion retained until reply or finalization
  ([Apple](https://developer.apple.com/documentation/webkit/wkusercontentcontroller/addscriptmessagehandler%28_%3Acontentworld%3Aname%3A%29?language=objc),
  [WebKit](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKUserContentController.mm)).
- Request parsing accepts only ASCII and the exact closed field inventory.
  Invocation, snapshot, and stable-node identities must be positive integers at
  or below JavaScript's exact integer ceiling (`2^53 - 1`); Rust rejects larger
  authority before dispatch. Stable identities use a `WeakMap` plus a bounded
  strong reverse index and mint only for retained semantic nodes. The reverse
  index is required to preserve wrapper/node identity across invocations;
  `WeakRef` guarantees current-turn read consistency, not survival across later
  turns. Every invocation synchronously drops disconnected/wrong-document
  entries and entries older than the immediately preceding snapshot generation.
  Since both the prior and current snapshots are capped at 512 nodes, the
  reverse index is capped at 2,048 entries with explicit headroom; capacity
  fails with a content-free identity-exhaustion code rather than retaining an
  unbounded DOM.
- One synchronous single-flight traversal is bounded by the admitted retained
  node/text/wire ceilings and a separate DOM/open-shadow inspection ceiling.
  Semantic depth, retained-node, retained-text, inspection, scope-boundary,
  unsupported-frame, and response-wire truncation remain distinct typed
  snapshot states. UTF-8 output size is counted before return; a parent-valid
  suffix is omitted under `wire_limit`, and even the minimum result refuses
  with `output_limit` if it cannot fit.
- The initial projection is viewport-filtered while retaining document,
  meaningful landmark, dialog, and focused-element semantics. Closed anchored
  region, subtree, table, frame-boundary, and surrounding-text requests resolve
  only a still-connected stable node from the exact document. The program
  descends open shadow roots but never obtains or guesses a closed root;
  cross-frame content remains a separate native-attested frame invocation and
  observation-assembly responsibility.
- DOM methods and Web IDL getters used by traversal are captured at document
  start rather than read from hostile element properties. The allowlist maps
  fixed HTML/ARIA roles, native/ARIA state, compatible operation classes,
  quantized geometry, labels, and safe primitive value summaries. Password and
  credential-labelled input values are never read into a returned payload;
  recognized token forms are replaced before serialization, with the hostile
  Rust decoder retaining its independent defense.
- Accessible-name extraction follows the standards priority for bounded
  `aria-labelledby`, `aria-label`, native labels, host-language alternatives,
  and name-from-content, but this implementation does not claim full user-agent
  accessibility-tree parity. The ordering is grounded in the
  [W3C Accessible Name and Description Computation 1.2](https://www.w3.org/TR/accname-1.2/)
  and [WAI-ARIA 1.2](https://www.w3.org/TR/wai-aria/). The rejected weak-only
  reverse-index alternative follows from the deliberately narrow current-turn
  liveness guarantee in
  [ECMAScript WeakRef](https://tc39.es/ecma262/2023/multipage/managing-memory.html#sec-weak-ref-objects).
- The macOS adapter creates the selected durable or ephemeral data-store
  configuration before `WKWebView` construction, registers exactly one
  `WKScriptMessageHandlerWithReply` in one private world, and installs the
  pinned program at document start for the main frame only. Before every
  authorized native load it removes the prior handler and script, retires that
  world epoch, and installs the same immutable bytes in a fresh uniquely named
  world. Only the active world, handler, and script are retained by Zephium.
  Every inbound message rechecks that active-world pointer plus the exact
  controller, handler name, web view, frame web view, main-frame bit,
  Objective-C string type, UTF-16 ceiling, and UTF-8 ceiling before allocating
  Rust text. It never calls `evaluateJavaScript`, `callAsyncJavaScript`, a
  page-world function, or a generic bridge.
- World rotation is a correctness boundary, not namespace decoration.
  WebKit internally attaches a document identifier to script-message frame
  data but public `WKFrameInfo` explicitly is transient and does not uniquely
  identify a frame across callbacks. WebKit also retains the posting JavaScript
  context until an asynchronous reply returns. Therefore a payload challenge
  in one fixed world cannot exclude a callback from the replaced document.
  Rotating the public content world before load makes an already-delivered old
  callback pointer-distinct and fail-closed without dynamic JavaScript or SPI.
  The process-local world-name counter is checked and nonwrapping; Apple states
  that a named world is reused only while that world instance remains alive,
  and the adapter retains only its current epoch.
  This follows the current WebKit
  [`frameInfoWithDocumentID` and async-reply implementation](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/WebProcess/UserContent/WebUserContentController.cpp#L2184-L2245)
  and Apple's
  [`WKFrameInfo` contract](https://developer.apple.com/documentation/webkit/wkframeinfo).
- The handler retains at most one dormant pull, one invocation, one completion,
  and two reply actions. Loading, ready, renderer-lost, exhaustion-notice,
  exhausted, failed, and retired phases are explicit. Navigation stops prior
  authority before native load, holds the new document's early pull until the
  exact Wry commit, and never restores semantic authority after provisional
  failure. Renderer loss,
  cancellation, document replacement, close, and shutdown each settle the
  retained task with a closed content-free failure.
- The immutable program refuses every invocation while the browser-derived
  `Document.readyState` is still `loading`. Only `interactive` or `complete`
  can produce a snapshot labelled complete/truncated; an early committed
  navigation therefore returns the fixed content-free `document_loading`
  fault instead of misrepresenting a partially parsed tree. The getter is
  captured at document start in the isolated world, and the bounded native
  channel treats this typed runtime refusal as recoverable without weakening
  transport or generation checks.
- Every admitted macOS invocation retains one cancel-on-drop 15-second main
  queue watchdog. The timeout matches the exact invocation identity, settles
  its task once, and permanently fails that document channel so a late result
  cannot become authority for later work.
- The shared 16-task native ingress now accepts the already-encoded move-only
  invocation without adding another queue or worker. The host revalidates the
  exact context join, Observe capability, committed main-frame origin/trust,
  monotonically increasing invocation identity, consecutive snapshot
  generation, and absence of lifecycle work before dispatch. Resource audits
  count the single-flight invocation as an operation and fail closed if native
  channel state is internally contradictory.

The crate remains a zero-idle-cost functional core: decoding happens only for
an admitted observation and creates no timer, thread, page, queue, or worker.

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo test --locked -p zephium-agentic --features probe-harness
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
cargo clippy --locked -p zephium-agentic --all-targets --features probe-harness -- -D warnings
cargo xtask check-agentic-probe-boundary
node --check crates/zephium-agentic/assets/semantic-runtime-v1.js
node eval/agentic-browsing/semantic-runtime-smoke-v1.js
cargo check --locked -p zephium-engine \
  --features native-agentic-semantic-probe \
  --bin macos-agentic-semantic-probe
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
scope/role compatibility, deterministic compact escaping, model-line spoofing,
heading-level compatibility, byte refusal without partial output, exact token
quality/revision/limit admission, and the absence of page content from
diagnostics. Diff coverage includes commit-only acknowledgement, exact-content
digest binding, current-reference rebasing, non-actionable retired references,
field changes, sibling moves, add/remove ordering, empty deltas, context and
generation changes, truncation, stable-key role reuse/loss, entry ceilings,
unchanged-reference shifts and fan-out limits, compact diff determinism and
escaping, current frame freshness, byte refusal without partial output,
exact/estimated quality and revision admission, the 200-token target gate,
commit-only next-baseline acknowledgement, exact baseline/current payload and
receipt substitution refusal even when public coordinates match, and
fresh-snapshot fallback without leaked page content. Provider-continuation
coverage proves commit-only seed construction, destruction on non-tool or
mixed-output terminals, exact tool-only one-tool transport retention,
mandatory pricing settlement, same-call/provider-shape joins for both
providers, cross-response substitution refusal, exact
lineage/baseline/payload binding, bounded identity refusal, single-copy
argument ownership, and content-free diagnostics. Runtime-invocation tests
cover the sole compact initial shape, absence of URLs/selectors/scripts,
stable-anchor diagnostic redaction,
aggregate/per-frame budgets, exact anchor frame and generation joins, hostile
invocation substitution, fixed faults, unknown-fault rejection, and response
wire limits. Program gates additionally pin exact source bytes and digest,
source size, the single immutable global, exact JavaScript numeric authority,
the sole bounded pull/result channel, and absence of generic
bridge/evaluation/network/storage/event/scheduling surfaces. The deterministic
synthetic-DOM smoke exercises exact request
parsing, captured-method resistance, stable anchored expansion, password
redaction before return, open-shadow traversal, closed-shadow exclusion,
detached-anchor refusal, distinct node/inspection/wire truncation, byte-bounded
output, detached-node reclamation with reattachment identity recovery, exact
native-pull settlement, one dormant follow-up pull, and API immutability. Its
current content-free result is 7 initial
nodes, 6 expanded nodes, 620/374 encoded bytes, and an 881-byte wire-truncated
result under a 1,024-byte request ceiling.

The synthetic smoke and macOS compile/static gates are not live-browser
isolated-world evidence. They cannot prove pinned WebKit wrapper identity,
content-world separation, script-message timing, frame installation,
navigation replacement, style/layout equivalence, or teardown; those claims
remain blocked on the coordinated hostile fixed-DOM engine qualification.
Surrounding-text windows are admitted up to
8 KiB by the domain contract, but one runtime node field remains capped at
4 KiB, so a larger single-node context is returned with truthful `text_limit`
rather than silently widening the hostile wire schema.

## Remaining M3 work

1. pin the first OpenAI and Anthropic proof model/tokenizer revisions, implement
   their trusted counters, and record actual fixture token/latency measurements;
2. implement and qualify the fixed OpenAI stateless output-item/function-result
   replay and Anthropic assistant-tool-use/user-tool-result encoders that consume
   the provider-neutral bound continuation, then record actual fixture
   action-diff token/latency distributions for the pinned proof
   models/tokenizers. A standalone diff and remote stored conversation remain
   forbidden;
3. execute the fixed native qualifier with separate authorization, then extend
   live-engine hostile coverage for stale nodes, mutation pressure, and timing
   races beyond its current page-world bridge, prototype collision, frame
   boundary, redaction, world replacement, and teardown checks. Open/closed
   shadow behavior and source exclusion currently have deterministic
   synthetic/static coverage but
   still require that engine evidence.
