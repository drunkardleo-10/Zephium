# Retained local action qualification

Status: qualified on macOS through the actual bundled application at source
`d3956e05a13e39f922c399d67f0db525e2caa504`. The execution and retained-provider
review below qualify only the fixed local action described here.

Build `macos-work-retained-action-probe` with the existing
`desktop/tauri.work-navigation-probe.conf.json` override and production frontend,
then use a fresh empty application data root. Select only this diagnostic
objective. The desktop's existing foreground fence precedes credentials and
`admit_retained_trusted_work`; its ordinary observer and shutdown owners remain.
The feature inherits the optimized-build refusal and is absent from defaults.

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-navigation-probe.conf.json --features macos-work-retained-action-probe --bundles app --ci --no-sign
```

The composition starts the existing bounded fixture server on an ephemeral IPv4
loopback port. `/retained-local-form-v1.html` has one Draft textbox initially
containing `Unprepared`. It has no form, submission controls, page scripts,
handlers, remote assets or persistence; CSP also denies scripts, frames and
form submission. The trusted task permits only the exact local preparation goal
`Ready for review`. This effect attestation is specific to this owned fixture
and must never classify arbitrary remote website fields as harmless writes.

Luna uses the ordinary provider, controller, effect policy, retained WKWebView
action recipe, callback-return owners, fresh capture, action verification,
source mapper and durable terminal. Limits remain eight model calls / policy
operations, 100,000 aggregate tokens, 100,000 micro-USD, one context and one
150-second absolute deadline including credentials. Public diagnostic requests
use `store:true`; shipping defaults stay stateless. The trusted task owns the
fixture server and joins it before successful extraction acceptance; failure
drops that owner. This is separate from ordinary native resource shutdown.

Acceptance requires:

- An initial complete single-frame observation with exactly one Draft textbox
  and the differing initial value. An already-satisfied field cannot pass.
- Exactly one action proposal, action-active event and verified event, in order.
  Duplicate actions and extraction before verification fail the witness.
- A different observation, invocation and snapshot in the same document with
  the exact final value, after the controller's independent action verification.
- `draft_value` cites the exact final textbox TextValue / ValuePreview source,
  preserving observation, generation, frame, invocation, snapshot and reference.
  A matching paragraph, accessible label, stale source or truncated preview fails.
- One succeeded durable run record with no debt, failure or persistence
  uncertainty, and exactly one source-mapped result. Worker join and ordinary
  native shutdown must subsequently finish and be recorded independently.

This qualifies a fixed local write, not an open remote workflow or trusted-event
compatibility on every website. The terminal record is durable; this witness
does not enable persisted extraction artifacts beyond the current retained entry.

Cancellation and human takeover have deterministic coverage in
`zephium-app::work_resources_action_tests`: hold an admitted native action, seal
admission with stop, and then deliver the original applied terminal. Recovery
must retain that evidence without fresh verification, another action or a model
request; original resource owners drain. Another test polls inside the native
callback and proves terminal arrival cannot release debt before physical return.
These use isolated native fixtures. A manual stop after an observer event does
not prove it happened before callback return and must not be described that way.

Relevant checks (loopback tests require local socket access):

```sh
cargo test -p zephium-work-composition --features retained-action-qualification retained_action
cargo test -p zephium-agentic --features probe-harness retained_local_form
cargo test -p zephium-app --features work-execution-probe work_resources_action_tests
cargo xtask check-agentic-probe-boundary
```

For the authorized native witness, preserve the clean source commit,
bundle/executable/config hashes before and after, redacted logs, stored provider
response IDs and actual usage, stopped immutable database/integrity check,
action/provenance/durable acceptance, worker join and ordinary shutdown.
Investigate any refusal before another one-shot run. No automatic replay exists.

## First run and causal corrections — 2026-09-08

The first authorized candidate was built from clean source `f4732fe`; its arm64
executable SHA-256 was
`a66ce2c89d8cce486a135c77a2edb9019795cab6be3ddbc29b5f1fea7df59574`
and canonical whole-bundle manifest SHA-256 was
`e16b6a3549d5ee6cf62ee66c3a7e85a004931eea629c6151ce8c34fed08d671d`.
The probe configuration SHA-256 was
`93ce7e65840926e885c4da9ff0e0cb232a33ce4fef643296d6ddfa5c6bb04331`.
Executable, configuration and bundle inventory were unchanged after execution.
The complete redacted evidence is preserved under
`/private/tmp/zephium-retained-local-action-first.zRr9t9`.

The launch ran from `2026-09-08T21:37:29Z` through `21:37:35Z`. Luna correctly
proposed the one authorized Fill from the initial five-node observation. Its
stored response is
`resp_00253a8d11334fdc006aa0801d386087d2ac811937c004b1c1`; provider inspection
confirmed 1,756 input and 114 output tokens and the exact bounded action. The
native boundary synchronously refused it before dispatch as
`Browser(Action(Failed(Timeout)))`. No page mutation occurred. The terminal was
therefore uncertain and unaccepted, the observer still joined, and clean normal
shutdown was intentionally not claimed. The stopped Store passed integrity;
it contained one non-terminal run, no artifacts, deliveries or audit events.

The preserved failure exposed two generic defects rather than a WebKit or action
capability limit:

- Controller qualification timestamps were relative to each task's local
  `Instant`, while native ingress compared them with the process Work monotonic
  epoch. The apparent future request was rejected before native dispatch.
  `deee62d` projects the existing absolute deadline into the single native Work
  clock without adding grace, renewing the deadline or widening policy.
- A completely accounted synchronous rejection left an action pending in the
  controller, so terminal sealing reported Recovery instead of the exact failed
  receipt. `d3956e0` consumes only an exact, non-scheduled rejected receipt with
  no execution, settlement or audit debt and produces a durable Failed terminal.
  Unknown, mismatched, callback-active and audit-loss cases remain Recovery and
  cannot retry automatically.

Both corrections have deterministic regression coverage, including a delayed
37-second setup, clock rounding, a real Sqlite failed-terminal round trip, and
the negative audit-loss path. The failed launch was not relabeled as success or
silently replayed.

## Qualified run — 2026-09-08

The second separately authorized candidate was built from clean source
`d3956e0`; its arm64 executable SHA-256 was
`78086a23890178e9b11672c211171f71640a91677fd88f1f54ca31029d087717`
and canonical whole-bundle manifest SHA-256 was
`27c1a94a9b10c13821e794a97b0462adfd07caf620c88bc6731aac92989af42a`.
The configuration retained SHA-256
`93ce7e65840926e885c4da9ff0e0cb232a33ce4fef643296d6ddfa5c6bb04331`.
Executable, configuration and bundle inventory were byte-identical after the
run. Full redacted evidence is preserved under
`/private/tmp/zephium-retained-local-action-second.jQChCj`.

The launch ran from `2026-09-08T22:12:46Z` through `22:13:02Z`. The controller
reached `ToolProposed(Act)` at 3,948 ms, `ActionActive` at 3,949 ms, began the
fresh observation at 4,220 ms and recorded `Verified` at 4,629 ms. Luna then
selected Extract and the reserved mapper returned `draft_value` as exactly
`Ready for review`, citing only the fresh textbox value source. The terminal was
published at 12,709 ms with exactly one proposed, active and verified action,
verified source mapping, verified durability, no failure and accepted success.
The retained WKWebView was healthy, idle and reusable before shutdown; the
observer joined and normal application shutdown was clean.

The three inspected stored Responses, in execution order, are:

1. `resp_0efd488421761623006aa08863732087d2a44ac933882d68c0` — Luna chose the
   exact Fill; 1,756 input / 100 output tokens.
2. `resp_0efd488421761623006aa08866c85c87d29116126aec7e8a9a` — after the fresh
   diff showed the new value, Luna chose Extract; 1,959 input / 39 output tokens.
3. `resp_0efd488421761623006aa08869e9e487d2878dbe6b2098295f` — the constrained
   mapper returned the exact value with source `@r3`; 1,217 input / 81 output
   tokens.

Together the calls used 4,932 input and 220 output tokens, cost 1,095 micro-USD
under the provider accounting ceiling, and took 11,579 ms of model latency. The
stopped immutable Store passed integrity and foreign-key checks. It contains one
profile, one terminal run, zero result artifacts, one audit delivery and eleven
audit events. The 96-byte durable run record reads back as terminal.

This proves the shipping path for one explicitly approved bounded local Fill:
real Luna decision, policy, retained WKWebView action, physical callback return,
fresh semantic observation, independent verification, exact source mapping,
durable terminal publication and clean application shutdown. It does not prove
arbitrary remote writes, signed-in services, every trusted-event-sensitive site,
Windows, persisted result artifacts, concurrent Browse/Work pressure or
multi-agent orchestration. Those remain separate real-workflow qualifications.
