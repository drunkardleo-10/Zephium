# Security maintenance runbook

Zephium deliberately treats the operating-system WebView runtime and the
application build graph as security-sensitive release inputs. The checks
described here fail closed. A red check is an instruction to review evidence,
not a deadline or advisory string to extend mechanically.

## Ownership and cadence

The release security owner reviews all three native-engine channels at least
weekly and again immediately before freezing a release commit. A second
maintainer reviews every floor change used for a signed artifact. The review
must happen sooner when Apple, Microsoft, WebKitGTK, RustSec, npm, a supported
distribution, or the Tauri or Wry projects publish a security notice.

Current deadlines are encoded next to their evidence in:

- `crates/zephium-core/src/macos.rs`
- `crates/zephium-core/src/webview2.rs`
- `crates/zephium-core/src/webkitgtk.rs`
- `deny.toml` for temporary advisory exceptions

CI enforces these dates with:

```sh
cargo xtask check-engine-floors
cargo xtask check-advisory-exceptions
```

The production workflow uses the stricter release-only gate:

```sh
cargo xtask check-release-engine-security
```

It also fails when a vendor has publicly acknowledged a browser-engine fix
that is not yet available in its supported Stable runtime. Development may
continue on the newest admitted runtime, but an unavailable vendor patch is
never treated as production release evidence.

Calendar alerts should be set for seven days before every encoded deadline.
The alerts are operational backup only; CI remains the authoritative
fail-closed control.

These calendar gates are deliberately stricter than installed-runtime
behavior. An overdue review blocks CI and release publication, but does not
make time alone terminate an already installed browser. Runtime still rejects
known-obsolete or malformed engines, preview/development channels, provenance
failures, security overrides, and missing mandatory native capabilities.
Falling behind the newest reviewed recommendation, crossing the review SLA,
or encountering a newer stable release line is projected as a typed,
non-fatal privileged-chrome advisory. Independent facts are retained
together: no severity ranking may discard review age, a patch recommendation,
or a newer unreviewed release line.

Runtime startup never downloads this policy and never scrapes Apple,
Microsoft, or WebKitGTK pages. Those pages are unstable, unsigned inputs from
the client's perspective and would add latency, failure modes, and an
unnecessary first-party network observation. The embedded reviewed policy is
authoritative for startup. If Zephium later refreshes policy independently of
an application release, it must do so asynchronously through Zephium-signed,
rollback-resistant update metadata, persist only an authenticated newer
policy, and retain the embedded hard floor as the offline fallback.

## Native-engine floor review

For each platform:

1. Read the vendor's current security advisory and release-note source, not a
   search result or version aggregator.
2. Identify every vendor-supported operating-system or stable runtime line
   Zephium claims to support. Preview and development channels remain hard
   failures. Decide explicitly whether a newer stable line preserves the
   mandatory native capabilities and can run with an unreviewed-runtime
   advisory; never infer that from an arbitrary numeric version alone.
3. Record the hard minimum fixed version, newest recommended version, their
   publication dates and source URLs, and the next review deadline together.
4. Confirm startup admission, CI packages, packaged-artifact checks, and the
   documentation all describe the same boundary.
5. Add or update boundary tests for the exact version below the hard floor,
   the floor itself, the newest recommendation, a newer stable line, a
   preview/development line, a clock before hard-floor publication, and the
   first instant after review expiry. Prove that only the hard failures reject
   runtime admission and that recommendation/review states map to the correct
   sanitized advisory.
6. Run the platform's packaged hostile/native suite. A source-level version
   check is not renderer-confinement or process-mitigation evidence.
7. Preserve links, command output, package metadata, and test artifacts with
   the reviewed release commit.
8. When the admitted hard floor is older than the newest recommended security
   release, name that deliberate compatibility gap in release notes. Do not
   imply that every admitted installation is fully patched merely because the
   candidate passed its release gate.

If the vendor has disclosed fixes but has not yet published a supported stable
runtime, the release remains blocked. Do not extend the review date merely to
make CI green. Development may continue on a documented non-release branch,
but no signed production artifact may bypass the gate.

## Dependency advisories

Rust dependency policy is enforced with the locked graph and `cargo deny`.
An exception in `deny.toml` requires an owner, reason, removal plan, and expiry
within the bounded review horizon. On expiry, remove or upgrade the dependency;
extending an exception is a new security decision and requires review.

The complete pnpm graph is intentionally checked at `high` severity:

```sh
pnpm audit --audit-level high
```

This includes development dependencies because Vite, package plugins, and
other build tools execute while producing signed artifacts. When this check
fails:

1. Confirm the advisory and affected resolved version against the package
   registry/advisory source.
2. Determine whether the dependency executes in development, CI, release
   production, or the shipped application.
3. Upgrade or remove it and regenerate the lockfile deliberately.
4. Re-run frontend checks, the production build, and the release-script tests.
5. If no fixed dependency graph exists, keep production release blocked. Do
   not lower the audit severity or omit development dependencies as a release
   workaround.

Registry availability is different from a vulnerability result. A network or
registry failure should be retried from the controlled release environment;
it must not be converted into a passing audit.

## Native-boundary fork maintenance

The in-tree Tauri, Tauri Runtime Wry, and Wry forks are narrow native security
adapters and therefore part of Zephium's trusted computing base. Monitor
upstream security releases and the files touched by every upstream change.
Follow the `REBASE.md` in each fork for every update; a version-only bump or
blind merge is prohibited. Tauri and Tauri Runtime Wry share an upstream
repository and commit, but their complete deltas, lockfiles, and validation
results remain independently reviewable.

Before a stable release, all native-platform checks must run on the immutable
candidate commit. Passing macOS tests does not substitute for Windows or Linux,
and unit tests do not substitute for packaged hostile-page, teardown, erasure,
and endurance tests.

