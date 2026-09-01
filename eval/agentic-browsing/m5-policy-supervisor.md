# M5 policy and supervisor

Status: the immutable approved run-manifest contract, mutable plan-lease/model
input accounting, single-flight semantic effect policy, and immutable
non-widening delegation topology are implemented. The first mutable bounded
run-tree scheduler is also implemented; cancellation/drain propagation,
context assignment, provider adapters, audit sink, and live qualification
remain pending.

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

## Implemented mutable model-input boundary

- One policy instance installs exactly one unique mutable lease for every
  approved plan node. Missing nodes, duplicate nodes, and duplicate leases fail
  before any provider or native resource exists. Lease identity is separate
  from immutable plan-node identity so copied manifest facts cannot spend.
- A trusted-shell call request joins one strictly increasing nonzero call ID,
  exact lease, exact context/account attestation, monotonic admission time, and
  explicit input-envelope/output/cost ceilings. It is not authority; policy
  mints a non-cloneable admission only after all checks pass.
- Model disclosure requires the explicit `Read` effect at both run and node
  scope in addition to exact run, context/document/cancellation generation,
  profile, account, canonical source origin, non-secret sensitivity, current
  account attestation, manifest/node lifetime, and mutable budget.
- The semantic observation or bounded-read payload must have been encoded from
  the exact supplied source. Its measured semantic input tokens plus bounded
  envelope input, output, cost, and one operation are reserved before any
  provider transport receives bytes. Consumed plus every pending reservation
  is checked jointly against run and node budgets.
- At most four prepared or delivered calls exist per run policy. Refusal or
  pre-delivery cancellation releases the complete reservation but never
  reopens its monotonic call ID. No provider, task, timer, queue, or worker is
  created by this functional core.
- Taint is committed only when the exact opaque delivery acknowledgement or
  read receipt matches the source guard retained at admission. Callers do not
  resupply model-facing bytes, selectors, page JavaScript, DOM, or source facts
  at this boundary, preventing post-transport source substitution.
- The content-free run-global taint ledger is keyed by exact source
  context/document/cancellation authority, observation identity/generation,
  account, and canonical origin. It retains the oldest trusted
  account-attestation time plus maximum sensitivity, worst trust, and the
  canonical union of opaque references actually committed to the model.
  Secret values remain mechanically absent; visible metadata of a redacted
  secret node is conservatively treated as sensitive.
- Taint persists after provider failure or post-delivery cancellation because
  the provider already received the input. Prepared and committed candidate
  unions are capped at 128 exact cohorts and 4,096 opaque references, so
  concurrent admissions cannot race either persistent ceiling.
- Actual provider usage atomically replaces its reservation in both run and
  lease accounting. A provider-reported overage records actual token/cost
  usage, removes the terminal call, and seals the run; mismatched or missing
  delivery/admission identity seals while retaining ambiguous reservations.
- SHA-256 admissions bind the manifest revision, call, lease, node, input kind,
  exact context, source projection, token/cost ceilings, and every redacted
  taint fact. Public diagnostics expose only redacted opaque identities,
  bounded counts/classes, accounting, and redacted guards.

## Implemented semantic effect boundary

- Effect authorization accepts only one exact prepared semantic action, its
  independently classified actual effect/canonical destination, a strictly
  increasing effect identity, exact mutable plan lease, current account
  attestation, and an atomically sampled lifecycle/context state from the
  bounded context registry. Construction of an assessment or account binding
  is not proof; the eventual fixed adapter remains responsible for sourcing
  those facts independently of model output.
- Policy requires that the exact action observation generation, frame origin,
  opaque target reference, and select option reference (when present) were
  actually committed to the model. A bounded read cannot be used to guess an
  undisclosed `@aN` reference. No selector, DOM, page JavaScript, raw content,
  provider response, or generic native bridge crosses this boundary.
- Every committed taint cohort is checked as a source before a sink can be
  admitted. Cross-profile context is a hard refusal. Sources outside the exact
  node scope pause for scope expansion. Cross-origin/account non-read transfer
  requires the exact manifest flow rule, sensitivity ceiling, and effect.
  Same-endpoint writes need no synthetic transfer rule. Read effects have no
  outward sink. Cross-origin writes remain `NeedsHuman` until a fixed adapter
  can independently attest destination account state.
- Capability boundaries, scope expansion, absent data-flow approval, human
  control, and unproven cross-origin writes produce content-free,
  non-authorizing `NeedsHuman` transitions. They reserve no operation and
  cannot be converted into a permit by the model.
- One non-cloneable permit freezes additional model input, binds the complete
  immutable manifest revision and committed taint ledger, and reserves exactly
  one run/node operation. At most one effect is authorized or dispatched per
  run policy. Pre-dispatch refusal releases the reservation, while dispatched
  success or typed failure consumes it exactly once.
- Dispatch requires a second actor-owned account/lifecycle sample immediately
  before the native boundary. Changed context, cancellation, freshness,
  control, expiry, or account authority consumes the one-shot attempt identity
  and releases the reservation without minting an active effect. Attempt
  replay or permit/action substitution seals the policy while retaining
  ambiguous accounting.
