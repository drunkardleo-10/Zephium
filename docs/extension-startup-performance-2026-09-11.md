# Extension startup and popup follow-up — September 11, 2026

## Saved-session startup

Previously, Shell waited for extension recovery and hydration before reading
and projecting the saved browser session. Chrome therefore displayed its
initial new-tab state throughout extension startup. The increasing observation
retry delay could also postpone session restoration after the worker was ready.

Shell now projects a validated saved-session preview without creating a Shell
window, native page, persistence state or operation authority. The preview is
read once, only from a valid loaded session with no pending profile deletions.
Full bootstrap still re-reads the authoritative session and waits for extension
readiness before admitting page execution or persistence. Corrupt, absent and
deletion-pending states are not guessed into a preview.

The existing worker accepts one replaceable startup wake callback. A successful
or terminal settlement wakes Shell immediately, outside the state mutex; Shell
then reobserves actual readiness. An unavailable attempt retains normal retry
backoff. This adds no worker, polling loop or background timer.

## Measured work and optimization

A three-second native stack sample located most sampled startup-worker work in
`authenticate_runtime_manifest_bindings_for_profile`: each extension activation
reopened the whole installation cohort. Startup now retains up to 32 authenticated
manifest metadata entries, bounded to 1 MiB plus fixed container storage, during
one serialized hydration attempt. Exact object provenance and fresh high-water
records key the entries. A September 12 follow-up also retains authenticated sibling
package receipts once, within a separate 8 MiB cap, to avoid redoing CRX decompression
and deterministic compilation. Selection still verifies the complete sealed archive,
output and evidence; native admission retains its own checks. These receipts expire
at the end of the hydration attempt. The cache contains no native lease or grants,
and no reuse survives process restart.

The repository's private immediate handoff also no longer rehashes a just-verified
artifact for a second time merely to bind its structural object ID. Reopening
reuses the already authenticated archive over immutable original bytes instead
of repeating CRX verification/ZIP preflight. Constructor verification, native
planning/custody checks and delegated resource validation remain intact.

The same four-extension macOS QA profile produced these observations:

| Build and implementation | Session preview attempt | Extension-gated bootstrap settled |
| --- | ---: | ---: |
| Development build, preview and wakeup, before metadata reuse | 2 ms | 6,165 ms |
| Development build with startup metadata reuse | 2 ms | 4,053 ms |
| Optimized QA, including redundant-pass removal | below 1 ms | 2,610 ms |

A second final optimized-build restart measured below 1 ms for the preview
attempt and 2,682 ms for extension-gated bootstrap, with the saved tabs and all
four extension actions restored in the real browser.

These are elapsed times from the first Shell bootstrap request. They do not
measure process launch, first paint, network page completion, cold disk startup
or production installer performance. The first two rows compare the same build
profile; the last also enables compiler optimization and must not be attributed
solely to the code changes. The final QA build retains debug assertions and its
isolated identity, but is not the shipping release configuration.

Reproduce the optimized QA build with:

```sh
CARGO_PROFILE_DEV_OPT_LEVEL=2 pnpm -C desktop exec tauri build --debug --bundles app --config tauri.external-qa.conf.json --features external-extensions-qa
```

The locally trusted QA bundle must retain its appropriate developer signature
for native-app integration. No keys or credentials belong in this command or
repository. Idle CPU, WebKit/helper memory, cold-start distributions and Windows
performance still require separate measurements; these checks do not establish
minimum resource use or universal instant startup.

September 12: enabled sha2's runtime-checked Apple Silicon acceleration and
removed a duplicate full verification immediately before provenance construction
inside one synchronous native-admission step. With five installed extensions,
the latest optimized QA run reached native-session readiness in 1,788 ms; an
earlier run recorded 3,332 ms. These are indicative local observations, not a
controlled cold-start comparison (profiling and disk pressure also differed).
All five action buttons restored, including Todoist. Pages still wait for
extension registration; deferred first-page activation is not implemented.

## September 12 shared-script memory correction

