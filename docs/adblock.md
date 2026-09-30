# Ad and tracker protection

## Status and release scope

The desktop implements network blocking, static cosmetic hiding, per-site pause,
and personal element hiding on macOS and Windows. The quick menu exposes site
controls and the picker; Privacy settings owns the live profile toggle, source
refresh, and retry. Rust owns durable state and exact operation settlement.

The implementation is on `adblock-release`. macOS native fixtures and the
isolated browser QA build have been exercised. Windows cross-compilation is
not WebView2 runtime qualification: the separate Windows machine must complete
[the handoff](adblock-windows-qualification.md). Merge remains gated on user QA.
Linux is not a release qualification target for this change.

Protection remains opt-in for new profiles, and existing saved preferences are
preserved. Both enabled and disabled profiles receive a locally prepared,
explicit empty policy at startup without joining the compiler queue. Enabled
profiles prepare real protection in the background. Browsing is available as
soon as the empty native policy is acknowledged; the UI never describes that
policy as active protection. Updates retain the installed policy throughout.
Already issued requests cannot be blocked retroactively.

Scriptlets, procedural cosmetics, resource replacement, response-CSP mutation,
URL-parameter rewriting, and YouTube-specific ad blocking are out of scope.
There is no proxy, TLS interception, local root certificate, or replacement
HTTP stack. Supported syntax and losses are reported, not advertised as complete
uBlock/Safari parity. The pinned adblock-rust fork is documented in
[`vendor/adblock/UPSTREAM.md`](../vendor/adblock/UPSTREAM.md).

## Ownership and startup

- `zephium-blocker-update` validates immutable catalogs and owns the official
  HTTPS source store, conditional fetches, candidate staging and recovery.
- `zephium-blocker-service` coordinates source preparation, native preflight,
  durable activation, the compiler worker and monotonic catalog status.
- `zephium-blocker` parses bounded sources, prepares immutable platform policies,
  resolves cosmetic exceptions, and persists compiled artifacts.
- `zephium-app` owns profile preference revisions, site-control revisions,
  compile/install generations, private-session state and operation settlement.
- `zephium-engine` owns exact native registrations, view/frame lifetime,
  pause gates, fixed stylesheet delivery and host-initiated picker evaluation.
- Svelte projects the Rust state and sends typed intent. Admission does not
  imply durable or native success.

A source-cache hit avoids downloading; a compiled-policy hit avoids parsing;
a native artifact hit avoids WebKit compilation. Cache formats bind compiler
versions, limits, source identity and supported resource vocabulary. The actual
release-corpus restart regression proves the worker skips source loading on a
warm hit, including after releasing declarative JSON.

Worker retirement is ordered after accepted work and in-progress callback
completion. Exact generations independently reject stale application/native
callbacks. Shutdown shares one absolute deadline through service, updater,
compiler and native cleanup; missing termination proof is unclean. The new
local startup artifact does not weaken worker retirement barriers.

## Official list maintenance

Desktop builds use these fixed publisher endpoints:

- `https://easylist.to/easylist/easylist.txt`
- `https://easylist.to/easylist/easyprivacy.txt`

HTTPS authenticates the server/transport; these downloads do not have an
independent publisher signature. The app trusts the official publisher's site.
The signed application also embeds a licensed offline EasyList/EasyPrivacy pair.
Optional TUF support remains independently tested, but no TUF publication
infrastructure or signing keys are required by this release design.

The updater uses system TLS verification, bounded streaming, no redirects,
cookies or referrer, conditional ETag/Last-Modified requests, and one worker.
It persists a due time approximately 24–30 hours after a successful check,
respects longer server backoff, and performs no startup fetch while fresh.
Manual refresh coalesces and is rate-limited. Existing application maintenance
triggers due work; the blocker adds no idle polling timer.

