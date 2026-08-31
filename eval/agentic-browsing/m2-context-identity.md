# M2 context identity and lifecycle core

Status: pure domain contract, bounded registry, and closed shell/native port
implemented; platform adapters pending.

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

`ContextRegistry` is the single-owner admission and accounting boundary. It
retains at most eight live rows, at most four execution permits, and never
evicts. Rows above the execution ceiling remain queued; successful suspension
and renderer loss release a permit, while resume and deferred recovery must
reacquire one. A separate owned-native reservation count distinguishes new
owned/handoff resources from borrowed-tab leases. Terminal rows remain until
the shell observes an exact `Destroyed`, `TransferredToBrowse`, or
`ExistingBrowseRetained` disposition.

The shutdown seal permanently rejects admission, drops only never-started
rows, enumerates a bounded exact cleanup cohort, and reports quiescence only
after every active row reaches and exposes its terminal disposition. Bounded
run/profile indexes support cancellation and profile-erasure barriers without
granting authority.

`AgentBrowserPort` is a closed imperative boundary. Its request and event
vocabulary carries exact joins, validated web navigation targets, typed native
failures, paired platform construction attestations, post-revocation
cancellation, and a validated native resource audit. It cannot represent page
JavaScript, selectors, DOM data, CDP, native handles, cookies, profile paths,
headers, provider data, or platform error strings.

Construction requests must match immutable context kind, complete capability
kind, and an owned/borrowed/handoff source. Borrowed tabs use a redacted,
process-local lease identity rather than exposing `ItemId`. Successful native
construction cannot settle without one exact paired storage/extension proof:
macOS owned selected-profile storage with no extension controller or script
principal; Windows selected-profile or stable automation-subprofile storage
with an empty extension inventory; or an explicit borrowed/handoff proof that
normal Browse extensions remain active.

Native requests are bounded to at most sixteen retained tasks across the eight
live contexts. Resource audits contain only validated counts and reject limits
or contradictions. Navigation targets reuse the browser's 8 KiB absolute URL
gate and reject credentials, local files, internal principals, and dangerous
schemes; their debug representation is redacted. This gate is input validity,
not run policy authorization.

Visibility and input ownership now settle against exact native callbacks.
Refused show/hide operations leave presentation unchanged. Human takeover
immediately revokes agent input and stales pending presentation/navigation;
native refusal never silently returns input to the agent. Returning input also
requires exact native success and then a complete fresh observation.

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
```

The tests cover canonical/redacted identity, kind-scoped capability sets,
construction and navigation correlation, stale settlements, exact
presentation settlement, visibility and ownership separation, takeover,
cancellation, suspension, deferred renderer recovery, adoption/release
exclusivity, non-wrapping exhaustion, both registry ceilings, no-eviction
pressure, terminal resource disposition, shutdown quiescence, closed native
request classes, construction-proof compatibility, unsafe URL rejection,
redacted debug output, and bounded/consistent native resource snapshots.

## Remaining M2 work

1. bind registry execution/native reservations to the engine's authoritative
   resource leases through platform adapters;
2. implement extension-free owned construction and inventory assertions on
   both platform adapters without weakening current extension principals;
3. implement explicit profile leasing, the bounded Windows cookie bridge, and
   the sign-in-handoff transaction skeleton;
4. connect existing native suspension, renderer-loss, teardown, and resource
   ledgers while proving no tab/session/extension projection;
5. run named-device lifecycle, idle-resource, cancellation, recovery, and
   shutdown qualification.
