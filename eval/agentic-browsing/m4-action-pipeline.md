# M4 action pipeline

Status: bounded pre-policy action proposals, exact observation binding,
one-action rolling checkpoints, typed settle/verification contracts,
fresh-snapshot structural revalidation, and the event-driven bounded settle
and independent effect-verification cores are implemented. Policy permits,
platform execution, visibility/occlusion checks, native observation adapters
and timer driving, diff settlement,
read/extract/screenshot, and live qualification remain pending.

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
  attempt, action ordinal, declared postcondition, adjacent snapshot when used,
  and a diagnostics-redacted SHA-256 action-contract guard. The token is not a
  policy permit and cannot execute or retry. The future native shell remains
  responsible for sourcing evidence from an observation adapter independent of
  the backend completion response; the pure constructors do not claim to attest
  that imperative separation.

The action core has zero idle overhead. It allocates only when an admitted
proposal batch is constructed and creates no page/runtime/native work.

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

No platform action backend or live page is exercised by these tests.