Headers, titles, URLs, lengths, hashes, rule counts and suspicious shrinkage are
validated. Both sources form one candidate. The worker prepares the policy,
then macOS preflights both network and native cosmetic artifacts before durable
activation. Windows prepares its frozen matcher before publication. Failed
candidates leave the working policy installed. Current, previous and bundled
source material provide recovery; current bytes are verified with a bounded
streaming buffer. A 304 is accepted only for the exact verified retained bytes
and validators actually sent; otherwise one unconditional retry is allowed.
Unchanged source bytes avoid policy recompilation.

The current bundled pair is **202609300903**, 3,568,061 uncompressed bytes.
`assets/blocker-seed/v1/` contains exact compressed/raw identities, upstream
attribution, license, and deterministic quality report. Release tooling and SBOM
checks bind those artifacts to the shipping graph (`release-bundle` plus
`official-https`), rather than claiming downloaded bytes are release-signed.

## macOS

Network filtering uses digest-addressed `WKContentRuleList` artifacts, installed
before navigation. Network exceptions remain in the same artifact as their
blocks. A separate subscription cosmetic artifact contains native
`css-display-none` rules for the top document, with selector exceptions,
positive/negative domain scope and generic-hide controls resolved correctly.
Personal edits never rebuild either subscription artifact.

Native experiments established two relevant WebKit behaviors: attaching a
cosmetic list after load does not apply it to the existing document; removing a
preloaded list does not undo its CSS. Constructed stylesheets provide the live
fallback and personal hide/undo. Documents loaded with the matching native
cosmetic artifact skip parsing a duplicate subscription sheet. Pausing a site
removes only Zephium's subscription registrations; an explicit reload is offered
to undo native CSS and requests that already ran. Personal hides remain separate.

Native domain/frame predicates are not used to guess a child's destination
scope. Child HTTP(S) documents use an isolated-world lifecycle handshake bound
to the native view/frame, then host-initiated fixed script execution. Lookup
uses that document's own origin and URL. The page world gets no browser command
bridge. Same-origin, cross-origin and nested frames are supported, bounded to
64 tracked frames per view and one delivery in flight per view. Opaque,
`about:blank` and `srcdoc` frames are not currently qualified/covered by this
HTTP(S) lookup; picker selection offers their outer container.

Network and cosmetic compilation share the bounded native queue: one physical
compile, at most two distinct active/queued jobs, at most 64 MiB of encoded
artifacts, and a 60-second watchdog. Timeout never reuses a physical slot before
its exact callback returns. Native cache collection removes only digest-shaped
Zephium entries and protects installed, previous, queued and preflight policies.

The canonical literal-host boundary optimization reduced the July-corpus cold
network compile from **42.043 s to 2.311 s**. With the September pair, an isolated
native run measured **2.187 s network + 2.620 s cosmetics**. These are compiler
measurements, not end-to-end page-load guarantees. The native CI network budget
is 15 seconds. Credential/trailing-dot URL-conversion edge cases observed in a
CSS-document fixture predate the optimization and remain a qualification item;
the rewrite preserves their existing behavior.

## Windows

WebView2 uses the shared immutable Rust matcher on `WebResourceRequested`.
A native per-view pause/provisional gate bypasses matching before URL/method
conversion. No per-tab regex compilation is performed. The callback does no
filesystem, network, actor, UI, deferral or blocking-channel work; it never waits
for a matcher lock. Values and evaluation work are bounded. Matcher/budget or
response-construction errors fail open and increment aggregate health counters.
Blocked responses are empty `403 Blocked`, with `Cache-Control: no-store`.

Document-sourced stylesheet, image, media, font, script, XHR, fetch and beacon
contexts are represented. Top/subdocument navigation, WebSockets, object/other
contexts, service workers and already-running shared-worker requests are not
intercepted by the current adapter. Registering environment worker requests on
every tab would multiply synchronous work; they need a future environment owner.

WebView2 does not supply an exact initiating frame URL in this callback. The
matcher therefore applies source-independent decisions and conservatively
allows requests where an unknown-attribution exception could matter. It never
substitutes the mutable top-level URL or invents third-party classification.
The report records this coverage loss separately from resource-type losses.

