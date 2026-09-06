# macOS actual-application Work resource retention

Status: **not yet qualified**. The deterministic harness and native plumbing
do not prove that a real WebKit document survives two actor execution leases.

## Scope and acceptance

One compiled anonymous loopback fixture, one ephemeral Work/profile/resource,
two distinct actor runs and leases, and the already independently qualified
public-API foreground rendering holder. The original exact main/key/responder
admission is required; no programmatic activation, focus transfer, private API,
provider, keychain, public site, arbitrary evaluation or page-authored authority.

Construction goes through the production persistent-resource adapter with its
one frozen target, no redirects/reloads and exact native document gate. Both
observations use its original bounded all-role semantic channel. Revoking A
must synchronously reject an original A read without dispatch or a native
counter increment, and account that returned core request before ending A.
B must preserve the exact resource, native view, navigation ID and isolated
world while increasing the native completed-invocation count by exactly one.
The private identity stamp never leaves memory. Both snapshots independently
require the current exact fixture document, complete semantics and the fixed
readiness markers, including the animation-frame reveal.

The rendering holder is an independently owned, release-excluded qualification
aid inside the original native resource cohort. Retirement must restore and
release it before exact resource destruction. Qualification also requires both
exact lease-ended receipts, resource-core quiescence, all nine original native
cohort counters zero, unchanged human ownership, fixture closure, ordinary
application shutdown and the exact weak page/surface/store drain.

Limits remain eight semantic observations total, seven maximum A readiness
samples plus one B read, one five-second rendering opportunity, a 15-second
driver bound, four mailbox slots and five-second cleanup. No retry creates a
new page, lease budget, invocation ceiling, profile or native cohort.

The isolated app is built using `macos-work-resource-probe` and the existing
`tauri.work-rendering-probe.conf.json`. Each reviewed launch requires a fresh
isolated data root and a pinned executable hash. Machine-local paths and profile
locations are not committed. A refused run remains a refused run; it is not
automatically retried or promoted to a successful observation.

## Result

No actual-application two-lease result is recorded at this checkpoint. No
resource-retention, real-site, provider, authenticated-account, product-rendering,
human-takeover, restart-resumption or open-objective qualification is claimed.

## Prelaunch build and deterministic checkpoint (2026-09-06)

The resource-feature engine passes 533 tests and strict all-target Clippy;
ordinary agentic engine passes 486 and the existing foreground feature 527,
both with strict all-target Clippy. The isolated desktop feature check and
debug app bundle build pass. Both architecture commands, hostile semantic
JavaScript smoke, two resource-boundary and six foreground-boundary mutation
tests, workspace formatting/diff checks, and default Browse dependency isolation
pass. Tests include renderer-owner/incarnation substitution, native stamp
substitution and counter reset, exact render phase/lease slot, terminal callback
permit retention, and cancellation of a watchdog before a late timer block.

Built with Node.js 24.18.0 and the repository-pinned pnpm 11.17.0:

```sh
# Run from desktop with Node.js 24.18.0 on PATH.
node node_modules/@tauri-apps/cli/tauri.js build --debug --bundles app \
  --features macos-work-resource-probe \
  --config tauri.work-rendering-probe.conf.json --ci --no-sign
```

The inspected artifact is an arm64 application with isolated bundle identifier
`app.zephium.work-rendering-probe`. Executable SHA-256:
`20f98db5a7fc237a2ea36de7eabfc5a22b23c8cc919656e5536e7253ddbfae29`.
This hash pins an unlaunched candidate, not a qualified run. Initial sandboxed
packaging could not fetch the pinned package-manager signatures; the same
command with authorized network access completed without bypassing verification
or changing dependencies, lockfiles or configuration. No provider or GUI action
was taken to obtain these build/test results.
