# Windows Work implementation status

Updated October 3, 2026, on `windows/work` in the isolated Windows runtime
checkout. This records implementation and actual Windows validation; it does
not certify the eleven end-to-end parity scenarios or authorize release.
The accumulated Windows Work implementation is being saved as one local
checkpoint commit on `windows/work` before subsequent native UI work. No push
or PR is part of this checkpoint. Build outputs, logs, and local diagnostic
screenshots remain outside version control.
The preparation baseline is recorded in the local `target/windows-work-handoff.txt`.

## October 3: native startup paint and bounded retail-site triage

The user's follow-up confirmed clipped Work rendering and canvas previews work,
but the loaded dark app still exposed a white native sidebar/window ground for
several seconds at startup. Moving the Acrylic repair after show had not fixed
that symptom. The Windows main-window builder now explicitly sets transparent
black as its background. This reaches both the native parent and WebView2;
the latter already used transparent black. Tao otherwise delegates the initial
native background erase without filling the client area. The change adds no
timer, CSS mask, recurring redraw, renderer, or browsing lifecycle work. Acrylic
configuration is unchanged. Zero RGB is consistent with Microsoft's documented
[DWM glass initialization](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe).

The explicit native regression creates two owned hidden native Tauri windows,
without WebViews, plugins or user profiles, and sends their actual window
procedures a background erase against a white memory DC. The old configuration
leaves all three sampled pixels white; the new configuration immediately erases
them black. It passes in `target/windows-work-startup-native-test.log`. This
tests native erase behavior, not visible DWM/Acrylic composition: the packaged
application's full cold-start visual check remains outstanding. The new test's
native-window API and GDI dependencies are Windows-only development dependencies.

The Windows Human path shares the product admission/commands and presents the
same retained Wry controller and native child window, restoring observation
after Continue/release. No missing Windows implementation or replacement-page
behavior was found. Prior native Human presentation/retirement qualification
still applies; actual Allegro CAPTCHA completion has not been qualified.

The existing QA log distinguishes the retail-site failures:

- Allegro repeatedly reports `ContextLost` before any snapshot, model call or
  native action. That prevents challenge observation and a Human-help request.
  A failed navigation carrying a usable HTTP error/challenge document is a
  possible cause, not established by these logs. Do not turn failed navigation
  into successful admission without exact document/provenance evidence.
- Amazon reaches snapshots and model proposals, but the recorded attempts
  dispatch zero native actions. The first reports `ActionProposalLoop` after
  13 calls, seven snapshots and five action refusals. The previous trace dropped
  the exact refusal reason. `ReadFailureTrace` now retains and reports the latest
  closed `SemanticActionBindingError` enum, without proposal targets, text or page
  contents. This supports a precise follow-up instead of guessing a modal fix.

No site-specific policy relaxation, CAPTCHA workaround or unverified retail
fix is included. Deeper Allegro/Amazon diagnosis is deferred as requested.

The focused refusal-trace regression passes, including latest-reason retention
through unrelated events and saturated counts. Desktop strict Clippy with all
targets and `webext-qa`, workspace formatting and diff whitespace checks pass.
Evidence: `target/windows-work-site-refusal-tests.log`,
`target/windows-work-startup-clippy.log` and
`target/windows-work-startup-format.log`. Frontend code/assets did not change
in this follow-up; the previous frontend validation remains applicable.

The separately named QA build is
`target/windows-work-qa/Zephium Work QA Windows 2026-10-03 Startup Polish.exe`
(115,464,192 bytes, SHA-256
`C713F77F161BCB1ABD40BA1737356726CDE0B7BBB3449186A7A0BDA539DC8130`).
Build and metadata are `target/windows-work-startup-qa-build.log` and
`target/windows-work-startup-artifact.json`. The copied executable matches the
build output and has not been launched. It retains `app.zephium.webext-qa` and
the user's QA data; close the previous QA instance before opening this one.

## October 3: clipped native rendering and canvas preview polish

This supersedes the opaque Work backing described in the historical section
below. Windows observation now installs an empty native region on the exact
owned WebView child before revealing its controller. The renderer remains
active at its fixed 1280 by 800 logical viewport, but exposes no page pixels
through the sidebar or window margins. The product no longer changes the
chrome background for observation. Normal Acrylic can therefore remain in
place without a CSS mask, blur pass, additional WebView, or viewport resize.

The original native region is restored exactly after the original capture
callback drains. A per-owner window property protects against stale/reused
handles and reentrant native calls. Human takeover refuses outstanding capture
or restoration debt; it then uses the normal visible, enabled controller and
restores the original observation viewport on retirement.

The real WebView2 fixture passed at this machine's 100% display scale: fresh
full-viewport captures under empty native clipping, animation frames, trusted
CDP input and changed pixels, repeated exact rounded/absent/empty-region
restoration, pending-capture refusal, actual Human presentation and retirement,
and fresh observation after Human exit. Foreground focus remains unchanged.
Actual 125% and 150% display runs have not been performed. Evidence:
`target/windows-work-canvas-human-native.log` and
`target/windows-work-canvas-work-backing-control.log`.

Windows can request one early preview after an admitted navigation commit,
before semantic readiness. The visual stamp binds the exact resource,
navigation epoch and location revision; it grants no semantic-read authority.
Fast readiness cancels the queued early request. Captures remain serial and
coalesced with a 250 ms minimum interval; no continuous idle polling is added.
Closing the original session discards a late picture while retaining the
original callback until cleanup. Only the first usable picture and final
settlement wake the UI immediately; ordinary replacements retain the existing
one-second activity cadence.

Work preview media preserves the bounded native PNG instead of performing
another resize and JPEG conversion. The frontend shares one intrinsic decoded
bitmap per generation across card widths inside the existing 16 MiB cache,
then draws directly into the existing display-sized canvas. Ordinary photo
rendering is unchanged. Larger originals can evict sooner; this removes work,
but is not a measured global memory or speed improvement.

Startup material repair now runs after the initialized main window is shown,
instead of depending on its first focus event. No startup timer is introduced.
The user's original startup white flash still needs a visual check in the
packaged app.

Frontend static gates and all 646 tests across 117 files pass, as do the Work
thumbnail component lifecycle test, production build and bundle budgets,
emitted CSS check, strict Desktop production and Windows probe Clippy, and the
agentic production/diagnostic boundary check. Evidence is recorded under
`target/windows-work-canvas-*.log`.

The first-preview cancellation fixture also exposed a process-erasure shutdown
proof race. Erasure and shutdown now share ownership of the same originally
opened process HANDLE. A tombstoned erasure can retain that HANDLE and its
matching observer after releasing the environment; an orphaned observer is
still invalid. The existing global five-second shutdown bound covers both the
exact Environment5 proof and the original HANDLE signal. Late queued host
callbacks cannot reclaim ownership already transferred to the shutdown worker.
Page admission also rechecks retained erasure pairs, releasing only an exact
Exited proof plus its signalled original HANDLE before counting process
capacity. This handles a delayed HANDLE signal without another event or timer.
Focused profile/provenance tests (14) and native process-proof tests (12) pass;
evidence is `windows-work-canvas-{profile,process}-tests.log`.

Concurrent anonymous-profile erasure can remove a child during the final
generation scan. A scan-only `NotFound` now restarts the complete root identity,
ownership marker and bounded tree validation within the existing eight-attempt
budget. It never proves success from partial disappearance. All seven runtime
cleanup tests pass, including deterministic concurrent removal, changed-marker
refusal and retry exhaustion preserving the registry proof:
`target/windows-work-canvas-runtime-cleanup-tests.log`.

The final combined first-preview/cancellation native run passed. It decoded
the expected synthetic blue 640 by 400 PNG before semantic completion, with
zero semantic reads. A release-excluded one-shot diagnostic callback then
closed the original anonymous session immediately after native capture
dispatch, proving rejection of the late picture, original capture drainage,
constructor refusal, exact process-group exit and clean private-storage and
Shell/Store shutdown. It adds no shipping callback, delay or polling. Evidence:
`target/windows-work-canvas-first-preview.log` and
`target/windows-work-canvas-capture-hook-tests.log`.

The retained native lifecycle run passed: the page advanced throughout a
45-second clipped observation lease; burst reads produced a newer final PNG
before revocation; the final generation stayed unchanged during 45 seconds
idle; reopening the durable origin and final native-zero/Shell/Store shutdown
all succeeded. Evidence: `target/windows-work-canvas-retained-measurement.log`.
The same run sampled its synthetic probe process family (eight processes),
not the full product or a collection of real sites:

| Phase | Sampled duration | Median private memory | Peak private memory | CPU time / one-core utilization |
| --- | --- | --- | --- | --- |
| Leased, clipped page | 19.06 s | 180.83 MiB | 191.77 MiB | 0.734 s / 3.85% |
| Idle retained page | 18.93 s | 176.32 MiB | 182.20 MiB | 0.078 s / 0.41% |

These are short workload observations, not an A/B regression, frame-rate,
startup, energy, or leak benchmark. Exiting processes can be missed. Raw
samples and the exact probe hash are under
`target/windows-retained-20261003-181712/`. The renderer/preview implementation
was frozen for this run; the later cleanup scan retry changes shutdown only.

