# Actual-application one-hop qualification

Status: actual-app adapter prepared and offline-tested; no actual-app execution
or provider evidence yet.
The retained Pimoroni result does not qualify this ordinary-controller path.

The prepared boundary is Tauri trusted Rust admission into the existing
`MacosWorkComposition::prepare` / `PreparedAgentWork::try_new` /
`AgentWorkController`, with the original application engine, Store, provider,
worker and native lifetime owners. It does not use retained resources,
`prepare_public_qualification`, a replacement event loop or a second shell.

The frozen task is shared with the prior standalone React navigation harness:
independently attest the Quick Start heading at `https://react.dev/learn`, allow
one exact native hop to `https://react.dev/learn/your-first-component`, attest a
fresh Your First Component heading under the exact successor navigation epoch
and frame generation, and extract only that destination heading with one fresh
heading-text citation. No objective parser or model output chooses authority.

The development-only request has one ephemeral Owned context, Public / Read /
Anonymous authority restricted to `https://react.dev`, eight model turns,
100,000 tokens and a 100,000 micro-USD reservation ceiling (not expected cost).
Luna's catalog-bounded first reservation requires 77,830 micro-USD. Exact runtime
accounting, the original lease and the single 150-second absolute deadline
remain unchanged across the hop; credential loading consumes that deadline.
Production provider requests remain stateless. No durable result publication,
successor run, action, imported session, redirect or additional route is allowed.

The ordinary controller's existing maximum 64 pre-dispatch NotReady readiness
polls at 50 ms are retained. These are not provider, navigation or effect
replays; malformed/stale/terminal receipts do not enter that loop. They cannot
renew the deadline, budgets, account sample or old document references.

This is a hidden native context with the production scheduling policy. It does
not qualify animation-frame-dependent pages or change the retained rendering
holder. Departure evidence is independently checked but is retired as execution
authority before navigation. Only fresh destination evidence may be cited.
Cross-document evidence retention/synthesis, route discovery, authentication,
actions, Work UI and general multi-page browsing remain separate capability
gaps, not claims made by this witness.

Offline gates run the same task and adversarial tests through the shared module:

```sh
cargo test --locked -p zephium-work-composition --features navigation-qualification --lib
cargo clippy --locked -p zephium-work-composition --features navigation-qualification --all-targets -- -D warnings
```

The qualifier feature is absent by default and compile-rejected in optimized
or non-macOS builds. Source tests do not constitute native/site qualification.

## Actual application admission and closure

`macos-work-navigation-probe` adds only a development observer and a fixed Rust
admission call. It uses the ordinary app shell and `admit_trusted_work`, not an
IPC command or another native/resource/controller owner. Its isolated bundle
identity is `app.zephium.work-navigation-probe`; the build requires the matching
bundled configuration and startup refuses a nonempty application data root.
It cannot be combined with the retained/rendering or extension-lab/staging
witness. Shipping defaults are unchanged.

The worker waits at most 30 seconds for ordinary chrome readiness, then uses
the existing 15-second/301-check foreground admission gate. It reads the actual
main window's visible/focused state and never activates, focuses or presents a
window. Failure to select the window defers before credential lookup or Work
admission. The initial human chrome window remains outside the owned task.

After foreground admission, the single worker loads the existing development
Keychain credential. Security.framework lookup is synchronous and
noncancellable: a quit retains this worker and the app event loop until lookup
returns. Cancellation and actual trusted admission share a mutex fence, so a
late credential result is dropped without admitting work. There is no detached
credential worker or claim of a bounded Keychain cancellation time. The original
150-second deadline starts before lookup; an expired lookup cannot admit.

The observer drains the existing content-free event journal, checking exact run
and sequence, typed proposals and settled usage. It samples terminal publication
before draining, so a racing final usage event cannot be omitted from acceptance.
Acceptance requires ordinary `Succeeded`, no failure/persistence error, exactly
one same-run debt-free success record, no durable artifact, exactly one Navigate
proposal, nonzero settled model calls, and one consume-once extraction satisfying
the shared fresh-destination source verifier. The task/controller independently
attest departure and the exact native hop; a model proposal is not that proof.

Cancellation requests stop on the exact observed run. If no terminal is visible
by 160 seconds from the original start, the observer reports failure, requests
stop, relinquishes its projection and hands any remaining debt to ordinary
application shutdown. This observer grace cannot renew the task deadline or
runtime budgets. The original worker is joined before `ShutdownCoordinator`
receives exit. Qualification additionally requires that coordinator's clean
normal exit; task failure and clean shutdown are reported separately. This is
ordinary controller/runtime/native closure evidence, not the retained witness's
separate weak-resource census. No second native cleanup path is installed.

## Build and explicitly authorized one-shot run

Build only, from the repository root (does not launch):

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-navigation-probe.conf.json --features macos-work-navigation-probe
```

Before any run, record the clean source commit and SHA-256 of the resulting
arm64 bundle executable. Preserve a fresh isolated application-data directory
for this exact bundle identity; never reuse or delete production app data.
Launch this bundled application once only after explicit provider/Keychain
authorization, capture its content-free standard output, and select its actual
window within the foreground window. Do not use `tauri dev`, a standalone probe,
a retained resource, or an automatic retry. Preserve the isolated data root on
success or failure. The ordinary Store is expected to contain the terminal Work
record and audit delivery but no published result artifact.

Record admission, all settled call/token/cost/request-byte/semantic-byte events,
typed terminal failure (if any), the source-mapping and durable-terminal checks,
worker join and normal shutdown. A failed native/site boundary is evidence to
diagnose, not permission to change the route, loosen matching or retry. No live
result is claimed until this exact candidate has passed that procedure.

Additional offline adapter gates (no credential/provider/native execution):

```sh
TAURI_CONFIG="$(<desktop/tauri.work-navigation-probe.conf.json)" cargo clippy --locked -p zephium-desktop --features macos-work-navigation-probe --all-targets -- -D warnings
TAURI_CONFIG="$(<desktop/tauri.work-navigation-probe.conf.json)" cargo test --locked -p zephium-desktop --features macos-work-navigation-probe --test navigation_probe_configuration --test foreground_probe_configuration
TAURI_CONFIG="$(<desktop/tauri.work-navigation-probe.conf.json)" cargo test --locked -p zephium-desktop --features macos-work-navigation-probe --lib navigation_probe
cargo test --locked -p xtask work_composition_boundary
cargo xtask check-agentic-probe-boundary
```
