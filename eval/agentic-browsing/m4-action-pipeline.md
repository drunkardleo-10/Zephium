# M4 action pipeline

Status: bounded pre-policy action proposals, exact observation binding,
one-action rolling checkpoints, typed settle/verification contracts,
fresh-snapshot structural revalidation, event-driven bounded settlement,
independent effect verification, and acknowledged action-result diff/fresh-state
finalization and exact bounded batch-terminal aggregation, plus a one-shot
policy-to-native execution handoff and exact consuming native-to-settlement
transition with a move-only bounded settlement owner and one-shot consuming
terminal verification through exact policy accounting and result finalization,
bounded semantic read/transport, closed
structured extraction admission, exact tool-result/constrained-output
extraction transport, and the one-shot viewport screenshot contract are
implemented.
Policy permits, platform execution,
visibility/occlusion checks, native observation adapters and physical timer installation,
installed visual token counters, and live action/screenshot/provider
qualification remain pending. Exact one-shot visual policy and fixed OpenAI /
Anthropic tool-result wiring are implemented. Production macOS and Windows owned-context
viewport adapters are implemented and mechanically bounded, but neither was
executed in this evidence pass and therefore neither carries a live-pixel or
platform qualification claim. The Windows host keeps screenshot dispatch
unsupported until its physical semantic-runtime qualifier promotes exact
snapshot-generation lifecycle support.

This evidence describes a functional-core seam. It does not claim that an
action can currently reach a native view or page, that model-declared effects
are trusted, or that M4 is complete.

## Implemented boundary

- A model proposal can express only `click`, exact text-replacement `fill`,
  observed-option `select`, one of fourteen fixed `press` keys, or a fixed
  direction/magnitude `scroll`. It contains opaque semantic references rather
  than selectors, DOM, JavaScript, native handles, key strings, pixel deltas,
  or generated HTML.
- Standalone waits have a distinct model contract from action-local settlement.
  Target-state and scroll-position waits must carry one canonical opaque
  `@aN` reference; action-local waits continue to use their already-bound
  primary action target. Missing, noncanonical, selector-like, or extra target
  fields fail in the strict provider decoder before any observation or timer
  authority exists. This prevents a future wait driver from guessing which
  page node a target-scoped condition means.
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
- Final policy dispatch can now split one exact non-cloneable active effect into
  a retained policy-authority half and one closed native request. The join binds
  effect and action-attempt identities, the complete private action guard,
  exact frame/document authority, semantic invocation and snapshot generation,
  private stable target identity, fresh pre-execution geometry, and trusted
  monotonic request/deadline coordinates. Missing/empty geometry, action/permit
  substitution, and deadline overflow return the still-dispatched authority so
  the policy ledger can settle it terminally instead of leaking a reservation.
- The native request exposes only the fixed action class and its already-bounded
  recipe: at most one 4 KiB fill copy, one observed option stable identity, one
  of fourteen keys, or a closed scroll direction/magnitude. It contains no
  selector, raw DOM, JavaScript, property path, native handle, URL, model
  output, generated HTML, backend order, timer, worker, or retry surface.
  Private stable identities and fill content are redacted from diagnostics.
- Native execution has a separately bounded absolute window of at most five
  seconds, including port queue and backend work. A claimed application must
  rejoin the exact request, retain exact current frame/document authority, use
  non-regressing times, and carry fresh nonempty geometry plus either an exact
  visible/unoccluded hit-target proof or an exact connected scroll-target
  proof. Visible claims must intersect a nonzero viewport capped at 32768 per
  dimension. Late terminals become typed timeouts; premature timeout claims,
  cross-request results, clock regression, incompatible readiness, and
  contradictory geometry are distinct fail-stop contract violations.
- Backend class is content-free diagnostic attribution only: fixed semantic
  recipe, engine-native input, or in-process accessibility. The enum grants no
  selection authority and no production backend has been selected or wired.
  An applied terminal is explicitly not effect proof; it only returns the exact
  active policy authority and a same-clock start instant for the existing
  settle/independent-verification core. Typed backend failures and contract
  violations likewise return that authority for one charged terminal policy
  settlement, with no blind retry path.
- The native-to-settle join consumes the complete non-cloneable execution
  outcome, rechecks the returned policy authority against the exact prepared
  action, and mints a tracker only for an `Applied` disposition. The tracker
  begins at the trusted native completion instant, while its start owner keeps
  content-free backend/readiness/timing attribution for metrics. Typed native
  failure, execution-contract failure, action substitution, or settlement
  deadline overflow returns policy authority without creating a tracker. The
  transition owns no task, timer, queue, retry, native object, or content
  buffer.