GitHub-hosted macOS images may temporarily lag the embedded hard floor or ship
a Safari application whose build does not match the loaded WebKit framework.
Ordinary push and pull-request CI may still use such an image for source,
Clippy, unit, and API-availability coverage, but it must label that boundary and
mint no native runtime evidence. The reusable production-release CI call still
executes exact runtime admission, principal isolation, and WKWebExtension probes
and fails closed until an admitted runner is available. Never lower a floor,
ignore a build mismatch, or relabel hosted source coverage as release evidence
to make branch CI green.

### Fedora native CI sandbox environment

The 2026-09-07 hosted run at `fd9010f` compiled the Linux native test binary but
aborted when bubblewrap could not create a nested namespace inside GitHub's
default Docker job. This was an environment refusal, not passing confinement
evidence. [Docker's default seccomp policy](https://docs.docker.com/engine/security/seccomp/)
deliberately restricts namespace creation; adding `SYS_ADMIN`, disabling WebKit's
sandbox, or accepting that inherited filter as WebKit evidence is not a fix.

Host-policy setup and native execution run only on an exact main-branch push or
reviewed main release call. The native job checks out `github.sha` only and
refuses a different `checkout_ref`; its helper independently checks event,
branch and actual checkout before **any** mode, including host-policy cleanup.
Pull requests retain source/policy/platform-neutral gates and explicitly mint
no Fedora native-runtime evidence. They never run this host-policy job.

The native job pins the Fedora **base image digest**, then live-resolves packages
with `dnf upgrade` and the explicit WebKitGTK/JSC updates-testing transaction.
Those dependency versions and repository state are not pinned or reproducible
from the base digest alone. Package installation uses ordinary Docker policies,
then the native gates run as UID/GID 10001 in a separate container:
all capability sets dropped, no-new-privileges, read-only root and source/toolchain
mounts, isolated writable home/build/temp, no host devices, Docker socket, tokens,
or host PID/network namespace. Dependency acquisition has network access; the
bridge is explicitly disconnected and Cargo goes offline before native gates.
The unchanged Fedora package/vendor/signature/integrity and engine-floor checks
still precede WebKit execution. The job logs the installed package NEVRAs and
actual shared host-kernel release for traceability, not as a repository pin.

That test container intentionally has no outer seccomp filter and uses a named
`flags=(unconfined)` AppArmor user-namespace grant. It has **no outer container
seccomp or LSM confinement**. Loading that profile modifies host policy only in
this trusted job; it does not change global AppArmor settings or userns sysctls.
The disposable hosted VM is the outer boundary for repository test code, as for
ordinary non-container CI; this container is a controlled Fedora userspace, **not
a claimed additional hostile-code sandbox**. This explicit exception is scoped
to the capability-free, network-sealed native test job, never release artifacts
or product startup. A custom partial syscall allowlist would still require the
mount/pivot/user-namespace LSM exceptions while obscuring which filter the probe
actually measured. See Ubuntu's [user-namespace policy](https://documentation.ubuntu.com/security/security-features/privilege-restriction/apparmor/)
and bubblewrap's [unprivileged namespace model](https://github.com/containers/bubblewrap).

An early timed bubblewrap preflight must demonstrate different user/mount/PID
namespaces, non-root identity, zero capabilities, no-new-privileges, no external
interface and zero inherited seccomp filters. The renderer test independently
requires no-new-privileges, different user/mount/PID namespaces, and a filter
count above its parent. The count excludes inheritance alone; it does **not**
identify the filter's installer, content or WebKit provenance. The former
observer-side `/proc/<renderer>/root/path` read denial was invalid as renderer
filesystem evidence: userns/ptrace rules can deny the observer even when the
renderer could read the file. That assertion and its evidence claim are removed.
A real in-renderer or equivalent-credential filesystem-denial probe remains a
separate qualification requirement. These native state checks do not prove it.
Within an admitted trusted job, a refusal remains red; no fallback, automatic
retry or replacement context-property proof exists.
Workflow-policy mutations pin these restrictions. Local macOS source checks do
not qualify this Linux environment: the next hosted execution must supply the
actual namespace and native-test evidence before it is called green.

The Linux Wayland device pass must also force GlobalShortcuts portal denial
(and separately restart the portal process) and verify that the configured
launcher chord still opens from the focused main window and closes from the
focused launcher panel. After permission is restored, verify one activation per
press, immediate fallback after an authoritative `ShortcutsChanged` removal,
and no activation after browser shutdown begins. Run this against the packaged
artifact whose installed entry is exactly `app.zephium.desktop`; raw `cargo`
or `tauri dev` execution does not prove portal application identity.

A teardown-only GLib-GIO warning from a sandbox child saying that release of an
`app.zephium.Sandboxed.WebProcess-*` bus name failed because its connection was
already closed is a WebKit child-process cleanup diagnostic. Do not hide it
with a GLib log handler or by weakening the WebKit sandbox. It is acceptable
only when Zephium exits cleanly and it is not accompanied by an
`engine: web process terminated` event, a core dump, a hang, or a surviving
auxiliary process. Any of those accompanying symptoms turns it into a native
runtime failure: retain the journal and core, record the exact WebKitGTK/GLib
versions, and keep the candidate blocked pending engine-level investigation.

## Release evidence

For every candidate, retain:

- the exact commit and annotated tag;
- native runtime and OS versions for every runner/device;
- engine-floor and advisory review output;
- locked dependency manifests and SBOMs;
- packaged hostile/native and profile-erasure results;
- resource/endurance measurements;
- signing, notarization, provenance, and installer verification output.

Any source, lockfile, floor, exception, build image, signing input, or workflow
change invalidates the prior evidence and requires a new candidate run.