Final strict Desktop all-target and Windows probe Clippy, source boundary,
Rust formatting, tracked whitespace and Desktop QA build all pass on the
final source graph. The frontend had no changes after its recorded checks.
The separately copied executable's SHA-256 matches the successful build:

`target/windows-work-qa/Zephium Work QA Windows 2026-10-03 Canvas Previews.exe`

115,462,144 bytes; SHA-256
`707FE890F761F9C71F8B827EF9D9E595319CECE2224504FA946F0B8950F6BA07`.
Metadata is `target/windows-work-canvas-artifact.json`; build evidence is
`target/windows-work-canvas-qa-build.log`. The executable was not launched.
The existing running QA application and earlier copies were preserved. Close
the previous QA instance before opening this one because both use
`app.zephium.webext-qa`. Visual startup, real-site appearance and actual
125%/150% display checks remain; the earlier release/security limitations
recorded below have not been waived.

## Earlier October 3: reported close, send, read, and backdrop problems

The user's Fresh Previews QA run confirmed working browsing and canvas frames,
but reported an unsaved-changes close warning, a refused Slack send, a failed
Wikipedia read, and brief page-color flashes behind the chrome. The following
changes address verified defects; the historical send/read causes and the
reported flash are not claimed resolved without a matching reproduction.

- **Close:** the dormant Windows launcher exists as a blank WebView until first
  use. Enlisting it after only the main window touched resources waited for a
  close listener that did not exist and produced the generic unsaved warning.
  Only resource-touched privileged hosts now participate; hidden touched hosts
  still flush. A host first touched during a flush triggers another flush.
  Resource admission checks terminal shutdown under the same mutex as final
  close, preventing a late first caller from entering after the terminal marker.
  Real draft failures and stale/wrong-window replies remain failures.
- **Slack:** Enter on a named message composer is now classified as a
  communication when the compact snapshot omits its Send button. Confirmation
  takes the targeted composer's draft rather than an unrelated earlier textbox.
  Purchase/destructive effects retain their stronger classification. Changed
  drafts and stale approvals remain refused; approval is for one unchanged step.
  The Work lead's obsolete instruction to never send is aligned with this
  existing person-confirmed workflow. Reading remains read-only by default.
- **Failure evidence:** bounded, content-free counters now preserve the closed
  browser failure category, last tool, model/action counts and refusal counts.
  Final queued events are consumed before terminal accounting, so a late settled
  model call clears its in-flight reservation. No additional provider calls,
  timers, page strings, message text, credentials or budget increases are added.
- **Backdrop:** the approved opaque native Work backing intentionally suppresses
  site colors behind the sidebar while Work pages render. Canvas previews remain
  sharp. A corrected native fixture uses transparent chrome and opaque white
  page backgrounds like the product. First reveal, navigation dispatch/completion,
  overlapping owners, retirement/reacquisition and final restoration pass strict
  owned-screen checks. The reported full-product flash is not reproduced.
  Transition-only `webext-qa` logs now record native background RGBA, theme,
  timestamp, window state and chrome order; no shipping polling or blur is added.

Provider-free direct Wikipedia qualification passed for `www.wikipedia.org`
and `en.wikipedia.org/wiki/Wikipedia`: four fresh semantic snapshots, exact
origins, expected headings, joined revocation, resource destruction, native zero,
and clean Shell/Store shutdown. The portal snapshot was Complete (124 nodes);
the article truthfully reported `Truncated(NodeLimit)` (128 nodes). This proves
direct reads, not the user's entire agent plan or same-resource cross-origin
navigation. Evidence: `target/windows-work-polish-wikipedia-{build,read}.log`.

The user's original production log recorded Slack send at 11 model calls and
one native action, and Wikipedia at two model calls and two actions. It did not
retain the exact controller failure. These counts do not prove exhaustion of
the configured page budget. No provider/account actions were replayed and no
real Slack message was sent during this investigation.

The observed running Fresh Previews app process family was sampled for 29.33
seconds (25 samples, 23 processes): median private memory 914.02 MiB, peak
919.63 MiB, sampled aggregate CPU 0.21875 seconds (about 0.75% of one core).
This is a snapshot of that existing workload, not controlled idle, energy,
frame-time, startup, or leak qualification. Exiting processes can be missed.
Evidence and exact executable identity are under
`target/windows-polish-observed-app-resources-20261003/`.

Frontend type, lint, style, format, and unused-code gates pass. The initial
parallel unit run timed out two tests while Cargo compilation competed for CPU;
with unchanged timeouts and one worker, all 642 tests across 117 files pass.
Production frontend builds and bundle budgets pass. Existing build warnings
about the motion launch-state capture and unresolved wordmark are unchanged.
Native regression and lint evidence from the final source graph:

| Check | Result | Evidence under `target/` |
| --- | --- | --- |
| Native close participants, token binding, failed flush, terminal admission race | 6 passed | `windows-work-polish-close-final-tests.log` |
| Site-work composer, exact approval, stronger consequences | 11 passed | `windows-work-polish-slack-policy-tests.log` |
| Terminal usage, final queue drain, closed failure trace | 10 passed | `windows-work-polish-closed-final-tests.log` |
| Work lead reading/approved-action instruction contract | 1 passed | `windows-work-polish-prompt-tests.log` |
| Native backdrop first reveal, navigation and retirement transitions | 1 passed | `windows-work-polish-flash-transitions.log` |
| All frontend unit tests, unchanged timeout, one worker | 642 passed | `windows-work-polish-frame-unit-serial.log` |
| Frontend static gates and production builds | Passed | `windows-work-polish-frame-{check,build}.log` (unit retry described above) |
| Strict Agentic/Composition/Desktop production all-target Clippy | Passed | `windows-work-polish-production-clippy.log` |
| Strict Windows native probe Clippy | Passed | `windows-work-polish-probe-clippy.log` |
| Rust formatting and tracked diff whitespace | Passed | `windows-work-polish-format.log` |
| Emitted frontend CSS and production/diagnostic source boundary | Passed | `windows-work-polish-{styles,boundary}.log` |

Updated QA executable:
`target/windows-work-qa/Zephium Work QA Windows 2026-10-03 Work Polish.exe`
(115,466,240 bytes; SHA-256
`9648A2E0E73C6CC1466A66274F7E7D08A7F78CB243D3A14246EA9EE1457DF91C`).
The copy hash matches the successful default-work-product/webext-qa build;
metadata is `target/windows-work-polish-artifact.json`, build evidence is
`target/windows-work-polish-qa-build.log`. The executable was not launched.
The running Fresh Previews app and older executables were preserved. The app
identifier remains `app.zephium.webext-qa`; close the prior instance before
opening the new one. Retest a new Work with Wikipedia, a person-approved Slack
send, the reported flash, and normal close with the launcher never opened.
If the old build alone blocks normal close without real pending edits, opening
its launcher once initializes the missing flush listener; retry normal close.

The final release-security check again passes the source boundary but fails
the preexisting macOS/WebKit review deadline (September 18); the separate
advisory-exception check fails the security-owned exception expired September
30. Windows WebView2 154.0.4258.53 remains reviewed through October 9 UTC.
No deadline or security policy was weakened. Evidence is
`target/windows-work-polish-{release-security,advisory-exceptions}.log` and
`target/windows-work-polish-gates.json`. This build is for QA, not a declaration
that the Windows Work release or all performance/manual scenarios are qualified.

## October 2 integration qualification

**October 2 status:** implementation is ready for Windows QA; real-account, visual,
and complete performance acceptance remain pending. All 22 native scenario assertions pass on the integrated
implementation (`windows-work-all-changed-handoff-fixed.log`). This run exits 1
at its final shutdown barrier, so it is not a clean full-suite qualification.
Tracked native resources are zero and the exact parent-owned journal is retired;
the particular failed shutdown stage was initially unknown. The Changed
case exercises the newer-handoff decline and preserves its original stale
approval/zero-POST assertions. Apps asks once for all three authenticated sites;
Views succeeds for all six apps with zero read model calls.

The traced repeat (`windows-work-all-shutdown-traced.log`) identifies the
shutdown failure as `agent_lifecycle=false`: terminal coordination, Store,
readers, native process proof, private cleanup, and blocker cleanup all pass.
The targeted Churn run proves the exact cause: its intentionally interrupted
operation retains classified `RecoveryRequired` with `UNKNOWN` debt after its
original native zero proof succeeds. The established contract deliberately
returns Unclean in that state; successful physical cleanup does not erase
uncertain operation history. No production verdict or deadline was changed.
`windows-work-churn-classified-recovery.log` records the scenario passing,
`native_zero=true debt_unknown=true`, all other shutdown phases passing, and
exact parent-owned journal retirement. The aggregate 22-scenario run therefore
cannot be called a clean overall exit. Positive runs qualify clean shutdown
separately. Six applicable recovery tests and strict App Clippy pass in
`windows-work-classified-recovery-{tests,clippy}.log`; two additional explicit
recovery-contract tests are macOS/Linux-only and were inspected as source, not
reported as Windows test passes.

