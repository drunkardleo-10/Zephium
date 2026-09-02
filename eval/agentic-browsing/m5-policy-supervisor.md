# M5 policy and supervisor

Status: the immutable approved run-manifest contract, mutable plan-lease/model
input accounting, bounded per-origin semantic effect policy, and immutable
non-widening delegation topology are implemented. The first mutable bounded
run-tree scheduler and its exact cancellation/drain tree are also implemented;
manifest-bound context assignment is implemented over the existing bounded
context registry, and content-free semantic progress is projected directly by
the supervisor. A bounded semantic-audit ledger and typed persistence port are
implemented. The provider-neutral identity, usage, failure, retry-after,
stream-budget, and bounded SSE framing contracts are implemented. OpenAI
Responses and Anthropic Messages plain-text, tool-call, error, stop, and usage
stream normalization are implemented behind one provider-neutral decoder.
The closed browser-tool proposal decoder and fixed request/tool-schema encoding
for both providers are implemented with atomic model-input commitment and a
fixed, provider-attested billing class. Exact one-shot screenshot tool-result
encoding, whole-multimodal-input admission, sensitive visual taint, and
content-free delivery receipts are implemented for both providers. Exact
schema/read-bound extraction tool-result replay, tool-free constrained-output
requests, bounded response collection, and Rust output admission are also
implemented for both providers. The durable Store adapter and fixed BYOK HTTPS
transport are implemented. Terminal transport evidence now selects
exact-zero, reservation-ceiling, or move-only pricing-required policy
settlement without exposing active authority. Checked provider pricing and its
distinct catalog-ceiling accounting are implemented without product rate
entries. A priced terminal receipt retains content-free provider/billing,
catalog revision, exact schedule digest, and cache/cache-write/reasoning usage
subsets; it retains no model label, tokenizer label, rates, response, prompt, or
credential. Live provider and production pricing-catalog qualification remain
pending.

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
- The semantic observation, exact diff, or bounded-read payload must have been
  encoded from the exact supplied source. A diff additionally requires that
  its exact baseline fingerprint is already committed in this policy; reused
  observation/generation coordinates cannot stand in for a different
  baseline. Its measured semantic input tokens plus bounded envelope input,
  output, cost, and one operation are reserved before any provider transport
  receives bytes. Consumed plus every pending reservation is checked jointly
  against run and node budgets.
- At most four prepared or delivered calls exist per run policy. Refusal or
  pre-delivery cancellation releases the complete reservation but never
  reopens its monotonic call ID. No provider, task, timer, queue, or worker is
  created by this functional core.
- Taint is committed only when the exact opaque observation acknowledgement,
  baseline/current diff receipt, or read receipt matches the source guard
  retained at admission. Callers do not resupply model-facing bytes, selectors,
  page JavaScript, DOM, or source facts at this boundary, preventing
  post-transport source substitution.
- The content-free run-global taint ledger is keyed by exact source
  context/document/cancellation authority, observation identity/generation,
  private source fingerprint, account, and canonical origin. Exact diff
  admission transforms each committed per-origin reference union: retired
  references are removed before current/add/rebase references are installed,
  unchanged references persist, and same-origin subframes cannot reintroduce a
  retired token. It retains the oldest trusted
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
- Each non-cloneable permit freezes additional model input, binds the complete
  immutable manifest revision and committed taint ledger, and reserves exactly
  one run/node operation. Up to four distinct prepared/dispatched effects may
  coexist, matching the executing-agent ceiling; the exact same prepared
  action guard cannot be authorized twice.
- Durable `ExternalWrite`, `Communication`, `Purchase`, and `Destructive`
  effects hold one serialization slot for their canonical independently
  assessed destination origin from authorization through terminal settlement.
  A second durable write to that origin is refused without eviction. Reads,
  local writes, and approved durable writes to different canonical origins may
  progress independently. Every pending effect remains fully included in
  run/node operation accounting. Pre-dispatch refusal releases only its exact
  reservation, while dispatched success or typed failure consumes it once.
- Dispatch requires a second actor-owned account/lifecycle sample immediately
  before the native boundary. Changed context, cancellation, freshness,
  control, expiry, or account authority consumes the one-shot attempt identity
  and releases the reservation without minting an active effect. Attempt
  replay or permit/action substitution seals the policy while retaining
  ambiguous accounting.
- Success settlement requires the exact non-cloneable active effect and a real
  independent `SemanticVerifiedAction` proof for the same prepared action and
  attempt. Pre-verification failure settlement consumes the exact opaque
  execution/settlement refusal and derives its closed failure internally; the
  raw caller-selected method is crate-private. Verification refusal likewise
  maps its exact error once. Public diagnostics and receipts remain content-free
  and redact origins, account state, action guards, and page values.
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
  retains the executing slot in accounting.
- A strictly increasing one-shot cancellation update applies to an activated
  subtree under one of seven closed content-free reasons. It revokes later
  execution and delegation immediately. Queued leaves cancel synchronously;
  non-running ancestors remain cancellation-pending until every activated
  descendant is terminal.
- Running nodes stay both live and executing after cancellation is requested.
  The returned batch contains at most four content-free signal targets and
  grants no execution, provider, action, page, or native authority. Capacity is
  released only when the original non-cloneable execution token reports
  terminal drain. A racing ordinary wait/completion callback is forced to the
  pending cancellation outcome and cannot claim success. Current targets can
  be projected again without mutation so a shell can reconcile a lost signal.
