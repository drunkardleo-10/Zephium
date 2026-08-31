# M2 context identity and lifecycle core

Status: pure domain contract implemented; bounded registry and native adapters
pending.

This evidence records code properties only. It does not claim that an owned
native context, extension inventory proof, Windows cookie bridge, borrowed-tab
lease, or sign-in handoff has shipped.

## Implemented boundary

- `ContextId` and `ContextRunId` are canonical durable ULIDs. Debug output is
  redacted; native objects, `ItemId`, URLs, paths, page data, and provider data
  are absent from the contract.
- `ContextIdentity` permanently joins context, owning run, selected
  `ProfileId`, and one closed kind: owned, borrowed tab, or human sign-in
  handoff.
- Every asynchronous operation carries context generation, navigation epoch,
  main-frame identity and generation, run-cancellation generation, operation
  id, and operation class. Late, substituted, or duplicate settlements fail
  exact comparison.
- Ownership, presentation, exclusive input control, suspension, observation
  freshness, native residency, and terminal disposition are independent
  fields. Showing or hiding a page cannot adopt it or transfer input.
- Navigation, suspension/resume, renderer loss/recovery, human takeover,
  cancellation, close, release, and adoption invalidate the required
  generations without wrapping. Exhaustion seals automation but retains a
  teardown path.
- Human takeover preempts a pending navigation, keeps the same durable context
  identity, and requires a complete fresh observation before automation can
  resume. Renderer recovery restores pre-crash human control instead of
  silently returning input to an agent.
- Run cancellation is sticky, makes old results stale, and never takes input
  from a person. A cancelled owned context can only proceed to close; a
  borrowed or handoff context can only proceed to release.
- Owned adoption requires explicit human control. Borrowed and handoff
  contexts cannot adopt; borrowed contexts cannot be destroyed through the
  owned-context close transition.

The aggregate is a functional core. Construction creates no timer, thread,
native page, queue, or background work, so the unused product has zero runtime
agent overhead.

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
```

The tests cover canonical/redacted identity, kind-scoped capability sets,
construction and navigation correlation, stale settlements, visibility and
ownership separation, takeover, cancellation, suspension, renderer recovery,
adoption/release exclusivity, and non-wrapping exhaustion.

## Remaining M2 work

1. add the bounded process registry, hot/native resource leases, shutdown
   barrier, and shell/native port;
2. implement extension-free owned construction and inventory assertions on
   both platform adapters without weakening current extension principals;
3. implement explicit profile leasing, the bounded Windows cookie bridge, and
   the sign-in-handoff transaction skeleton;
4. connect existing native suspension, renderer-loss, teardown, and resource
   ledgers while proving no tab/session/extension projection;
5. run named-device lifecycle, idle-resource, cancellation, recovery, and
   shutdown qualification.
