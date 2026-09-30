# Adblock release design

Status: implementation and macOS QA, 2026-09-30. The user selected Quick menu.
Windows physical qualification and user QA remain release/merge gates. See
[the Windows handoff](adblock-windows-qualification.md).

## Agreed scope

Ship network blocking, static cosmetics, site pause, and personal element hiding
on macOS and Windows. Linux is not a release qualification target for this work.
YouTube-specific blocking, scriptlets, procedural selectors, and resource
replacement are deferred. Keep personal hides independent from subscription
updates. Private-profile pauses and hides are memory-only and disappear when
that private session closes.

Work on `adblock-release`, in focused commits. Do not merge until the user
has tested the QA build. Windows agents use separate worktrees/checkouts; assign
ownership of shared native files before combining changes. Do not perform a
parallel implementation of this adapter in the extensions checkout.

## First navigation and status

Use the existing typed `AllowAll` payload as a provisional native policy. It
registers no matcher callbacks and compiles no native JSON. It has small normal
state/dispatch overhead; it is not a claim of literally zero CPU instructions.
Do not manufacture a compiled empty list or remove the explicit-policy gate.

Track desired protection separately from applied policy. Permit initial browsing
under that explicit provisional policy. Show "Protection starting" only while
accepted preparation/installation is outstanding. Transition to active on exact
native installation settlement, not compiler completion. A terminal failure
becomes unavailable with retry, not an endless starting state. Previously sent
requests and executed scripts are not retroactively protected. Retain installed
protection throughout updates; never fall back to allow-all just to update lists.

## macOS implementation

Reuse digest-addressed native rule lists. Fix the persistent cache reader's
resource vocabulary and count limit before other work. Regression coverage must
restart the worker with the real seed and prove that a warm hit skips source
loading, including after declarative bytes are released.

Compile supported subscription cosmetics to native `css-display-none`, resolving
selector exceptions, positive/negative domains, and generic-hide controls before
emission. Do not translate an unhide exception into an independent hide rule.
Whether cosmetics share an artifact with network rules or use their own artifact
is a measured decision; preserve network exception semantics and avoid rebuilding
the network artifact for personal edits.

Maintain bounded personal-hide policy separately from subscription rules. Native
probes on macOS 26.6.2 found that attaching a list after load does not apply its
cosmetics to that document, and removing a preloaded list does not undo its CSS.
Personal rules therefore use browser-owned constructed stylesheets, which the
same probe successfully applied and removed under `style-src 'none'` and
`script-src 'none'` without changing CSP. Use this for live preview and undo;
do not build a general procedural filtering runtime for this release.

Native probes also showed that `if-domain` and frame-URL predicates on child
document loads use the initiating context, not the destination child's scope.
The native cosmetic emitter is explicitly top-document-only. Child document
cosmetics must use the exact document lookup and the bounded stylesheet adapter;
they must not inherit the top-level site's selectors or cosmetic exceptions.

Select the subscription registrations per view using the native top-level site
and Rust-owned pause state. Remove only Zephium-owned registrations. Re-evaluate
before navigation, on redirects, and after cancellation/failure; test history,
popups, restored views and spare reuse. A failed navigation must not leave the
old document under the destination's pause policy. Personal hides have a separate
control and are not implicitly deleted or disabled by subscription pause.

Measure native compilation for the final network-plus-cosmetic policy, not only
the current network artifact. Record cold compilation, cache lookup, installation,
peak/retained process-family memory, and temporary disk use. Do not assume a tiny
personal list compiles or applies within a particular time until measured.

## Windows design and early qualification

### Network policy and site pause

Retain the shared immutable Rust matcher and bounded source-independent matching.
The native callback owns a cheap per-view policy gate set by native navigation
and Rust settings. A paused/provisional view bypasses evaluation before URL/method
conversion. A site pause is a top-level browsing preference, not fabricated frame
attribution for domain/party filter predicates. Preserve current unsupported
request-source/resource classifications.

Start with a correct bounded registration lifecycle, then measure whether paused
views benefit from removing their registration cohort altogether. Avoid repeated
COM registration churn as a speculative optimization. Policy replacement must
not introduce an interception gap or let an old callback overwrite new state.