The main-process footprint included large script-transfer buffers mapped by
WebKit content processes. A root `footprint --unmapped` report attributed two
74 MiB cohorts to those processes; one contained eight approximately 8.4 MiB
regions matching the large Return YouTube Dislike script's UTF-16 representation.
The native grant compiler now removes subsumed HTTP/HTTPS host grants after
validation (for example, `www.youtube.com` under an already granted
`*.youtube.com`). Original script routes, Core grants and separate site denials
remain unchanged. It adds no cache, timer or renderer and uses the existing
bounded grant vector. Tests compare URL access across 32 grant combinations.

With the same five enabled extensions, the observed QA startup peak fell from
699 MiB to 305 MiB. Later observations were 143 MiB at idle and 246 MiB after
switching tabs. These are main-process physical-footprint observations, including
charged shared memory, not the total browser process family or a controlled
benchmark. All five action buttons and page navigation restored. No package
transformation or backend policy contract changed.

## Popup startup ordering follow-up

The user reports intermittent loading popups in earlier builds too, including
1Password and Todoist; reopening can succeed without restarting the browser.
This is not established as a release-only or desktop-pairing failure. Logical
browser-surface publication now performs the same first-ready background wake
as native-view binding and reconciliation. This covers the opposite arrival
order without adding polling or waking backgrounds on ordinary metadata edits.
The complete intermittent-spinner issue remains unqualified; this change is
not evidence that every vendor initialization case is fixed.

Service-worker popup actions now use native action dispatch directly; only
document-background adapters receive the explicit warm-up. In the release QA
app, Vimium 2.4.2's popup rendered its controls and direct Dark Reader-to-Vimium
switching worked. This resolves the separately observed standalone popup refusal,
without changing package bytes, permissions, or the document-adapter path.

## Conditional source update checks

Update requests now carry the version from the authenticated installed catalog.
Google's no-update response ends the check without package download/preparation.
It is reported as "No newer version offered by the store", not as authenticated
policy freshness. Dark Reader 4.9.130 returned HTTP 204 in a real check; installation,
grant, and upstream-history records stayed unchanged. Offered packages still use
the existing CRX authentication, compatibility and update-consent path. Checks
initially remained manual. The follow-up below adds automatic scheduling.

## September 12 automatic source updates

The existing one-minute Shell heartbeat now schedules source checks after a
startup delay, with a six-hour interval, bounded local jitter, and exponential
failure backoff. Only the next-check time and failure count are persisted;
desktop flushes that checkpoint off-actor before network I/O. There is no new
polling thread, server-side inventory, client identifier, or browsing dependency.
Checks discover regular profiles without opening the manager, skip paused/private
profiles, and process at most one package at a time. No-change callbacks do not
rehydrate native runtimes. A successful update refreshes catalog CAS selectors
before remaining packages are considered. Permission reviews take priority.

Native popup/options admission and replacement now exclude each other through a
small process-local guard. Refusal keeps the current installation/runtime; tabs
and background resource capacity remain independent. Pending reviews survive
failed, unsupported and no-change checks. Closing an unused install review
releases its exact candidate so it cannot indefinitely block automatic checks.
CRX download verification runs on the blocking pool with a single admission slot
that stays held if a canceled verifier is still finishing.

Scheduler/service/resource regression checks and release popup switching passed.
A live automatic newer-version replacement and Windows native behavior still need
qualification; these checks are not a claim of complete release readiness.

## Dark Reader popup dismissal

Dark Reader's declared popup and options page use the same document. The former
close handler interpreted equality with the options URL as an options request,
even when that was the popup's original URL. It now infers navigation only when
a known different initial page has changed to the options page. Unknown initial
URLs do not fabricate a request. Explicit native options requests retain their
existing path.

In the actual QA app, clicking the page outside Dark Reader closed the popup and
did not open an Extension Settings window. The close log reported
`options-navigation=false`. Deliberately choosing Settings in the extension
manager still opened the settings window with working controls.

## Validation

The App suite passed 316 tests, the external service suite passed 204, distribution
passed 84, and popup tests passed eight. Regression checks cover read-only preview
without native/mutation authority, normal full bootstrap afterward, one-shot
wakeup replacement and lock release, unavailable-attempt backoff, metadata reuse
only within startup, and rejection of a tampered selected package despite cached
metadata. No backend public-policy contract or extension transform output changed.
