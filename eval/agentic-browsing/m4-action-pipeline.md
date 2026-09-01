# M4 action pipeline

Status: bounded pre-policy action proposals, exact observation binding,
one-action rolling checkpoints, typed settle/verification contracts,
fresh-snapshot structural revalidation, event-driven bounded settlement,
independent effect verification, and acknowledged action-result diff/fresh-state
finalization, plus bounded semantic read/transport, closed structured
extraction admission, and the one-shot viewport screenshot contract are
implemented.
Policy permits, platform execution,
visibility/occlusion checks, native observation adapters and timer driving,
multi-action result aggregation, native screenshot adapters, visual policy and
model-provider wiring, and live qualification remain pending.

This evidence describes a functional-core seam. It does not claim that an
action can currently reach a native view or page, that model-declared effects
are trusted, or that M4 is complete.

## Implemented boundary

- A model proposal can express only `click`, exact text-replacement `fill`,
  observed-option `select`, one of fourteen fixed `press` keys, or a fixed
  direction/magnitude `scroll`. It contains opaque semantic references rather
  than selectors, DOM, JavaScript, native handles, key strings, pixel deltas,
  or generated HTML.
- A batch admits one through eight sequential proposals. Exact fill text is
  capped at 4 KiB per action and 16 KiB per batch. Each action carries a
  nonzero settle budget no longer than 30 seconds; aggregate batch settlement
  is capped at 60 seconds. A mutation-quiet condition is nonzero and capped at
  one second. These values allocate no timer, worker, queue, view, or thread.
- Fill text rejects control/invisible-directional characters, except literal
  tab/newline/carriage-return data, and recognized credential, authorization,
  token, and private-key forms. Empty text is an explicit clear. Text content
  is redacted from diagnostics. The secret recognizer is defense in depth; it
  is not provenance or a proof that arbitrary text is public.
- Every proposal declares an effect class and an independent expected
  verification. The declaration is untrusted and is not a permit. A closed
  compatibility table rejects action/wait/verification combinations that
  cannot prove the claimed outcome. An immediate observation is allowed only
  with a separate verification requirement; it is not blind success.
- Generic mutation or semantic-change signals are settle triggers, never effect
  proof. Fill requires an exact match to the requested bounded input, select
  requires the exact bound option, and state proof requires the target to
  transition from the opposite baseline. An already-satisfied state, selection,
  or exactly projected fill postcondition is refused at binding and again at
  the pre-execution checkpoint.
  Fixed key recipes may prove a target value/selection change; click proof is
  limited to an exact target state, navigation, or dialog transition.
- One batch must use a homogeneous declared effect class. External write,
  communication, purchase, destructive, and capability-boundary declarations
  permit exactly one action so an eventual policy/executor cannot amortize a
  consequential decision over an opaque batch. Policy must independently
  derive and authorize the actual destination, data flow, effect, plan, cost,
  profile/account, origin, and run authority.
- Binding accepts only an exact native-current frame cohort for the complete
  observation. Every frame must have the same context/document/cancellation
  join and exact frame generation as observed; unknown, stale, missing,
  duplicate, added, or removed frame authority refuses the batch.
- Opaque targets resolve through the existing exact snapshot capability and
  must carry the requested closed operation class. A select option must be an
  observed option in the same exact frame and snapshot and descend from its
  observed combobox/listbox. Password or secret targets refuse ordinary fill
  and require a separate credential capability.
- Bound actions retain a private stable-node identity and a diagnostics-redacted
  SHA-256 structural guard over role, heading level, operation inventory,
  sensitivity, trust, parent stable identity, and bounded accessible name.
  Stable checked/selected/expanded/required/invalid state is guarded
  separately. The digest and private identity never enter the model contract.
- Every bound action also retains the exact batch and source observation
  identity/generation. Those coordinates enter its private verification guard,
  preventing an otherwise identical action proof from being substituted across
  a batch or model-visible baseline.
- Fresh-snapshot revalidation requires the exact frame/document authority, a
  non-regressing snapshot generation, the same stable node, an enabled target,
  the required operation, unchanged stable semantics/state, and unchanged
  select ancestry. Focus and geometry may be freshly sampled rather than
  treated as stable identity. Fill refuses if the target becomes password or
  secret between binding and execution.
- A batch is prepared one action at a time against the complete snapshot
  immediately preceding that action, after the prior action has independently
  settled and verified. Preparation repeats structural and operation
  revalidation, refuses an already-satisfied projected postcondition, and
  privately checkpoints the native invocation, generation, target value/state,
  option state, and a diagnostics-redacted contract guard. Only the current
  action needs a prepared value, so rolling batch state remains O(1).