Document-created fixed scripts and constructed sheets provide static cosmetics
and personal hides. Live updates use `ExecuteScript`, with exact document token,
URL, navigation identity and policy generation. Frame2 lifecycle events and
Frame7 nested-frame notifications drive per-document lookup. No CDP, WebMessage
or host-object capability is enabled. The frame count and in-flight work are
bounded as on macOS. Physical qualification must establish supported-runtime
CSP behavior, frame timing and visible flash. Page-world styles are page-mutable;
this is not a privileged boundary against hostile page removal.

## Cosmetics, site controls and picker

Static selectors are parsed/validated, with explicit rejection of procedural
syntax. The compiler resolves hide exceptions, includes/excludes and
`generichide`; it does not emit exception selectors as independent hide rules.
Generic CSS is shared across unaffected documents. Subscription CSS relies on
the browser's selector engine for new DOM elements, without continuous scans or
mutation observers.

Site scope is the exact canonical HTTP(S) host across schemes and ports;
subdomains do not inherit a pause automatically. Durable pauses and personal
hides have bounded storage and compare-and-swap revisions. Private-profile
changes stay in memory and disappear when that private session closes. Source
updates never erase or recompile personal edits into public cache artifacts.

The picker only tracks pointer movement while open, uses animation frames for
highlighting, prevents selected clicks from reaching page handlers, and expires
after two minutes. Escape, navigation, tab/profile teardown and Cancel clean it
up. Preview precedes Save. Save rereads the selection through native evaluation,
validates bounded selector/label/count data in Rust, and binds admission to the
current profile/site/native document. Closed shadows, canvas and frame interiors
use container selection. Layout-dependent selectors are labelled as such.

Personal/preview styles include a bounded fallback for author `!important`
display rules. It preserves original inline values and restores only values the
browser still owns. This explicit-edit/initial-DOM path is capped at 1,000
matched elements and does not turn subscription CSS into a page scan.

## Budgets and evidence

| Resource | Ceiling |
|---|---:|
| One source / all sources | 16 MiB / 32 MiB |
| Source count / candidate rules | 32 / 250,000 |
| Physical line / lines | 64 KiB / 500,000 |
| WebKit rules / JSON | 140,000 / 32 MiB |
| Runtime regexes / raw pattern bytes | 1,152 / 2 MiB |
| Filter evaluations per request | 256 |
| Request/source URL | 32 KiB each |
| Cosmetic rules / portable policy | 50,000 / 16 MiB |
| One selector / document stylesheet | 4 KiB / 1 MiB |
| Paused sites / personal hides | 256 / 256 |

The September report records 134,081 network candidates, 106,244 accepted input
rules, 27,518,677 bytes of WebKit network JSON, 1,024 prepared runtime regexes,
and 24,066 accepted static cosmetic rules. Network report rejections include
cosmetic syntax intentionally processed by the separate cosmetic compiler;
do not add those counts or describe them as total unsupported rules.

macOS whole-browser QA verified actual request blocking; generic-hide exceptions;
main/child cosmetic hiding; picker preview/undo/save; hide reload persistence;
live Show/Remove; site pause/reload/resume; and the real Privacy settings control.
Independent native fixtures cover strict CSP, nested/cross-origin frames, live
style clearing, stale document/generation rejection and click suppression.
No test result claims Windows runtime, installer, battery or broad-web coverage.

Use `cargo xtask check-blocker-seed` for exact bundled artifacts; blocker
property/fuzz/fork-contract tests for compiler boundaries; and
`synthetic_blocker_lab` for bounded matcher measurements. The native probe
`macos-blocker-frames-probe` requires `native-isolation-probes` and is excluded
from normal desktop builds. The `adblock-qa` desktop feature builds a separate,
debug-only `app.zephium.protection-qa` application and data directory.

Before merge/release: run the [Windows qualification](adblock-windows-qualification.md),
complete packaged endurance/process-family CPU/RAM/battery measurements, test
representative daily-use pages and user QA, and resolve any release-blocking
findings. These are evidence gates, not claims inferred from cross-compilation.