- Nested ancestor cancellation does not overwrite an earlier child-branch
  cancellation. A wrong cancellation identity, substituted execution token,
  or ambiguous drain seals mutation and retains executing capacity. Once the
  exact running drains settle, resource-free cancelling ancestors terminalize
  from leaf to root. Root cancellation is therefore sticky across the whole
  activated run tree without retaining an idle task, timer, worker, or channel.

## Implemented context scheduling join

- A running node can reserve a context only by presenting its exact current
  non-cloneable execution token, the exact manifest revision already bound to
  the topology, and a complete `ContextIdentity`/capability request to the
  existing `ContextRegistry`. The supervisor does not create a second native
  owner, view ledger, lifecycle, execution permit, or construction path.
- Admission requires the exact run owner and a profile in the node's canonical
  manifest scope. Run-global and node-local concurrent context budgets are
  checked before the registry mutates. The manifest hard ceiling remains four
  contexts per run; the existing process registry independently retains its
  eight logical/four executing ceilings and refuses without eviction.
- Assignments are canonical by opaque `ContextId`, unique within the run, and
  expose only redacted immutable identity and responsible plan node. A node
  cannot become success/failure terminal while any exact assigned context
  remains; it releases the agent execution slot into an explicit context wait
  instead.
- Context budget is released only through one of two existing registry-proven
  dispositions: cancellation of a never-started queued row, or reaping an
  active terminal row after its native resource was destroyed, transferred to
  Browse, or its borrowed Browse tab was retained. Cleanup remains available
  after supervisor fail-stop because it can only remove exact retained state.
- Subtree cancellation returns at most four assigned context cleanup targets
  in addition to running agent targets. Draining the agent execution alone does
  not terminalize its node while a context remains. Current context targets can
  be projected again after signal loss; exact registry cleanup then cascades
  resource-free cancellation terminality from leaf to root.
- The scheduling join stores no URL, origin, objective, prompt, provider data,
  page content, profile path, account label, native handle, or extension
  identity. When no run reserves a context it has zero task, worker, timer,
  queue, model, page, or native overhead.

## Implemented semantic progress projection

- Every activated scheduler row owns one current semantic-progress projection
  bound to the exact immutable manifest revision, mutable supervisor
  incarnation, and responsible plan node. It is current state rather than a
  transcript and adds no queue, task, timer, worker, channel, provider, page,
  native view, or persistence operation.
- The closed vocabulary separately represents operation class, opaque active
  resource, state, typed result, and typed blocker. It cannot represent an
  objective, prompt, hidden reasoning, provider output, raw tool chatter, page
  content, origin, selector, JavaScript, native handle, or arbitrary error.
- Queue/start/delegation/wait/completion/cancellation and exact context
  assignment/release update progress inside the same single-owner supervisor
  mutation. Explicit planning/observation/read/verification/persistence
  activities require a valid operation/resource pair and the exact current
  non-cloneable execution token.
- Model admissions/calls and effect permits/active attempts/receipts now carry
  their exact manifest revision in addition to their existing SHA-bound
  authority. Progress accepts their typed active/result evidence only when the
  manifest and responsible node match the exact executing supervisor.
- A policy-derived `NeedsHuman` transition can atomically project its exact
  effect/reason/context and release the execution slot only when the context is
  already assigned to that supervisor and node. The projection grants no
  approval or resume authority; later execution still re-enters through normal
  scheduler and policy checks.
- Public node snapshots include the current projection. Diagnostics expose
  only redacted identities and closed enums. Transition history and durable
  delivery are intentionally not simulated by this projection; the bounded
  audit ledger is a separate persistence boundary.

## Implemented bounded semantic-audit port

- A ledger can be created only against the exact queued-root supervisor before
  execution or context allocation begins. It records only the exact current
  supervisor-owned progress projection; callers cannot supply a different
  state. Strictly increasing event identities, non-regressing trusted
  monotonic time within the manifest lifetime, exact manifest/supervisor/node
  joins, and unchanged-projection refusal make accidental stale or duplicate
  append explicit.
- At most 64 undelivered events and one 16-event delivery prefix are retained.
  The queue never evicts or silently drops an event. Full capacity backpressures
  before consuming the candidate event identity. Empty/oversized delivery,
  concurrent delivery, and reused/regressed delivery identity fail before the
  retained prefix changes.
- Versioned SHA-256 event and delivery guards bind the manifest revision,
  supervisor, event/delivery identities, monotonic recording time, responsible
  node, closed operation/resource/state/result/blocker vocabulary, and exact
  ordered batch. A lost local handoff can reconstruct the same in-flight batch
  and proof without changing the queue.
- Only an exact durable `Committed` settlement removes the corresponding
  prefix. Proven pre-commit refusal and cancellation clear the attempt but
  retain every event for a later explicitly identified retry. A missing or
  substituted settlement fail-stops mutation and retains the complete
  ambiguous prefix; a later exact acknowledgement may still drain it.
- The Store adapter contract is transactional and idempotent by exact delivery
  identity plus proof. Reconciliation may replay only that same pair. Refusal
  or cancellation must prove that no event committed; uncertain or partial
  durable outcomes remain unsettled instead of being misreported as safe
  retry authority.
- Shutdown sealing rejects new events while preserving delivery and drain.
  Quiescence requires the seal, an empty queue, and no in-flight delivery. The
  ledger and port own no storage, I/O, task, timer, thread, channel, provider,
  page, or native resource; the durable adapter remains a separate imperative
  shell boundary.
