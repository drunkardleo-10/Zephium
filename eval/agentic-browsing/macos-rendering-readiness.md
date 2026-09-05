# Hidden macOS rendering readiness diagnostic

Status: release-excluded, provider-free measurement; no production policy change.

## Why this diagnostic exists

The separately recorded 2026-09-05 `commerce-product` attempt navigated exactly,
then correctly refused a complete bounded 17-node successor snapshot with none
of its three required facts. The retained output cannot identify those nodes
or establish a public-site root cause. See the [failed witness](m6-production-qualification.md).

Source inspection establishes two separate candidate seams:

- The native navigation gate settles an exact commitment, not load finish.
  The semantic channel becomes dispatchable at commit, and the immutable
  runtime accepts `interactive` or `complete` document readiness. Snapshot
  `Complete` describes the bounded capture, not future page stability.
- Read-only inspection of the [public product response](https://demo.vercel.store/product/acme-geometric-circles-t-shirt)
  found the product description inside a hidden streamed container. Its reveal
  script can queue that content behind `requestAnimationFrame`. The reviewed
  [pinned product template](https://github.com/vercel/commerce/blob/3761e52e60df9c6a316e067dbfd7032e494d3634/app/product/%5Bhandle%5D/page.tsx)
  wraps the description in a Suspense boundary with an empty fallback. This
  does not prove the deployed revision or the failed native page's state.

The production owned WKWebView remains hidden and attests the existing
`WKInactiveSchedulingPolicy::Throttle`. WebKit describes inactive-page RAF
stopping independently from timer throttling in its
[power guidance](https://webkit.org/blog/8970/how-web-content-can-affect-power-usage/).
[Apple's public scheduling policy](https://developer.apple.com/documentation/webkit/wkpreferences/inactiveschedulingpolicy-swift.enum)
distinguishes throttling from suspension; it does not itself promise that RAF
runs in a hidden view. Thus a load-complete check or an arbitrary delay is not
yet a justified fix. Neither projection changes nor relaxed commerce predicates
are justified by these observations.

## Closed experiment

Build the existing release-excluded semantic probe, then use its exact new mode:

```sh
cargo build --locked -p zephium-engine --features native-agentic-semantic-probe --bin macos-agentic-semantic-probe
target/debug/macos-agentic-semantic-probe --ci-hidden-rendering-readiness
```

The fixed loopback fixture reveals three separate initially hidden paragraphs
through a Promise microtask, one zero-delay timer, and one animation-frame
callback. Separate fixed markers report document-ready changes and the load
event. No external resource, keychain, provider, session, user input, page
permission or public-site dependency exists. The unchanged fixture server is
loopback-bound, budgeted, joined, and serves only fixed routes with a restrictive
CSP; the new page has no subresource or connection path.

The new mode reuses the exact production owned-view constructor, standard
1280×800 logical viewport, ephemeral profile, hidden/non-key/non-active focus
guard, semantic isolated-world channel and native policy/runtime attestation.
It admits only Observe and Navigate, not Act. The original exact navigation
operation supplies native-finish facts before and after each capture; no page
marker replaces that authority. The existing no-redirect navigation behavior
and semantic runtime remain unchanged.

At most eight semantic invocations sample offsets 0, 50, 100, 200, 400, 800,
1600 and 3200 ms from the committed-navigation return, under one five-second
measurement deadline. Each slot performs at most one exact invocation, with
fresh invocation/snapshot identities and unchanged initial capture limits.
This is a diagnostic sampling window, not a new production readiness retry,
fixed wait, or success predicate. The native run loop continues pumping its
existing bounded slices. A typed DocumentLoading receipt remains distinct from
an empty successful snapshot; other faults stop the diagnostic.

Successful samples require exact document/origin/frame joins, complete bounded
snapshots, no child boundary and unique public fixture markers in the expected
roles. Missing control markers do not prove RAF starvation. Negative RAF
classification requires converged native-finish/document-complete/load/
microtask/timer controls and completion of the whole 3200 ms window. Positive
classification can stop early once all controls and the RAF reveal are seen.
Missing RAF means only **not observed within this bounded window**, not never.

Content-free sample records and their disposition are returned only after the
same original policy/runtime registrations, page, window, ephemeral store and
fixture worker drain. A teardown fault returns failure, not a successful report.
No production scheduling preference, visibility, semantic program, provider
contract, Work predicate or default Browse path changes in this diagnostic.

## Proof boundary

Deterministic tests cover incomplete/contradictory controls, shortened negative
windows, wrong contexts/origins, truncated snapshots, frame boundaries,
duplicate markers and wrong-role/sensitive substitutes. Architecture mutations
guard sampling/deadline bounds, exact native-finish joins, teardown, the closed
CLI, independent fixture controls and absence of scheduling/script/provider
authority. Actual native results are recorded separately after a pinned run.

Even a confirmed hidden-view RAF stall on this fixture is not proof that a
particular scheduling change fixes the public commerce site. Any production
policy change requires separate design, lifecycle/resource review and native
qualification; this diagnostic never changes policy to force a result.

## 2026-09-05 — pinned provider-free native witness

One invocation of `--ci-hidden-rendering-readiness` used clean source HEAD
`cc74d293ee3a0f151454a0bad21b7f3f8ebbf484` and exited 0. The tree was clean
before and after the invocation. A host clock reading immediately before
launch was `2026-09-05 19:34:51 UTC`; the exact native process start was not
emitted and is not inferred from that reading.

All eight successful semantic captures contained six nodes, with the fixed
document marker `Complete` and load, microtask and timer reveal controls true.
The exact original navigation's native-finish fact changed from false before
to true after the first capture. It was true before and after every later
capture. The animation-frame reveal marker was absent in every capture.

| Elapsed from committed-navigation return (ms) | Native finished before / after | Nodes | Document / load / microtask / timer | RAF reveal |
| --- | --- | --- | --- | --- |
| 44 | false / true | 6 | Complete / true / true / true | false |
| 64 | true / true | 6 | Complete / true / true / true | false |
| 108 | true / true | 6 | Complete / true / true / true | false |
| 208 | true / true | 6 | Complete / true / true / true | false |
| 408 | true / true | 6 | Complete / true / true / true | false |
| 808 | true / true | 6 | Complete / true / true / true | false |
| 1608 | true / true | 6 | Complete / true / true / true | false |
| 3280 | true / true | 6 | Complete / true / true / true | false |

The returned disposition was `AnimationFrameNotObservedWithinWindow`.
Closure reported `fixture=loopback-only`, `provider=absent`,
`profile=ephemeral`, `presentation=hidden`, `scheduling=throttle` and
`original_native_teardown=verified`. No provider/keychain/public-site request,
native action, runtime-policy change or content-bearing output was involved.

This demonstrates a bounded hidden-view animation-frame liveness gap on the
fixed fixture even after independently observed native finish, document
complete and load. It rules out treating load completion alone as sufficient
for this fixture's semantic reveal. It does not prove permanent RAF starvation,
identify the 17 nodes from the failed commerce run, establish that site's
root cause, or show that a scheduling-policy change would fix it.

The existing `--ci-hidden-fixed-dom` native regression then also exited 0 on
the same source pin: ten snapshots, four world epochs, verified fixed click
and expanded postcondition, exact-value page-world compatibility fill across
text input/search input/textarea, stale-anchor refusal and mutation recovery,
epoch-rotation fill, redacted secrets, zero user activation, admitted popups,
focus theft and retained views. Its synthetic event trust remains `untrusted`;
this is not an OS-native typing or trusted-activation claim.

### Gates and known unrelated failure

Passed on the diagnostic source pin:

- all 469 engine library tests with `native-agentic-semantic-probe`;
- all eight `probe-harness` fixture-server tests, including the new closed
  rendering fixture;
- strict all-target engine Clippy with the native probe feature, the debug
  native semantic-probe build and the non-probe `agentic-browser` engine check;
- both actual architecture gates (`check-agentic-probe-boundary`, including
  hostile immutable-runtime JavaScript smoke, and
  `check-agent-controller-boundary`);
- the new rendering diagnostic architecture-mutation test; and
- formatting and diff-whitespace checks, plus both native invocations above.

An additional complete `xtask` unit run had **183 passed, one failed**:
`agentic_probe_boundary::tests::supervisor_progress_requires_exact_manifest_revision_provenance`.
Its synthetic positive fixture repeats the exact revision join seven times,
while its validator requires nine. Both the seven-join fixture and nine-join
validator are already present in `cc74d29^` (`cdf7742`); the diagnostic does
not edit either. The actual production architecture gate passes. This
unrelated unit-fixture mismatch is reported, not repaired or represented as
a green full gate in this diagnostic checkpoint.

The next decision is a separately reviewed runtime-policy investigation of
hidden-view rendering liveness with resource, privacy and lifecycle bounds.
No production change or further public/provider attempt is authorized by
this result alone.

## Closed public-native opportunity comparison

The follow-up remains release-excluded and provider-free. It evaluates two
supported native APIs on the same exact production-owned hidden view and
semantic adapter, without changing the production constructor or runtime:

```sh
target/debug/macos-agentic-semantic-probe --ci-hidden-rendering-unthrottled
target/debug/macos-agentic-semantic-probe --ci-hidden-rendering-snapshot
```

Each command creates its own fresh ephemeral/loopback lifetime and commits the
same fixed fixture. `unthrottled` checks the original public scheduling policy,
temporarily sets `WKInactiveSchedulingPolicy::None`, checks the applied value,
then runs the original at-most-eight/five-second measurement. Explicit checked
restoration to `Throttle` occurs before any report, including measurement
refusal; a scoped destructor also attempts restoration on early refusal or
unwind. The original complete owned-view attestation and teardown must still
pass. There is no shared policy guard or new production request capability.

`snapshot` leaves `Throttle` unchanged and requests exactly one fixed 1×1
logical-pixel native snapshot with `afterScreenUpdates=true` and output width
one. It allows at most one second for the exact callback, refuses a missing,
failed or duplicate terminal, and retains that terminal through measurement.
It never dereferences, encodes, retains or exports the returned image. This is
an excluded rendering experiment, not admission through the product screenshot
policy or proof of screenshot provenance. It then runs the unchanged bounded
semantic measurement. In both comparison reports, sample offsets start after
opportunity acquisition; acquisition time is separately reported. No capture
or model ceiling increases, navigation replay or page-content criterion changes.

The public API comparison is deliberately separate from a production fix.
[WebKit's preference implementation](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKPreferences.mm)
maps scheduling policy to process assertions/throttling, while its
[page visibility implementation](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/Page.cpp)
separately suspends and resumes scripted animations. This source supports
testing rather than assuming that normal process scheduling supplies RAF.
Neither API requests visibility, foreground focus or input activation. The
existing native guard remains unchanged and is sampled throughout each run.
No private SPI, arbitrary script, framework scheduler replacement, public
network target, provider or keychain access is introduced.

Deterministic tests guard missing/failed/duplicate snapshot callbacks and
sticky refusal; source mutations guard restoration and attestation, the exact
single dispatch and fixed size/deadline, original navigation/teardown joins,
closed CLI and absence of presentation/script/provider authority. Native
outcomes must be recorded against a pinned commit before selecting any
production-policy correction.

### 2026-09-05 — pinned public-native comparison outcomes

Each approved command ran once on clean HEAD
`4168507c4dffc0fd27476008f51f2c71e21479cc`, in a separate fresh native lifetime,
and exited 0. The tree remained clean after both invocations. Pre-invocation
host clock readings were `20:36:02 UTC` for `unthrottled` and `20:36:24 UTC`
for `snapshot`; neither is claimed to be the exact process start.

Both reports returned `AnimationFrameNotObservedWithinWindow`. Every sample
had six nodes, a `Complete` document marker, and true load/microtask/timer
controls; the RAF reveal marker was false throughout. The exact original
native-finish fact crossed false→true during the first unthrottled capture,
then remained true before/after all later captures. It was already true
before and after every snapshot-comparison capture.

| Fixed opportunity | Acquisition (ms) | Actual sample offsets after acquisition (ms) | RAF reveal |
| --- | --- | --- | --- |
| Public scheduling `None` | 0 | 5, 56, 106, 206, 406, 806, 1606, 3278 | absent in all eight |
| One native 1×1 logical-pixel snapshot | 49 | 5, 55, 106, 207, 407, 807, 1607, 3223 | absent in all eight |

The zero-millisecond scheduling acquisition is rounded elapsed time, not a
zero-cost claim. The snapshot's exact successful callback preceded measurement;
no pixels were exported. Both reports retained `fixture=loopback-only`,
`provider=absent`, `profile=ephemeral`, `presentation=hidden`,
`original_scheduling=throttle`, `scheduling_restored=verified` and
`original_native_teardown=verified`. No focus/input or original isolation
attestation was relaxed to obtain either outcome.

Preparation gates passed: all 470 native-feature engine library tests, both
rendering architecture-mutation tests, strict all-target native-feature engine
Clippy, debug native-probe build, non-probe `agentic-browser` engine check,
both actual architecture gates including hostile immutable-runtime JavaScript
smoke, formatting and diff-whitespace checks. The previously disclosed full
`xtask` seven-versus-nine fixture failure was not changed or rerun; no new green
full-`xtask` claim is made.

The unchanged `--ci-hidden-fixed-dom` path also passed once with the same
built executable: ten snapshots, four world epochs, verified click/fill,
stale-anchor refusal and mutation/epoch recovery, redacted secrets, zero user
activation, admitted popups, focus theft and retained views. Event trust
remains `untrusted`. Only this evidence document was being edited during that
regression; no source or fixture changed after the pinned build.

These results reject the two tested public non-presentation interventions as
sufficient fixes for this fixture within their bounded windows. They do not
prove every public WebKit path impossible, permanent starvation, the failed
public commerce page's exact root cause, or a general battery/CPU budget.
The production constructor, runtime, scheduling policy, task facts, model
contract, authority and Browse path remain unchanged. There was no commercial
or other public/provider retry.

The measured boundary now requires separate review: either a genuinely
presented, non-key, input-isolated Work rendering surface with explicit native
visibility/resource ownership, or a deliberately approved compatibility
mechanism that changes page scheduling semantics. Neither is equivalent to
the current permanently hidden contract. Transparent/offscreen presentation
tricks, private SPI, and a page-world scheduler replacement are **not** adopted
by this diagnostic. A load-complete wait, higher ceiling or looser task
predicate does not resolve the measured gap.

This platform decision is separate from open-objective agent planning.
The task-authored route qualifiers prove the kernel's exact transitions; they
do not prove general objective interpretation or model-selected plans. A
rendering solution should make bounded real-page evidence available to that
future decision loop, not replace it with more site-specific task predicates.

## Honest presented-surface viability contract

The next closed experiment separates actual rendering presentation from focus
and input instead of treating a permanently hidden view as a rendered page:

```sh
target/debug/macos-agentic-semantic-probe --ci-presented-rendering-readiness
```

This command intentionally presents the fixed public-domain synthetic fixture
on screen for at most a five-second deadline. It is **not hidden presentation**
and may briefly cover other content. No real website, provider, keychain,
personal page, user data or OS-wide input is involved. It creates no additional
view/window beyond the original exact owned-page probe cohort.

After the original hidden construction and exact fixture navigation, the
closed scope uses a borderless window which cannot become key or main,
sets mouse-event exclusion before presenting, keeps the application inactive,
and confines the internal first responder to its original identity or the
exact owned WKWebView (see the measured refinement below). Public
[`orderFrontRegardless`](https://developer.apple.com/documentation/appkit/nswindow/orderfrontregardless%28%29)
is documented to preserve key/main windows while presenting an inactive app's
surface. There is no app activation, focus request, injected input, private
SPI, transparency/offscreen trick, scheduling override or JavaScript shim.
`Throttle` remains the native policy.

The full unchanged 1280×800 logical viewport must fit within the selected
screen's visible frame; a smaller screen refuses rather than scaling or
clipping the task viewport. The window is genuinely opaque and on-screen,
and the exact child view/parent/window geometry is attested. AppKit's public
occlusion `Visible` bit must show that the window has visible screen pixels;
this is **not** proof that every pixel is uncovered. The child visible
rectangle separately proves absence of parent clipping. Loss of these facts
after initial convergence refuses; acquisition and the unchanged at-most-eight
semantic sampling schedule share the outer five-second presented deadline.
The native guard is checked around every existing run-loop slice.

Presented and hidden native guards are separate closed variants. None of the
existing hidden modes may ignore visibility violations. Both guards preserve
sticky failure and exact input/focus isolation. The scope hides the page and
window before restoring geometry and input policy on every normal/refusal
path, with a destructor backstop. Successful reporting additionally requires
checked restoration, original hidden-view/profile/runtime attestation and
exact native/store/listener teardown. Missing render opportunity, an expired
deadline or a focus/isolation change is not a success or permission to retry.

This is a provider-free native viability experiment, not a production lease,
new model capability, Work UI or proof of open-objective behavior. If it proves
RAF progress, production admission still needs explicit rendered-resource
ownership, deadline/cancellation/revocation, observation freshness and truthful
native accounting, distinct from visibility, human takeover and suspension.

### 2026-09-05 — first presented attempt (refused before measurement)

One invocation used clean source HEAD
`5310f99a6c5b6021f66b7523a2c9420b500392a4`. The pre-invocation host clock read
`20:51:42 UTC`; the exact process start was not emitted. The command exited 1
with `stage=presented_responder_changed`, before any semantic measurement or
RAF outcome. This is not a negative RAF result.

The guard checks deadline, application inactivity, absence of key/main-window
status and authority, and mouse exclusion before the responder comparison.
Those checks did not refuse that native sample. The exact internal responder
relationship was not captured, so the output does not establish whether it
belonged to the owned page. The original hide/restore and native teardown path
completed without replacing the reported stage. No provider or public-site
request occurred and no success is claimed. Diagnosis must distinguish an
internal non-key-window responder transition from user-input/focus authority,
without removing the independent native isolation checks.

The follow-up diagnostic at `51adfbd` refines only this refusal into absent, exact-window,
exact-page, exact-owned-page-descendant or foreign responder. It classifies
the same retained native responder used for the comparison, exports no class
name, pointer or page data, and still refuses every changed responder. It
does not infer that a descendant responder is safe or permit it to continue.

The single refined attempt used clean
`51adfbd9b86807f364a2e19bd992870971374162`, with a pre-invocation host clock
reading of `20:55:20 UTC` on 2026-09-05 (not an exact process start). It exited 1
with `stage=presented_responder_changed_exact_page`. AppKit's changed responder
was therefore the exact owned WKWebView itself, not a foreign object or an
arbitrary descendant. All earlier isolation checks still held at that sample;
the scope restored and the original teardown did not replace the refusal.
No semantic measurement or RAF outcome exists for this attempt. This evidence
supports evaluating a narrowly identity-bound internal-responder contract,
not permitting general focus changes or declaring the page input-safe from
its responder relationship alone.

The identity-bound correction admits only the unchanged original responder
or the exact retained WKWebView during the explicit presentation scope. An
absent replacement, arbitrary descendant, foreign native responder or invalid
page identity remains refused. All independent app-inactive, non-key/non-main,
mouse exclusion, visible geometry and deadline facts must still hold; native
responder ownership by itself grants no input authority. A report records
whether the exact page responder was observed. Hiding must restore the original
strict hidden guard, including its original responder, before reporting.
No responder is assigned by the probe, no keyboard event is synthesized, and
no `makeFirstResponder`, focus, key-window or app-activation API is introduced.

One corrected attempt used clean `2e3e75ce82502f477ecb932c7a8fe002d1a81366`
(pre-invocation host clock `2026-09-05 20:58:46 UTC`, not exact process start).
It exited 1 with `stage=presented_surface_geometry`, after the independent
app/key/main/mouse/responder/visibility/opacity guards, but before semantic
measurement. It therefore supplies no RAF result. The compound geometry
refusal does not yet identify window-frame, exact child-size or parent-window
identity mismatch; diagnosis must distinguish those rather than relax them.
The original hide/restore and teardown did not replace that refusal.
