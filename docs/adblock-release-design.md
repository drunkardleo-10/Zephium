# Adblock release design

Status: implementation in progress, 2026-09-30. The user selected Quick menu;
implementation and native qualification remain separate completion gates.

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
  separate from the still-unoptimized network artifact.
- Current constructed-sheet helper passed a real WKWebView test under
  restrictive CSP: apply and undo succeed, stale token/generation and conflicting
  identity are rejected, unrelated page sheets remain present.
- Site-control persistence, reconciliation, exact tab/site/revision admission,
  private memory-only routing, and navigation during selection-save have actor
  regression coverage. Picker preview/cancel and late-start cancellation have
  WebKit component interaction coverage. These are not full-browser QA.
- Native child-frame cosmetics and native subscription-sidecar installation are
  still outstanding. Current live style delivery covers the top document.
- Direct HTTPS acquisition and native-validated candidate activation remain
  outstanding; the product is still composed with its embedded release seed.
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