- Revalidation is deliberately not an execution proof. The future native
  adapter must still re-resolve connectedness and current geometry, prove
  visibility/occlusion and backend compatibility, hold an exact policy permit,
  execute one fixed recipe, observe under cancellation and one absolute
  deadline, verify the declared effect independently, and compute an admitted
  bounded diff before reporting success or continuing a batch.
- Settlement starts only after the imperative executor claims one backend
  request terminally applied. One nonzero action-attempt identity, exact
  prepared frame/stable target, rolling checkpoint generation, monotonic
  completion time, and per-action budget derive one overflow-checked absolute
  deadline. An
  `Immediate` condition becomes ready for verification without allocating or
  consuming an event; it still does not become success.
- The settle core consumes at most 128 coalesced facts/snapshots and retains
  O(1) state. Facts can express only tick, exact next navigation, document
  ready, URL/title change without content, dialog state, semantic change,
  mutation, scroll-position change, exact cancellation, renderer loss, or
  human takeover. Cross-attempt, clock-regressing, wrong-frame, stale-context,
  skipped lifecycle-transition, and forged transition joins are rejected
  without advancing state.
- Navigation commits require the exact next document join. Cancellation,
  renderer loss, and human takeover carry both their exact prior join and the
  full-invalidation successor, so a race after one admitted navigation can be
  checked without guessing skipped authority. Explicit human takeover and
  cancellation terminally preempt settlement.
- Target-state settlement looks up the private stable identity in the exact
  adjacent complete-frame snapshot. Mutation quiet restarts on every admitted
  coalesced mutation and can complete only after its bounded quiet interval
  and no later than the same absolute action deadline. Missing deadline wakes
  therefore cannot create an indefinite wait; the shell must own one timer.
- Ready settlement means only that independent verification may run. The core
  has a closed failure taxonomy and non-authorizing recovery hints for abort,
  fresh observation, explicit capability, or human control. There is no retry
  counter, retry loop, or outcome path that treats a wait condition as effect
  proof.
- Verification admits only a separately sampled, exact attempt-correlated
  evidence class at or after settle completion and no later than the same
  absolute deadline. Semantic proofs require the exact adjacent complete
  snapshot, private stable target identity, unchanged role/trust/sensitivity,
  and non-credential semantics. Missing, skipped, incomplete, cross-frame, or
  secret-upgraded targets fail closed.
- Exact fill evidence carries transient borrowed before/after strings, each
  capped at 4 KiB. The before value must differ from the requested input and
  the after value must match it byte-for-byte; ordinary snapshot values are not
  reused because their safe projection is normalized and capped at 1 KiB.
  Evidence diagnostics retain none of the requested or observed text.
  Selection proof resolves the private bound option again and requires its
  selected state and current target ancestry.
- Navigation proof requires a second exact-next-document authority sample;
  dialog proof requires an opposite-to-expected transition in the same exact
  context; scroll proof requires bounded before/after samples moving in the
  declared direction, plus post-action visibility for `IntoView`.
- Successful verification mints a content-free opaque token joined to the
  attempt, action ordinal, declared postcondition, exact current context,
  evidence time, sole absolute deadline, native invocation and adjacent
  snapshot when semantic evidence was used, and a diagnostics-redacted SHA-256
  action-contract guard. The token is not a
  policy permit and cannot execute or retry. The future native shell remains
  responsible for sourcing evidence from an observation adapter independent of
  the backend completion response; the pure constructors do not claim to attest
  that imperative separation.
- Finalization consumes that non-cloneable proof and the complete current
  observation. It requires the action's exact source observation and a
  content-digest-bound acknowledgement minted only by committed model delivery.
  Semantic proof must name an invocation/generation actually present in the
  assembled current observation. The current observation carries its trusted
  monotonic capture time and must be no earlier than the independent proof and
  no later than the same absolute deadline. Cross-batch, altered-baseline,
  current-context, clock, invocation, or generation substitution fails closed
  before success.
- The existing conservative diff core then returns either the complete bounded
  action diff or a typed fallback that owns the exact current observation for
  full-snapshot encoding. A diff-limit, context change, generation gap, or
  another ordinary diff premise failure therefore cannot discard current state
  or invent a delta. An unacknowledged baseline is a lease failure, not a
  fresh-snapshot success. Diff and full-snapshot bytes still require the
  existing token-admission and committed-delivery boundary.
- Read derives only borrowed primitives from the validated semantic observation
  vocabulary; it cannot introduce DOM, selectors, HTML, script, attributes,
  native handles, or arbitrary page objects. It retains at most 256 fields and
  64 KiB process-wide (128 fields/32 KiB by default) and creates no content
  copies. Fixed Booleans/ordinals have conservative rendering charges.