The traced repeat passes 21 scenario assertions; SPA
fails after two unsupported model Scroll proposals with zero native actions
(`Reference(OperationDenied)` followed by `ActionProposalLoop`). The earlier
integrated SPA pass used supported actions. No capability check was relaxed
and no model-only failure was relabeled a successful run.

The final four-write rerun passes with exit 0 and clean shutdown
(`windows-work-writes-final-integrated.log`): Slack send and Enter each produce
one exact message, Linear creates one exact issue, and Notion saves the exact
final text twice through autosave. Each has one successful confirmation. The
closed diagnostics prove native process obligations settled, private cleanup
succeeded, Shell shutdown was Clean, and the exact journal scope retired.
Final serial library tests pass: Agentic 929 and Engine 428, with two existing
Engine ignores (`windows-work-final-engine-agentic-tests.log`).
Final strict production and diagnostic Clippy, the agentic source boundary,
the strengthened refusal-marker regression, xtask Clippy, formatting, diff
checks, semantic action contracts, and the compact runtime digest all pass.
Evidence is in `windows-work-final-{production-clippy,diagnostic-clippy,
agentic-boundary,navigation-boundary-test,xtask-clippy,format,semantic-node}.log`.
The release-security gate reaches the existing expired macOS/WebKit review and
fails there (`windows-work-final-release-security.log`).

**Earlier diagnostic run:** the broader native replica run passed 19 of 22 scenarios
(`windows-work-all-navigation-fixed.log`). The three failures were an exact-text
fixture goal, a permitted consent form POST blocked by missing native request
headers, and a multi-site authentication/permission batching fixture. The
Autopost goal now explicitly requests the existing exact sentence; its exact
content and zero-POST assertions remain unchanged and pass. Consent now passes
both form and fetch submissions. The native guard correlates a missing
destination header only with the exact active admitted navigation and existing
unused write authorization; it does not classify every unknown request as a
main-frame request. All 27 gate tests and the native unapproved-POST control pass,
and Autopost still produces zero POSTs on the corrected binary. Evidence:
`windows-work-consent-navigation-unit-tests.log`,
`windows-work-consent-native-union-tests.log`, `windows-work-consent-joined.log`,
and `windows-work-consent-joined-autopost.log`.

The multi-site fixture now explicitly authenticates each isolated native origin.
All three reads then succeed with cookies and zero model calls, but two
permission questions were still asked instead of one. The batching fix now
tracks already-started native presence queries before publishing the combined
question. Its six focused tests pass, including a third query completing after
the original gather interval, cancellation, and separate later-site questions
(`windows-work-app-entry-batch-tests.log`). It does not increase the timer.
Human-handoff POSTs now have their own exact Started URI/native ID/revision
correlation under the existing handoff deadline and site policy; admitted native
precommit redirects replace that token with one for the exact new target. Four
focused tests cover success, redirects, expiry, cancellation, stale identity,
one-time consumption, and ended/successor handoffs. Confirmed Work action tokens
remain separate. The existing native Consent run proves the native
Started-before-resource ordering; a real human sign-in UI flow remains pending.
The fresh native Apps rerun now passes: one question names all three services,
all three authenticated reads succeed with zero model calls, and native shutdown
and exact journal retirement succeed (`windows-work-apps-batched-presence.log`,
exit 0). The final integrated results and their deliberate recovery boundary
are recorded above; manual parity acceptance remains pending.

The next integrated run passed its first eleven scenarios through Search and
Approve, then the Changed fixture waited on a generic human-decision pane that
its scripted person did not decline. No native capture or page-health failure
was observed. Its exact owned child was stopped and reaped, and the parent
retired its exact journal (`windows-work-all-final-integrated.log`); this is an
interrupted run, not a complete-suite pass or clean native exit. The fixture is
corrected to decline only a strictly newer handoff from the same approved
execution, attempt, and Read step. Closing that follow-up settles the original
undispatched approval as failed; zero-POST and stale-approval assertions remain
unchanged. The later integrated run exercises and passes this exact branch.

The earlier native write checkpoint passed Slack send, Slack Enter,
Linear issue creation, and Notion editing. Each has exact expected content and
exactly one successful confirmation. Slack and Linear each create exactly one
server object; Notion's autosave endpoint receives two saves with the exact final
text. Native resources settle and the parent retires its exact disposable journal
scope (`windows-work-writes-settlement-fixed.log`, exit 0). Six-app reads also
passed with zero model calls for the reads. The focused Search rerun now also
passes both native forms, exact results, one session-choice prompt, zero native
debt, clean shutdown, and exact journal retirement
(`windows-work-search-navigation-guard-fixed.log`, exit 0). The final four-write
rerun is recorded above.

The first broader integrated suite passed
Session, Always, Never, Route, Redirect, Leave, Autopost, Held, and Wall, then
fails Search with an outstanding native preview callback and unsuccessful page
retirement. Later attachment cannot proceed. The exact owned probe child was
stopped after identity verification; its parent reaped it and retired the exact
journal scope. No clean native shutdown or full-suite pass is claimed for that
run (`windows-work-all-integrated.log`). Its lifecycle cause is corrected below;
manual parity acceptance remains pending.

The Search follow-up corrects two platform seams. Native Enter now includes
Chromium's fixed carriage-return text only on key-down, enforced by the closed
CDP decoder; no form-submission script fallback was introduced. Preview debt
travels with its presentation across reads/actions. Retirement revokes input
immediately and defers hiding until the exact capture callback; dropping an
already retired or never-shown presenter cannot hide its successor. Native
action dispatch also waits for a pending preview within its original deadline.
The native two-capture retirement/successor test passes with exact teardown
(`windows-work-native-frame-retirement-tests.log`). Ten native protocol tests,
the runtime digest pin test, and the coherent build pass. The focused Search
rerun submits with Enter but still fails retirement after a later preview is
stranded (`windows-work-search-frame-enter-fixed.log`); its parent retires the
exact journal, but native cleanup is not proven. Investigation identified the
separate navigation commit handler hiding the native controller while the
retained Work presenter still considers itself visible. Work now relies on its
existing presenter for rendering rather than installing Wry's no-op hide hook;
ordinary Browse and non-Work owned views retain that hook. Initial Work views
remain hidden, and navigation still synchronously invalidates document/action
authority. The native comparison proves ordinary navigation hides both HWND
and controller while Work-style navigation preserves the owned presentation
and completes its next preview. Three controls pass with exact teardown
(`windows-work-native-navigation-frame-tests.log`); focused Search then passes
as recorded above.

The separate protected Windows Work journal is implemented
and desktop startup selects it using the application identity and QA session.
Five focused native storage tests, the closed selector test, strict Store
Clippy, desktop compilation and its startup invariant, and two QA-session
routing tests pass. Fixed-purpose native admission also passes on the actual
account without enabling the generic namespace. The default Store suite passes
289 tests (two existing ignored), the actual application artifact subprocess
passes, and strict Store/App lint passes. The full host's cookie-loss cause has
been isolated to passing Windows verbatim path spelling to WebView2; changing
only that spelling reproduces failure in the previously passing direct control.
Both WebContext creation sites now project only the API argument to ordinary
Win32 spelling, preserving canonical ownership checks. The original four path
tests passed, including an existing directory beyond 260 characters. The full-host witness
now retains its cookie and serves the authenticated response with zero model
calls. The complete witness also passes with the existing short local-temp UDF
selector: exact process-group event, signaled retained process handle, and clean
Shell/Store shutdown (`windows-work-shutdown-short-udf.log`). The deeper protected
fixture initially failed final process exit with every admission predicate valid and
every tracked resource count zero, even when observation is skipped. This is
isolated to path depth: a shorter protected sibling with identical inherited
permissions also passes. The full six-app views replica now passes on that
protected location with the production typed journal: six successful reads,
zero model calls for those reads, one session-choice prompt, clean shutdown,
and exact parent-owned journal retirement. Its preceding fresh-session seed
used two real OpenAI calls. Evidence: `windows-work-views-protected-fixed.log`.
The first write run exposed a document guard consuming fetch/XHR requests from
WebView2's controller-wide filter union. It now checks native ResourceContext
before applying document policy. Its native regression proves one fetch POST,
zero unapproved document POSTs, and exact teardown. A controlled rich editor
also passes exact trusted input while the hostile beforeinput refusal remains
intact. The Notion fixture now expects its requested AllowedForRun confirmation.
The next real write run delivered exactly one correct Slack message per Slack
scenario and exact Notion text, but post-action verification failed with
EvidenceAfterDeadline and correctly records OutcomeUnknown. The cause is now
fixed: Windows marked every successful action for a three-second passive
lifetime hold, exceeding the two-second verification budget before observation
could start. Matching macOS, that hold now applies only to uncertain effects;
revoked effects retain it. The shared clock, deadlines, and verifier are unchanged.
Two focused regressions and the native positive/hostile input guard pass,
including successful completion without the uncertain-effect hold. The subsequent
integrated run passes all four write scenarios as recorded above. Evidence:
`windows-work-native-settlement-tests.log`,
`windows-work-positive-rich-settlement.log`, `windows-work-writes-network-fixed.log`,
`windows-work-native-network-tests.log`, and `windows-work-positive-rich-action.log`.
Remaining replicas are pending; no full parity result is claimed.

