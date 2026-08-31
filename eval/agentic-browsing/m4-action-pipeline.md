# M4 action pipeline

Status: bounded pre-policy action proposals, exact observation binding, typed
settle/verification contracts, and fresh-snapshot structural revalidation are
implemented. Policy permits, platform execution, visibility/occlusion checks,
event observation, cancellable deadline driving, effect verification, diff
settlement, navigation/dialog state machines, read/extract/screenshot, and
live qualification remain pending.

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
- Revalidation is deliberately not an execution proof. The future native
  adapter must still re-resolve connectedness and current geometry, prove
  visibility/occlusion and backend compatibility, hold an exact policy permit,
  execute one fixed recipe, observe under cancellation and one absolute
  deadline, verify the declared effect independently, and compute an admitted
  bounded diff before reporting success or continuing a batch.

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

Coverage includes every fixed action shape; action/wait/verification
compatibility; count, text, settle, and effect-boundary ceilings; current-frame
cohort joins; wrong-operation and stale-reference refusal; credential targets;
invalid option ancestry; diagnostic redaction; stable identity across harmless
sibling insertion, focus, and geometry changes; structural/name/state change;
missing targets; removed operations; disabled targets; regressed generations;
credential escalation; and option drift.

No platform action backend or live page is exercised by these tests.