- Success settlement requires the exact non-cloneable active effect and a real
  independent `SemanticVerifiedAction` proof for the same prepared action and
  attempt. Failure settlement uses only the existing closed semantic failure
  taxonomy. Public diagnostics and receipts remain content-free and redact
  origins, account state, action guards, and page values.
- This is an allocation-only functional core with no idle task, timer, queue,
  worker, provider, page, native view, or native input. The only persistent
  additions are bounded policy rows and content-free provenance.

## Implemented delegation topology

- One canonical topology is bound by SHA-256 to the exact manifest revision,
  run, sole root, every plan-node identity, direct parent, and computed depth.
  Input ordering cannot change its revision. It retains no objectives, prompts,
  model output, page content, origin text, tasks, queues, workers, timers, or
  native resources.
- Every topology node must already exist in the exact manifest. Duplicate
  nodes, zero or multiple roots, missing parents, self-edges, indirect cycles,
  and disconnected chains fail before mutable scheduling exists. The topology
  itself remains bounded by the manifest's 64 approved plan nodes.
- Delegation depth is proven from the complete parent chain and hard-capped at
  two. The later mutable supervisor will separately enforce the initial eight
  live/four executing defaults; pre-approving a plan node does not make it live
  or allocate execution resources.
- Every direct child must be a subset of its parent across profiles, account
  states, canonical origins, effects, sensitivity, all four budget dimensions,
  and expiry. This is stricter than the manifest's run-level inheritance and
  prevents an otherwise run-valid child from widening delegated authority.
- Public projections expose only redacted plan/run identities, parent links,
  depth, counts, and a redacted guard. Exact manifest and topology revision
  matching prevents a topology from being reused after approval changes.

## Implemented bounded run-tree scheduler

- One process-local supervisor incarnation owns one validated topology and
  initially activates only its root as queued. It creates no model stream,
  future, task, worker, timer, channel, provider, context, page, or native
  resource. Pre-approved topology nodes consume nothing until their exact
  executing parent activates them.
- A child can activate only once, only through its pre-approved direct parent,
  and only while a non-cloneable exact parent execution token is current. Up to
  eight nonterminal nodes may be live. Terminal history stays bounded by the
  topology's 64-node ceiling, frees live capacity, and can never reopen.
- Up to four nodes may execute concurrently. A successful admission mints a
  non-cloneable token bound to the supervisor incarnation, topology revision,
  plan node, and strictly increasing attempt identity. A capacity refusal does
  not consume the attempt; an admitted attempt can never replay.
- Waiting is an explicit content-free state that retains no execution slot or
  asynchronous work. A node waiting for descendants cannot resume while any
  activated descendant remains live. A terminal request made too early safely
  becomes this descendant wait instead of losing execution authority.
- Success and the closed supervisor failure taxonomy are terminal only after
  all activated descendants settle. Status and zero-allocation node iteration
  report activated/live/executing/queued/waiting/terminal counts without
  objectives, prompts, model output, provider text, page content, or origins.
- Cross-incarnation, stale, substituted, or duplicate execution tokens cannot
  change node state. Ambiguous same-supervisor settlement seals mutation and
  retains the executing slot in accounting. Exact cancellation and drain
  acknowledgement will extend these states next; the scheduler does not yet
  claim that running external work has been cancelled.

## Current tests

Default crate tests exercise canonical order independence, canonical ULID
round trips, redacted diagnostics, every collection/budget/lifetime ceiling,
secret/read/same-endpoint/out-of-scope/widening/duplicate flow refusal, and
plan-node identity/scope/budget/expiry inheritance. Ten additional policy tests
execute real semantic observation/read encode, exact token admission, mutable
reservation, committed delivery receipt, taint, cancellation, and provider
settlement paths. Seven further policy tests execute real action bind/prepare,
registry freshness acknowledgement, exact observation/read delivery,
source-to-sink decisions, permit reservation, final dispatch revalidation,
independent semantic verification, cancellation, failure, and accounting.
Together they cover nonadjacent duplicate leases, four-call, 128-cohort, and
4,096-reference limits; aggregate and effect operation budgets; replay; empty
reads; stale/future account authority; wrong run/profile/account/origin/effect/
sensitivity; guessed read references; absent/exact flow rules; pending model
calls; capability `NeedsHuman`; stale dispatch state; action substitution;
payload/receipt substitution; provider failure/overage; and redacted
diagnostics. They allocate no native or provider resource and perform no I/O.
Three delegation-topology tests additionally cover canonical order, exact
manifest revision binding, the valid depth-two boundary, every malformed tree
shape, the 64-node preflight ceiling, and parent widening through origin,
effect, sensitivity, operation budget, or expiry.
Three mutable-scheduler tests execute a complete depth-two tree, descendant
wait/resume/terminal ordering, sequential live-slot reuse, the exact eight-live
and four-executing boundaries without eviction, non-consumption of capacity
refusals, admitted-attempt replay refusal, cross-incarnation token rejection,
terminal failure projection, and redacted diagnostics.