An existing shorter Windows alias also resolves the deep protected UDF shutdown
failure without relocating data. Production now prefers that verified alias at
the WebView2 API boundary and retains ordinary spelling when none is available.
All eight path tests pass, including exact directory identity, actual rename
refusal while its native handle is held, and rejection of mismatched bindings.
Access, lookup, malformed-result, and identity errors still refuse construction;
only explicit native API non-support permits fallback after a failed query.
The full deep-path witness passes with full observation, a retained persistent
cookie, an authenticated read with zero model calls, and exact native/Shell/Store
shutdown (`windows-work-shutdown-deep-short-alias-pinned.log`). This diagnostic
result does not qualify deep paths on volumes without shorter aliases.
Evidence for the production helper is
`windows-work-native-short-path-production-tests.log`; the coherent probe build
passes in `windows-work-short-path-production-probe-build.log`.

A separate native SDK control at an ordinary UDF length of 451 UTF-16 units
refuses controller construction before any cookie operation
(`windows-work-cookie-long-udf-sdk.log`). The filesystem roundtrip test therefore
does not certify WebView2 support at that length. No arbitrary 260-character
cap was added; unsupported native construction still refuses through the
existing error path. The failing control retains only its fresh synthetic UDF
as evidence and does not claim successful native exit or cleanup.

The protected-journal probe now runs inside a parent-owned disposable QA scope.
The parent retains a liveness pipe, bounds the child lifetime, and retires only
its exact scope after actual child exit. The failure-path native fixture proves
retirement without provider calls (`windows-work-probe-owned-journal-cleanup.log`).
Four persistence-boundary mutation tests and the complete agentic boundary pass
with the new retired-profile guard; formatting and whitespace checks pass.
An independent caller review found no actionable issue in identity/session
routing, configured startup recovery, deletion ordering, historical evidence
selection, or probe retirement. Forced parent termination can leave its fresh
disposable scope; no broad orphan cleanup or successful retirement is claimed.

**Initial qualification failure:** after the user saved an OpenAI key, native credential
loading passed, but the first `--loopback-site views` run failed during retained
execution journal admission (`Contract`, persistence `Unavailable`). The shared
Windows private-filesystem namespace adapter is still disabled in production.
Retained browser execution uses that journal even though final product artifacts
use a separate profile SQLite store. The earlier characterization of this
namespace as only a legacy fixture dependency was incorrect. The replacement
below separates Work's fixed database capability from the generic namespace gate.
Evidence: `target/windows-work-live-views-20261002.log`. No full scenario passed.

A subsequent debug-namespace-enabled replica run reached Running and completed
the initial real-provider page read with one archived artifact and no persistence
failure. OpenAI returned successful responses. The following multi-page view
run then failed native process-reuse admission (`observer_matches=false`,
`ContextLost`, `NativeRefused`). It was stopped after capturing the failure;
the user's QA process was not touched. Evidence:
`target/windows-work-debug-views-20261002.log`. This is partial integration proof,
not a passing views scenario or shipping admission.

Further native traces isolated two independent lifecycle defects. WebView2 can
start a successor browser process after the last controller closes while the
host still holds the prior generation's exit observer. Recovery must settle
both the original Environment5 exit proof and the exact retained process handle
before admitting that successor. Separately, the temporary Work cookie source
closed its controller before dispatching the asynchronous cookie query. That
could report a remembered signed-in store as absent. Fixes are being validated
together; failed or incomplete presence queries now remain unknown and require
the existing session-choice policy rather than implying cookie absence.
Evidence: `target/windows-work-debug-views-process-20261002.log` and
`target/windows-work-process-cohort-stderr.log`. These are diagnostic failures,
not completed scenario evidence.

The combined fixes compile and pass 11 process-exit/generation regressions,
two late/error cookie-result regressions, and strict scoped native Clippy.
The Windows replica fixture now constructs, verifies, and closes a real ordinary
blank tab before its unchanged fresh-session seed check. This binds the selected
native profile without pinning a permanent browser controller. The next live run
passes the seed and constructs all six later pages through actual browser-process
replacement with no native admission failure. It still fails the views scenario:
the persistent fixture login cookie is absent on subsequent pages, all six ask
to sign in, and the expected six reads are missing. Cookie storage/retention is
under investigation; no views pass is claimed. Evidence:
`target/windows-work-views-initialized.log`,
`target/windows-work-rollover-tests.log`,
`target/windows-work-cookie-presence-tests.log`, and
`target/windows-work-rollover-clippy.log`.

Further bounded native queries prove that the seeded cookie is persistent and
has a future expiry before close. The next exact profile generation reports no
cookie. Neither `Cookies` nor `Network/Cookies` exists in the attested profile,
although other network databases do persist. A second disposable browser UDF
under normal LocalAppData/Temp reproduces the failure, so the protected test
ancestor is not sufficient to explain it. Native error logging did not identify
a cookie error. Provider-free direct WebView2 controls pass for the default
profile, a short named profile, and the exact Work-derived profile name: each
retains its persistent cookie across proven native exit/reopen. Subsequent
controls also pass with exact Work browser arguments, enabled extensions with
only the admitted runtime components, real loopback HTTP Set-Cookie ingress,
retained seed metadata, and the actual durable Work constructor with empty
session seeding. Retaining the bootstrap COM environment also preserves the
cookie. A suspended first cycle preserved it across restart, although that
control's second cleanup suspension was unproven and the whole control is not
reported as passing. The full host remains the outstanding comparison.
Evidence:
`target/windows-work-session-cookie-disk.log`,
`target/windows-work-session-cookie-local-appdata.log`, and
`target/windows-work-session-cookie-errors.log`, plus
`target/windows-work-cookie-sdk-control-binding.log` and
`target/windows-work-cookie-sdk-work-name.log`,
`target/windows-work-cookie-sdk-http-response.log`,
`target/windows-work-cookie-sdk-metadata.log`,
`target/windows-work-cookie-native-constructor.log`, and
`target/windows-work-cookie-native-lifecycle.log`.

The full Engine/Shell provider-free witness reproduces cookie loss with the same
browser PID, exact canonical native ProfilePath/UDF hashes, private=false, and a
nonrestricted/non-elevated host token. The first real HTTP response produces one
persistent future-expiry cookie; after destroying that Work page, the next page
at the same origin sees none. No cookie database exists in either observation.
This rules out provider calls, profile-path substitution, and process restart
as necessary causes. Histogram queries attempted after suspension were
unavailable and do not diagnose SQLite. Evidence:
`target/windows-work-cookie-full-host-build.log` and
`target/windows-work-cookie-full-host.log`.

The same direct SDK controls also pass inside the full probe executable,
eliminating executable identity as the difference. Source inspection then found
the full host passes `prepare_profile_directory()`'s canonical `\\?\` path into
WebContext, while direct controls pass ordinary Win32 paths. A single named SDK
case changing only that UDF input spelling reproduces cookie loss: one persistent
future-expiry cookie before exact native exit, zero after reopening the same
native profile/path. Canonical hashes had hidden this spelling difference.
This is causal evidence for an API-only path projection, preserving canonical
ownership and attestation separately; no keeper controller, delay, cookie export,
or permission relaxation is justified. Evidence:
`target/windows-work-cookie-full-executable-sdk.log` and
`target/windows-work-cookie-verbatim-sdk.log`.

The follow-up native filesystem suite now passes 38 tests. After the user enabled
Windows Developer Mode, its separately run mandatory reparse-point test also
passed under the same non-elevated token. The earlier Win32 error 1314 fixture
creation failure is superseded by this successful test. Three actual native defects
were corrected: directory reopening now uses held volume/file identity with
full identity and security-descriptor revalidation; directory unlink keeps the
named held-parent handle; post-unlink settlement can recognize an already-held
zero-link file while new admission still requires exactly one link. Cross-process
locks, sealed publication/recovery, exact deletion, and quarantine tests pass.
The expanded suite additionally proves simultaneous first lock creation,
forced-owner termination and independent reacquisition, exact canonical/staging
lock residue handling, and native junction rejection at leaf/root/ancestor
positions. A losing initializer may conservatively refuse an exact-name race
with `IdentityAmbiguous`; the test still requires exactly one owner and intact
canonical identity/marker after forced termination. Strict all-targets adapter
Clippy passes. Evidence:
`target/windows-private-fs-expanded-live-validation.log`,
`target/windows-private-fs-expanded-mandatory-reparse.log`, and
`target/windows-private-fs-expanded-clippy.log`. Earlier blocked
attempts remain recorded in `windows-private-fs-mandatory-reparse-blocked.log`
and `windows-private-fs-symlink-permission.log`. Production admission remains
closed; these results alone do not certify complete Work runs.