- Events contain only opaque identities, trusted monotonic time, and the closed
  semantic progress fields. Objective text, prompts, hidden reasoning, model
  output, raw tool chatter, page content, origins, selectors, JavaScript,
  secrets, paths, native handles, and arbitrary errors are unrepresentable.

## Implemented durable Store audit adapter

- The port transfers a one-shot typed completion callback with an accepted
  delivery. A definite durable commit or proven pre-commit refusal invokes it
  exactly once. A commit-ambiguous or identity-conflicting result deliberately
  drops it, leaving the ledger's exact delivery in flight for replay instead of
  manufacturing retry authority.
- `SqliteStore` implements the port on its existing single-owner actor. It
  admits at most eight audit batches independently of the 256-command Store
  mailbox, uses nonblocking admission, rejects work once terminal shutdown is
  admitted, and serializes accepted appends before a later Store shutdown
  barrier. Completion callback panics are contained after the durable result so
  they cannot terminate the Store actor. Its admission counter allocates
  lazily; no agent task, worker, timer, connection, or queue exists while
  unused.
- META migration 16 stores app-global audit rows because one run may span
  profiles and the delivery intentionally contains no profile or page content.
  Each event is a canonical 128-byte version-one record of opaque identities,
  monotonic time, and closed progress codes. Delivery rows retain the exact
  ordered range and count. Internal
  manifest guards are never persisted because they are derived from scoped
  origins/accounts. Strict tables constrain every BLOB width, version, batch
  index, and foreign key; rows are immutable.
- One SQLite transaction inserts the delivery and all 1–16 events. Replaying
  the same manifest/supervisor/delivery identity verifies the proof and every
  event byte before acknowledging the prior commit; a substituted identity or
  payload is never settled. Insert failure is reported as refusal only after a
  proven rollback. Commit failure re-reads exact durable truth and otherwise
  remains unsettled.
- Durable history has a 262,144-event/delivery denial-of-service ceiling backed
  by constant-time state counters and triggers. Append never evicts an audit
  event. Retention/deletion therefore requires a future explicit product
  migration rather than becoming an implicit data-loss side effect.

## Implemented provider-neutral stream boundary

- The shared contract names only the OpenAI Responses and Anthropic Messages
  protocols, one bounded pinned model revision, exact output-token and stream
  ceilings, and content-free correlation copied from an existing policy
  admission or active call. Correlation omits the one-shot policy guard and
  cannot commit input, spend budget, settle a call, or authorize a retry.
- Every response caps accepted SSE events, plain-text bytes, client tool calls,
  aggregate wire bytes, and aggregate incomplete/complete tool arguments.
  Process maxima are 2 MiB of response body, 4,096 events, 64 KiB of plain
  text, eight tool calls, and 32 KiB of tool arguments; the normal per-call
  budget is lower where possible. No provider task, socket, credential, timer,
  queue, or worker exists while the feature is unused.
- The common incremental SSE framer accepts arbitrary transport chunk
  boundaries, CR/LF/CRLF, comments, multiple data lines, and unknown SSE
  fields. It validates UTF-8 and independently caps a line, assembled event,
  and total event count. Invalid UTF-8, an unterminated final event, or any
  ceiling breach fails closed without returning provider-authored text.
- Normalized terminal usage preserves authoritative input/output totals plus
  cached/cache-write/reasoning subsets without double counting. Real provider
  overage is not hidden by the adapter; policy remains responsible for
  accounting and sealing an exceeded reservation.
- Failures use a closed content-free taxonomy. Retry-after is nonzero, capped
  at one day, accepted only for rate-limit/overload/timeout/transport classes,
  and never grants retry authority. Any retry still requires a new supervisor
  decision, monotonic call identity, policy reservation, and cancellation
  check. Vendor codecs remain responsible for typed event sequencing; raw tool
  arguments cannot cross the public boundary.

## Implemented closed browser-tool boundary

- The model-facing vocabulary is exactly `navigate`, `back`, `forward`,
  `reload`, `snapshot`, `locate`, `act`, `wait`, `read`, `extract`,
  `screenshot`, `show_for_human`, and `resume_after_human`. Unknown tools and
  unknown JSON fields fail closed. JavaScript, selectors, XPath, DOM, HTML,
  CDP, native handles, paths, shell-minted operation IDs, and generic bridge
  requests have no representable variant.
- Arguments are capped before parsing and preflighted at a maximum JSON depth
  of 16. Provider tool-call identities are bounded opaque ASCII correlation.
  Navigation is capped at 8 KiB and reuses the shipping HTTP(S) target
  validator; semantic references must use canonical `@aN`; locate input is
  bounded hostile natural-language text explicitly forbidden from selector
  APIs; progressive scopes are closed reference/window proposals that still
  require exact acknowledged-observation binding.
- The provider locate proposal now moves directly into the shipping bounded
  semantic-query type, so provider and matcher limits cannot drift. Closed
  initial/region/subtree/table/frame scopes convert without copying the query;
  `surrounding_text` is deliberately refused for locate rather than silently
  widened. The matcher independently rejoins committed observation and current
  frame authority and returns only ranked opaque references; this conversion
  still grants no model continuation, policy, or browser authority.