### Static cosmetics and personal hides

Use a shared Rust cosmetic index to resolve applicable selectors and exceptions.
Keep generic selectors reusable and document-specific payloads bounded. Do not
serialize the entire subscription into each tab or rebuild the matcher on edits.

Use `AddScriptToExecuteOnDocumentCreated` for the fixed document-start bootstrap;
retain the returned registration ID and remove that exact registration on
replacement/teardown. Register before admitting the corresponding navigation.
Inject validated selector data into fixed browser-authored code through JSON
serialization, never interpolate arbitrary filter text as JavaScript.

Maintain independently replaceable subscription and personal style slots in each
document. For live updates use bounded `ExecuteScript` calls on the current view
and supported live frames. Capture native view/frame identity, document epoch,
policy generation and operation ID; discard stale callbacks. This requires a
narrow implementation beyond `set_user_content`, which currently refuses changes
affecting live views. Preserve other owners' registrations throughout.

Resolve frame scope from native frame navigation where available. The initial
top-level document can receive its site-specific payload before navigation; child
frames need their own scope/lifetime handling. The small early Windows spike must
establish when frame scripts can run and which nested frames are addressable on
the supported runtime. Do not inject a top-level site's selectors into every
frame or claim universal pre-paint injection. If an API cannot address a case,
record the actual limitation and an explicit reload requirement instead of
silently claiming live coverage.

Use constructed stylesheets and native CSS matching for dynamic elements. Test
support and CSP behavior on the actual admitted WebView2 runtime. Disconnect the bootstrap observer
as soon as a root exists. No continuous DOM scans, polling, or page repair loop.
Windows page-world styles are page-mutable; they are not a privileged isolation
boundary or a guarantee against hostile page removal.

The early spike must also test restrictive `style-src`/`script-src` CSP and
sandboxed documents. Native script registration does not establish that a DOM
style insertion will be accepted. Do not weaken page CSP or browser security
settings to make the fixture pass; select a supported native mechanism or report
the limitation before committing the Windows adapter design.

### Picker boundary

Picker highlighting and pointer movement stay local and exist only while the
picker is open. Save/preview controls remain privileged chrome. Retrieve the
bounded selected candidate using host-initiated script evaluation when the user
requests preview/save; treat every returned selector and label as untrusted.
Rust binds the operation to the current profile/site/document and validates the
selector, scope and budgets before storage. Keep ordinary page WebMessage IPC
disabled; do not expose filesystem, arbitrary commands or a durable-rule write
bridge. Cancel on navigation, tab closure, profile retirement or Escape.

Closed shadow roots, canvas internals, nested frames and sandboxed frames need
explicit fixtures. Offer container selection where exact element access is not
available. Selection/preview is not proof that a persisted selector will remain
stable after the website changes.

### Windows measurements and pass conditions

Run the small native spike early, before the bulk of the UI implementation. The
separate Windows agent should report exact commit, OS, runtime version, hardware,
build mode and commands alongside raw results.

| Area | Measurement / required behavior |
|---|---|
| Callback | p50/p95/p99/max including COM extraction and blocked-response construction; separate matcher-only timing; fail-open/budget counters |
| Concurrency | 1, 10 and 30 resident tabs; shared matcher ownership; no per-tab regex compilation; paused callback cost |
| Cosmetics | document-start timing and visible flash; live pause/resume; dynamically inserted elements; no idle observer activity |
| Frames | same/cross-origin and nested frames; CSP and sandbox restrictions; frame navigation/destruction; stale script completion rejection |
| Picker | highlight/selection latency, preview, save, undo, navigation cancellation and reload persistence |
| Resources | process-family peak/retained memory, CPU time, idle wakeups, repeated updates and tab churn |
| Privacy | no persisted private pauses/hides or selector-bearing private diagnostics; session teardown clears transient state |

Tests on macOS cannot qualify the Windows COM path. Do not call the Windows work
complete until the physical-machine tests and resulting fixes pass.

## Direct HTTPS updates

