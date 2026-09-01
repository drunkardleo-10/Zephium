# M5 policy and supervisor

Status: the immutable approved run-manifest contract is implemented. Mutable
plan-lease consumption, committed-model-input taint state, effect permits,
`NeedsHuman` transitions, the bounded run tree/scheduler/cancellation tree,
provider adapters, and live qualification remain pending.

This evidence describes policy input facts only. A manifest cannot authorize a
browser action, model call, tool call, data transfer, cost, or native resource.

## Implemented manifest boundary

- One exact manifest revision binds an opaque durable identity to the existing
  exact `ContextRunId`. It owns no timer, worker, task, queue, provider, native
  context, page content, or effect permit and therefore adds zero idle runtime
  overhead.
- A run names one through four exact browser profiles, one through eight exact
  account states, and one through 32 canonical HTTP(S) origins. Account states
  are either explicitly anonymous or an opaque ULID; usernames, service labels,
  cookies, and page account text are not retained. Every list is sorted for a
  deterministic guard and duplicate identities fail instead of being silently
  collapsed.
- Account state remains additive to the existing high-fanout context identity.
  A closed attestation joins an opaque attestation identity, anonymous/exact
  account, the complete current context/document/cancellation authority, and a
  trusted monotonic observation time. Construction alone is not proof; the
  future fixed account adapter must source the fact and policy must re-check
  exact authority/freshness at the actual effect boundary.
- The closed effect allowlist uses the existing seven semantic effect classes.
  It is approval scope, not the model's authority: later execution must compare
  it with an independently derived actual effect rather than trusting the
  proposal declaration.
- Secret visibility or transfer cannot be approved. Public/sensitive global
  scope is explicit. Cross-origin or cross-account data transfer requires one
  exact source-origin/account to destination-origin/account rule, a maximum
  non-secret sensitivity, and a non-read effect allowlist. Same-endpoint rules,
  duplicate endpoint pairs, unknown endpoints, read “sinks,” secret scope, and
  effect/sensitivity widening fail closed.
- Run and plan-node budgets jointly cap operations, model tokens, provider/tool
  cost in micro-USD, and concurrent browser contexts. Hard ceilings are 4,096
  operations, 10 million tokens, $100, and four contexts. The immutable budget
  does not spend or reserve anything yet; mutable atomic consumption remains a
  separate pending boundary.
- A manifest is valid for a nonzero monotonic interval no longer than 24 hours
  and owns one through 64 explicit visible plan nodes. Every node independently
  names a subset of profiles, accounts, origins, sensitivity, and effects; its
  complete budget cannot exceed the run budget and its expiry cannot exceed the
  manifest. Duplicate nodes and every widening dimension fail closed.
- SHA-256 binds the exact manifest revision, run, canonical scopes, flow rules,
  budgets, monotonic lifetime, and canonical plan nodes. Debug output exposes
  only identities already redacted by type, bounded counts/classes/budgets, and
  redacted guards; it never emits profiles, accounts, origins, or content.

## Current tests

Default crate tests exercise canonical order independence, canonical ULID
round trips, redacted diagnostics, every collection/budget/lifetime ceiling,
secret/read/same-endpoint/out-of-scope/widening/duplicate flow refusal, and
plan-node identity/scope/budget/expiry inheritance. They allocate no native or
provider resource and perform no I/O.
