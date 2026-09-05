# Trusted macOS Work composition

`zephium-work-composition/macos-work` connects the actual desktop engine and
Store to the existing durable application coordinator. It does not implement
another controller, browser port, policy authority or task interpreter.

The optional `zephium-desktop/macos-work` root installs a dormant, move-only
`MacosWorkComposition` containing the same `Arc<WebviewEngine>` and
`Arc<SqliteStore>` already passed to the ordinary shell. Installation claims no
Work namespace and starts no worker, timer, provider request or native page.
The default desktop dependency graph does not include this adapter or the
agent controller/runtime/transport. Browse initialization and scheduling are
unchanged. This is structural evidence, not a resource or battery benchmark.

## Explicit trusted admission

`admit_trusted_work` is a Rust application port, not a Tauri command or UI/IPC
endpoint. Its move-only `TrustedWorkRequest` must already contain the approved
manifest, exact context/profile/initial target, deadline/model configuration,
credential and `AgentWorkTask`. The trusted task supplies both task-level
completion and independent effect/account assessment. There is no default task
and no interpretation of model text as product authority.

Trusted local-only form workflows can supply
[`AgentWorkFormTask`](agent-work-forms.md) with explicit bounded goals. The
product must independently attest local effect semantics for those fields;
the task cannot infer that an arbitrary form is non-submitting or non-autosaving.

Preparation first borrows the exact engine/Store owners under the desktop
admission mutex, without attachment, persistence claim or native acquisition.
It rechecks the original absolute deadline. Validation failure leaves the
desktop slot intact for a new valid request; no restart or recreated native
authority is required. Only successful preparation takes the slot for attachment;
successful attachment restores the exact composition for explicit successors.
Attachment then binds the ordinary
shell's exact engine and Store owners; preparation binds that same engine and
Store's audit port. The shell checks both allocation identities, then checks
the prepared owner again. A different engine or Store fails before any native
factory call or runtime creation. The original `Arc<SqliteStore>` serves all
three roles: ordinary Store, Work journal and audit.

The deferred `FnOnce` factory calls the actual
`WebviewEngine::take_agent_browser_lifetime_factory` once, then issues one fresh
port only after each application's exact durable `Admitted` and `Running`
acknowledgements. The process factory is mutually exclusive with legacy direct
port acquisition. No ordinary tab is supplied,
and no sealed port is reopened. Native events enter the existing bounded
runtime event sink; publication failure remains runtime recovery debt.
Attachment-mailbox refusal returns the original prepared owner and exact
composition, allowing an explicit attachment retry without preparing again.
Admission-mailbox refusal returns the original prepared owner and exact
application handle. Neither recreates the task or native factory.

The application's existing incarnation fence, immutable terminal CAS,
cancellation, audit reconciliation, worker join and shutdown proofs remain the
only execution authorities. See [persistence](agent-work-persistence.md) and
[execution](agent-work-execution.md). Each native lifetime remains single-use.
`admit_successor_trusted_work` accepts fresh trusted input and the exact completed
predecessor handle; independent application/native proof barriers allow serial
[new lifetimes](agent-work-lifetimes.md), never reopening old ports. Undelivered
events/results and all unknown cleanup/persistence debt block replacement.
Review does not replay an action, and restart does
not restore a credential, task predicate, observation/ref or approval authority.

Trusted extraction inputs may explicitly request [profile-owned durable
results](agent-work-artifacts.md) before preparation. This does not change the
deferred one-shot native factory or exact Engine/Store allocation identity.
Archive retrieval after restart is non-executing historical data.

## Qualification and remaining authority

The release-excluded public qualifier uses the same composition, actual shell
actor, native EngineHost, SQLite Store and application shutdown. Its sole
preparation difference is the explicit retained-public provider constructor;
production/BYOK uses `store:false`. Both preparation constructors join the same
private ownership constructor. Retained qualification is compile-refused in
optimized builds and absent from the default desktop graph.

The qualifier supplies an explicit public test task and an isolated native
profile. It has no ordinary Browse tabs: its chrome adapter refuses all
presentation. It pumps the real macOS dispatcher and requires independently
verified effects, durable success, clean application teardown and no focus
theft. Application success owns the one-shot engine shutdown; the wrapper does
not issue a second shutdown. Host-only qualification and failed application
runs retain bounded host cleanup. The [M6 evidence](../eval/agentic-browsing/m6-production-qualification.md)
records exact results and the initial wrapper ownership defect.

No user-facing task/plan authoring or approval authority is invented here. A
future trusted product layer must supply the typed task/effect/account contract
before calling this Rust port. The full Tauri window/bootstrap/IPC path has not
been live-qualified; the shared production composition and shell/native path
have. The desktop feature remains opt-in. Richer tools, concurrent native lifetimes,
native suspend/resume, concurrent Browse, other platforms and battery/resource
qualification remain separate work.