- A zero-idle settlement coordinator retains the complete applied authority
  while a typed settle condition remains pending. Its empty `Vec` allocates
  only on first use; it admits at most four applied actions process-wide and
  one per logical context. Every accepted fact or borrowed exact snapshot
  consumes a move-only reservation and returns either one replacement
  reservation carrying the current exact wake or the complete terminal owner.
  Premature wakes, replaced schedules, wrong attempts, malformed snapshots,
  unknown/cross-coordinator reservations, duplicate identities, context
  overlap, and capacity refusal do not release retained debt. Shutdown seals
  admission but preserves terminal drain, including an already-ready immediate
  action. The coordinator owns no physical timer, callback, task, native
  object, snapshot copy, page content, or retry loop.
  Pending start state has no public mutable-tracker or destructuring escape;
  only the coordinator-minted terminal owner releases tracker and policy
  authority for verification or typed failure.
- Independent verification consumes that terminal owner and exactly one
  borrowed evidence value. The borrow-only tracker verifier is crate-private,
  so production code cannot mint a proof from a copied tracker or retry a
  refused proof opportunity. Success retains policy authority, content-free
  execution attribution, terminal settlement, and the opaque proof in one
  non-cloneable owner. Refusal consumes the terminal and returns the same
  authority, execution/settlement metrics, and closed verification error for
  one typed failed policy settlement. Borrowed snapshot/text evidence is never
  retained, and neither path owns a timer, task, queue, callback, or retry.
- Run policy consumes the joined verified or refused terminal directly. The
  raw “active authority plus borrowed proof” settlement method is crate-private.
  Verified charging returns an immutable effect receipt still joined to
  content-free execution attribution, terminal settlement, and the opaque
  proof required by result finalization. Refused charging maps the closed
  verification error exactly once to its action failure and returns the failed
  receipt with timing/error state. Pre-verification native and settlement
  failures keep the separate public typed-failure path; no fabricated proof is
  needed to charge them.
- Result finalization consumes that policy-accounted verified owner rather than
  accepting a loose proof. It validates the exact action, committed baseline,
  proof observation coordinates, current context, and capture clock while the
  complete current observation is still borrowed. Only a valid join
  destructures the charged owner and computes the bounded diff/fresh-snapshot
  outcome. Refusal returns both the charged proof owner and exact current
  observation, preventing an authority mistake from discarding post-action
  state. The loose finalizer is compiled only for crate tests.
- A zero-idle single-owner coordinator now retains the active policy half while
  native work is outstanding. Its empty `Vec` allocates only on first use; it
  admits at most four requests process-wide and one per logical context,
  rejects duplicate effect or attempt identities, and returns policy authority
  on every preparation/capacity/shutdown refusal. Exact settlement, synchronous
  port refusal, cancellation, or a shell-driven deadline releases one entry.
  Unknown, substituted, replayed, and premature-timeout terminals do not release
  retained debt. Shutdown seals new admission but keeps accepted entries until
  their terminal drain. The move-only reservation retains the exact absolute
  deadline after its content-bearing native request leaves the shell, so one
  timer can be scheduled without keeping or copying that request.
- `AgentBrowserPort` has a move-only semantic-action callback contract: a
  `Scheduled` native request owes exactly one settlement created by consuming
  that request, while synchronous `Rejected` or `Unsupported` results owe no
  callback. The production engine currently returns `Unsupported` before task
  admission on every platform, so this seam adds no queue, task, native object,
  timer, or idle cost and cannot silently select a backend before M1 physical
  qualification. Coordinator-owned dispatch reconciliation retains scheduled
  debt and exhaustively maps every synchronous shared-port refusal into one
  typed semantic failure before releasing it, avoiding caller-specific charging
  or leaked authority. A static gate locks the fail-closed engine result, this
  mapping, and the coordinator bounds.
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
  therefore cannot create an indefinite wait. While pending, the tracker
  exposes exactly one next wake: the current quiet boundary capped by the
  absolute deadline, or the deadline for every other condition. Terminal state
  exposes no wake. This is a pure scheduling plan, not a timer or polling loop;
  the future native shell must install, replace, and cancel the physical timer.
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
- Batch aggregation begins only from an exact already-bound batch and retains
  at most eight content-free expected action guards, eight content-free
  completion summaries, and the latest full result. An exact per-position
  SHA-256 guard prevents an action from another separately bound batch from
  being admitted even if a shell violates its process-local uniqueness
  obligation and reuses a batch ID. Previous full diffs/snapshots are dropped
  as each later action succeeds, so aggregation remains O(one bounded current
  state plus eight fixed summaries), not eight page-state payloads.