With the adapter explicitly enabled only for debug validation and the protected
fixture root, the restored Windows Store journal suite passes 19 tests. It covers
Claim/CAS, lost acknowledgements, partial restart rollback, result custody,
profile erasure, and a real child-process fence that survives Store shutdown and
releases at process exit. The real actor callback-loss/panic fixture also passes.
Its intentionally panicking callback is contained and does not kill the Store.
Logs: `windows-work-store-native-validation.log` and
`windows-work-store-callback-native-validation.log`. The child-only ignored entry
is exercised by its parent fixture. These are diagnostic-feature results, not
evidence that default shipping admission is enabled.

The restored actual application subprocess fixture also passes: publication in
one OS process survives exit and is read in a second process without restoring
execution. Log: `windows-work-app-journal-native-validation.log`. The previously
removed Windows coverage is back; it requires the diagnostic namespace feature
until production admission is qualified.

The suite used a unique disposable fixture under the protected
`windows-runtime` parent. This machine's normal Temp and AppData ancestor ACLs
grant unrelated principals mutation rights and correctly fail the private-root
policy. No machine ACL was changed, no principal was added to the trust list,
and no product data directory was silently relocated.

Read-only ACL inspection located the first rejected product ancestor at
`C:\Users\user\AppData`: an explicit application-capability SID has effective
FullControl, inherited by Local. The named CodexSandboxUsers grant on those two
ancestors is read/execute only and is not that rejection's cause; Temp has
separate unrelated mutation grants. The capability's originating application
could not be established, so this evidence does not identify a clean Windows
default or justify removing it. A separate account is not required to continue
implementation. Inspection found that this account's Profile KnownFolder has
acceptable ancestors; an explicit read-only native check of the actual
KnownFolder Profile chain also passed. The fixed `.zephium-native-v1`
native-storage anchor now passes eight fixture tests, its default closed-gate
test, and strict default/validation Clippy. It creates a protected inheritable
ACL and retains pinned parent identities and an exclusive lifetime lease.
Existing permissions are never repaired. WebView2 data remains in its existing
AppData location. The generic namespace anchor remains diagnostic-gated. A
separate typed capability exposes only the fixed Work database, WAL and SHM;
its production entry is being qualified independently of generic extension-tree
operations. The selected design puts the actual execution records and
artifact bodies in a separate protected journal database. Existing settings,
profile databases, and browser UDFs retain their paths; legacy AppData journal
rows are not imported as protected authority. No actual product
directory was created and no user data was moved. These results do not establish
shipping admission; separate non-administrator qualification remains open.
Evidence: `target/windows-native-storage-anchor-tests.log`,
`target/windows-native-storage-knownfolder.log`,
`target/windows-native-storage-default-gate.log`,
`target/windows-native-storage-default-clippy.log`, and
`target/windows-native-storage-validation-clippy.log`.

The dedicated database now passes five native fixture tests and one selector
test, with strict all-targets Store Clippy. It retains atomic record/artifact
publication, process-lifetime ownership, restart recovery, and historical reads
without Claim. Profile deletion uses protected intent before metadata removal,
then securely deletes bodies and requires a successful truncating WAL checkpoint;
restart reconciliation retries unfinished erasure. Tests verify synthetic body
bytes are absent from the database and sidecars while terminal evidence remains.
Ordinary Browse startup installs the selector without creating a journal.
Evidence: `target/windows-protected-work-store-tests.log`,
`target/windows-protected-work-selector-test.log`, and
`target/windows-protected-work-store-clippy.log`.
Desktop integration evidence:
`target/windows-protected-work-desktop-startup-test.log` and
`target/windows-protected-work-desktop-session-tests.log`. These compile and
exercise the isolated QA configuration; they do not launch the user's UI.

The fixed-purpose entry passes native checks on this account with the generic
namespace feature absent: exact database ownership, orphan sidecar and hardlink
refusal, real symbolic-link refusal with an unchanged external target, and
cross-process exclusion followed by recovery after forced owner termination.
The initial run exposed an additional read/execute ACE on the otherwise empty
test-created top anchor. Its cause was not proven. The three empty directories
were retired using their recorded full identities and exact parent handles,
then recreated in the next test; no ACL was repaired. The read-only grant
reappeared between separate escalated tool invocations. Review found the top
container has only application directories, never payloads: its predicate now
matches other namespace ancestors (no untrusted mutation, pinned full identity
and immutable ACL snapshot). Every application/session root and inherited file
still requires the exact private ACL. There is no new trusted principal.
Regressions prove a new capability accepts read-only top access, mutation of an
already-held top quarantines it, outside top write/delete-child rights refuse,
and added read rights on actual application/session roots refuse. The final
feature-free suite passes five entries; scoped fixture/hostile ACL tests pass
eight, and strict all-targets Clippy passes. No Product or existing user QA data
was accessed or removed. Evidence: `target/windows-work-fixed-native-tests.log`,
`target/windows-work-fixed-fixture-tests.log`,
`target/windows-work-fixed-native-clippy.log`, and
`target/windows-work-owned-empty-retirement.log`. The temporary one-off cleanup
test was removed from source after executing; its log is retained as evidence.

Shared Windows journal regressions now select fresh owned QA sessions through
this typed path. The debug-only fixture owner proves exclusive creation and
retains the original full identity; cleanup reopens only existing components,
refuses unknown entries, and removes only the fixed database/sidecars/lease and
its exact session. Full replica probes use a parent-owned fixture and a child
process so cleanup occurs only after the original process-lifetime journal
fence is released by actual OS exit. No shipping manifest enables this fixture
feature; optimized builds reject it.

The migrated default Store suite passes 289 tests with two existing ignored
entries and no generic namespace feature. The actual Shell artifact subprocess
passes publication in one process and historical read in a second, then retires
its exact owned QA session. The deletion integration regression exercises real
authorization/finalization/restart hooks: failed protected purge preserves the
pending tombstone and body; retry erases the body while preserving terminal
records; failed restart admission preserves cleanup debt until successful
reconciliation. Strict Store/App all-targets Clippy passes. Evidence:
`target/windows-protected-work-store-default-tests.log`,
`target/windows-protected-work-app-artifact-test.log`, and
`target/windows-protected-work-store-app-clippy.log`.

## Platform mappings

| macOS seam / shared contract | Windows implementation |
| --- | --- |
| Provider Keychain, page-agent credential, Jev/TypeSafe decision credential | `crates/zephium-credentials` uses native generic Credential Manager records through `CredWriteW`, `CredReadW`, and `CredDeleteW`. Agentic provider and development loaders share native vault serialization and preserve the existing bounded retry. `desktop/src/work_decision.rs` and `work_provider.rs` use those loaders on Windows. Secrets remain native; the frame receives closed presence/status facts. |
| Connection OAuth/bearer persistence, previously a non-macOS stub | `crates/zephium-mcp/src/keychain_windows.rs` stores bounded chunked credentials in Credential Manager, with publication and cleanup preserving the previous secret on failed updates. |
| `ReadPublic`, `Run`, and `Start`; durable Work composition | Windows is admitted through the shared desktop provider/composition interfaces. `crates/zephium-engine/src/host/work_windows.rs` constructs owned WebView2 contexts behind the engine API. Construction, navigation, actions, observation, screenshots, history, cancellation, and teardown retain the shared authority and deadline checks. |
| Extension-free owned agent views | Windows Automation environments disable extensions and retain native profile/extension attestation. Work pages remain excluded from extension tab access. Anonymous contexts have separately tracked profile erasure and terminal ownership. |
| Selected signed-in site store | Windows uses a persistent, extension-free Work site profile inside the selected logical profile's Automation environment. Its identity preserves registrable site, scheme, and port. Exact-origin initial cookie seeding refuses destination identity collisions, journals writes, and rolls back only attempted cookies. Unproven cleanup quarantines admission. It never blanket-clears a retained store. |
| Durable Human authentication and session presence | Human login remains in the stable Work site profile across pages/runs. `work_seed_metadata.rs` publishes a nonsecret exact-origin decision after successful initial admission, before navigation/Human exposure, preventing later Browse reseeding after Human logout. Hashed, bounded scheme/port descriptors allow presence queries to locate previously constructed stores after restart. Profile erasure owns both stores and metadata. |
| Semantic actions and native input | Shared digest-pinned semantic runtime plus Windows CDP observation/input adapters in `platform/windows/semantic_action.rs` and `semantic_runtime.rs`. Rich fill uses native editing while retaining beforeinput target-change refusal and postcondition checks. Selection, key, pointer, and scroll actions keep shared approval authority. |
| Human page presentation and restoration | `desktop/src/work_human.rs`, shared composition, and `platform/windows/work_presentation.rs` present the exact owned child HWND with checked parent/DPI/bounds and restore it on release. Separate Human activity authority prevents leased/acquiring or Human-active views from being suspended by a late callback. Continue waits for native idle settlement with a bounded deadline and retained callback debt. |
| Covered/minimized leased pages versus retained idle pages | The controller remains visible and resumed while leased or Human-active. Retained idle views use `MemoryUsageTargetLevel Low`, hidden controller visibility, and `TrySuspend`; native suspension failure refuses the transition. Successful initial construction and Human release reconcile idle state. |
| Work file grants and protected paths | `crates/zephium-core/src/work/runtime.rs` resolves the Windows home/Known Folders and protected-root policy. `crates/zephium-app/src/work_files.rs` validates canonical drive paths and opened identities, rejects reparse escapes/hard-link hazards, and pins native directories and mutation sentinels against replacement. |
| Exclusive file moves, folder picker, reveal/open, media | Moves use native `MoveFileExW` without replacement. The shared folder dialog remains available on Windows. `desktop/src/media.rs` routes Windows shell reveal/open and bounded `?w=` thumbnails; no separate frame fork was added. |
| Local shell execution and computer tools | The Windows runner uses native PowerShell with fixed noninteractive arguments and encoded input, bounded output, cancellation, deadlines, and job-owned process trees. Policy classifies only supported literal commands and preserves approval scope. Delegation quoting is native, while public relative paths use stable `/` separators. The tool wire name stays unchanged. |
| MCP stdio child ownership | `crates/zephium-mcp/src/stdio_windows.rs` starts suspended, assigns the exact child to a kill-on-close job before resuming, suppresses consoles, and retains bounded async pipes. Stop/shutdown owns descendants even after their original parent exits. Overlong newline-delimited output fails before exceeding its read contract. |
| UI memory release on switch/idle | `desktop/src/work_memory.rs` applies the main UI WebView2 memory target and bounded heap collection, restoring Normal on activity/failure. It does not clear cookies, origin storage, disk caches, or agent stores. Frame thumbnail requests use the existing media route. |
| Windows canvas input and visual frame | The shared canvas uses the correct Windows Control modifier while preserving macOS Meta behavior. Automated shared-frame and Chromium checks passed; themes, native window controls, DPI, and smoothness still need the user's manual validation. |
| Native qualification and boundaries | The Windows probe reuses loopback replicas and shared composition. The native hostile rich-fill guard and provider-free retained lifecycle probe passed. The complete replica suite and real-account acceptance remain unverified. Boundary checks were extended to require Windows retained-seed safeguards and exclude diagnostic output from production modules. |