Use fixed official EasyList/EasyPrivacy URLs with system TLS verification and
bounded transport. Retain release-bundle, HTTPS-origin and local user provenance
as distinct facts. HTTPS is transport/server authentication, not independent
publisher signing or signed freshness. Arbitrary filter URLs are out of scope.

Persist a due time; check approximately daily with bounded jitter (proposed
24-30 hours), independently of how often the browser restarts. Honor longer
server retry/backoff instructions. A not-due startup performs no list request.
If overdue, refresh after startup without blocking browsing. No rapid polling or
per-tab scheduler. Manual refresh coalesces with existing work and has a cooldown.

Use ETag or Last-Modified conditional requests when available. A 304 is useful
only with the exact still-valid cached source bytes; missing/corrupt material
requires a bounded unconditional retry. Do not recompile if effective source
bytes are unchanged. Treat Expires/list version as publisher hints, not signed
anti-rollback evidence. Keep failure backoff bounded and avoid wake-from-sleep
refresh storms.

Stage both sources as one candidate; validate input limits, headers, parser
coverage and native compilation, then activate atomically. Retain current and
previous working policies and the bundled fallback. A valid but bad upstream
list can still break websites: preserve user site pause and recovery. Revisit the
trust model before adding executable replacement/scriptlet resources.

## UI review and acceptance

The sketches cover a shield menu, site pause, picker preview and saved-hide
management. They are interaction proposals, not native runtime evidence. Keep
technical diagnostics in Settings; no invented blocked counts or savings badges.
Use precise starting/active/paused/unavailable states. Pausing offers reload
explicitly; it does not silently navigate a page with unsaved work.

Personal hides have labels and site scope with show/hide and remove actions.
The private variant says "For this private session". Preview precedes saving and
undo is immediate. Use the approved Quick menu direction with compact, accessible controls.
The user has authorized implementation; merge remains gated on their QA.

## References