- The locate result continuation is now distinct from page diffs. Only the
  exact prior tool-only `locate` correlation can append one token-admitted
  `ZLOC1` result to the bounded stateless transcript; OpenAI and Anthropic use
  their fixed matching tool-result shapes. A pinned local counter measures the
  complete immutable replay before policy admission. Policy rejoins the exact
  previously committed observation/account/source guard and proves every
  returned reference exists in exactly one retained source cohort. Commit
  merges only those unchanged baseline cohorts, so locate cannot add taint or
  authority, while its content-free receipt preserves the same acknowledgement
  for the next bounded turn. Refusal/cancellation drops the move-only delivery
  authority.
- Read result continuation is independently typed from both locate and diff.
  Only an exact prior tool-only `read` correlation can append its token-admitted
  `ZREAD1` bytes. The read must match the complete already-acknowledged source
  fingerprint, not only observation/generation/context coordinates. Both fixed
  provider replays require an `ExactLocal` latest-result count and whole-body
  count before policy admission. Policy rejoins every exact committed baseline
  cohort, proves each returned reference belongs to exactly one matching
  origin, and merges those cohorts unchanged; empty results remain admissible
  without adding taint or references. The read receipt itself still exposes no
  observation acknowledgement. A successful bound request carries the prior
  acknowledgement separately and move-only so a later tool turn remains
  possible, while standalone reads cannot manufacture diff or progressive-
  scope authority. Diff binding explicitly rejects locate, read, extract, and
  screenshot results.
- Extraction result continuation is terminal and independently typed from
  read, locate, diff, and screenshot. The exact decoded `extract` correlation
  retains its shell-registered schema ID. Only that schema, the exact committed
  baseline, a bounded read derived from it, a strictly newer same-plan call,
  and a token-admitted combined `ZEXTRACT1` schema/read payload can construct
  the mapping draft. Schema definitions with the same ID but changed fields or
  bounds, read/fingerprint substitution, and generic diff substitution fail
  before policy mutation.
- `act` converts immediately into the existing click/fill/select/press/scroll,
  effect, wait, verification, and settle types. Existing secret/control-text
  refusal and outcome compatibility run during decode. The preflight also caps
  eight actions, 16 KiB aggregate fill text, 60 seconds aggregate settle time,
  homogeneous effects, and one-action durable/capability boundaries before an
  observation is touched; normal exact observation binding repeats the
  relevant checks.
- Standalone waits carry a typed condition and relative timeout from which the
  shell must derive one absolute deadline. Extraction selects only an opaque
  shell-registered schema ID. Screenshot remains the already implemented
  viewport-only v1 scope. Human pause/resume values are closed supervisor
  proposals, never direct control or resume authority.
- Every decoded tool call remains non-authorizing. The provider stream must
  reach a valid terminal `ToolCalls` conclusion; the supervisor must then bind
  current context/observation state and policy must independently classify and
  authorize any effect before a native adapter can run.

## Implemented fixed provider request and commit seam

- An approved objective is capped at 8 KiB/4,096 exact tokens, rejects unsafe
  controls and recognized credential/authorization forms, and binds the same
  tokenizer revision as the semantic payload and provider model profile. Its
  content is absent from diagnostics and reused by reference across turns.
- OpenAI and Anthropic bodies are generated only from one immutable
  instruction, the approved objective, one existing token-admitted
  `ZSEM1`/`ZREAD1` payload, and the same 13 closed browser tools. OpenAI sets
  streaming, `store:false`, disabled truncation, and
  `parallel_tool_calls:false`, plus `service_tier:default`. Anthropic uses one
  user turn with two ordered text blocks because its API combines consecutive
  same-role turns, and sets `tool_choice:auto`,
  `disable_parallel_tool_use:true`, `service_tier:standard_only`, and
  `inference_geo:global`. The stable Messages API does not receive its
  beta-only `speed` field. Neither body has an arbitrary system prompt,
  conversation state, metadata, provider-native browser tool, selector,
  JavaScript, DOM/HTML, CDP, native handle, or secret field.
- Every function uses strict structured output. All object fields are required
  and every object recursively has `additionalProperties:false`. OpenAI
  receives the complete schema ranges. Anthropic receives a deterministic
  projection without unsupported range/length/cardinality keywords while the
  full Rust decoder remains the authoritative bounds check, matching the
  provider's documented schema-transform guidance. The 13 strict tools and 16
  union parameters are checked against Anthropic's current 20/16 compiler
  ceilings before serialization. Local decoding and policy checks remain
  mandatory even when a provider claims strict conformance.