Windows initial migration is a bounded cookie bridge, not a copy of macOS's full
selected WKWebsiteDataStore. Browse local/session storage and extension state
are not imported into the extension-free Work store. Sites requiring those for
authentication may require Human sign-in in Work. There is no broad profile
directory cloning or origin-storage synchronization.

### Bounded canvas page previews

The person-facing Windows frame path now reads the exact physical controller
bounds before CapturePreview. The former 1280×800 evidence budget rejected a
1920×1200 capture at 150% DPI. Canvas previews instead use a separate bounded
PNG path, reduced to at most 640 pixels on either side (640×400 for the standard
Work viewport) and 1 MiB encoded. Semantic evidence capture is unchanged. The
page viewport is not resized to obtain the thumbnail.

The decoder accepts only the expected RGB/RGBA8 PNG dimensions, with a 16 MiB
raw stream ceiling, a 9,216,000-pixel source ceiling, and a separate 48 MiB codec
allocation limit. Decoded RGBA can additionally occupy 35.2 MiB; the reduced
pixels occupy at most 1.6 MiB. These are separate bounds, not a 48 MiB total
allocation claim. At most eight source/byte jobs and two decoder workers run
globally. Fractional area weights preserve equal footprints at non-integral
scales such as 125%, and premultiplied alpha avoids transparent-edge fringes.
Output is re-encoded without source metadata and refused before exceeding the
encoded byte ceiling.

Only bytes enter the worker thread. The original non-Send completion, document
fence, and callback debt stay on the originating STA through delivery.
A bounded one-shot timer polls only while a job is pending; worker panic or
channel disconnection produces a terminal failure. There is no idle image
worker, recurring capture timer, or new UI polling cadence. Capture remains
driven by settled observations/actions, with the existing minimum 250 ms
spacing and one pending capture per resource; the active canvas continues its
roughly one-second activity reads.

All ten focused image, stream, and completion tests passed. The final debug-build
1920×1200 high-entropy conversion took 232 ms and produced a 640×400,
923,169-byte PNG. A synthetic native 150% test delivered three bounded frames
in 259, 247, and 213 ms, including CapturePreview, resize/encode, and STA
delivery; it also proved retirement/input revocation, successor rendering,
navigation, and exact browser cleanup. Evidence:
`target/windows-work-frame-regression-tests.log` and
`target/windows-work-frame-native-dpi-tests.log`.
These are content- and machine-dependent fixture timings, not an FPS,
zero-cost, or real-account smoothness claim.

Persistence acknowledgment now follows a successful frame-store write.
A failed write leaves the current in-memory picture available and retries on
a later activity read, at most once every five seconds. Retry and acknowledgment
bookkeeping follow the bounded recent pages (eight attempts, sixteen pages
each), with no background retry timer. Persistence gates historical/restart
availability; it does not gate immediate display of an in-memory frame.
The actual blocked-directory/recovery regression passed, verifying the live
picture survives failure, recovered storage is not hammered before the retry
deadline, successful persistence is acknowledged, and the same frame is not
written again: `target/windows-work-frame-persistence-tests.log`.

The separate QA executable is
`target/windows-work-qa/Zephium Work QA Windows 2026-10-03 Previews.exe`
(115,298,816 bytes; SHA-256
`4C842D97FE71B7CB4B2B283D0C0CA257B31B799B7D0194BAA6587E39BB91C8B3`).
It retains the `app.zephium.webext-qa` identity and includes these preview fixes;
the background behavior is unchanged. Build and strict production lint pass in
`target/windows-work-preview-qa-build.log` and
`target/windows-work-preview-production-clippy.log`. The source boundary and its
dependency mutation regression pass. The optional PNG-only image dependency is
explicitly checked without adding diagnostic authority to production.
Strict Engine all-target diagnostic lint, xtask lint, formatting, and whitespace
checks also pass (`windows-work-preview-diagnostic-clippy.log`,
`windows-work-preview-xtask-clippy.log`, `windows-work-preview-format.log`).
The executable was staged without launching it or replacing the prior QA build.
Fresh page activity is needed for cards whose earlier captures were rejected;
missing historical pixels cannot be reconstructed from saved titles.

### Preview freshness and active native chrome backing

The user corrected the earlier display setting to 125% and is now testing at
100%. The original 150% capture remains a synthetic qualification case, not a
measurement of the user's earlier display. Missing high-DPI frames and stale
frames have distinct causes.

Settled page changes now coalesce into one pending capture demand instead of
being dropped while a capture is busy or within the 250 ms minimum interval.
The last demand drains before rendering retirement. Successful asynchronous
Windows image completion wakes the existing application projection receiver;
it does not wait for another model event. Product projection retains its last
bounded frame through engine destruction, and terminal composition samples
once more before settling the page. Desktop retains at most eight data-only
page mailboxes (sixteen pages each), harvesting completed work before releasing
it. Reading another work cannot consume that work's unread final picture.
Bounded mailbox eviction saves the newest pixels under their original scope.
These mailboxes retain no execution, browser, or permission authority.

Windows chrome now uses its existing native WebView2 controller's opaque
theme-matched background while observation pages actually render underneath.
Registration is dormant until the first rendering owner. The exact original
background is restored after the last owner releases it, including outstanding
capture callbacks. Native rendering admission requires successful activation;
theme changes update an active backing, and HWND destruction detaches old
leases. Switching the selected UI mode does not prematurely remove protection
from an agent page that still renders. Inactive/disabled browsing adds no
visual effect. There is no extra window, blur layer, or background polling loop.
This is not a claim of zero cost: bounded preview resize/encode still costs
CPU and memory when pages change, and native color transitions still repaint.

The provider-free retained lifecycle run passed: initial generation 1 became
generation 3 with changed PNG pixels after the burst of final observations;
the last demand drained before revoke. Generation remained unchanged during
45 seconds retained idle. Durable origin reopening, original health-owner
retirement, frame removal, native zero, and Shell/store shutdown all passed.
Evidence: `target/windows-work-freshness-retained-native.log`. Focused tests
also pass for the coalesced application wake (one), final product/mailbox
retention (included in six `final_` regressions), pending capture scheduling
(three), and native backing ownership (five). These are synthetic tests;
the user's Google Flights consent/result sequence still needs a fresh QA run.

Native capture/retirement cases passed at physical source sizes 1280×800,
1600×1000, and 1920×1200 (the 100%, 125%, and 150% equivalents of the standard
logical viewport). Each produced three 640×400 frames with exact successor
navigation, input revocation, and native cleanup. These tests set controller
geometry; they do not change Windows monitor scaling. Debug-build total capture
times were 165–211 ms, 249–347 ms, and 265–298 ms respectively, on synthetic
pages. Logs: `windows-work-freshness-100-tests.log` and
`windows-work-freshness-dpi-tests.log`. Both desktop persistence regressions
passed. Strict all-target production Clippy for Engine/App/Agentic/Composition/
Desktop and the production/diagnostic source boundary also passed:
`windows-work-freshness-production-clippy.log`,
`windows-work-freshness-persistence-tests.log`, and
`windows-work-freshness-boundary.log` under `target/`.