- Each admitted success must be the exact next ordinal, match both the original
  bound action and its rolling pre-execution checkpoint, use a previously unseen
  attempt identity, and carry non-regressing independent-proof time. A complete
  batch returns only after every action independently reached finalization.
  Incomplete aggregation cannot be represented as success.
- A verified navigation, dialog transition, or context/document/cancellation
  change establishes a mandatory terminal stop before any remainder. Trusted
  orchestration may also explicitly stop a nonempty successful prefix for
  meaningful unpredicted state, but it cannot label an ordinary prefix as a
  navigation/dialog stop without matching evidence. Complete and deliberately
  stopped outcomes retain only the latest exact state update.
- A failed next action returns its one-based position, the closed pipeline
  failure, and the minimum non-authorizing recovery boundary. It discards the
  prior full state because a failed attempt may have changed the page without
  proving it. There is no retry count, automatic retry path, or way to continue
  after an established stop boundary.
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
  a content-bound read-delivery receipt which also privately binds the exact
  full source-observation fingerprint; refused/cancelled transport consumes the
  payload without authority. The receipt is intentionally a distinct type,
  cannot authorize a semantic diff or progressive scope, and fails to match a
  same-coordinate altered observation, different sensitivity projection,
  content cohort, or capture time.
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
- The provider mapping path is separate from ordinary read and diff
  continuation. Only an exact prior tool-only `extract` call selecting the same
  trusted schema ID can append deterministic `ZEXTRACT1` input: trusted closed
  field declarations followed by the exact hostile `ZREAD1` evidence. Its
  private delivery guard binds every field kind and bound plus the read,
  observation fingerprint, context, generation, and capture time. Generic diff
  binding now explicitly rejects extract correlations.