- An initial read must cover an initial observation. A progressive read requires
  the exact scope predecessor and its content-digest-bound committed-delivery
  acknowledgement; parent, anchor, observation generation, and context must all
  join. A guessed or altered expansion baseline fails before content projection.
- Every returned primitive carries exact observation and progressive generation,
  context/run/profile/document, canonical origin, frame and frame generation,
  native invocation, snapshot generation, opaque reference, source trust,
  sensitivity, and monotonic capture coordinates. Diagnostics expose only
  counts/classes; content remains borrowed and redacted from `Debug`.
- Secret nodes and explicit redacted values are never returned. Sensitive data
  requires a trusted-policy-selected limit; the enum itself grants no authority.
  Source truncation, sensitivity refusal, secret redaction, item exhaustion,
  and byte exhaustion are a complete omission bitset with aggregate counts, so
  a partial read is never represented as complete.
- Deterministic `ZREAD1` encoding marks content untrusted, carries common
  capture freshness and omission metadata, assigns non-actionable result-local
  `@rN` provenance tokens, aliases canonical source frames, and emits only
  fixed field/role/source/sensitivity labels plus quoted or primitive values.
  Quoting prevents hostile line/grammar injection, and the encoder rechecks
  fragment order, source coordinates, type compatibility, and the absence of
  secret fragments before exposing bytes.
- Encoded read bytes stay private until the selected tokenizer port admits the
  exact revision, quality, byte, and token ceilings. Committed transport mints
  a content-bound read-delivery receipt; refused/cancelled transport consumes
  the payload without authority. The receipt is intentionally a distinct type,
  cannot authorize a semantic diff or progressive scope, and fails to match a
  different sensitivity projection, content cohort, or capture time.
- Extraction accepts hostile model output only after that exact bounded read
  has a committed delivery receipt. The caller supplies a nonzero trusted
  schema identity and one through 64 unique schema-ordered ASCII fields. The
  closed v1 schema permits only bounded text, Boolean, bounded unsigned integer,
  and bounded text-list values; it cannot express nested objects, arbitrary
  JSON Schema, executable data, selectors, DOM/native identity, or markup.
- The fixed extraction output envelope is capped at 64 KiB before JSON decode,
  rejects unknown or duplicate keys, requires the exact grammar version and
  trusted schema identity, and admits at most 256 primitive values, 64 KiB
  retained text, and 1,024 provenance edges. Individual text, list, item,
  schema-name, field-name, and per-value source limits are independently
  smaller. Optional fields may be omitted, but present fields must remain in
  schema order and required fields cannot disappear.
- Every scalar, list mapping, and list item cites one through four canonical,
  strictly increasing `@rN` tokens. Rust resolves each token in the exact
  delivered read and reapplies the requested public/sensitive ceiling; missing,
  reordered, duplicated, noncanonical, secret, or over-policy citations fail
  closed. Model-produced strings are separately bounded, screened for forbidden
  invisible/control characters and secret-like forms, and redacted from
  diagnostics.
- An admitted result is explicitly `ModelMapped`, not browser-attested truth.
  Values retain a flat bounded table of the exact borrowed read fragments. Each
  opaque source span carries a full SHA-256 guard over the delivered read,
  trusted schema identity, sensitivity allowance, and raw model output, so a
  span from a different result cannot resolve against a coincidentally similar
  table. Extraction creates no observation acknowledgement, action authority,
  policy permit, or durable page identity.