The native opaque-chrome fixture passes with three real WebView2 controllers:
strict owned-client screen pixels match the light native backing; overlapping
render owners and pending capture hold protection; a sharp black/white frame
is followed by fresh blue/red document pixels beneath opaque chrome; final
release restores the exact original color; every controller and browser exits.
Evidence: `target/windows-work-backing-native-tests.log`. The initial fixture
omitted the product's existing CSS/native theme pairing and rendered a dark
Chromium canvas despite a light native background getter. Mirroring the actual
product's `color-scheme` and PreferredColorScheme transitions resolved that
fixture error without production CSS changes or relaxing pixel assertions.
The test also verifies owned-screen visibility before reading any screen crop.
It does not replace visual acceptance of the complete Tauri UI.

The updated QA executable is
`target/windows-work-qa/Zephium Work QA Windows 2026-10-03 Fresh Previews.exe`
(115,448,320 bytes; SHA-256
`A1E2BFF6A3061BB7771551ADC259405D9257F2965EF6B44F385C6D6B036FF7BF`).
Its copy hash matches the successful default-work-product/webext-qa build.
Metadata: `target/windows-work-freshness-artifact.json`; build log:
`target/windows-work-freshness-qa-build.log`. Final diagnostic all-target Clippy
also passes (`target/windows-work-freshness-diagnostic-clippy.log`). The app
was not launched, older executables were preserved, and the visible product
name remains Zephium Extensions QA (`app.zephium.webext-qa`). Close the previous
QA instance before opening this executable. Historical stale cards are not
reconstructed; test the Google Flights sequence in a new run.

### Native blur feasibility (not enabled)

The provider-free prototype proves native Gaussian blur of an actual synthetic
WebView2 page in isolation. Its composition-controller control reduced screen
contrast from 1,175,040 to 87,552 while CapturePreview remained sharp at
1,175,040 and 1280×800. Removing the effect admitted a trusted native mouse
click; restoring it preserved navigation and preview completion. A copy of the
test EXE with a Windows 10 compatibility manifest also proved that the existing
layered Wry child HWND can feed `CreateSurfaceFromHwnd` with the same page/blur
and sharp-preview results. The unmanifested child refused the layered style.
This is fixture evidence, not qualification of full Human input or performance.

Neither path produced the required blur beneath the transparent HWND-hosted
chrome. With sharp controls and an opaque central panel above the native page,
the exposed region was flat rather than the underlying blurred page. The first
chrome assertion incorrectly accepted a flat background; it was replaced with
an exact baseline comparison and a required spatial variation. The final
Windows 11 SDK `DWMWA_REDIRECTIONBITMAP_ALPHA` experiment returned success but
both paths still failed: exposed-grid difference 1,175,520 and variation zero.
The source-change check was consequently not reached. All controllers closed
and exact browser exit was proven. Evidence:
`target/windows-work-native-blur-alpha.log`,
`target/windows-work-native-blur-compat.log`, and owned-client PNG artifacts in
`target/windows-work-blur-prototype/`.

No native blur, shipping manifest change, composition hosting rewrite, or Human
input rewrite is enabled. The interactive feasibility test is explicitly
ignored in ordinary test runs and retains its strong assertions for deliberate
native experiments. Thumbnail generation does not certify production blur,
both-theme appearance, negligible overhead, or user visual acceptance.

## Verified checks

The logs below are local, ignored artifacts under `target/`. The earlier
affected-crate snapshot passed 2,472 tests (six ignored), both workspace strict
Clippy graphs, formatting, and the diagnostic boundary check. That snapshot
predates the Jev settings and private-filesystem follow-up changes. It also
excluded the Windows artifact-journal subprocess fixture, which was a coverage
gap, not evidence that the journal was unnecessary. Current follow-up results
are recorded separately; the old green snapshot does not certify shipping Work.

| Check | Recorded result / evidence |
| --- | --- |
| Work app tests | Earlier expanded graph: 636 passed, 3 ignored; `windows-work-final-regressions.log`. Native files, PowerShell lifecycle, policy, computer-tool, product artifact persistence and SQLite reopen regressions passed. The incorrectly excluded Windows artifact-journal fixture is explained below. |
| Core and desktop tests | Core 296 passed; desktop 141 passed, 1 ignored; `windows-work-final-regressions.log`. |
| Agentic and engine tests with durable Windows probe graph | Agentic 928 passed; engine 403 passed, 2 ignored; `windows-work-final-regressions.log`. Includes retained-cookie COM failure/rollback, metadata, Human callback lifetime, and anonymous-cycle tests. |
| Native seed metadata | All 3 tests passed on Windows after correcting exclusive creation flags: exact/idempotent decisions and erasure, hostile stamp/hard-link/replacement refusal, bounded remembered descriptors and reopen. Included in the successful engine rerun. |
| MCP and Work composition | MCP 16 passed; composition 50 passed; `windows-work-final-regressions.log`. Native job-tree tests cover descendant cancellation/timeout and parent exit. |
| Credential Manager | Credentials crate: 2 passed in `windows-work-final-regressions.log`, including the unique nonsecret native roundtrip. This proves the OS adapter, not Settings-to-provider acceptance. |
| Blocker cache / update validation | Required `check-blocker-security-fork` completed successfully; `windows-work-blocker-final.log`. Windows warm-cache fixture now uses the existing protected test root. TUF/update tests: 80 passed; `windows-work-tuf-final.log`. Production ACL checks were not relaxed. |
| Shared frame | 641 tests passed; check, build, and styles passed; `windows-work-frame-final-{check,build,styles}.log`. Three targeted Chromium component tests passed; `windows-work-component-tests.log`. |
| Native hostile rich-fill guard | Passed: refusal, trusted beforeinput, original and decoy unchanged, exact teardown complete; `windows-work-native-guard.log`. These are bounded closed facts, not evidence of all replica actions. |
| Native retained Work lifecycle | Passed with exit 0; `windows-retained-20261002-055526/{stdout,stderr}.log`. Initial and reopened construction had exact joins and Current health, two complete nine-node observations, covered leased progress, bounded PNG capture, revoke settlement, idle retention, same-origin durable reopen, and complete native/Shell/store teardown. |
| Strict lint and formatting | Workspace diagnostic and production strict Clippy both passed; `windows-work-workspace-clippy-final.log` and `windows-work-production-clippy-final.log`. Formatting passed. |
| Shared probe fixtures | All 16 platform-independent probe fixture tests passed on Windows; `windows-work-portable-probe-tests-final.log`. |
| CI boundaries | Model catalog, controller, runtime, and security-fork lock checks passed; corresponding `windows-work-check-*-final.log` files. Final `check-agentic-probe-boundary` passed, including shared semantic smoke and the negative parent-brand control; `windows-work-agentic-boundary-final.log`. All 170 xtask regressions passed, including bounded rendering and screenshot refusal checks. |

The cached Windows validation environment is loaded with
`. .\target\windows-dev.ps1` (two Cargo jobs, bounded debug output, no incremental
build). The optional AWS-LC assembly graph needed NASM; official NASM 3.02 was
extracted under ignored `target/windows-tools/nasm/nasm-3.02`, without a global
install or disabling assembly. The official archive had no published checksum
at the inspected release endpoints; the local computed digest is recorded in
`target/windows-tools/nasm/download-proof.json`.

## QA application

The final isolated QA application is
`target/windows-work-qa/Zephium Work QA Windows 2026-10-02.exe`
(114,740,224 bytes), SHA-256
`46116077A6E8A605EA7C303DD46190CBFC8F8E53C00721E2F7B40B9139050964`.
It builds from the verified sources with `work-product(default),webext-qa`;
the copy hash matches the built executable. Evidence:
`windows-work-final-qa-verified-build.log` and
`windows-work-final-artifact.json`. It was not launched and older builds were
preserved. Its visible name remains **Zephium Extensions QA**, identity
`app.zephium.webext-qa`. Close any previous QA instance before opening it.
This build is ready for the user's manual acceptance; it is not a release
certification. The scoped recovery shutdown contract is confirmed above.

The integrated Windows QA candidate built successfully with default
`work-product` plus `webext-qa`, without launching or replacing the user's app:
`target/windows-work-qa/Zephium Work QA Windows Candidate 2026-10-02.exe`
(114,739,712 bytes), SHA-256
`7FAA2DD8044BCC432EBB3742D7C8B67C535BC98551D811E7060726D3A6323FFD`.
Build evidence: `windows-work-final-qa-build.log`; verified copy metadata:
`windows-work-candidate-artifact.json`. It contains the protected journal,
native runtime fixes, and Jev settings. It predates only the final failure-only
recovery diagnostic. Product identity remains
`app.zephium.webext-qa`; close an older QA instance before opening this one.

The earlier isolated application, including Jev key setup, built successfully
with the existing `desktop/tauri.webext-qa.conf.json` identity and `webext-qa`
feature. The executable is
`target/windows-work-qa/Zephium Work QA Jev 2026-10-02.exe` (114,470,912 bytes),
SHA-256 `1A985504DE601DE012CF84B34DC196EB6A9DB65292A3F3CE2DAF278F38AD7DB7`.
The visible product name remains **Zephium Extensions QA** and its identity is
`app.zephium.webext-qa`. Build evidence: `windows-jev-qa-build.log`.
It predates the protected journal and final native runtime fixes and is not the
final Work implementation artifact. It contains no namespace-validation feature.
It was not launched or used to replace a running QA app.
Close the previous QA instance before opening it because both share the same
isolated application identity. The previous executable remains available.

