# Native network blocker

## Status

This document describes the backend and exact bundled policy in the current
tree. It is a usable native network blocker, not yet a claim of complete
filter-language parity or continuously maintained online protection.

- Signed application builds embed one release-authenticated, immutable
  EasyList + EasyPrivacy seed. The exact compressed and raw bytes, upstream
  versions/commits, license metadata, compiler reports, and platform artifacts
  are reproducibly verified by `cargo xtask check-blocker-seed`.
- The authenticated TUF updater, parser/compiler, persistent caches, profile
  lifecycle, native enforcement adapters, and bounded cache maintenance are
  implemented. Production TUF keys and repository origins are deliberately
  not provisioned yet; bundled mode performs no source-network request.
- New profile preferences default to `enabled = false`.
- Privileged main chrome has a minimal focused-profile diagnostics control for
  source status, enable/disable, and exact-generation retry. Manual refresh is
  exposed only for a future TUF-backed source. It has no profile selector,
  filter text, URL/request telemetry, or raw-page command surface.
- The embedded seed is authoritative but protection remains opt-in. Enabling
  compiles or loads the exact platform artifact and holds first navigation
  until native installation settles. It never labels an empty rule set as
  protection.

Stable product claims remain gated on authenticated online maintenance,
packaged native validation, and operational evidence in
[Enablement gates](#enablement-gates).

## Scope

v1 enforces network blocking rules only. Sources may use the supported
network subset of ABP/uBlock syntax or hosts-file syntax. Parsing uses the
vendored, pinned `adblock-rust` 0.13.2 fork described in
[`vendor/adblock/UPSTREAM.md`](../vendor/adblock/UPSTREAM.md).
The current durable per-profile preference is only an enabled bit; every
enabled profile would share the same immutable process catalog.

The compiler rejects or reports, rather than silently promising, syntax
outside the audited cross-platform policy:

- cosmetic selectors and procedural cosmetics;
- redirect and redirect-rule resources;
- scriptlets;
- response CSP mutation;
- URL-parameter rewriting (`$removeparam`);
- generic-hide controls;
- tag-driven rules.

WebKit conversion additionally reports rules it cannot represent exactly,
including `$important`, HTTP-method predicates, full regular expressions,
mixed positive/negative domain constraints, and unsupported resource
semantics. WebKit's `svg-document` and `other` categories are the closest
available representations of adblock `$object` and `$other`; affected input
rules are counted in the separate resource-approximation dimension. A final
ABP `^` becomes one terminally anchored optional suffix: WebKit's documented
group and `?`/`*` subset represents both a separator byte and exact end-of-URL
without unsupported alternation or rule fan-out. `$badfilter` is resolved
before publication. Compilation fails
if the selected platform would receive no blocking-rule entry. Every
published artifact carries the post-control count of structurally represented
native blocking entries; exceptions and platform-omitted rules are not
counted. This count is an admission invariant, not proof that exceptions
leave every entry semantically reachable for some request.

There is no application proxy, TLS interception, local root certificate, or
second page-derived HTTP stack. Blocked Windows requests receive an empty
`403 Blocked` response with `Cache-Control: no-store`; Zephium does not run
redirect resources or page-world replacement code.

## Ownership and data flow

The boundary is intentionally split by responsibility:

1. `zephium-blocker-update` defines and validates the canonical catalog
   format. In online mode it also authenticates a fixed-origin TUF repository,
   enforces the release's exact license policy and source budgets, and
   crash-safely stages immutable source catalogs.
2. `zephium-blocker-service` admits either the signed release bundle or the
   independently authenticated TUF authority, owns updater/compiler
   coordination, and exposes one monotonic catalog state to the application.
3. `zephium-blocker` consumes only an already-authenticated immutable catalog.
   One bounded worker canonicalizes source order, parses rules, builds the
   platform artifact, and publishes an immutable result.
4. `zephium-app` owns the authoritative per-profile state machine. Durable
   preference revisions and process-local compile/install generations are
   separate identities.
5. `zephium-engine` installs one exact generation on the profile's existing
   views, warm spare, and every later-created view. It reports applied,
   retained-previous, or unavailable with exact generation identity.
6. The native adapter owns platform registrations and their teardown.

The first create/navigate effect for a profile is held until an explicit
native policy exists. Disabled profiles compile an explicit allow-all
generation; absence is never interpreted as allow-all. A successful
replacement is installed across the profile cohort before the prior
registrations are retired. If replacement fails cleanly, a previous
known-good generation remains authoritative. If no generation exists, raw
navigation stays held.

The backend exposes typed, bounded source refresh, focused-profile status,
durable enable/disable, and exact-failed-generation retry commands only to
privileged main chrome. Only failures classified as plausibly transient are
retryable. Candidate source repair admits at most three attempts for one exact
candidate identity per process: the initial automatic attempt and at most two
explicit retries. There is no raw-page bridge, profile selector, automatic
native retry loop, or silent durable disable.

An enabled compile that cannot read the exact installed package reports its
authenticated revision and manifest digest to one process-bounded repair signal.
Failures from multiple profiles coalesce onto one automatic authenticated
current-package refresh for that exact identity and process. A successful
same-identity repair advances a checked, monotonic, process-local material
epoch exactly once. The application then retries only the exact failed
profile/desired generation whose compile attempt used that installed identity
at an older epoch; stale and duplicate callbacks cannot arm unrelated work.

Later loss of the same material, or a retryable transport failure during the
automatic attempt, enters an explicit-refresh-required state instead of
looping. Unsafe file identities are never replaced and terminalize
enabled-policy admission when authenticated repair reports storage failure.
A strictly newer repository package enters the ordinary candidate
prepare/commit/activate barrier and never mints a same-identity material
epoch. Shutdown seals the repair signal, so a late compiler callback cannot
start network or retry work.

Privileged diagnostics distinguish an active source-material repair from an
explicit-refresh-required repair. Both reject new enabled-policy admission.
Only active repair receives bounded internal settlement polling; explicit
repair remains idle until the user refreshes sources.

Status delivery is revision-exact. Rust derives effective protection from the
durable preference, desired generation, proved retained native generation,
applied coverage, and exact current/installed source identities; JavaScript
does not infer `active` from an enable bit. Main chrome subscribes before its
bounded bootstrap query and accepts only a newer fixed-width projection
revision, so an older query cannot replace a newer event. Revision zero is a
static unavailable fallback and cannot supersede actor state. Source
identities expose only bounded public package revision/SHA-256 metadata, never
profile IDs, URLs, request decisions, filter text, or native/parser strings.
The minimal diagnostics control treats command admission as pending—not
success—and enables a blocking preference only when current and installed
package revision and manifest digest match exactly with no candidate
activation pending and no active or explicit-refresh-required source-material
repair.

Preference mutation is one bounded operation per profile. The application
does not change desired or applied policy before the store's exact
compare-and-swap callback. A successful durable change retains its operation
identity through compilation and exact native settlement; disabling is
complete only after an explicit allow-all generation applies. Conflicts fold
the returned authoritative row, while an indeterminate commit is reported as
indeterminate and starts a token-tagged asynchronous single-row
reconciliation. During that bounded retry/backoff path the UI reports
preference authority as reconciling or unavailable instead of guessing.
An atomic return/callback handoff gives each accepted compile completion one
prompt actor wake whether it publishes before, during, or after `compile`
returns. The profile-keyed mailbox coalesces duplicates; if overload rejects
the wake, maintenance still drains the bounded result inbox.
Shutdown/profile retirement settle or invalidate every retained operation id.

Profile deletion retires compiler results and native policy ownership before
the profile identity can be reused. Blocker compilation participates in the
same process shutdown deadline as the application actor, storage, and native
engine.

Blocker shutdown receives the caller's absolute deadline; compiler, updater,
and service layers never replace it with fresh per-layer budgets. The service
serializes shutdown only within the remaining time, seals catalog admission
and the material-repair signal, starts updater shutdown, and asks the compiler
to stop against that same instant. Compiler and updater teardown therefore
normally overlap instead of consuming consecutive full timeouts; failure to
spawn the helper falls back to updater teardown within the same remaining
deadline.

The compiler seals compile/retirement admission, disconnects its bounded
queue after the already accepted cohort, and reports clean only after exact
worker-exit proof, `is_finished`, and a successful join. The updater seals
refresh and candidate-transition admission, publishes terminal status, orders
shutdown behind any already accepted capacity-one refresh, and likewise
requires exact exit proof and join. Its request and refresh-attempt deadlines
bound internal I/O but never extend the outer deadline; it receives only the
remaining duration. An unavailable updater is a clean absence, while timeout
detaches a still-finishing worker and is unclean. The service reports clean
only when compiler shutdown is clean and updater shutdown is complete or
unavailable. Lock contention, missing terminal-status publication, panic,
missing exit proof, failed join, or deadline expiry cannot be reported as
clean. Once shutdown serialization records an outcome, later calls return
that outcome; a caller that cannot acquire the serialization/result locks
before its own deadline returns unclean without inventing success.

Worker retirement is an ordered lifecycle barrier, not a process-lifetime
tombstone. One bounded admission entry records whether a profile is compiling,
delivering, or retiring. A retirement marker is ordered after already accepted
work; it suppresses a queued compile which has not acquired delivery and waits
for an already-running completion callback to return. Only then does it remove
the admission entry, invoke retirement completion, and permit a later
lifecycle using the same profile identity. This avoids unbounded tombstone
growth during create/delete churn. The application and engine still
generation-check independent native/store callbacks, and the worker never
holds its admission mutex while arbitrary callback code runs.

In unwind-enabled builds, each completion callback is contained so one
misbehaving consumer cannot kill the sole compiler worker, strand later jobs,
or prevent clean shutdown notification. A release configured with
`panic = "abort"` still terminates the process on any panic by design.

## Platform enforcement

### Windows / WebView2

Windows publishes a frozen in-process matcher and installs one
`WebResourceRequested` registration cohort on each raw WebView. Registration
uses exact HTTP/HTTPS filters for document-sourced requests in these native
contexts:

- stylesheet;
- image;
- media;
- font;
- script;
- XMLHttpRequest;
- fetch;
- ping/beacon.

Top-level documents, subdocuments, WebSockets, objects, and the native
`other` bucket are not intercepted in v1. Neither are requests sourced by
service workers or by an already-running shared worker. WebView2 raises those
environment-scoped worker requests once for every matching WebView; registering
them per tab would multiply synchronous matcher work with resident-tab count.
They remain unsupported until one profile/environment-owned registration can
be swapped and retired with exact lifecycle proof. The `Document` source kind
still includes ordinary frames, dedicated workers, and a shared worker's
initial script request. Because every native context filter excludes the
other source kinds, compilation conservatively marks every represented rule
as having partial request-source-kind reachability; overlap with resource-type
loss is counted once in the aggregate.

WebView2 does not provide the exact initiating frame URL at this callback.
Zephium therefore does not substitute the mutable top-level `Source` or
classify an absent source as third-party.

The fork's source-independent path applies only rules whose block decision
does not require a source domain or first/third-party classification. If a
normal generic block matches but an attribution-sensitive exception could
also match for the unknown initiator, the request is allowed. A
source-independent `$important` rule retains its priority. This is an
intentional fail-open coverage loss, recorded in the compilation report, not
a claim of full list parity. The shipping Windows graph does not include the
public-suffix resolver: this hot path parses only the target URL and cannot
accidentally infer source attribution.

The callback reads the native context before copying URL/method strings,
accepts at most a 32-KiB URL and 32-byte method, performs no filesystem,
network, actor, UI, deferral, or blocking-channel work, and never waits for
the matcher lock. Malformed native values, oversized values, lock contention,
matcher errors, and response-construction failures allow the request.
Availability failures must not become an application-wide network outage.

All regex-backed rules are compiled transactionally on the worker before the
matcher is published. Frozen matching does not compile, evict, or mutate regex
state. Preparation has aggregate and per-regex count/byte budgets, explicit
regex NFA/DFA construction limits, and a 256-filter-evaluation ceiling shared
by block, exception, redirect, and remove-parameter scans for an
exact-attribution request. Source-independent block, exception, and
unknown-attribution checks share the same ceiling. Exhausting either path
returns matcher-unavailable, and the native adapter allows that request.

### macOS / WKWebView

macOS receives canonical WebKit content-blocker JSON. The engine derives the
native identifier from the exact artifact digest, looks up that identifier in
Zephium's content-rule store first, and normally compiles only on a verified
cache miss. The one repair exception is `WKErrorDomain` code 9 for that exact
identifier: Zephium removes only that corrupt cache entry and performs one
bounded compile attempt. Other lookup errors are not reclassified as misses.
Cache hits, repaired entries, and newly compiled lists must report the exact
expected identifier before the policy can be installed as a
`WKContentRuleList`.

### Linux / WebKitGTK

Linux uses the same canonical WebKit JSON and digest-derived identifier. It
loads from a Zephium-owned `WebKitUserContentFilterStore` before saving on an
exact not-found result. Both load and save validate the returned filter's
non-null, valid-UTF-8, exact identifier. The filter registration is owned per
raw WebView and removed during replacement/teardown.

### Shared declarative compiler control

WebKit native compilation is asynchronous and process-bounded:

- identical artifact digests coalesce onto one physical operation;
- at most one physical native compile is active;
- active plus queued distinct jobs are capped at two;
- encoded artifacts retained by that queue are capped at 64 MiB;
- each physical attempt has an exact non-wrapping identity and a measured
  120-second watchdog. The exact release artifact cold-compiles in roughly
  39-50 seconds on the current macOS release test host, while a fresh-store
  exact cache lookup is below one millisecond;
- at timeout, logical waiters and queued jobs fail and new cache-miss work is
  rejected while the physical attempt remains unresolved;
- on Linux, timeout cancels the load/save chain through its retained
  `GCancellable`, but the physical slot is released only by that exact GLib
  completion callback;
- macOS exposes no `WKContentRuleListStore` cancellation handle, so its
  physical slot remains occupied until the exact native callback returns;
- the one-shot watchdog and native terminal callback share a dedicated
  fixed two-slot FIFO which remains admissible after ordinary host ingress is
  sealed, preserving their arrival order through native re-entry;
- shutdown cancels advisory work and clears logical waiters, but it does not
  report clean until the exact terminal callback has released the compiler
  context, result, cancellation handle, and retained byte debt. The existing
  process-wide shutdown deadline remains the bound if WebKit never replies.

A late or duplicate callback cannot settle another attempt. Cache reuse is
digest-exact. macOS and Linux native stores have bounded namespace-owned
garbage collection. Maintenance accepts only Zephium's exact digest-shaped
identifiers, shares the single native compiler/maintenance slot and shutdown
barrier, and revalidates protection immediately before deletion. Applied,
previous-known-good, queued, in-flight, and physically compiling digests are
protected. One bounded page of candidates is handled per attempt; a complete
clean scan is required before a cycle is considered settled. Maintenance is
triggered only after an explicit policy transition, not as unbounded startup
work.

## Resource and performance budgets

The default compiler ceilings are process invariants, not recommended source
package sizes:

| Resource | Ceiling |
|---|---:|
| source count | 32 |
| one source | 16 MiB |
| all source bytes | 32 MiB |
| one physical line | 64 KiB |
| physical lines, including blanks/comments | 500,000 |
| candidate rules | 250,000 |
| emitted WebKit rules | 140,000 |
| one expanded WebKit URL filter | 8 KiB |
| canonical WebKit JSON | 32 MiB |
| prepared runtime regexes | 1,152 |
| runtime raw regex pattern bytes | 2 MiB total |
| patterns in one prepared regex | 256 |
| raw pattern bytes in one prepared regex | 128 KiB |
| one regex NFA / DFA construction limit | 96 KiB / 16 KiB |
| runtime filter evaluations per request | 256 |
| runtime request URL | 32 KiB |
| runtime source URL (where exact attribution exists) | 32 KiB |

The pinned 2026-07-24 EasyList + EasyPrivacy seed is deliberately below every
hard ceiling:

| Exact release-seed measurement | Value |
|---|---:|
| uncompressed sources | 3,669,674 bytes |
| compressed source assets | 1,217,782 bytes |
| candidate / accepted / rejected input rules | 138,595 / 110,941 / 27,654 |
| Windows runtime regexes | 1,003 / 1,152 |
| Windows retained regex-pattern bytes | 19,500 |
| Windows source-independent blocking entries | 102,600 |
| WebKit emitted rules | 110,909 / 140,000 |
| WebKit canonical JSON | 29,347,341 / 33,554,432 bytes |

The compiler-quality manifest binds these measurements, every drop reason,
the exact policy digest, the WebKit artifact digest, all compile limits, and
the format/adblock-engine versions. Updating either list without regenerating
and reviewing the report fails CI. These counts describe the network-only v1
policy: the rejected EasyList rules are predominantly cosmetic syntax, which
this backend intentionally does not claim to enforce.

The catalog validates source count, duplicate IDs, per-source bytes, and
aggregate bytes before a job can clone source strings. A single worker uses a
bounded profile-sized queue and serializes expensive compilation. It builds
only the artifact used on the current OS, so Windows does not retain WebKit
JSON and WebKit platforms do not retain the runtime matcher.

Three independent persistence layers avoid unnecessary network, parse, and
native compilation work:

- The bundled seed is validated before the compiler worker starts, but its
  gzip bodies are not inflated on browser startup. A persistent
  compiled-artifact hit never reads them; a miss performs one bounded inflate
  and exact raw length/SHA-256/header verification, then drops the raw source
  strings after compilation. The signed binary retains only the compressed
  source bytes and compiled policy. Bundles also install the exact
  CC-BY-SA-3.0 legal text and EasyList/EasyPrivacy attribution notice.
- After current TUF metadata is authenticated, unchanged source targets can be
  read from the private content-addressed store only when their signed digest
  and length match. Metadata is still refreshed and authenticated; this is not
  an offline trust shortcut.
- The compiled-artifact cache key binds either the exact authenticated
  manifest digest (whose signed targets bind every source digest and length)
  or every inline source byte, plus the compile target, compiler/artifact
  format versions, platform and architecture, pointer width and byte order,
  and every compile limit. Records contain one exact cache/policy/WebKit
  format header, are checksummed and bounded, are opened without following
  unsafe filesystem objects, and are decoded and natively revalidated before
  use. `current`, `previous`, and crash-only `stage` records are protected by
  a lifetime lock and directory synchronization. Cache unavailability or
  corruption falls back to authenticated source compilation. Deferred source
  loaders serialize clone access, memoize only successful material, and remain
  reusable after a source-store failure so an authenticated same-process
  repair is not hidden by a cached error.
- macOS and Linux reuse exact digest-addressed native content-rule entries and
  apply the bounded namespace garbage collection described above.

Policy and artifact digests include explicit format versions (currently
policy format 4 and WebKit artifact format 3). Coverage travels with the
artifact as source, accepted, rejected, platform-omitted, and aggregate plus
resource/source-kind/attribution approximation counts, together with the
structural native blocking-entry count. Core admission checks that accepted
plus rejected equals source, omitted does not exceed accepted, each detailed
approximation count does not exceed the unique aggregate, and at most one
blocking entry exists per represented input rule. A blocking artifact with an
inconsistent report or zero blocking entries is rejected. The exact
declarative bytes mint their own typed cache digest inside core and are
bounded again at the core/engine boundary. These controls bound
application-owned work; they do not replace real-list peak-RSS,
request-tail-latency, startup, CPU wakeup, battery, or long-running
native-cache measurements.

The request callback does not record page URLs or per-request match telemetry.
Policy and artifact digests identify public source/configuration bytes, not
browsing decisions.

## Failure policy

“Fail closed” and “fail open” apply at different boundaries:

- Before a first raw navigation, no policy means no view creation/navigation.
  An enabled profile with missing/invalid sources cannot browse as though it
  were protected.
- A disabled profile uses an explicit versioned allow-all policy.
- A failed update retains the previous exact native generation when cleanup
  proves that generation remains installed.
- Contradictory generations or ambiguous registration cleanup are never
  accepted as a valid new policy.
- Inside Windows' synchronous per-request callback, malformed metadata,
  resource exhaustion, or temporary matcher unavailability fails open for
  that request. Blocking on uncertain data would create a page-triggerable
  browser outage and could misapply source-scoped rules.
- Unsupported list capabilities are counted and omitted/rejected during
  compilation. They are not approximated silently at request time.

The blocker does not promise tracker completeness, anti-malware protection,
fingerprinting resistance, cosmetic hiding, YouTube ad removal, or parity
with Brave/uBlock Origin. Native engine limitations and filter-list quality
remain observable product behavior.

## Authenticated source packages

The current source authority is the signed application release itself.
`assets/blocker-seed/v1` is a closed seven-file set containing the two
deterministically compressed source files, canonical catalog and packaging
manifests, exact compiler-quality report, attribution notice, and reviewed
CC-BY-SA-3.0 legal text. The publisher command accepts local source snapshots
and license bytes only:

```text
cargo xtask update-blocker-seed \
  --easylist PATH \
  --easyprivacy PATH \
  --license PATH
cargo xtask check-blocker-seed
```

Generation validates the exact ABP headers, source URLs, upstream versions
and commits, canonical timestamps, source hashes, license hash, deterministic
RFC 1952 representation, closed file inventory, and both shipped compiler
feature graphs. Publication replaces the directory transactionally and
rejects rollback or same-revision equivocation. Verification runs offline and
rebuilds both exact artifacts; it does not trust the recorded report.
Release assembly additionally pins the reviewed revision and every one of the
seven seed-file SHA-256 identities outside that self-describing file set.
Artifact runners prove the seed, packaging configuration, build guard, and
SBOM finalizer still match the exact release commit both before compilation
and immediately before staging. Updating a subscription therefore requires a
reviewed seed regeneration and an explicit release-anchor update; an
internally consistent replacement package cannot self-attest.

The release catalog has an explicit freshness interval. Expiry does not erase
the last-known-good bundled policy or make startup depend on a network
service; status becomes stale and active protection is reported as degraded.
Only a newly reviewed application release can replace this authority today.
The release workflow refuses to publish a build with less than 24 hours of
remaining seed validity. The UI therefore offers no misleading refresh action
in bundled mode.

The source updater is a separate browser-owned component and accepts no
page-derived configuration. Once provisioned, its release configuration will
bind one stable repository identity, an embedded TUF root, fixed metadata and
target HTTPS base URLs, a private storage namespace, exact accepted license
expressions, and hard resource limits. There is no environment-variable or
writable runtime override.

The transport uses the system TLS verifier and proxy discovery, disables
redirects and content encoding, admits only exact descendants of the two
fixed base paths, and bounds connection, request, response, metadata, target,
and total attempt work. Diagnostics redact target paths. TUF expiration and
sequential root rotation are enforced by `tough`; delegated targets are not
accepted in this format.

The canonical signed catalog manifest has a monotonic revision, creation and
expiry times, an exact target set, deterministic source IDs and formats,
exact lengths and SHA-256 digests, and per-list license, attribution, and
provenance metadata. Its license expression must byte-match the release's
bounded allowlist. Sources must be UTF-8 and remain within the compiler's
source count and byte ceilings before a catalog is published.

Activation uses one lifetime-exclusive private namespace, no-follow
file-identity checks, content-addressed objects, a journal and checkpoint,
directory synchronization, a durable clock high-water record, and distinct
current, previous, and candidate packages. An authenticated candidate first
becomes durable without being represented as current. The compiler prepares
and persists its exact artifact; only that exact candidate can then be
committed as durable current and activated from the already-prepared compiler
state. A crash or failure between commit and activation leaves the exact
candidate transition visible and recoverable rather than silently claiming
the new policy is installed. Deterministic compiler rejection is bound to the
exact compiler-policy fingerprint; transient source/storage failures are not
made into permanent package rejection.

TUF-backed startup either recovers one exact committed/candidate state or reports
storage/clock unavailable; corruption and equivocation are not interpreted as
first run. Every durable package also binds the canonical exact license
allowlist and source/manifest admission limits under which it was verified.
A missing or corrupt regular manifest may restore as deferred repairable
material only when that fingerprint still matches; a policy change cannot use
a compiled-artifact hit to bypass current manifest admission. Successful
authenticated same-package verification rebinds the fingerprint.

Object garbage collection is bounded and protects the current, previous,
candidate, and committed TUF object graph. If any retained manifest is missing
or regular-file corrupt, collection protects its manifest digest, performs no
destructive object pass, and retains the already-validated crash-recovery
capacity bound for that open. The smaller steady-state bound is enforced only
after the complete retained reference graph is readable again. Unsafe
filesystem identities and I/O ambiguity remain terminal.

TUF refresh admission and status are bounded, manual refresh runs on one worker,
failures use bounded backoff, and stale/expiry state remains visible. Shutdown
seals new work, orders its barrier after an already accepted refresh, and must
publish terminal status and prove and join worker exit within the caller's
unchanged absolute deadline.

Production TUF keys, root metadata, endpoints, repository identity, and the
signing/rotation/recovery runbook still belong to the next implementation
phase and release operations. The dormant updater performs no network access
in the current build; the source service publishes the distinct
`release_bundle` provenance instead of falsely labeling bundled bytes as TUF.
Provisioning must preserve the embedded package as the trusted offline
baseline. Refresh capability and current-package authority are separate
state: startup begins with `(release_bundle, embedded identity)`, and only an
exact, authenticated, non-older TUF package may atomically supersede it as
`(tuf_repository, identity)`. The application and service state machines must
carry that provenance with both current and candidate identities. Merely
switching the service constructor to repository mode would discard the
offline baseline and is explicitly not an acceptable integration.

The vendored adblock fork retains no third-party list fixture or
replacement-resource payload. Fixture-dependent upstream tests use minimal
Zephium-authored reserved-domain vectors and deterministic generated corpora
instead. The separately licensed release seed lives outside the fork with its
own manifest, attribution, and legal text. The fork manifest declares one
explicit `fork_contract` integration target, and CI runs it under every
production feature graph to pin exact-attribution behavior, conservative
unknown-attribution behavior, preparation budgets, typed matcher failures,
and serialization.
A clean-upstream differential remains separate rebase evidence because normal
builds must not fetch executable source from the network.

Production SBOM finalization independently requires pinned Syft to discover
exactly one Cargo component for each security-critical blocker/update
dependency at its lock-pinned version: `zephium-blocker`,
`zephium-blocker-service`, `zephium-blocker-update`, `adblock`,
`tough`, `reqwest`, `rustls`, `rustls-platform-verifier`, and `aws-lc-rs`.
The finalizer checks each component's package URL, lockfile discovery source,
platform location, and uniqueness. It records the runtime compiler feature
graph selected for the release platform and the exact update-transport
features.

The finalizer also revalidates the staged closed release-seed inventory,
boundedly inflates both gzip assets, cross-checks their raw and compressed
digests against both the canonical manifests and independent release anchors,
and emits CycloneDX 1.6 `data` components for the exact EasyList and
EasyPrivacy versions. Those components carry the upstream source URL and
commit, raw/compressed SHA-256 identities, package revision, and
CC-BY-SA-3.0 license. Finalization derives the only acceptable installed paths
from the platform and already-proven executable locations, then requires
byte-exact copies of the EasyList/EasyPrivacy notice, CC-BY-SA-3.0 text, and
Zephium's MPL-2.0 text in every extracted installer. The RPM metadata declares
the aggregate `MPL-2.0 AND CC-BY-SA-3.0` expression and the release audit
checks it after RPM rewriting.

The standalone fork lock and provenance records are staged only after that
graph scan, so their development dependency graph cannot duplicate or pollute
the product inventory. The finalizer then binds the `adblock` component and
release metadata to the reviewed upstream commit/tree/archive, selected
platform feature graph, fork and inventory records, active/source manifests,
standalone lockfile, and exact reviewed license bytes; only then does it add
the verified MPL-2.0 declaration Syft's lockfile cataloger cannot supply.
Every record is also hashed in the staged payload inventory, so a between-read
replacement fails release assembly.

## Deterministic quality tooling

The source tree includes four complementary quality paths:

- property tests generate and mutate bounded ABP/hosts policies under reserved
  `.invalid` domains, then check deterministic compilation, source-order
  independence, canonical WebKit output, digest stability, and stable runtime
  decisions;
- `synthetic_blocker_lab` measures bounded compile time and runtime
  p50/p95/p99/max decision latency and reports exact rule, artifact, and error
  counts as machine-readable JSON lines;
- an independently locked `cargo-fuzz` package has source-admission,
  request-match, and canonical-WebKit targets with retained Zephium-authored
  `.invalid` seeds. CI audits that graph and runs a fixed-count deterministic
  smoke campaign for every target with timeout, input-size, RSS, and artifact
  bounds;
- `check-blocker-seed` independently inflates the licensed EasyList and
  EasyPrivacy snapshots, reconstructs both shipping feature graphs in offline
  subprocesses, and requires byte-exact agreement with the reviewed compiler
  report;
- release CI materializes that exact 29,347,341-byte declarative artifact and
  compiles and cache-reloads it through the supported native WebKit stores
  under bounded deadlines on macOS and Fedora.

The synthetic tools use no third-party lists, browsing data, live traffic, or
exploit traffic. The release-seed gate uses only the explicitly licensed,
attributed public subscriptions committed for distribution; it never uses
browsing data. These deterministic gates prove that the harnesses build and
exercise the boundaries, but they are not substitutes for sustained fuzzing,
sanitizers, native hostile tests, or packaged endurance measurements.

## Enablement gates

Do not market stable, continuously maintained protection until all of these
are complete:

1. Provision and review the production TUF trust domain: offline root and
   online role keys, thresholds, fixed origins, stable repository identity,
   initial signed metadata/catalog, exact redistributable lists and license
   allowlist, rotation/revocation/recovery runbook, and release provenance.
   Exercise root rotation, rollback/equivocation, expiry, unsafe clocks,
   storage loss, endpoint outage, and recovery against the exact release
   infrastructure. Preserve the embedded release bundle as the offline
   baseline and prove the provenance-carrying, monotonic
   release-bundle-to-TUF transition.
2. Run packaged hostile enforcement tests on real supported Windows, macOS,
   and Fedora hosts. Cover block/exception priority, domains and party
   predicates, methods and protocols, redirects, iframes, workers, service
   workers, cache hit/miss/corruption, policy replacement, native callback
   timeout/re-entry, profile deletion, crash recovery, and shutdown.
3. Record cold/warm/source-CAS compilation, request-match p50/p95/p99/max,
   the percentage of Windows decisions allowed because the 256-check budget
   was exhausted,
   peak and retained RSS, startup impact, CPU wakeups, disk growth, battery,
   and 24-hour endurance with representative redistributable lists.
4. Run sustained fuzz and sanitizer campaigns for source admission,
   parser/converter boundaries, canonical WebKit serialization, runtime
   matching, compiled/source-cache decoding, updater manifest/state recovery,
   and native request construction. Retain minimized regressions. The bounded
   deterministic CI campaign is a smoke gate, not completion of this item.
5. Differentially test the fork against its exact upstream revision and
   re-review every patch according to
   [`vendor/adblock/REBASE.md`](../vendor/adblock/REBASE.md).
6. Preserve and hostile-test the focused-profile privileged source status and
   refresh, enable/disable, and exact retry controls without granting raw
   pages a blocker command surface.
7. Obtain an external review of the updater/cache/parser/matcher/native-install
   boundary
   before describing the feature as stable protection.
8. Complete release/legal approval for the selected CC-BY-SA-3.0
   redistribution path and its aggregate package metadata.

Until then, documentation and UI must distinguish the usable
release-authenticated seed from a maintained TUF source, show stale/degraded
state honestly, and avoid claims of full EasyList semantics. Profiles must
remain disabled by default; “enabled” is valid only after a non-empty exact
artifact is installed for that profile.