- Screenshot v1 is deliberately viewport-only. Apple WebKit can natively select
  a rectangle and width, but WebView2 `CapturePreview` supplies encoded PNG/JPEG
  for what the WebView is displaying and documents that a call before the new
  document's `ContentLoading` can capture the page being left. A raw-RGBA or
  semantic-region common seam would therefore add a Windows full-frame decode,
  crop, second image allocation, and decoder surface before evidence shows that
  cost is needed. The native request instead requires an explicit exact-document
  content-available attestation. See the primary
  [WebKit snapshot configuration](https://developer.apple.com/documentation/webkit/wksnapshotconfiguration)
  and [WebView2 capture contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#capturepreview).
- A screenshot can be prepared only from an exact content-digest-bound
  observation acknowledgement. Every observed frame must be complete, every
  retained frame boundary observed, and no node may be secret or explicitly
  redacted. This is the default no-known-secret path, not proof that arbitrary
  image/canvas pixels are public; a future visual override requires a separate
  exact policy capability rather than another value in this contract.
- Preparation produces one content-bound request which a zero-idle coordinator
  splits into non-cloneable pending/native halves. At most two captures may be
  pending globally and one per exact context. Duplicate identities, cross-result
  halves, wrong-context cancellation, stale context/document/cancellation joins,
  pre-request capture, clock regression, and post-deadline completion fail
  closed. Every terminal admission failure or exact cancellation releases its
  slot without retry authority; the sole deadline is at most five seconds.
- The standard image budget is 1280x800, 1,024,000 pixels, and 5 MiB native PNG;
  hard ceilings are 2048x2048, 2,097,152 pixels, and 9 MiB. The platform stream
  must enforce the selected byte ceiling while encoding, before returning a
  buffer. Admission scans at most 1,024 chunks, requires matching nonzero
  dimensions, fixed non-interlaced 8-bit RGB/RGBA, exact IHDR/contiguous
  IDAT/IEND order, and valid CRCs. Unknown critical chunks fail; ancillary
  rendering/ICC extensions also fail, bounded color declarations are retained,
  and only a fixed non-rendering metadata vocabulary is CRC-checked and removed
  in place without a pixel decode or second full image allocation.
- Admitted pixels remain explicitly browser-rendered hostile page content and
  conservatively `Sensitive`. PNG bytes and all page/native metadata are
  redacted from diagnostics; the core creates no file, persistence, timer,
  worker, native view, or thread. It mints no policy/provider authority and the
  current tree has no native capture adapter, so no live screenshot is taken by
  these contracts or tests.

The functional core has zero idle overhead. It allocates only when a bounded
action, read, or extraction operation is invoked and creates no timer, worker,
queue, page/runtime task, native view, or thread.

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
cargo test --locked -p zephium-agentic --features probe-harness
cargo clippy --locked -p zephium-agentic --all-targets \
  --features probe-harness -- -D warnings
```

Coverage includes every fixed action shape; rolling second-action checkpoint
preparation and fill replay refusal; action/wait/verification
compatibility; count, text, settle, and effect-boundary ceilings; current-frame
cohort joins; wrong-operation and stale-reference refusal; credential targets;
invalid option ancestry; diagnostic redaction; stable identity across harmless
sibling insertion, focus, and geometry changes; structural/name/state change;
missing targets; removed operations; disabled targets; regressed generations;
credential escalation; and option drift.

Settlement coverage includes immediate readiness, exact deadline derivation
and overflow, mutation-quiet restart, exact adjacent target-state snapshots,
attempt and monotonic-clock correlation, exact navigation/cancellation/human
takeover transitions, terminal idempotence, event exhaustion, typed recovery,
and content-redacted diagnostics.

Verification coverage includes exact fill before/after match and redaction,
pre-existing-value refusal,
adjacent target-state proof and skipped-generation refusal, exact option
selection, fixed-key value change, independently correlated navigation/dialog
transitions, directional/visibility-aware scroll proof, coordinate ceilings,
pending/wrong-attempt/wrong-action/deadline refusal, typed failure mapping, and
opaque proof/action binding.

Action-result coverage includes exact committed-baseline/content binding,
source observation and batch guard substitution, exact verification
invocation/generation membership in the assembled observation, post-proof and
pre-deadline observation ordering, complete diff success, navigation successor
fresh-state fallback, diff-budget fallback without losing the current
observation, typed failure mapping, and content-redacted diagnostics.

Read coverage includes initial and exact acknowledged progressive authority,
same-coordinate altered-content acknowledgement refusal, public-only and
policy-admitted sensitive projection, unconditional secret/redacted omission,
zero-copy source identity, exact provenance coordinates, item/byte ceilings,
truthful source truncation and omission accounting, invalid budgets, and
content-free diagnostics. Encoding coverage includes deterministic escaping,
frame/provenance aliases, non-actionable `@rN` identities, byte refusal,
token quality/revision/count gates, content-redacted diagnostics, committed-only
delivery receipts, projection/capture mismatch, and the mechanical distinction
from full-observation acknowledgement authority.

Extraction coverage includes trusted schema names, uniqueness, ordering, and
all field bounds; exact read-delivery/capture/projection binding; all four value
shapes; optional and required fields; malformed, unknown, duplicate, reordered,
extra, and type-mismatched model output; input, text, list, unsigned, primitive,
and aggregate provenance ceilings; secret/control-character refusal; canonical,
resolved, unique, ordered, and sensitivity-admitted citations; content-free
diagnostics; exact borrowed provenance; and cross-result source-span
substitution refusal.

Screenshot coverage includes exact acknowledged-observation binding;
incomplete/unsupported/known-secret refusal; request deadline and image-budget
validation; duplicate/global/per-context coordinator capacity; exact
cancellation and terminal cleanup; cross-request pending/native substitution;
stale context and capture-clock refusal; RGB/RGBA dimensions and classification;
PNG signature, chunk framing/order/count, CRC, terminal-IEND, fixed-format, and
critical-chunk rejection; consecutive IDAT retention; ancillary metadata
removal before and after image data; content-redacted diagnostics; and
move-only canonical PNG delivery.

No platform action backend or live page is exercised by these tests.