Settings -> AI now contains the Jev (TypeSafe) API key row under Page decisions.
Recommended remains the default; a missing Jev key visibly falls back to
Standard. Saving/removing a key preserves an explicit mode choice. The native
vault holds the secret; settings never return its stored content or claim remote
acceptance merely because it was saved. The follow-up passes 641 frontend unit
tests, 11 Chromium component tests, two native decision/cache tests, the native
TypeSafe loader-bound test, frontend checks/build/styles and affected native
strict Clippy. Logs: `windows-jev-frame-check.log`, `windows-jev-components.log`,
`windows-jev-frame-build.log`, and `windows-jev-frame-styles.log`. Jev code remains
in the lazy AI settings graph, outside initial entrypoints and Work settings.

Historically, the restriction of the `attach_work` artifact-journal subprocess fixture
to macOS/Linux did not resolve Windows execution-journal support. Although final
product artifacts use profile SQLite `RuntimeUpdate` settlement and their
store-reopen tests passed, `attach_retained_work` also requires the Store's
`AgentWorkJournalPort`. The separately qualified fixed-purpose Windows storage
now supplies that port, and the native journal fixtures are restored and pass.
No generic production namespace restriction was relaxed to claim qualification.

## Native lifecycle measurements

The provider-free native retained fixture passed with exit 0. During its covered
45-second leased phase the fixture's counter advanced by 91. It captured a
bounded 1280×800 PNG (17,992 bytes, generation 1), received the physical revoke
callback, retained the resource through a 45-second idle phase, and reopened the
same durable origin. Both original health owners retired, the frame was gone,
native resources reached zero, and Shell/store shutdown was clean. The two
observations were Complete with nine nodes each. This proves the tested native
fixture lifecycle, not real-account authentication or the full Tauri UI.

The run exposed and corrected four native integration defects: the counted
bootstrap controller must survive first and existing-environment construction
and revalidation; Prepared presentation ownership must be valid before first
presentation; fresh operations renew their own deadline while existing fence
snapshots retain an immutable `Instant`; and preview capture must drain before
hiding or suspending the controller. None of these fixes removes the exact
identity, confinement, deadline, or teardown checks.

Measurements attach to process family PID 20820, including the diagnostic host
and WebView2 children, with eight processes in both phases. Each phase has 18
samples spanning approximately 20 seconds inside its 45-second window; startup
and the first CPU interval are excluded. Hardware: Intel Core i3-1115G4, two
cores/four logical processors, Windows 11 Pro build 26200, Balanced power plan.

| Metric | Covered leased | Retained idle |
| --- | ---: | ---: |
| Sample duration (seconds) | 19.88796 | 19.73849 |
| Median private committed memory (MiB) | 183.2734 | 183.0703 |
| Peak private committed memory (MiB) | 185.0078 | 183.1875 |
| Median summed working set (MiB) | 392.9375 | 387.02734 |
| Sampled aggregate CPU time (seconds) | 0.46875 | 0.265625 |

Private memory was nearly unchanged; this is not evidence of a meaningful RAM
reduction. Summed working sets double-count shared pages. CPU is a sampled lower
bound because processes exiting between samples may be missed; the result is a
single fixture comparison. There is no energy, wakeup, FPS, frame-time,
minimized-window, or ten-work claim. The sampler metadata's generic “full product
process family” wording describes this probe process family, not the full Tauri
application. Raw CSV, summaries, and metadata are under
`target/windows-retained-20261002-055526/{leased_covered,idle_retained}/`.
Both metadata records identify the exact probe binary SHA-256 as
`345F28E53D549E490D457BBD24F7C874F039514515538D65D59AFF46192E7C45`,
based on dirty source at preparation revision `1ac3e853`.

| Remaining measurement | Current result |
| --- | --- |
| Human release to native idle; late acquire/resume | Contract regressions passed; manual Human interaction remains unverified |
| Memory after ten works and 60 seconds idle | Pending user acceptance |
| Heavy canvas panning/zoom at 100% and 150% scaling | Pending user acceptance |

## End-to-end parity checklist

Manual UI testing and acceptance screenshots remain reserved for the user.
Automated Chromium settings fixtures produced dark/light component screenshots
under `target/work-models/`; these use fixture data and do not certify native
UI or real credentials. No manual parity items are inferred from launcher
startup, compilation, unit tests, or the hostile-input guard.

| Requested scenario | Status |
| --- | --- |
| 1. New Work / product research with live frames, helper status, and sources | Pending real-provider run and user UI check |
| 2. AWS/Vercel/Hetzner/Cloudflare comparison with logos and diagram | Pending real-provider run |
| 3. LEGO product cards with photos | Pending real-provider run |
| 4. SF trip using Airbnb and Google Flights | Pending real-provider and signed-in-site run |
| 5. Slack/Linear signed-in reads with zero model calls | Six-app native views replica passed with zero read model calls and one session-choice prompt; real-account acceptance pending |
| 6. Slack post, Linear issue, Notion edit with exact content and one confirmation | All four native write replicas passed exact content and one successful confirmation; real-account acceptance pending |
| 7. Connected MCP server used in a Work run | Native persistence/process contracts tested; real connection/run pending |
| 8. Documents summary through folder ask, picker, and approved write | Native grant/write/move contracts tested; user UI flow pending |
| 9. Stop then Continue without repeated questions | Runtime lifecycle contracts tested; end-to-end acceptance pending |
| 10. Minimized run continues | Native covered leased/idle fixture passed; actual minimized-window run and user acceptance pending |
| 11. Ten-work idle memory and heavy canvas smoothness | Pending measurements and user check |

## Remaining blockers and release boundary

- The user saved the OpenAI credential and its native loader succeeds. The
  protected execution journal passes native admission on this account, all 289
  default Store tests, the actual App artifact subprocess, strict Store/App
  lint, and desktop startup/session checks. The native UDF spelling fix restores
  persistent cookies. Full native shutdown and all six views reads pass at the
  qualified protected path; a verified shorter API spelling also resolves the
  deeper-path native exit failure without relocating data. Native request-context
  filtering and action settlement timing are corrected. All four write replicas
  passed exact content and one successful confirmation. Search's preview debt
  and Enter issues are corrected and its native replica passes. Multi-site
  permission batching now passes its focused native run; human-handoff POST
  authorization passes its four focused regressions. Final integration and
  write results are recorded above. Aliasless deep paths remain unqualified.
- The missing TypeSafe/Jev key setup has been implemented under Settings -> AI,
  alongside the one Recommended/Standard/Off selector. Recommended remains the
  default and uses Jev with OpenAI fallback; saving a key does not overwrite a
  person's explicit mode choice. Keys use the existing native vault target,
  never a preference string or saved-secret readback. Saved presence does not
  claim remote provider acceptance. Full frame checks (641 tests), 11 focused
  Chromium settings tests, and two native preference/cache tests passed. The
  integrated QA executable includes the final runtime fixes. Live Jev acceptance
  is pending.
- The full Windows native replica checks (spa/heavy/apps, Slack/Linear/Notion
  writes, off-site navigation refusal, and unapproved autopost refusal) have
  passing functional evidence. The deliberate Churn recovery case correctly
  retains an Unclean lifecycle verdict even with native-zero proof; no aggregate
  clean-exit claim is made. The retained lifecycle/resource
  fixture passed. Earlier workspace strict Clippy graphs and affected-crate
  regressions passed. Final strict production Engine/App/Desktop all-targets
  Clippy passes with `-D warnings` (`windows-work-final-production-clippy.log`);
  the final diagnostic graph, source boundary, xtask lint, and formatting also
  pass. The larger-run shutdown result is explained by the proven recovery
  contract above, not an unresolved physical-cleanup failure.
  Current diff/format checks and both semantic runtime Node checks pass.
  Provider-backed and manual acceptance
  remain required before calling the full port ready for a PR.
- Windows WebView2 has a reviewed floor and installed runtime of
  **154.0.4258.53**. The review expires October 10 at 00:00 UTC. See
  [the vendor evidence and machine inventory review](windows-webview2-security-review-2026-10-02.md).
  This session does not extend its deadline or substitute a passing unit suite
  for packaged hostile/native release qualification and maintainer review.
- Global `check-engine-floors` still fails for the preexisting expired
  macOS/WebKit review (September 18). `check-advisory-exceptions` still fails for
  the preexisting security-owned dependency exception expired September 30.
  The final `check-release-engine-security` passes the agentic boundary, then
  fails at that macOS review (`windows-work-final-release-security.log`).
  Those are separate release blockers; their review dates were not moved as
  part of this Windows port. macOS native builds were not run on this Windows host.
- The user still owns both-theme visual inspection, Tasks/Settings/start/switcher
  flows, 100%/150% DPI checks, real-account confirmations, and screenshots.
  No full macOS parity or release readiness is claimed by this report.