- OpenAI receives a tool-free strict `text.format` JSON-schema response turn;
  Anthropic receives a tool-free stable `output_config.format` response turn.
  Both use one fixed universal output envelope rather than a dynamic schema, so
  shell-registered field names and page values do not enter Anthropic's
  documented 24-hour compiled-schema cache. The Anthropic projection removes
  unsupported numeric/length/cardinality constraints and Rust revalidates the
  original closed schema after streaming, consistent with the current
  [OpenAI structured-output](https://developers.openai.com/api/docs/guides/structured-outputs)
  and [Anthropic structured-output](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)
  contracts.
- The full immutable mapping request requires an `ExactLocal` count from the
  pinned provider/model/tokenizer before policy mutation. Exact commitment
  rejoins the unchanged observation baseline taint and returns only a
  content-free extraction receipt; it creates no acknowledgement or reference
  growth. The response collector retains at most the existing 64 KiB hostile
  extraction ceiling, clears and permanently poisons itself on wrong-call or
  tool output, and admits only an exact completed, tool-free terminal whose
  reported text bytes equal the retained bytes. Schema/read substitution,
  refusal, incomplete output, cancellation, and terminal mismatch fail closed.
  Extraction is terminal and cannot seed another provider/browser turn.
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
  Both halves now retain the trusted request clock anchor and exact main-frame
  semantic snapshot generation. The macOS host compares that generation with
  the current owned-view semantic runtime before dispatch, so a screenshot
  cannot use a merely same-document but different observed snapshot as native
  authority.
- The standard image budget is 1280x800, 1,024,000 physical output pixels, and
  5 MiB native PNG. The owned-context layout is independently fixed at
  1280-by-800 logical/CSS pixels; device scale therefore does not silently
  widen the screenshot pixel/byte admission contract. Hard ceilings are
  2048x2048, 2,097,152 pixels, and 9 MiB. The platform stream
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
  worker, native view, or thread. Capture alone mints no policy/provider
  authority.
- Provider delivery can bind only an exact prior `screenshot` tool-only stop,
  a strictly newer call in the same manifest/lease/node and fixed provider
  configuration, and a screenshot matching the continuation's exact committed
  observation/generation/context. A separate SHA-256 delivery guard covers the
  source fingerprint, complete context join, validated metrics, capture time,
  and every canonical PNG byte. Tool, configuration, lineage, baseline,
  observation, pixel, or receipt substitution fails closed.
- Model-facing PNG is capped again at 1,300,000 bytes, the replayed private
  text transcript at 64 KiB, and the complete serialized provider body at the
  existing 2 MiB ceiling. Base64 is produced into one exactly preallocated
  string; the canonical PNG is then dropped after the immutable request body
  is built. A provider-specific synchronous local counter must count the exact
  complete multimodal body with the pinned model/tokenizer and return
  `ExactLocal` before policy reserves it. Provider-backed preflight is not
  accepted because it would itself disclose the image.
- OpenAI receives the exact prior `function_call` followed by a
  `function_call_output` containing one high-detail PNG data URL. Anthropic
  receives the exact prior assistant `tool_use` followed by an adjacent user
  `tool_result` containing one base64 PNG image block. Anthropic sets
  `transformations.oversized_image` to `error`, so a model-tier mismatch cannot
  silently rescale pixels. These shapes follow the current
  [OpenAI Responses API](https://developers.openai.com/api/reference/resources/responses/methods/create)
  and [Anthropic image-coordinate contract](https://platform.claude.com/docs/en/build-with-claude/vision-coordinates).
- The immutable model instruction explicitly treats screenshot pixels as
  hostile page data and grants them no opaque-reference or action authority.
  On exact transport commitment, policy records a separate `Sensitive` /
  `UntrustedPage` cohort for every observed frame origin, with zero references.
  Refusal or pre-commit cancellation releases the reservation without visual
  taint. A committed screenshot returns only a cloneable content-free receipt
  and never creates a continuation seed, so image bytes cannot enter replay.
- The macOS port carries capture results through a dedicated move-only callback,
  never the cloneable context event bus. It shares the existing bounded native
  ingress and adds exactly two process-wide physical capture permits. Logical
  timeout, cancellation, renderer loss, or context retirement can settle
  immediately, while the non-cancellable WebKit operation retains its physical
  permit until the native callback/block is actually destroyed. This prevents
  timeout from reopening unbounded native work. Native resource audits expose
  that physical debt separately from context-bound lifecycle operations, so a
  timed-out capture cannot disappear from content-free accounting merely
  because its logical context has already settled.
- The macOS adapter uses `WKWebView`'s public asynchronous snapshot API with an
  exact viewport rectangle, conservative budget-derived width, and
  `afterScreenUpdates = true`. The result is rejected before encode when native
  dimensions/pixels exceed the request. ImageIO then writes directly to a
  Rust-owned `CGDataConsumer`; every write is checked against the selected PNG
  ceiling before bounded growth, and allocation failure/overflow settles as a
  closed resource refusal. No Foundation-backed mutable buffer, file, raw
  bitmap copy, generic page bridge, JavaScript evaluation, selector, global
  input, or focus operation is introduced. The existing core subsequently
  verifies CRC/shape and removes admitted ancillary metadata in place.
- The Windows adapter uses WebView2's public asynchronous `CapturePreview` API
  with its fixed PNG format and a custom Rust-owned `IStream`. `Write`, `Seek`,
  and `SetSize` all enforce the caller-selected byte ceiling before growth;
  allocation failure and writes or seeks beyond that ceiling become a typed
  resource refusal. The stream uses no file, generic system-memory stream,
  unbounded callback buffer, decoder, second full-frame copy, JavaScript,
  selector, CDP method, input, or focus operation. Before handing the move-only
  PNG to the core, it reads only the fixed first IHDR dimensions and rejects
  zero, overflowing, or over-budget pixels; the core still performs complete
  chunk/CRC/format admission and in-place metadata removal. Unsupported stream
  operations and COM reentrancy fail closed, stream/callback unwinds are
  contained, and a synchronous dispatch/callback race cannot settle twice.
- Windows capture remains deliberately unreachable from the production port:
  the host does not yet retain the exact semantic invocation/snapshot
  generation needed to authorize an image of the current document. The
  private ownership seam already requires the exact runtime's content-free
  WebView2 `ContentLoading` lifecycle fact and re-attests the hidden parent and
  fixed viewport; those facts are necessary but do not substitute for the
  missing snapshot-generation join. The adapter is compile- and static-gate
  evidence only: no `CapturePreview` call ran, no native screenshot bytes were
  produced, and no platform support claim is promoted by this change.
- The host uses the port-admission `Instant` as the native elapsed-time anchor,
  maps start/completion into the request's trusted monotonic domain, includes
  queue and encode time in the sole deadline, and suppresses encoding after a
  logical cancellation. Exact context join, owned kind, `Observe` capability,
  committed target, renderer health, no competing operation, dormant semantic
  pull, and exact snapshot generation are all required before capture. This
  path is compile/unit/static-gate evidence only: no native screenshot command
  was run and no screenshot bytes or page content were recorded.

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
cargo test --locked -p zephium-engine --features agentic-browser
cargo clippy --locked -p zephium-engine --features agentic-browser \
  --all-targets -- -D warnings
cargo check --locked --release --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser --tests
cargo xtask check-agentic-probe-boundary
```

Coverage includes every fixed action shape; rolling second-action checkpoint
preparation and fill replay refusal; action/wait/verification
compatibility; count, text, settle, and effect-boundary ceilings; current-frame
cohort joins; wrong-operation and stale-reference refusal; credential targets;
invalid option ancestry; diagnostic redaction; stable identity across harmless
sibling insertion, focus, and geometry changes; structural/name/state change;
missing targets; removed operations; disabled targets; regressed generations;
credential escalation; and option drift.

Native-execution handoff coverage includes all five closed recipe projections,
fresh-geometry retention, action/active-effect substitution, missing geometry,
deadline overflow, cross-request completion, current-context mismatch,
clock regression, late and premature timeout, viewport intersection,
readiness/action compatibility, typed native failures, policy-authority return
on every terminal, settle-clock handoff, and fill/page-content diagnostic
redaction. The static release boundary also rejects generic script/evaluation,
selector, OS-event, WebKit/WebView2, filesystem, and thread surfaces in this
functional core.

Native-execution coordination coverage adds exact-once release, replay,
duplicate effect/attempt identity, one-per-context serialization, process-wide
capacity, preparation/refusal authority recovery, cross-request settlement,
premature and exact-deadline timeout, shutdown seal-and-drain, zero-idle status,
all synchronous port-dispatch mappings, and diagnostic redaction. The engine
port is compile-checked on both host and Windows targets while it remains
mechanically `Unsupported` pending M1 evidence.

Settlement coverage includes immediate readiness, exact deadline derivation
and overflow, mutation-quiet restart, exact adjacent target-state snapshots,
attempt and monotonic-clock correlation, exact navigation/cancellation/human
takeover transitions, terminal idempotence, event exhaustion, typed recovery,
and content-redacted diagnostics. Settlement-owner coverage additionally
includes zero-idle immediate passthrough, move-only wake replacement,
premature/replaced wake refusal, exact borrowed-snapshot routing, reservation
recovery after rejected facts/snapshots, one-per-context and four-process-wide
capacity, duplicate identity, cross-coordinator substitution, seal-and-drain,
and content-bearing action diagnostic redaction.

Verification coverage includes exact fill before/after match and redaction,
pre-existing-value refusal,
adjacent target-state proof and skipped-generation refusal, exact option
selection, fixed-key value change, independently correlated navigation/dialog
transitions, directional/visibility-aware scroll proof, coordinate ceilings,
pending/wrong-attempt/wrong-action/deadline refusal, typed failure mapping, and
opaque proof/action binding. The terminal join additionally covers exact
success authority/attempt preservation, one-shot refusal with authority
recovery, action substitution, backend attribution, and content-redacted
diagnostics; the static gate rejects a public borrow-only verifier or terminal
recovery retry path. Policy-join coverage runs the complete
dispatch→native→settle→verify→charge success path and the refused-evidence
failure path, asserting exact receipt/attempt/proof/failure/backend retention;
the static gate rejects a public raw verified-effect settlement method.
Accounted-result coverage additionally refuses an altered baseline acknowledgement,
recovers the same charged owner and current observation, then succeeds with the
exact acknowledgement while retaining receipt/attempt/settlement/proof and
redacted diagnostics. The static gate rejects a shipping loose finalizer or a
refusal that drops current state.

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
from full-observation acknowledgement authority. Provider-result coverage also
binds the read to its exact prior observation fingerprint and refuses
same-coordinate result substitution.

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
Platform adapter coverage additionally includes bounded seek/overwrite/sparse
extension, pre-growth write/seek/size refusal, fixed-IHDR dimension admission,
and mechanical exclusion of Windows file/system-memory streams, decoding,
generic script/CDP calls, OS input, and focus changes. The Windows adapter is
cross-compiled only; no native capture or live page is exercised by these
tests.

No platform action backend or live page is exercised by these tests.