- [Microsoft: document-created scripts](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2.addscripttoexecuteondocumentcreatedasync)
- [Microsoft: frame lifetime and script execution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/frames)
- [WebKit: native content blocking](https://webkit.org/blog/3476/content-blockers-first-look/)
- [Current implementation and its existing claims](adblock.md)

These native APIs establish implementation candidates. Windows minimum-runtime
behavior, cross-frame timing and live cosmetic changes still require the stated
probes. Do not enable the unrelated, unqualified Windows CDP userscript probe as
a shortcut for this adapter.

## Current local evidence

- Native macOS 26.6.2 grouped top-document cosmetic artifact: 2,019,933
  bytes, 3.569 seconds cold compilation, successful native admission. This is
  an earlier corpus measurement; the current pair is measured below.
- Current constructed-sheet helper passed a real WKWebView test under
  restrictive CSP: apply and undo succeed, stale token/generation and conflicting
  identity are rejected, unrelated page sheets remain present.
- Site-control persistence, reconciliation, exact tab/site/revision admission,
  private memory-only routing, and navigation during selection-save have actor
  regression coverage. Picker preview/cancel and late-start cancellation have
  WebKit component interaction coverage. Subsequent full-browser QA verified
  blocking, generic-hide exceptions, picker preview/undo/save, reload persistence,
  live Show/Remove, site pause/reload/resume and the real Privacy controls.
- Native subscription sidecars and bounded child-frame adapters are implemented.
  Actual Rust/macOS probes verified three same/cross-origin/nested frames under
  restrictive CSP, live clearing and no page-world command bridge. HTTP(S) frames
  are covered; about/srcdoc/opaque frames remain outside that lookup. Windows
  Frame2/Frame7 paths cross-compile but require physical qualification.
- Official HTTPS acquisition is now composed into desktop startup. An isolated
  live run fetched both September 30 lists, compiled their network artifact in
  2.187 seconds and cosmetic artifact in 2.620 seconds, activated the pair and
  shut down cleanly. Desktop preflight now chains both artifacts before activation.
- Picker cleanup has a single two-minute expiry while open, plus navigation,
  explicit cancel, Escape and UI context teardown; it creates no idle polling.

### UI graph budget review

The site menu and picker are lazy after the first Quick-menu opening. A clean
HEAD snapshot built successfully before the changes (browser 294,245 bytes JS;
WebExtensionManager's complete static graph 422,559 bytes). Loading the new
controls eagerly raised both graphs by 6,920 bytes; the lazy implementation
reduces the browser addition to 1,284 bytes (295,529 total). The manager shares
that browser chunk, so its measured complete graph is 423,843 bytes although
its own feature is unchanged. Its JS allowance is deliberately adjusted from
423,000 to 425,000 for these typed IPC/menu-entry additions, after this comparison;
the browser and panel limits remain unchanged. The new lazy ProtectionMenu graph
measures 302,972 JS / 90,190 CSS bytes, with 310,000 / 93,000 limits. This is a
bundle-size review, not a claim about runtime RAM or navigation latency.

### Network compilation and current fallback

The hostname-only boundary rewrite preserves the existing converter prefix and
separator semantics for canonical network URLs. Native WebKit normalizes bare
HTTP authorities to include `/`; path patterns, wildcard hosts and explicit
right anchors retain their end-of-URL branches. The July corpus cold compile
fell from 42.043 seconds to 2.311 seconds. The current September 30 corpus takes
2.187 seconds in the same isolated native fixture. Artifact identity format 4
is now shared by Core and the compiler to prevent version drift. CI's cold
network compile gate is 15 seconds; production's cancellation watchdog is 60
seconds and still waits for the physical native callback before reusing its slot.

Native URL fixtures also exposed pre-existing credential/trailing-dot quirks in
the converter prefix. The optimization preserves those results; they are not
claimed as fixed. Real network qualification must distinguish these from the
CSS-document URL fixture and record any remaining conversion limitations.

The embedded fallback is now the exact official 202609300903 pair (3,568,061
uncompressed bytes). Publisher updates accept the actual ABP 2.0 EasyList and
ABP 1.1 EasyPrivacy headers. A deliberately strict first live attempt rejected
the older header and retained the release pair; the corrected run succeeded.
The source store verifies a retained current with a fixed streaming buffer at
startup and chooses previous/bundled material if corrupt. Compilation-cache
hits still skip filter parsing. Conditional HTTP success refreshes only exact
verified source material, and never extends a TUF signed expiry.

A real native picker click under restrictive CSP now verifies preview, undo,
and zero page click-handler invocations, including an inline important display
rule. Personal inline overrides are bounded and reconciled only on explicit
style changes, preview, or initial DOM readiness; subscription CSS never starts
a DOM scan. Same-site navigation and native document-presentation changes
invalidate a selection that is still awaiting persistence admission.

### Final local qualification additions

Initial disabled profiles now bypass the compiler queue just like enabled
profiles' provisional policy. The startup change retains exact native settlement
and worker retirement ordering; the application suite passes 329 tests.

The isolated debug QA host spent 0.01 seconds of process CPU over a 109.74-second
idle observation with protection enabled and Privacy settings open. RSS changed
from 133,776 to 107,712 KiB. This is host-only, without a matched baseline or
WebKit process-family attribution; it establishes neither an overall RAM saving
nor a battery claim. Matcher-only release measurements and the remaining Windows
measurement matrix are in the handoff.

The repository coalition sampler also recorded a 20.013-second idle interval:
12 processes, 362,276,632-byte peak physical footprint, 1.916 ms total CPU and
5 package idle wakeups, with no disk I/O. This was the isolated debug browser
with a local fixture and Settings, not an on/off comparison. Raw matcher and
process-family results are retained in [the QA record](qa/adblock-2026-09-30.json).

The final QA build was restarted with saved protection enabled. It rendered the
restored page during preparation, transitioned to Active, and blocked/hid the
fixture after reload. Existing compiled-policy/native/source cache files were
unchanged across restart. Public-page smoke checks loaded BBC News (including
consent dismissal and working site navigation) and Wikipedia's Web browser
article with protection active. These two pages are compatibility smoke evidence,
not a representative benchmark or comprehensive site qualification.
