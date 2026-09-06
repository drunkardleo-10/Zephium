# Persistent Work browser resources and execution leases

Status: production functional-core and typed-port boundary; **native adapters
remain explicitly Unsupported**. Deterministic tests do not claim that a real
page has yet survived two actor leases. The existing run-owned qualification
path and its all-zero shutdown proof remain unchanged.

## Why this boundary exists

The actual-application foreground witness establishes a bounded supported
rendering opportunity for one exact page. It does not solve ownership: current
`ContextIdentity` permanently owns a run, normal cancellation stops loading and
fails the isolated document channel, and current successful runs destroy every
native context. Reusing those mechanisms as persistent Work resource lifetimes
would either lose the useful page or silently change the meaning of Clean.

`WorkBrowserResources` owns one selected Work/profile's browser-resource
identities. `WorkId`, `WorkBrowserResourceId`, profile and `ContextId` are stable
resource coordinates containing no actor run. Only this non-executing identity
is serializable. Each process-local resource join additionally carries an
incarnation and private registry-allocation binding: reconstructing identical
durable IDs cannot accept another instance's request or receipt. Construction
does not itself attest durable Work/profile admission; the trusted application
must supply that original owner. No Store migration or restart resumption is
implemented at this checkpoint.

An independently run-bound `WorkBrowserExecutionLease` adds its exact run,
monotonic generation and original absolute deadline without relabeling the
resource or page. It is neither an account/effect/navigation capability nor a
model-facing tool. Future page operations must also join the approved policy,
account, current document, observation and operation-specific native permit.

## Exact lifetime protocol

Construction, acquisition, revocation and destruction publish their pending
owner before returning a move-only native request. A completion consumes that
original request. The core checks exact resource/incarnation/private authority,
operation class and lease; an ordinary audit or another resource's zero counters
cannot substitute. These are trusted imperative-port facts, not cryptographic
proof: the native adapter must inspect its actual original owners before
reporting them.

Acquisition acknowledgement receives trusted monotonic time. An expired lease
or a shutdown seal yields `RevocationRequired`, never `Leased`; its same native
owner remains available for explicit revocation. The adapter must independently
check that original deadline at actual dispatch and completion. The Work-lifetime
clock domain is retained across leases, not rebased to each actor run. Clock regression
quarantines execution but does not drop an exact native terminal or prevent
physical cleanup. Native absence is not a clean clock, audit or task verdict.

Revocation seals execution admission before native dispatch. Only an exact
terminal with zero lease-owned task/observation/action/navigation/capture/callback
debt **and the same resource still retained** produces `WorkBrowserLeaseEnded`.
The page disappearing is not lease-only success. A new actor requires a fresh
explicit lease and all higher product/policy/account admission again. Old lease
coordinates remain stale; no lease or receipt is deserialized or automatically
replayed. Human takeover is not implemented. Quarantine or shutdown racing
acquisition cannot be undone by a late success.

The resource owns long-lived page/profile/isolated-world pull, location and
renderer channels. Per-invocation results, actions, captures and their callbacks
belong to the execution lease. Document-wide invocation ceilings must persist
across leases; acquisition must not reset counters, replace the isolated world,
reload the page, or call the legacy cancellation path. No such native adaptation
is enabled yet.

## Failure, cleanup and bounds

Uncertain/refused/malformed native results quarantine only the exact resource.
Quarantine retains callback and execution-capacity debt; unrelated resources
remain usable within the capacity ceiling. It cannot unquarantine itself after
a late receipt or create a replacement empty cohort. Engine-global faults and
whole-process shutdown remain separate authorities.

One independent destruction slot remains available for a quarantined resource
even if its original callback is lost. Destruction must attest actual page,
resource-owned profile lease and resource-channel closure, not deletion of
profile data. It cannot erase the earlier callback:
destruction-before-callback and callback-before-destruction converge without
resurrecting the lease. Runtime bookkeeping can be reaped only after both exact
obligations settle. A rejected or unproven destruction remains quarantined; no
blind retry occurs. Cleanup operation identities are reserved by their resource
or lease, so exhausting acquisition identities cannot prevent cleanup.

Port non-admission is lossless: an Unsupported/rejected adapter returns the
original request and transfers no callback obligation. The application accounts
that typed refusal rather than inventing a native completion. A never-admitted
construction proves absence; a never-admitted acquisition preserves the prior
retained resource. Rejected revocation/destruction does not erase the existing
native owner. All current adapters use the default Unsupported path.

The registry retains at most the existing eight resource rows and four execution
reservations, counting quarantined leases until exact destruction. A row holds
one ordinary operation and one independently reserved cleanup operation; no
unbounded history, timer, worker, script, page content or provider dependency is
added. A native adapter must enforce these same shared process ceilings across
Work registries and the legacy path, not sum independent per-Work allowances.
No global native proof may ignore retained resources. Local `is_quiescent` is
not global native shutdown, policy/provider/audit drain, durable terminal
acknowledgement or a successful task. Default Browse remains dormant.

## Next integration proof

The next native slice must bind these resource identities to the existing
extension-free selected-profile page construction and shared native resource
accounting. In the ordinary application lifecycle, a provider-free witness must
exercise two distinct read-only leases over one unchanged native page, reject
old lease/observation authority, preserve the document invocation ceiling, and
eventually prove exact resource destruction and full application shutdown.
The rendering probe remains a qualification tool, not a product capability.
Presentation, input takeover, persistent Store ownership, general task planning
and open-objective provider qualification remain separate unimplemented joins.

Checkpoint gates pass: 17 focused resource/lease schedules, all 511 core
`probe-harness` library tests and 3 + 5 evidence-review tests, all 41 controller
harness tests, 465 ordinary agentic engine tests, strict all-target core,
controller and engine Clippy, both resource architecture adversaries, both
architecture commands with hostile semantic JavaScript smoke, workspace fmt and
diff checks, the opt-in `macos-work` desktop composition check, and default
Browse dependency isolation. These are deterministic
contract/regression results, not native resource-retention qualification.