- Extraction switches from browser tool calling to a separate tool-free
  constrained-output turn. OpenAI uses strict Responses `text.format` JSON
  Schema; Anthropic uses stable Messages `output_config.format`. Both serialize
  one fixed universal envelope with four tagged value variants and canonical
  `@rN` source arrays, never the caller's dynamic field schema. Product schema
  names and page values remain in bounded message content rather than
  Anthropic's documented 24-hour compiled-schema cache. Anthropic receives the
  deterministic unsupported-constraint projection, while Rust still enforces
  the original schema and all aggregate/value/provenance/sensitivity limits.
  This follows the current
  [OpenAI structured-output contract](https://developers.openai.com/api/docs/guides/structured-outputs)
  and [Anthropic structured-output/cache contract](https://platform.claude.com/docs/en/build-with-claude/structured-outputs).
- Provider/model/tokenizer/billing choice, a nonzero trusted pricing revision,
  the schedule's inclusive input range, exact pinned fixed-envelope/schema token
  count, objective tokens, semantic tokens, output ceiling, and request-body
  bytes are checked before transport. A request outside its bound price range
  fails before reservation or disclosure. Bodies are capped at 2 MiB and
  expose only a fixed endpoint enum plus exact redacted call correlation to the
  trusted HTTP shell.
- Construction validates and serializes before asking policy to reserve, so a
  malformed provider profile or encoding failure cannot orphan an admission.
  After reservation, construction is infallible and drops the original
  semantic string, retaining only the one body copy, content-free metrics, and
  private delivery authority. A one-shot settlement either commits exact
  observation/read taint and returns active usage authority joined to a
  cloneable content-free input proof and its exact input metrics, or releases
  the full reservation on pre-commit refusal/cancellation without surfacing a
  metrics sample. The fixed metrics value records serialized body bytes, the
  existing closed semantic/screenshot stats, the newest semantic token count
  when applicable, and a whole structured-input count only when that trusted
  local count ran. It is at most 64 bytes and contains no content, image,
  tokenizer label, URL, provider response, or serialization authority.
  Commitment additionally mints a copyable content-free metric receipt bound
  to the private canonical manifest revision and exact call/lease/node. The
  receipt is compile-capped at 192 bytes, cannot be publicly constructed, and
  grants no provider, policy, transport, continuation, or browser authority.
  Observation proof exposes the exact current acknowledgement needed to
  compute a later diff; a read proof cannot become diff authority. The
  stateless builders mechanically exclude `ZDIFF1`: a standalone diff would
  omit the acknowledged model context it modifies.
- A constrained extraction draft holds its schema/read delivery authority and
  fixed replay until a synchronous pinned local counter measures the exact
  complete body. Policy independently checks manifest/call/lease/node,
  account/context, exact schema/read guard, committed observation baseline, and
  unchanged taint before reserving that measured count. Refusal/cancellation
  drops the authority; commit returns a content-free receipt and a one-shot
  output binding, deliberately retaining no continuation transcript.
- The extraction collector begins only from the matching committed receipt,
  retains no more than 64 KiB, clears and poisons on wrong-call, tool, capacity,
  or byte-limit failure, and admits only an exact `Completed`, tool-free
  terminal with matching text-byte statistics. The existing Rust extraction
  decoder then validates strict JSON, exact schema definition and order,
  source provenance, sensitivity, secrets, and all limits. The admitted result
  remains `ModelMapped` and creates no browser or provider continuation
  authority. No provider request was issued for this evidence.
- A screenshot result is a distinct one-shot continuation rather than a generic
  body or standalone image turn. It requires an exact prior `screenshot`
  tool-only stop, fixed provider configuration, strictly newer same-plan call,
  and the exact committed observation/generation/context that authorized the
  viewport capture. The retained delivery guard covers every canonical PNG
  byte and validated content-free metric. OpenAI encodes the image as the sole
  high-detail `input_image` in the matching `function_call_output`; Anthropic
  encodes it as the sole image in the matching adjacent `tool_result`, with
  `oversized_image:error` to reject coordinate-changing server resize.
- Visual drafts cap canonical PNG at 1,300,000 bytes and private replay text at
  64 KiB inside the unchanged 2 MiB whole-body ceiling. Base64 is allocated
  once with checked exact capacity. Only a synchronous pinned provider-specific
  local counter over the exact complete multimodal body can admit the request;
  provider-backed counts and non-exact quality fail. Policy then revalidates
  manifest/call/lease/node, account/context, the exact source observation and
  every observed frame origin before reserving the measured tokens.
- Transport refusal or pre-commit cancellation drops screenshot authority and
  releases the reservation. Exact commitment mints a content-free visual
  receipt and adds `Sensitive` / `UntrustedPage` taint for every observed frame
  origin with zero opaque references. The receipt cannot recreate pixels or
  seed a diff/tool continuation, and the private transcript is deliberately
  dropped at commit. Pixel/receipt substitution seals policy.
- This exclusion follows the current primary contracts. OpenAI's
  [latest-model guidance](https://developers.openai.com/api/docs/guides/latest-model)
  requires stateless/`store:false` callers to replay the relevant returned
  output items, including encrypted reasoning items where applicable.
  Anthropic's
  [Messages guide](https://platform.claude.com/docs/en/build-with-claude/working-with-messages)
  states that the API is stateless and requires the full conversational
  history. The current decoder intentionally retains neither replay form, so
  adding a diff-only body would be incorrect rather than a token optimization.

## Implemented fixed HTTPS transport and policy-settlement join

- The imperative shell accepts only an already-built move-only provider input,
  one provider-matching zeroizing credential, exact cancellation, and the
  existing run policy. Production endpoints are compile-time constants for
  OpenAI Responses and Anthropic Messages; arbitrary URLs exist only in private
  test construction restricted to loopback.
- One shared rustls client uses the system proxy, refuses redirects, disables
  automatic retries and referer propagation, requests identity encoding, and
  applies bounded connect, idle-read, whole-request, and HTTP/2 header limits.
  At most four admitted attempts exist. A slot is acquired before semantic
  disclosure commits, is released on every terminal path, and seals the shared
  transport if a committed attempt is abandoned.
- A zero-active snapshot is explicitly `idle`, not terminal: admission remains
  possible until the shared transport's cancellation/admission seal is set.
  The transport can mint its move-only shutdown proof only from a sticky
  sealed, exactly zero-active snapshot; pending attempts and open admission are
  distinct refusals, and no shared clone can reopen a proved transport. The
  proof owns no task, timer, content, credential, policy, usage, audit, or
  native authority. The lifecycle must still terminally settle each provider
  attempt's separately retained policy authority before claiming clean exit.
  The async drain seam seals synchronously before it returns a future, then
  registers a notification before each state sample and waits only for the
  exact last slot or one caller-owned absolute deadline. Cancelling that future
  cannot reopen admission; one shared atomic admission bounds drain waiters,
  and no polling task or periodic timer is retained.
- A committed attempt exposes only the content-free
  observation/read/extraction/screenshot proof retained by request admission
  and the fixed input metrics that crossed that same commit. The transport
  input has no public metrics accessor, so a rejected credential, capacity
  refusal, shutdown, or pre-commit cancellation cannot be misreported as
  disclosure. The attempt also delegates the exact copyable metric receipt for
  an optional run-local reducer; it does not mint or reconstruct a sample in
  the transport shell. A shell may clone the observation proof
  before consuming the attempt so a future bounded continuation can compute an
  exact diff. A read receipt cannot expose an acknowledgement; only the fixed
  bound-read transport may carry forward the exact acknowledgement already in
  its private transcript. Screenshot proof is audit correlation only and
  cannot seed another continuation. Extraction proof is usable only by its
  purpose-bound terminal output collector and likewise cannot seed a
  continuation. Losing an eligible observation proof
  cannot widen authority and requires a fresh snapshot; request bytes,
  semantic strings, and image bytes are never duplicated for continuation.
- The codec is not a provider privacy qualification. OpenAI documents that
  image inputs are scanned and that a flagged image may be retained for manual
  review even under Zero Data Retention or Modified Abuse Monitoring; see
  [OpenAI data controls](https://platform.openai.com/docs/models/default-usage-policies-by-endpoint#image-and-file-inputs).
  Production visual enablement therefore still requires an explicit reviewed
  provider/model/data-handling catalog and installed local visual counter. No
  provider request or image disclosure was made for this evidence.
- Credentials are provider-bound, copied once into a sensitive header, and
  zeroized on drop. Debug output exposes only provider and byte count. Request
  bodies, authorization values, provider-authored error bodies, response text,
  semantic content, and model values are absent from transport diagnostics.
- Cancellation is sticky and checked before commitment, immediately before
  the HTTP send future can be polled, during send, and before every body chunk.
  A committed attempt proven not to have polled send carries
  `ExactZeroBeforeDispatch`; once send may have been polled, any missing usage
  carries `UnknownAfterDispatch`. Network errors, HTTP statuses, redirects,
  response-header violations, decoder failures, consumer cancellation, and
  body cancellation therefore cannot manufacture zero usage.
- The transport retains the exact provider/model/tokenizer configuration and
  fixed billing class, and joins them to the decoder conclusion and
  non-cloneable active policy authority. Active authority has no public
  extraction path. Pre-dispatch failure settles exact zero, preserving
  committed semantic taint while releasing unused token and cost reservation.
  Ambiguous post-dispatch failure/cancellation settles the complete reservation
  ceiling.
- A decoder conclusion with provider usage becomes a move-only
  pricing-required settlement. It exposes only the exact redacted configuration,
  normalized counters, and content-free conclusion to the trusted pricing
  adapter. Completed responses have no unpriced fallback: if the matching fixed
  pricing revision is unavailable, the object retains authority for retry or
  reconciliation rather than inventing cost or releasing the reservation.
  Provider terminal failures with exact usage follow the same pricing path;
  those without usage consume the reservation ceiling.
- The pure pricing schedule requires exact provider, model, tokenizer, billing
  class, pricing revision, and input-range identity. It prices uncached input,
  cached reads, cache writes, and inclusive output as four disjoint categories
  in micro-USD per million tokens, with checked integer arithmetic and one
  upward rounding. Reasoning is already included in output. Success produces an
  opaque `PricedCeiling` policy value rather than a raw caller-supplied cost.
  Identity, range, and arithmetic refusal return the boxed move-only settlement
  before policy is invoked, so corrected lookup or reconciliation cannot lose
  authority. No production model/rate schedule is implied by this contract.
- The transport issues at most one POST per admitted attempt and does not read
  non-200 response bodies. It streams bounded chunks directly through the
  existing provider-neutral decoder and exposes only normalized nonempty event
  batches to its callback. No raw SSE, generic HTTP response, provider-native
  browser tool, selector, JavaScript, DOM, HTML, CDP, retry authority, queue,
  worker, timer, or idle task crosses the public boundary.
- The repository boundary gate mechanically pins the two production endpoints,
  rustls/system-proxy dependency feature set, redirect/retry refusal, sensitive
  credential marking, identity encoding, pre-send cancellation proof, and
  move-only settlement route. It also pins both fixed billing classes, their
  request controls, their decoder attestations, and refusal of the beta-only
  Anthropic speed field on the stable endpoint. The gate additionally pins
  price identity/range/category checks, the opaque `PricedCeiling` join,
  authority recovery on pre-policy pricing refusal, and absence of the former
  raw-cost terminal API. It rejects public loopback/custom-endpoint
  construction, TLS-verification bypass, or active-authority decomposition.

## Implemented OpenAI Responses stream slice

- The incremental Responses decoder follows the official
  [Responses create/stream contract](https://developers.openai.com/api/reference/typescript/resources/beta/subresources/responses/methods/create).
  It requires the selected exact model revision, one bounded response identity,
  `created` before output, one unambiguous terminal event, complete SSE framing,
  and consistent terminal status. A mismatched response, model, event name,
  status, terminal, or event order fail-stops the decoder.
- Plain-text and refusal deltas are capped cumulatively and returned only as an
  explicitly untrusted plain-text type with redacted diagnostics. The decoder
  incrementally hashes deltas and requires the provider's corresponding `done`
  value to match before a successful terminal result. Terminal output classes
  must agree with the streamed class, so missing or substituted streamed text
  cannot be silently accepted.
- Completed/incomplete responses normalize exact input, output, cached,
  cache-write, and reasoning-token counters. `total_tokens` must equal checked
  input plus output; cached and cache-write counters are disjoint subsets and
  reasoning remains an output subset. Every lifecycle and terminal response
  must attest the requested `default` service tier before its usage is trusted.
  Output-limit, content-filter, and refusal stops are distinct from
  provider/cancellation failure. A failed stream may truthfully carry no usage
  so the later policy integration can apply a documented conservative
  settlement instead of inventing zero usage.
- Function-call IDs, names, ordered item lifecycle, delta bytes, final
  arguments, item completion, and terminal output are joined exactly. Up to
  the configured call/argument limits, each completed call is emitted only as
  the closed proposal above; raw JSON is dropped. Substitution, duplicate or
  partial lifecycle, invalid proposal JSON, provider built-in tools, hidden
  reasoning output, unknown output classes, invalid JSON/UTF-8, and premature
  `[DONE]` fail-stop the stream.

## Implemented Anthropic Messages stream slice

- The incremental Messages decoder follows Anthropic's official
  [stream lifecycle](https://platform.claude.com/docs/en/build-with-claude/streaming),
  including exact matching SSE/data event types, `message_start`, sequential
  indexed content blocks, one or more cumulative `message_delta` updates,
  `message_stop`, pings, mid-stream errors, and graceful ignoring of unknown
  future top-level event types. The selected model, assistant role, empty
  initial content, bounded message identity, null initial stop, complete block
  closure, and one terminal outcome are exact.
- Text streams directly as bounded untrusted plain text without retaining a
  second aggregate copy. Tool JSON remains private and bounded until block
  closure, but no proposal is emitted until the terminal stop reason is
  `tool_use`. An incomplete tool at `max_tokens` therefore returns a typed
  output-limit result and zero tool proposals; a complete tool is decoded once
  through the same closed Rust browser-tool contract.
- `input_tokens`, cache-read, and cache-creation counters are normalized using
  Anthropic's documented additive input total. Cumulative usage cannot regress;
  cache-read and cache-creation are disjoint within that normalized total, and
  output-reasoning detail remains a subset. Initial usage must attest
  `service_tier:standard` and `inference_geo:global`, matching the explicit
  stable request controls, before any provider usage is trusted. Natural
  completion, client tool use, output/context limits, refusal, custom stop, and
  paused turns map to the shared stop vocabulary.
- Thinking/signature blocks, server tools/results, citations deltas, fallback
  blocks, unknown tool names, a second tool block despite disabled parallel
  use, malformed arguments, index/interleaving errors, model mismatch, usage
  regression, `[DONE]`, and ceilings fail closed. Known stream errors map to
  the shared content-free failure taxonomy; provider error messages are never
  retained or exposed.
- One public provider-neutral decoder selects OpenAI or Anthropic only from the
  already fixed call configuration. The shell therefore receives identical
  bounded batches and terminal contracts without branching on vendor wire
  data.

## Implemented clean terminal policy settlement

- Terminal policy settlement consumes the single-owner `AgentRunPolicy`; there
  is no successful path that leaves model/effect admission reusable. It joins
  only the already-closed metric value and exact receipt-accounting reducer,
  including the private canonical manifest revision and supervisor. It also
  consumes the exact run audit ledger after shutdown seal and durable drain.
- A sealed policy or any pending model call, effect, serialized origin write,
  operation reservation, token reservation, or cost reservation refuses
  settlement. Run-wide and every canonical plan-node consumed operation,
  model-token, and cost total must equal receipt accounting, with checked token
  arithmetic.
- Refusal retains and returns the entire move-only policy for exact cleanup or
  corrected retry together with the complete audit ledger for drain or
  reconciliation. A fail-stopped, unsealed, pending, in-flight, foreign, or
  incompletely committed ledger cannot settle. Successful settlement is
  copyable, compile-capped at 256 bytes, redacted, and non-authorizing. It does
  not qualify native resources, sites, providers, devices, or production
  execution.
- The release boundary pins the consuming signature, retained-policy refusal,
  retained audit ownership, zero-debt checks, exact private joins, run/node and
  durable-event reconciliation, redacted diagnostics, size ceiling, and
  absence of a constructor, serialization, or runtime port.

## Current tests

Default crate tests exercise canonical order independence, canonical ULID
round trips, redacted diagnostics, every collection/budget/lifetime ceiling,
secret/read/same-endpoint/out-of-scope/widening/duplicate flow refusal, and
plan-node identity/scope/budget/expiry inheritance. Policy tests
execute real semantic observation/read encode, exact token admission, mutable
reservation, committed delivery receipt, taint, cancellation, and provider
settlement paths. Ten further policy tests execute real action bind/prepare,
registry freshness acknowledgement, exact observation/read delivery,
source-to-sink decisions, permit reservation, final dispatch revalidation,
independent semantic verification, cancellation, failure, and accounting.
Together they cover nonadjacent duplicate leases, four-call, 128-cohort, and
4,096-reference limits; aggregate and effect operation budgets; replay; empty
reads; stale/future account authority; wrong run/profile/account/origin/effect/
sensitivity; guessed read references; absent/exact flow rules; pending model
calls; four-effect and per-origin-write ceilings; duplicate action guards;
distinct-origin concurrency and out-of-order settlement; capability
`NeedsHuman`; stale dispatch state; action substitution;
payload/receipt substitution; exact diff-baseline absence/substitution;
add/rebase/current-reference transformation; provider failure/overage; and
redacted diagnostics. They allocate no native or provider resource and perform
no I/O.
Three delegation-topology tests additionally cover canonical order, exact
manifest revision binding, the valid depth-two boundary, every malformed tree
shape, the 64-node preflight ceiling, and parent widening through origin,
effect, sensitivity, operation budget, or expiry.
Three mutable-scheduler tests execute a complete depth-two tree, descendant
wait/resume/terminal ordering, sequential live-slot reuse, the exact eight-live
and four-executing boundaries without eviction, non-consumption of capacity
refusals, admitted-attempt replay refusal, cross-incarnation token rejection,
terminal failure projection, and redacted diagnostics.
Three cancellation tests additionally cover complete depth-two propagation,
queued immediate terminal state, resource-free ancestor draining, exact
execution-slot retention, one-shot cancellation replay, revoked delegation,
late-success suppression, nested branch/run cancellation identity, exact drain
acknowledgement, mismatch sealing without early capacity release, and redacted
content-free batches.
Three context-scheduling tests cover exact manifest revision/run/profile joins,
run and node context ceilings, duplicate refusal, atomic registry reservation,
context-wait terminal ordering, queued cancellation, active owned-context
construction/close/reap disposition, cancellation target reconciliation, and
retention of node liveness after execution drain until exact context cleanup.
Four semantic-progress tests cover queue/active/wait/failure projection,
operation/resource pairing, exact manifest/node typed receipts, model/effect
results, context ownership and cancellation, policy-derived human waits,
execution-slot release, and redacted diagnostics.
Five audit-ledger tests cover exact queued-root construction, current-projection
and authority joins, event/time replay, duplicate refusal, the 64-event and
16-event ceilings, non-consuming backpressure, single-flight delivery,
reconstruction, refused/cancelled retry, exact prefix commit, mismatch
fail-stop retention, shutdown quiescence, the closed port contract, and
redacted diagnostics.
Six Store-adapter tests cover schema widths/version/immutability/counting,
transactional append, exact idempotent replay across process reopen,
substituted-delivery refusal without mutation, independent eight-batch actor
admission, asynchronous settlement, callback-panic containment, and
retained-ledger reconciliation.
Provider/request-boundary tests cover configuration/usage/retry ceilings,
fragmented CR/LF/CRLF SSE framing, multiline data, comments, invalid UTF-8,
line/event/event-count/aggregate-wire exhaustion, exact model/response joins,
text hashing, terminal usage and billing-class attestation, output limits,
failed/incomplete responses, typed OpenAI function-call and Anthropic
content-block lifecycles, cumulative
cache-aware Anthropic usage, unknown future events, typed stream errors,
truncated/malformed/empty tool arguments, every closed browser tool and semantic
action class, URL/reference/query/schema validation, secret and mixed-effect
refusal, unknown/generic bridge field refusal, argument depth/size exhaustion,
unsupported built-in and reasoning output, objective secret/tokenizer/quality
refusal, recursive strict-schema completeness, atomic preflight/reservation,
observation/read one-shot commitment for both providers, exact input-proof
retention through transport admission, contextless-diff exclusion,
exact-fingerprint read-result binding, OpenAI and Anthropic `ZREAD1`
tool-result shapes, latest-result and whole-replay local counting,
same-coordinate substitution refusal, empty-result baseline rejoin, unchanged
taint/reference inventory, read-receipt non-promotion, and next-seed retention,
exact-schema `extract` correlation, combined `ZEXTRACT1` schema/read guards,
fixed OpenAI and Anthropic constrained-output request shapes, dynamic-schema
cache exclusion, exact whole-request local counting, unchanged baseline taint,
terminal extraction proof, bounded streamed-output retention, wrong-call
poisoning, exact terminal-byte joins, and schema/read substitution refusal,
byte-exact screenshot guards, OpenAI and Anthropic visual tool-result shapes,
silent-resize refusal, every-frame sensitive zero-reference taint, exact local
whole-body counting, visual byte/transcript ceilings, receipt substitution,
commit/cancel cleanup, no image continuation, provider-compatible schema
projection, fixed body fields, fail-closed ordering, and redacted diagnostics.
Checked pricing tests additionally cover four disjoint token
categories, single upward rounding, exact schedule identity, pre-disclosure and
terminal input-range refusal, overflow, hard rate/profile ceilings, and
redacted diagnostics. They use only deterministic in-memory values and wire
fragments and no provider, network, credential, task, timer, or retry.
Sixteen fixed-transport tests add synthetic ephemeral-loopback evidence for
exact endpoint/header/body construction, credential and diagnostic redaction,
single-POST/no-retry behavior, redirect refusal, closed status/retry mapping,
connection failure, bounded request reading, concurrency/duplicate/shutdown
accounting, lost-wakeup resistance, abandoned-attempt fail-stop, exact-zero
pre-dispatch cancellation, reservation-ceiling post-dispatch cancellation,
catalog-priced completed usage, catalog-priced terminal-failure usage,
mismatched-catalog authority retention, conservative terminal failure without
usage, and terminal OpenAI/Anthropic constrained extraction. They use synthetic
credentials and content only, never an external endpoint, account, profile,
provider key, or provider response.
