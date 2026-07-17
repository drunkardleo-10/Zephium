# Zephium security model

Zephium renders hostile, attacker-controlled code by design. This document describes
the guarantees implemented in the current tree, not planned product features. If code
and this document disagree, the code is the evidence and the mismatch is a release
blocker.

Zephium is a browser shell over OS WebViews. The application can harden its own
boundaries, data lifecycle, and native API surface, but it cannot turn WKWebView,
WebView2, or WebKitGTK into the same engine or promise controls that an engine does not
expose. In particular, a multi-process WebView is not automatically equivalent to
Chromium's full site-isolation model.

## Trust zones

1. **Rust/native process (trusted).** It owns authoritative application state,
   persistence, native windows, and the WebView lifecycle. A compromise here has the
   current user's OS privileges.
2. **Application WebViews (privileged, semi-trusted).** The `main` chrome and `panel`
   load bundled application assets and can invoke a deliberately small Rust command
   surface. Both are created as incognito/non-persistent WebViews so the application
   origin does not intentionally retain cookies, cache, service workers, or web storage
   between launches. An XSS in either view is still a security event; CSP and ephemeral
   storage are defense in depth, not reasons to trust its input.
3. **Tab WebViews (untrusted).** Raw Wry child WebViews render arbitrary web content.
   They receive no Tauri command bridge, no Wry IPC handler, and no Zephium custom
   protocol handler. The application currently injects only cosmetic page-world CSS;
   injected page-world code has exactly the page's trust level.

The structural boundary between zones 2 and 3 is the most important application-owned
control. A tab is a separate raw WebView, never a navigation of the privileged chrome.
The boundary between zones 1 and 2 is caller-labelled IPC, Rust validation, and Tauri
capabilities.

Profiles are a **web-data privacy boundary**, not a separate OS principal or a defense
against a compromised Rust process, engine sandbox escape, or local account. Native
engine cookie/storage contexts are separated per persistent profile. History and the
favicon cache use per-profile databases, while the profile registry, settings, and
complete restorable session for all non-private profiles share `meta.sqlite`. There is
currently no application-level encryption at rest.

## Enforced invariants

Any change that breaks one of these invariants must fail review and release:

1. A raw tab WebView is built without privileged IPC or a content-reachable custom
   protocol. Page script cannot invoke Zephium commands.
2. `main` and `panel` are constructed at exact `about:blank`, receive all mandatory
   native hardening, and only then navigate to the bundled application origin. Debug
   builds additionally allow the exact local Vite origin. Other navigation is rejected.
   On Windows the same policy is installed on WebView2's separate subframe-navigation
   event, and every request to launch an OS-registered URI scheme is cancelled.
3. Application-requested tab navigation is checked in core and again before the native
   load; page-initiated navigation is checked by Wry's navigation handler. Only `http`,
   `https`, and exact `about:blank` are allowed. URLs are limited to 8 KiB; credentials,
   reserved application hosts, `file:`, `javascript:`, external app schemes, and other
   `about:` pages are rejected. Explicit malformed or forbidden URL-like omnibox input
   (including local path-shaped input) is not converted into a search request, and the
   final percent-encoded search URL must fit the same native limit. Rust records a tab's
   URL only from the native committed-source observer, including same-document History
   API changes; a stale explicit-navigation failure cannot cancel a newer request.
4. Privileged commands authorize the Tauri-injected caller label and validate or clamp
   IDs, URLs, strings, and coordinates in Rust. The panel has no generic Tauri
   capabilities; the main view has only its required window controls. Rust projections
   are delivered directly to a fixed target label, so the panel cannot select the main
   view as a generic event-listener target.
5. Raw content permissions are denied by the pinned Wry permission callback. Raw and
   privileged downloads and popups/new windows are denied rather than silently
   forwarded to the OS or converted into a tab without trustworthy gesture metadata.
   Linux and macOS additionally cancel file-chooser requests from both raw and
   privileged WebViews; macOS also denies privileged media capture and device motion
   natively.
6. Both privileged WebViews are created with incognito mode and their platform-native
   store is checked to be InPrivate/ephemeral/non-persistent. That check and every
   platform hardening hook must succeed before bundled application assets or page script
   load, or startup aborts. Release WebViews have developer tools disabled; macOS also
   explicitly clears `inspectable`. Privileged responses receive a restrictive CSP plus
   `Permissions-Policy`,
   `Referrer-Policy: no-referrer`, and `X-Content-Type-Options: nosniff`.
7. Incognito profiles are excluded from session snapshots, history, and persistent
   favicon records. A private tab may keep its fixed-size favicon raster in process
   memory for the current run; the persistent store rejects an incognito profile again
   at its adapter boundary.
8. Shutdown is an ordered durability barrier: commands already accepted by the shell
   precede the final session snapshot and SQLite flush. A flush failure occurs before
   native shutdown is invoked and leaves the reusable shell component available for a
   host-controlled retry. The desktop close path is still terminal: the same eight-second
   deadline covers storage admission, durability, native teardown, acknowledgment, and
   UI exit, so an unproven flush exits non-zero instead of leaving an unclosable window.
   Once native shutdown is invoked, a negative native result, rejected main-thread
   dispatch, or missing acknowledgment is likewise terminal. On Windows that outer
   deadline contains the separate five-second WebView2 process-group proof plus bounded
   private-UDF removal. An unproven outcome is never reported as clean teardown.
9. Page-derived values remain untrusted and bounded before crossing into application
   state. Titles are stripped of control characters and limited to 512 characters, and
   HTML extraction has character and serialized-result limits. The Wry adapter accepts
   only primitive-string IPC and enforces a shared 64-KiB UTF-16/UTF-8 ceiling before
   constructing a Rust `String`; raw tabs have no IPC handler at all. Privileged local
   protocol requests independently cap method, header count/name/value/aggregate size,
   and stream at most 64 KiB of request body before invoking Tauri. Native engines may
   materialize their own request objects first, but Wry does not duplicate an oversized
   value into Rust. Favicon fetching and image decoding stay in the untrusted page
   renderer. Candidate scanning and polling are bounded, including at most one fresh
   pass after the exact document reaches load completion. Rust accepts only an exact,
   canonical 32-by-32 RGBA raster for the current navigation epoch and origin; chrome
   paints that fixed raster without parsing a page-controlled image container.
10. On Linux, browsing admits only the reviewed stable WebKitGTK 2.52 release line at
    patch 2.52.5 or newer. Older builds, odd-minor development builds, and other
    unreviewed major/minor release lines fail closed. Every Wry context, persistent or
    ephemeral, is constructed with top-level cross-site process swapping enabled. The
    requested Web-process sandbox flag and the construct-only swap policy are read back
    and asserted before the context can own a WebView. Those properties alone are
    configuration invariants, not confinement attestation. CI separately starts a real
    WebProcess on the supported Fedora image and checks its namespaces,
    no-new-privileges, seccomp state, and denial of a host-only path; that evidence is
    scoped to that image.
11. On Windows, privileged native hardening requires `ICoreWebView2Environment10`, and
    every privileged or raw view requires `ICoreWebView2_18` so external URI schemes can
    be cancelled natively. Raw views additionally require `ICoreWebView2Settings7` to
    remove PDF Save, Save As, and Print controls. Before any view is created, startup
    rejects every documented loader/browser-argument/channel and script-debugger
    environment override, including empty values, and the reported stable runtime must
    satisfy the currently reviewed security floor. Startup aborts if privileged hardening
    cannot install; an individual raw view is rejected before its first load if any
    mandatory setting, navigation, external-URI, or process-failure handler cannot install.
12. On macOS, startup occurs before any WebView construction and admits only the reviewed
    Sonoma, Sequoia, or Tahoe security-release lines. The canonical system Safari bundle
    and the framework actually supplying `WKWebView` must have the expected identifiers
    and exactly matching build versions. Malformed, stale, mismatched, and unreviewed
    future OS/Safari release lines fail closed.

## Implemented controls

**Chrome/content wall.** The chrome and panel are Tauri-managed local-asset WebViews;
tabs are independently constructed raw Wry WebViews. No Tauri API is injected into a
tab. The privileged CSP has no remote script source, no object source, no form target,
and no release `unsafe-eval`. This reduces an application XSS's exfiltration options,
but its real blast radius remains the commands and data available to that caller. Both
privileged views request an incognito/non-persistent engine store; authoritative UI
state still lives in Rust and is persisted only through the validated store commands.

**Navigation.** Omnibox input is normalized by the core policy and every native load is
checked again. Explicit forbidden URL-like input is rejected locally rather than leaked
to the configured search engine; search expansion is bounded before admission.
Page-initiated top-level navigation uses the same allowlist. On Windows, raw subframe
navigation also uses that allowlist, privileged subframes remain on the app origin, and
WebView2's external-URI event is cancelled for every view. No blocked scheme is
automatically opened through the OS shell. Popup creation is currently fail-closed.
Back, forward, and stop use native WebView operations instead of page-overridable
JavaScript history/window calls. Native URL/history observers are mandatory on all three
platforms. They deduplicate bounded state and close the view if an engine source escapes
the URL policy, so chrome never presents a pre-navigation URL over a forbidden document.

**Permissions and downloads.** Tabs deny every permission request exposed by Wry.
Windows and Linux install native permission denial for privileged WebViews. On macOS,
Zephium replaces Wry's permissive privileged UIDelegate with a retained deny-only
delegate for media capture, device orientation/motion, and file selection; optional
dialog and popup methods are deliberately omitted so WebKit takes its cancel/no-dialog
defaults. All privileged responses also deny ambient features through
`Permissions-Policy` where the engine supports each directive. There is no permission
prompt or persisted grant store. All downloads are disabled, so Zephium does not
currently claim destination validation, dangerous-file handling, Windows
Mark-of-the-Web, or macOS quarantine. Linux also cancels privileged file-picker
requests. Stable WebView2 exposes no supported file-chooser interception event, so a
raw Windows file input remains an engine-owned, user-selected native upload surface and
privileged Windows views have no equivalent native denial hook. This is an explicit
platform limitation, not a broker Zephium has implemented. Raw Windows views disable
browser accelerator keys and default context menus, and hide the built-in PDF viewer's
Save, Save As, and Print controls; installing those settings is mandatory before the
first load. WebView2 likewise exposes no event that can cancel page-initiated scripted
printing. Zephium locks the page and `Window`-prototype print functions and wraps and
locks both `Document.prototype.execCommand` and the document's own `execCommand` before
any page-owned script. The wrapper coerces a command exactly once, rejects
normalized `print`, and delegates other commands using that same primitive. A trusted
chrome print command calls WebView2's native print API directly. This renderer-layer
guard is defense in depth, not a native denial guarantee; packaged hostile-page testing
(including `window.print()` and `execCommand('print')`) and explicit acceptance or a
future native broker remain stable-release gates.

`HiddenPdfToolbarItems(PRINT)` hides WebView2's built-in PDF toolbar control; it does
not document a denial of embedded PDF actions. A PDF `/Named /Print` or clickable print
action can bypass the DOM guard through the PDF viewer. Until Zephium can disable or
broker the built-in viewer with a supported native API, packaged malicious-PDF testing
and an explicit risk decision are separate Windows stable-release gates. Zephium does
not claim that the current source tree denies this path.

**Profiles and storage.** Persistent profiles use distinct native engine data
partitions: profile paths/contexts on Windows and Linux and named WKWebsiteDataStore
identifiers on macOS. Windows also reuses one WebView2 environment per profile rather
than spawning an unrelated browser process group for every tab. SQLite runs in WAL
mode with `synchronous=FULL`; every opened connection also enables SQLite's defensive
mode and cell-size checking, disables trusted-schema behavior and double-quoted string
literals, and keeps temporary storage in memory. The complete restorable non-private
session is committed as one meta-database transaction. History is capped at 50,000 rows
per profile and the favicon cache at 512 origins per profile.

`user_version` is not treated as sufficient schema identity. Before migration DML and
after every committed migration step, the store compares a bounded inventory of every
`sqlite_schema` object (including indexes, triggers, FTS shadow objects, and their DDL)
with a reference manifest generated from the same immutable migration prefix. Unknown,
replaced, oversized, future-version, and non-boundary schemas fail closed before normal
writes begin. Settings admission likewise reserves each new key atomically before
reporting success, so the 128-key durable limit cannot accept an operation that the
storage actor later drops.

Database paths are derived from one canonical application-data root. Existing files
must be regular single-link files; new files are created exclusively with owner-only
permissions; SQLite opens the final component with `NOFOLLOW`; and the path's platform
file identity is checked again after open. These checks reject symlink and hard-link
aliasing present at admission. They do not create an OS security principal: another
malicious process already running as the same user can still race directory components,
SQLite WAL/SHM sidecars, or later filesystem operations. Eliminating that stronger
same-UID attacker requires descriptor-relative directory handles and durable ownership
identity throughout the store and erasure paths, and remains outside the current
website-threat guarantee.

A valid authoritative session is not held hostage by one damaged ancillary profile
database. Each existing registered profile file is first opened through the protected
read-only path and matched to an exact shipped schema before any read-write setup. A
future, altered, or unreadable schema is left untouched. Any profile that fails that
preflight—or later fails bounded migration/configuration/budget enforcement—is reported
in a sorted bounded degraded cohort and disabled for the rest of the process; it is not
retried, recreated, or used for reads or writes. The exact session and healthy sibling
profiles continue. A forged, duplicate, or foreign degradation report stops bootstrap,
while a profile without an authoritative session still fails closed instead of guessing.
The ordinary two-phase deletion journal may later remove that exact degraded file only
after the user-authorized profile deletion and native erasure proof.

The focused window's profile and space are an authorization scope for runtime tab,
split, popup, and native-layout operations. A same-profile favorite may appear across
that profile's spaces; ordinary tabs and every split leaf must belong to the focused
space. Restored state is checked against the same rule before any native view or engine
partition is created.

Session loading distinguishes a genuinely absent snapshot from corruption or I/O
failure. Failure leaves the shell uninitialized and unable to overwrite the recoverable
state. Authoritative JSON first passes an allocation-free lexical preflight that bounds
individual strings, scalar tokens, nesting, and total structural tokens. Schema-aware
visitors then bound profile, space, item, and split-tree allocation while rejecting
unknown fields. The decoded value must already equal its exact canonical form; Zephium
never uses canonicalization to silently drop damaged rows. A malformed, oversized,
noncanonical, or registry-mismatched snapshot preserves the source row, records a sticky
recovery marker (and the exact corrupt bytes when they fit the outer snapshot bound),
and leaves the store read-only until an explicit recovery flow exists. Profile-file
reconciliation or deletion is authorized only after an exact snapshot and its complete,
canonical, non-duplicate profile registry agree.

Incognito means no intentionally durable Zephium session, history, favicon record, or
privileged-UI website data. Each privileged WKWebView receives a newly created
non-persistent store, and each privileged WebKitGTK view receives a newly created
ephemeral context. Raw private tabs in the same profile share one retained
nonpersistent `WKWebsiteDataStore` on macOS and one explicitly ephemeral WebKitGTK
context on Linux; different private profiles cannot share either object. Every macOS
tab still receives a fresh configuration, and post-build attestation proves its
WebView uses the exact profile-owned store. The vendored Wry patch rejects a durable
supplied context when incognito mode is requested.

WebView2 requires a user-data directory even for an InPrivate controller. Each run uses
a fresh, non-guessable generation below Zephium's dedicated
`web-content/private-runtime` root. Privileged `main` and `panel` controllers use
separate UDFs in a fresh generation below `privileged-runtime`, so the less-capable
panel does not share the main view's cookies, storage profile, or WebView2 session. For
raw content and privileged chrome, cleanup of the current generation follows an exact
Environment5/PID/process-HANDLE proof. The privileged registry retains both environment
guards after its windows are destroyed, requires the exact `main`/`panel` label set and
two distinct browser PIDs, and gives apartment-affine exit callbacks a bounded two-second
message-pump tail after `run_return`. Missing, invalid, aliased, late, or HANDLE-
contradicted proof leaves the generation quarantined and makes a clean exit unsuccessful.

Both runtime roots use the same generation manager. It holds an exclusive root lease,
writes a versioned exact-generation marker, and creates a matching per-generation key
below a volatile HKCU registry key. [Windows discards volatile registry data when the
system shuts down](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regcreatekeyexw),
so a correctly marked generation whose volatile key is absent has crossed a reboot
boundary and may be reclaimed without inferring process death from filesystem access.
Pre-provenance Zephium directories are first bounded, marked, and classified as
same-boot; they cannot be reclaimed until a later reboot. Malformed or unowned objects
are never adopted or deleted. Startup is bounded to 32 same-boot generations, 128 total
including the new allocation, 2 GiB of quarantined logical file bytes, and 250,000
inspected entries. Saturation fails with an explicit full-Windows-restart remediation;
opaque data that remains after reboot requires the runtime directory to be moved aside
manually while Zephium is closed. Successful exact cleanup removes its volatile subkey,
so clean relaunches do not grow HKCU for the rest of the boot.

Separate UDFs cost an additional WebView2 browser session/process set and must remain in
resource benchmarks. Consequently, "incognito never writes a byte to disk" is **not** a
guarantee on Windows. Crash, power loss, swap, filesystem snapshots, and engine defects
remain relevant to private data on every OS. Windows does not provide the Unix directory
fsync durability used by Zephium's deletion journal. The cross-restart completed
tombstone prevents the application from forgetting cleanup authorization after a single
acknowledged unlink, but packaged NTFS power-cut erasure testing remains a release gate
rather than an asserted guarantee.

The engine now exposes a narrow, asynchronous profile-retirement primitive. It
tombstones the profile before teardown, rejects later view/navigation/spare work, drops
every known view and context, and invokes an exactly-once `Verified`, `Failed`, or
`TimedOut` completion. Linux captures and deduplicates strong website-data-manager
handles immediately after native construction and retains them across last-tab close
and failed post-build steps. It clears every retained manager and fetches all record
types afterward before removing the canonical profile directory. Opaque native-build
failure, manager mismatch, or failed clear/fetch creates sticky process-lifetime proof
debt; no later empty retry may authorize disk deletion, and successful handles are
released only for the exact attempt that verified disk absence. macOS clears and
fetch-verifies ephemeral stores and removes a named store before re-enumerating its
identifier. Windows closes the profile's controllers, requests an opportunistic
`ICoreWebView2Profile2` all-data clear, and requires the matching
`Environment5.BrowserProcessExited` event for the exact PID and monotonic environment
generation before deletion. The clear callback is not authoritative because WebView2
may release it without invoking it after controller close. At the Environment5 event,
a retained, non-reusable handle for that exact browser process must already be
signalled; only then are both UDF roots removed and verified. A
caller-visible timeout does not authorize an overlapping retry while native work might
still be running; the process-lifetime tombstone remains. Public attempts are admitted
once per profile before either the watchdog or native host queue is allocated, and both
the retirement/attempt maps and host tombstone/attempt maps are capped at the maximum
64 session profiles without eviction. Main-thread refusal, host-erasure refusal, or the
eight-second public timeout cannot stop an already-running page by changing Rust state,
so each reports its exactly-once outcome, seals event/content authority, and invokes a
mandatory composition-root fatal callback. The desktop callback terminates the process
immediately with a non-zero status; sealing alone is not described as native teardown.

`Verified` covers only the engine-owned native store and canonical engine directories;
it is not forensic RAM/media erasure. The application now coordinates this proof with
the SQLite phase for deletion of an inactive named profile. It first atomically commits
the exact canonical survivor session and a bounded authorization-journal row, then
applies the in-memory tombstone and starts native retirement. A deadline-ambiguous
authorization is reconciled against that journal. If reauthorization is legal, the
coordinator rebuilds the survivor snapshot from current aggregates and records a
process-local session revision, so mutations accepted while the earlier result was
unknown cannot be overwritten by stale deletion state; a newer survivor revision is
scheduled for a post-barrier persist.

After native `Verified`, finalization durably records that proof before scrubbing and
unlinking the exact journal-authorized profile database. Unix orders the unlink with a
directory durability barrier before clearing the row. Windows instead durably marks a
completed local tombstone and retains the authorization until a fresh process start,
after filesystem recovery, re-observes the canonical database plus WAL/SHM sidecars as
absent. Any observed artifact resets only the local phase while preserving native
proof. Startup resumes native retirement or local finalization from the same journal.
Profile deletion emits its correlated disposition only after the current process has
definitively completed both native and SQLite phases; retryable uncertainty remains
pending rather than being reported as success. Arbitrary orphan-shaped files are never
treated as authorized deletion work.
Packaged native tests across all engine data types and crash points remain a release
gate, so this coordinator is not yet evidence of complete real-OS erasure.

**Lifecycle and resource bounds.** Shell commands and engine events share one bounded,
ordered queue. Normal dispatch from native/UI callbacks is non-blocking. Replaceable
renderer-state events may be coalesced, displaced, or rejected under overload; ordinary
coalescing cannot cross a command or shutdown barrier. Normal work is capped at 960
entries. Native URL/failure/crash/profile-exit/split transitions use a reserved band up
to entry 4,095 and may replace an older fact for the same bounded native object or
displace ordinary work at that ceiling rather than leave Rust out of sync with native
state. Entry 4,096 is reserved for shutdown. Admitting that barrier
atomically seals further dispatch, so work behind it is rejected rather than falsely
reported as accepted and later drained. A failed pre-teardown store flush reopens
admission for a safe retry.

Native host work has a second bounded queue for main-thread reentrancy, notably when
WebView2 construction pumps the native event loop. Tasks are typed `Normal`,
`Maintenance`, `Observation`, `Lifecycle`, `Close`, `ProfileErasure`, or `Shutdown` and
keyed where one newer native fact subsumes another. Normal work is capped at 960;
lifecycle work has a bounded reserved band, one additional non-coalescible erasure slot
is reserved per maximum live profile, and entry 4,096 is a non-droppable shutdown slot.
Higher-priority keyed work may replace or displace lower-priority work, but the shutdown
barrier never evicts accepted lifecycle or erasure work. This gives teardown and
authoritative native state priority without permitting hostile callbacks to grow memory
or deadlock the active host borrow.

Every accepted privileged mutation reserves a slot in a separate 1,024-entry,
process-local result ledger before entering the shell FIFO. Pending and processed
entries are not evicted; capacity exhaustion rejects a new command before exposing an
operation id. The desktop records the actor's first disposition before fallible WebView
delivery. `Processed` means the shell actor made the named decision; it is not a claim
that WebKit/WebView2 finished later native work. Deferred dispositions name that
remaining boundary explicitly. Main-chrome-only status, reconciliation, and
acknowledgement commands let a reloaded UI subscribe first, recover missed dispositions,
and explicitly release them; acknowledgement is retried after transient IPC failures.
This ledger is not durable across process death. Only workflows with their own journal,
currently profile deletion, have cross-restart continuation.

Logical content views have three explicit watermarks. Twelve is the warm soft target;
above it, hidden idle pages become discard candidates. Above the pressure watermark of
24, eligible hidden pages may be probed without waiting for the long-idle grace. The
absolute application admission ceiling is 32, including the eight-slot visible
split/recovery allowance: an additional create is rejected synchronously rather than
growing without bound, and an unsafe page is never force-discarded to make room. The
engine independently caps native resources at 48 while counting live views, a warm
spare, in-construction reservations, and WebView2 cleanup debt that may still own a
controller. These are deterministic resource bounds, not evidence that the resulting
RSS, CPU, wakeup, or battery budgets have passed.

Every item-attributed native callback, native error, navigation observation, accelerator,
and asynchronous JavaScript result carries the immutable permit identity of its native
WebView generation plus the outer item token. A warm spare may bind its permit once;
revocation is terminal and the permit cannot be rebound to a later same-id view. Each
native generation also owns a non-wrapping navigation epoch. Adoption and explicit
loads pre-arm a fresh epoch; main-frame load URLs and queued source/history observations
must match both the native generation and the exact current epoch. Thus late
`about:blank` callbacks from a spare cannot be attributed to its adopted item. The host
also compares queued observation/crash work with the currently installed native permit.
For renderer crashes it revokes and physically removes the exact native view before
emitting `Crashed`; the shell therefore never sends a delayed id-only cleanup close that
could destroy the replacement generation.

URLs are policy facts, not navigation identities. The Wry adapter now carries one opaque
native identity across provisional start, every HTTP redirect, commit, finish, and failure:
WebView2 preserves `NavigationId`, WebKitGTK owns a bounded non-wrapping load-sequence id,
and WKWebView binds the exact returned `WKNavigation` (or the exact page-driven commit)
without a mutable global pending URL. At native commit, Wry first revokes a per-physical-view
atomic presentation permit and hides/makes transparent the raw surface before calling Rust.
The host accepts the final allowed redirect destination under that identity, then sends the
URL fact through the shell's reserved critical band before a matching opaque presentation
token. A fresh tab retains its real privileged New Tab UI and native frame during provisional
loading; there is no painted native placeholder. The exact presentation eval atomically applies
the URL/title projection, removes and verifies the active New Tab surface, and returns its
fixed-width monotonic revision. The actor accepts that callback only while it remains the last
emitted revision for the same item, then queues privileged-frame/content geometry before raw
reveal. A later same-tab or full projection invalidates an already-queued success callback;
unrelated-tab churn does not. Native finish idempotently re-drives the same fact if queue
coalescing replaced the commit notification. A two-second absolute bound applies only to
callback/native-dispatch retries and retires an unproven hidden view; page loading never creates
an intentional blank delay or timeout reveal.

Every native reveal revalidates both that exact atomic permit and the latest tab/split layout
revision immediately before and after re-entrant AppKit, COM, or GTK visibility calls. A
superseded pass hides first and retries only the newest retained model. Titles do not have a
portable native navigation id, so a cross-origin URL commit first replaces the old title with
a neutral host label; the finished document title is then queried from the native view only
after exact URL/epoch attribution. This is the browser-chrome anti-spoof boundary for redirects
and overlapping loads; packaged hostile redirect/re-entry tests on all three engines remain a
release gate.

Event delivery and profile/close retirement share a writer-preferring lifecycle barrier.
A waiting retirement writer blocks later events, waits for every earlier caller-sink
delivery to return, installs the tombstone, and only then lets delivery resume. The
caller sink must enqueue lifecycle commands rather than synchronously invoke `close` or
profile erasure. Same-thread lifecycle reentry is detected without waiting on itself,
seals the engine, and invokes the mandatory fatal callback instead of deadlocking.
Ordinary non-lifecycle engine calls may still be made by the sink because no retirement
mutex is held while external code runs.

Main-thread dispatch returns whether scheduling succeeded. Shutdown converts a rejected
dispatch into an immediate negative native result instead of waiting for a callback
that cannot arrive. After the durable store barrier, the composition root waits at most
eight seconds for native teardown. Windows spends at most five seconds of that budget
on its internal whole-process-group proof before bounded private-root cleanup. A
negative result or timeout is logged and permits forced process exit with a non-zero
status; the sealed actor never resumes. On Windows, a failed cleanup leaves that
generation quarantined. A later startup never reuses it: the shared bounded generation
manager reclaims only exact marked prior-boot data, baselines legacy data for one reboot,
and keeps same-boot or opaque state quarantined. Hitting a crash-loop budget produces a
recoverable full-Windows-restart instruction instead of an irreversible assertion.
Store worker queues are also bounded. Page-controlled history growth and extraction
results are capped. These are denial-of-service controls, not proof that arbitrary web
pages cannot exhaust an engine renderer.

**Page-derived native networking and images.** There is no generic native HTTP client or
page-derived fetch port. Such a client could not share the exact profile's proxy, DNS,
cookie, partition, and shutdown policy, so page-derived network access remains inside
the exact profile-scoped native engine session. Favicon network access and codec parsing
run inside the exact sandboxed site renderer. The host polls an asynchronous,
same-origin image load under the current immutable WebView generation and navigation
epoch, then accepts only a canonical base64 encoding of exactly 32x32 RGBA pixels
(4,096 bytes). Rust validates and stores only that fixed raster; privileged chrome
paints it with `ImageData` on a canvas and never invokes an `<img>` decoder or custom
protocol. Persistent caches are capped at 512 decoded origins; private-profile icons
are bounded in memory and never sent to SQLite. A page can choose or forge its own
pixels, but malformed containers, SVG/ICO/PNG parsers, cross-origin late results, and
unbounded payloads do not cross into the privileged renderer. Candidate discovery
considers at most 32 declared icon links, fetches at most four same-origin candidates
plus the same-origin `/favicon.ico` fallback, and uses seven timed polls. If that budget
expires while the document is still loading, the application retains one bounded marker
and makes one final discovery pass at the authoritative load-complete edge before
caching a negative result. Cross-origin/CDN-declared icons are intentionally unsupported
until a profile-scoped network broker can preserve the exact proxy, cookie, DNS, and
shutdown policy; such sites may still show a fallback icon.

Future application-owned downloads, including maintained filter lists and signed update
metadata, are distinct from browsing traffic. They require purpose-built bounded
components with explicit source, redirect, proxy, integrity, retention, and shutdown
policies; they must not restore a generic fetch capability for page-controlled URLs.

**Dependency policy.** Wry is vendored from the immutable upstream revision recorded in
`vendor/wry/UPSTREAM.md`. Relative to that revision, Zephium's source patch covers
constructing WebKitGTK contexts with process swapping, enabling/asserting the sandbox
before WebView creation, allowing an incognito view to use only an explicitly ephemeral
supplied context (including related-view construction), and the documented Windows
staged-construction/IPC/callback hardening. The direct Wry dependency is exact-versioned, and Cargo source
policy rejects unknown registries/git sources and requires revision-pinned git
dependencies. CI treats frontend high-severity audits and Cargo
license/advisory/source policy as gates. Every external GitHub Action is pinned to a
40-character commit and weekly action updates are configured. This reduces supply-chain
risk; it does not make upstream code trusted or mean the graph has no known advisories.
`deny.toml`
explicitly allowlists exact, currently unavoidable GTK3 and Tauri/Specta transitive
advisory IDs with reasons. Unmaintained and unsound advisories otherwise fail, every new
advisory fails until reviewed, and an obsolete ignored ID also fails the policy so the
exception must be removed. The allowlist is accepted debt tied to the GTK4/WebKitGTK 6
and upstream-dependency release gates, not a declaration that those dependencies are
safe.

## Inherited engine properties and non-guarantees

Same-Origin Policy, CORS, CSP enforcement, cookie semantics, certificate validation,
HSTS, mixed-content behavior, renderer sandboxing, and process allocation are supplied
by the installed native engine. Zephium does not reimplement them and must not disable
them. It also does not currently install a certificate-bypass callback.

These inherited properties must not be overstated:

- Zephium does **not** guarantee a renderer process per origin or Chromium-equivalent
  full site isolation. WebView2, WKWebView, and WebKitGTK have different process models,
  and their behavior can change with the installed runtime. Profile data partitioning
  is not proof of renderer/site isolation.
- TLS errors use the engine's default behavior. There is no Zephium certificate
  interstitial, HTTPS-only mode, certificate pinning, or independently tested TLS
  policy yet.
- Cookies, cache, service workers, storage quotas, and clearing semantics are chiefly
  engine-owned. Per-profile partition construction and a verified engine-store
  retirement primitive are implemented. A crash-resumable coordinator now spans
  full-profile native retirement, the authoritative session/registry, and the
  journal-authorized profile database. Cookie inspection/editing and origin-selective
  clearing are not implemented, and the full-profile coordinator still requires the
  packaged native data-type/crash matrix described in the release gates.
- "Zero telemetry" currently means no first-party Zephium telemetry client is wired
  into this tree. It does not mean pages make no requests or that OS WebViews and
  services make no vendor requests. For example, WebView2 content protection remains
  enabled and may follow Microsoft/OS policy. A production privacy claim needs a
  platform-by-platform traffic audit.
- A sandbox limits impact; it does not make engine vulnerabilities impossible. Native
  engine security updates are part of the product's security boundary.
- The dated engine-floor review is also a deliberate build expiry. Passing the minimum
  version is insufficient after the review deadline, even when no new vulnerability has
  been demonstrated; the advisories and policy must be refreshed first. This prevents an
  abandoned build from running indefinitely, but it is not yet a complete production
  update mechanism. Zephium still needs an operational release SLA, an update-required
  user path, and a client-side updater trust root so daily users receive a refreshed
  binary before each deadline.

## Platform-specific posture

### Windows / WebView2

- Raw persistent and private-content controllers use profile-specific WebView2
  contexts; the privileged Tauri user-data folder is not reused by raw content.
- The main and panel are InPrivate and use distinct UDFs inside a fresh per-run
  generation. Native hardening requires Environment7 and Environment10, verifies that
  each environment's
  reported UDF canonically equals its assigned direct (non-reparse-point) directory,
  admits that environment's own browser-version string, disables default script dialogs
  and context menus, verifies the resulting profile is actually InPrivate, disables
  password autosave/general autofill, and installs permission denial. Any failure aborts
  startup.
  Raw content requires Settings4, disables password autosave and general autofill, and
  reads both values back before navigation. It also disables browser accelerator keys
  and default context menus and requires Settings7 to hide PDF Save, Save As, and Print.
  Raw and privileged downloads are denied. Both view classes replace Wry's
  broader default browser arguments with only the `msWebOOUI`/`msPdfOOUI` suppressions,
  so Zephium does not deliberately disable SmartScreen.
- Every view also requires CoreWebView2_18. Raw and privileged subframe navigations are
  checked by native handlers, and OS-registered external URI schemes are always
  cancelled. HTTP Basic Authentication and client-certificate selection are also
  cancelled before WebView2 can fall back to native credential/certificate dialogs. A
  missing interface or failed registration rejects the raw view or aborts
  privileged startup before the window is shown. Raw process-failure event registration
  is also mandatory rather than a best-effort observability hook.
- Raw handlers install before the first content load. Privileged views start at the
  browser-generated exact `about:blank`; Tauri's IPC/document-start plumbing is already
  registered, but no bundled application asset or page script loads until all native
  handlers install and the backend explicitly navigates to the validated app URL.
- Every raw-controller build attempt creates a construction proof obligation before Wry
  can allocate native state. The fork exposes the selected environment before controller
  creation; at that boundary the engine retains an exact browser-process handle and
  installs the Environment5 observer. A Wry construction guard owns the parent-window
  subclass registration, controller, and child container HWND through every fallible
  step. It attempts all outstanding releases on failure and returns a typed, retryable
  cleanup debt if any detach, `Controller::Close`, or `DestroyWindow` contract remains.
  Ordinary drop also retains bounded fallback debt instead of forgetting native
  ownership. The engine associates debt with the exact profile, retries it, counts any
  still-owned controller against the native-resource ceiling, and quarantines/fails
  closed rather than creating around unproven teardown. The engine then verifies
  Environment7 before any content load. Its actual
  reported UDF must canonically equal the engine-owned per-profile directory, every
  path component must be a direct directory rather than a symlink, junction, mount
  point, or other Windows reparse point, and its environment-local browser-version
  string must independently pass the current Stable security floor. Only complete
  attestation clears the construction obligation. A failure before environment creation
  has no native process obligation; a later failure retains exact provenance and can
  retry only the controller. Missing or contradictory environment/process proof remains
  terminal and prevents an empty in-memory map from being mistaken for native absence.
- Before Tauri creates a view, the runtime version must parse as a stable four-component
  WebView2 version and meet the reviewed Microsoft Stable security floor. The current
  floor is `150.0.4078.65`, published July 9, 2026. Preview-channel and malformed strings
  fail closed. The process also rejects documented WebView2 environment overrides that
  can replace runtime/UDF selection, append browser flags such as `--no-sandbox`, select
  another channel, or attach script debuggers. CI, startup, and per-environment
  attestation expire this review after July 23; a clock before the reviewed release or
  after the deadline fails closed. This forces the version, source date, and next
  deadline to be reviewed together. Per-view Environment7/UDF/runtime, Environment10, Settings7, and
  CoreWebView2_18 checks remain independent capability gates.
- Microsoft acknowledged on July 14 that additional Chromium security fixes
  were not yet available in Edge/WebView2 Stable. Startup continues to admit
  the newest actually available Stable runtime above, but the production
  workflow fails `check-release-engine-security` until Microsoft publishes the
  fixed Stable build and the floor is reviewed again. Runtime availability is
  not allowed to turn a known vendor patch gap into release evidence.
- Every retained raw WebView2 environment owns one deduplicated RAII
  `NewBrowserVersionAvailable` registration. The first callback sets a sticky,
  queryable backend `restart_required` state, emits one typed engine event, and is
  replayed to reloaded privileged chrome through the typed `zephium:runtime-status`
  event. It does **not** recycle raw profiles or claim that the newer runtime was
  adopted: Tauri's privileged environments still run the old generation, so the only
  valid transition is the ordinary ordered whole-application shutdown followed by a
  restart. Automatic restart remains deliberately outside this callback path.
- A warm spare is admitted only while its profile still owns a real live
  controller. Closing the profile's final real view synchronously closes a
  matching spare as well, including typed cleanup-debt capture. WebView2 then
  performs normal process-group shutdown; Zephium retains the Environment5
  observer and exact process HANDLE until `BrowserProcessExited` and the
  signalled HANDLE agree on the same PID/generation, at which point the idle
  environment stops counting toward the eight-group ceiling. A new profile is
  rejected during that asynchronous overlap rather than briefly exceeding the
  native process/RAM bound. Packaged Windows tests must still measure exit
  latency and rapid close/open behavior under load.
- Main and panel Tauri environments install the same update event through a bounded
  UI-thread-local registry keyed by their fixed window labels. Each entry retains its
  exact environment, Environment5 exit observer, process HANDLE, and update token. Window
  destruction removes only the update handler; the exit observer remains on the same
  apartment through `run_return` and the bounded callback pump. Cleanup requires exactly
  both labels, distinct PIDs, matching exit events, and signalled exact HANDLEs before
  registrations are released. Each hardening call also verifies its Environment7
  `UserDataFolder` against the distinct assigned `main` or `panel` directory; a runtime
  that aliases those environments/processes therefore fails closed. COM references never
  cross apartments and no token is leaked.
  A sticky pre-shell bit replays a callback that fires during privileged hardening into
  the engine's global dedupe gate once startup reaches the shell. Queue saturation is
  repaired from that queryable gate on the bounded maintenance tick.
- InPrivate still creates runtime files. Raw and privileged current-generation cleanup
  is explicit and uses the same Environment5/PID/HANDLE gate. A failed generation stays
  quarantined and is never reused. Volatile per-boot provenance permits bounded
  prior-boot reclamation; same-boot state cannot be deleted and exceeding its budget
  requires a full Windows restart. Exact markers, root leases, reparse rejection, and
  byte/entry/count budgets apply equally to `private-runtime` and `privileged-runtime`.
- Engine profile retirement releases every controller and environment and arms the
  authoritative Environment5 exit continuation independently of Profile2's
  opportunistic all-data-clear callback. At that exit event the exact browser-process
  handle captured while the process was alive must already be signalled; the engine
  then verifies the persistent and private UDF directories are absent. An
  unproven process identity or process-exit timeout remains fail-closed for the process.
  Every engine-owned UDF root and typed profile directory rejects Windows
  `FILE_ATTRIBUTE_REPARSE_POINT` objects (including junctions/mount points), not only
  symbolic links. Every existing ancestor, root, and profile component must be a direct
  directory. Root/profile identity and that condition are read again immediately before
  every recursive-deletion retry, including engine shutdown cleanup and
  privileged-runtime cleanup after exit.
- **Release gates:** keep the expiring security floor aligned with Microsoft's current
  Stable security release, handle adoption of a newly downloaded runtime for long-lived
  processes, test InPrivate and native navigation-denial behavior for raw and privileged
  views, test privileged main/panel exact exit proof and separate-UDF cleanup after
  crashes/restarts/power loss on packaged Windows, and make an explicit ship/withhold or
  documented-risk decision for WebView2's missing supported file-picker interception
  surface. Environment-local UDF and runtime postconditions cover
  those registry/group-policy overrides, but neither they nor environment-variable
  rejection prove the absence of policy-injected browser arguments such as
  `--no-sandbox`: a native hostile-startup test must inspect the actual spawned
  browser/renderer command lines, tokens, job membership, and process mitigations before
  Zephium claims sandbox attestation. The current stable WebView2 COM
  surface has no supported file-chooser interception/cancellation event; do not replace
  this gate with DOM monkey-patching or experimental DevTools-protocol interception.

### macOS / WKWebView

- Persistent content profiles use named website-data stores; private content profiles
  and both privileged WebViews use non-persistent stores. Bundle metadata requires
  macOS 14.8.7 or newer.
- Before Tauri constructs any WebView, runtime admission requires Sonoma 14.8.7 or
  newer with Safari 26.5.2 or newer, Sequoia 15.7.7 or newer with Safari 26.5.2 or
  newer, or Tahoe 26.5.2 or newer. The canonical Safari bundle build must exactly match
  the loaded `com.apple.WebKit` framework build. Unknown major lines fail closed, and
  the floor review expires after July 27, 2026.
- Overlay configuration keeps Tao's allocated `TaoWindow` class and instance layout
  intact. Zephium does not use `object_setClass` to turn that live object into an
  unrelated `NSPanel`; true non-activating panel behavior remains deferred until an
  allocation-time Tao/Tauri constructor seam exists.
- Privileged chrome layout no longer stores an unretained global native pointer. The
  main thread owns a retained `WKWebView` with a non-reusable generation. Queued layout
  work resolves that retained state only when it executes, rechecks the generation and
  window attachment, and the destroy/drop path unpublishes the generation before
  releasing the retain. Chrome-coordinate state carries the same generation, so an old
  adapter cannot publish geometry for a replacement view.
- Raw content has the pinned Wry native permission denial. For each privileged view,
  Zephium verifies that the native website data store is non-persistent and immediately
  replaces Wry's permissive delegate with a retained native UIDelegate that denies media
  capture, device motion, and file selection. Optional dialog/popup delegate methods are
  omitted to retain WebKit's cancel/no-dialog default. `Permissions-Policy` remains an
  independent script-facing layer.
- Raw content also sets Wry's macOS fullscreen and Picture-in-Picture private
  preferences to false per view. Tauri's `macos-private-api` feature may enable the
  compiled fullscreen path for privileged chrome through Cargo feature unification;
  that compile-time availability is not inherited as authority by raw child views.
- Native delegate/inspector hardening must complete for the main and panel or startup
  aborts. Both views contain only the browser-generated blank document while the
  delegate is attached; application navigation occurs afterwards. Attachment is still
  not atomic with native construction.
- WKWebView does not expose Safari's complete browser UI or Safari-only Safe Browsing
  behavior to Zephium.
- Engine profile retirement clear/fetch-verifies retained non-persistent stores, removes
  the profile's named store only after releasing its WKWebViews, and re-enumerates data
  store identifiers before reporting native-store verification.
- **Release gates:** attach the deny-only UIDelegate during privileged WebView
  construction rather than immediately post-build, define migration/purge behavior for
  any legacy default WK data store, and test permission/file-panel denial, data-store
  isolation, and deletion on supported macOS versions.

### Linux / WebKitGTK

- The loaded library must be on the reviewed stable WebKitGTK 2.52 release line at patch
  2.52.5 or newer. Odd-minor development builds and every other unreviewed major/minor
  release line, including numerically newer lines, are rejected until an explicit
  security review updates the allowlist and deadline. The raw content WebContext sandbox
  flag and top-level cross-site process-swap policy are enabled and read back during
  context construction, before any WebView can launch a Web process. A rejected or
  disabled configuration therefore fails closed, but the public property is not an
  attestation of the confinement actually applied to a spawned process. The newest
  stable release reviewed in this pass is 2.52.5; the enforced 2.52.5 boundary is the
  first release fixed for WSA-2026-0004, and the review expires after August 9, 2026.
- Both privileged WebViews request non-persistent contexts. Their native permission and
  file-chooser denial handlers must install successfully, and the native context is
  checked to be ephemeral, or startup aborts. The vendored Wry adapter separately
  cancels file-chooser requests from raw tabs as well; file upload remains disabled
  until Zephium has an origin-labelled broker.
- Persistent profiles have separate on-disk contexts. Private tabs in one profile share
  a single verified in-memory context, so their cookies/storage form one process-lifetime
  incognito session without sharing across profiles.
- Download denial is installed once on each raw WebContext before its first load;
  opening and closing tabs does not accumulate context-global denial callbacks.
- The current integration uses the GTK3/WebKit2GTK 4.1 bindings. Top-level cross-site
  process swapping reduces process reuse but is not WebKit full site isolation, does not
  promise a process per origin/frame, and remains engine-controlled. Other WebContext
  callbacks are context-global and require explicit single-owner lifetime management.
- The Linux release path emits a Fedora 43 RPM only; Windows has separate NSIS/MSI
  artifacts. Before and after Linux compilation, the workflow proves the DNF-installed
  stable WebKitGTK package is on the reviewed 2.52 release line at patch 2.52.5 or
  newer, declares runtime dependencies on `bubblewrap` and `libseccomp.so.2`, and has
  Fedora vendor/signature metadata. Publication remains
  fail-closed while Fedora's 2.52.5 package is only in updates-testing; native CI may
  consume that signed testing package explicitly, but release artifacts may not.
  It explicitly installs and integrity-verifies those Fedora sandbox packages, embeds no WebKit/GStreamer
  runtime, and rewrites then verifies an install-time dependency of
  `webkit2gtk4.1 >= 2.52.5`. RPM dependency syntax here cannot encode the reviewed-line
  upper boundary, so runtime admission remains the final fail-closed gate. AppImage and
  DEB artifacts are blocked because the supported Ubuntu/Debian build packages are below
  the floor and a Fedora-built AppImage would silently raise its glibc baseline without
  Tauri's supported complete media bundling. This package-chain proof is not runtime
  sandbox attestation.
- CI's ignored hostile native test is invoked explicitly on the supported Fedora image.
  It spawns a real WebProcess, identifies it, inspects `/proc` for no-new-privileges,
  seccomp filtering, and distinct mount/user/PID namespaces, and proves that the
  renderer root cannot read a host-only path. This is evidence for that exact CI image,
  not attestation of an arbitrary installed machine or packaged application.
- **Release gates:** rerun the confinement probe through the packaged application on
  supported hosts and inspect its actual process tree. Also validate private-context and
  context-global-handler lifetime. Native erasure tests must write
  cookies, local storage, IndexedDB, Cache API data, and cacheable responses; destroy
  every view/context; clear and drain GLib callbacks; reopen the exact profile; and prove
  both API-level absence and that delayed processes do not recreate the directory. Keep
  distro/runtime floors synchronized with WebKit advisories and define a supported
  portable package path.
  The GTK4/WebKitGTK 6 migration must retire the explicitly allowlisted GTK3 advisories
  and re-audit the vendored Wry patch.

## Features deliberately not claimed

The following are roadmap items or disabled backends, not current security guarantees:

- a user-facing permission broker or remembered per-origin grants;
- downloads, safe filenames, destination mediation, quarantine/MOTW, or download
  scanning;
- extension installation, extension API mediation, or Chrome/Firefox extension
  compatibility;
- native content blocking/ad blocking (`set_content_rules` is currently a no-op);
- a custom certificate-error interstitial or anti-phishing service;
- an automatic updater client that embeds the update trust root and durably enforces
  the highest accepted signed release sequence;
- application-level encryption of profiles or session data;
- Chromium-equivalent full site isolation on every platform.

Keeping a feature disabled is the required safe posture until its complete broker and
tests exist. Documentation and marketing must not present these items as implemented.

## Production release gates

Before describing Zephium as a production-hardened privacy browser, all of the following
need an explicit implementation, native integration tests, or a documented accepted
risk. The recurring engine-floor, advisory, and fork-review procedure is defined in
`docs/security-maintenance.md`:

1. Close the platform integration gaps above: independently test the blank-bootstrap
   privileged construction path and native deny handlers, attach the macOS privileged
   UIDelegate at construction, decide and document the Windows file-picker and
   page-initiated print limitations,
   establish privileged process-group teardown seams, continuously refresh all three
   engine security floors, provide a release/update SLA before their review expiries,
   and handle WebView2 runtime replacement for long-lived processes.
2. Run packaged hostile-page tests on real supported Windows, macOS, and Fedora systems
   for IPC absence, custom-protocol absence, navigation/popup/download/permission denial,
   privileged file-picker policy, scripted print denial (top-level, initial blank,
   `srcdoc`, dynamic, and cross-origin frames), malicious PDF `/Named /Print` and
   clickable print actions, cross-profile cookies and storage, privileged-view
   non-persistence, renderer crashes, queue overload, process mitigations/confinement,
   and ordered shutdown. Source-level/unit CI is not a substitute for these tests.
3. Exercise the implemented crash-resumable profile-deletion coordinator through native
   engines. Write and then verify removal of cookies, cache, service workers, IndexedDB,
   local storage, WebSQL where present, HTTP authentication state, and engine caches for
   persistent and incognito profiles. Inject crashes and ambiguous storage outcomes at
   every authorization, native-proof, filesystem, and journal-finalization boundary.
4. Keep downloads, permission grants, extensions, and content blocking disabled until
   their brokers are complete and adversarially tested.
5. Rehearse the production workflow with protected publisher credentials and verify
   its signed/notarized artifacts, SBOM attestations, encrypted symbols, and signed
   rollback-resistant metadata on real installer hosts. Before auto-update is offered,
   implement the client-side trust root and durable sequence check. Track every
   explicit Cargo advisory exception to removal; adding one is a reviewed risk
   acceptance, not a routine way to make CI green.
6. Pass recorded 1/10/50/100-tab and 24-hour endurance budgets for RSS, process/handle
   growth, crash loops, database growth, CPU wakeups, battery use, startup, and shutdown
   latency under adversarial workloads. Queue and view-count bounds alone are not a
   performance or denial-of-service proof.
7. Audit network traffic and native-engine policy on each OS before making stronger
   privacy or telemetry claims.
8. Obtain an independent audit of the native/Wry/OS boundary and remediate its release
   blockers before calling the browser stable or trusted for a large daily audience.

## Threat model

In scope are hostile pages attempting to reach native APIs, privileged-view XSS,
cross-profile data leakage, malicious navigation and protocol handling, permission and
download abuse, page-controlled resource exhaustion, persistence of private state, and
unsafe teardown/recovery.

Not defeated by this architecture are a native engine sandbox escape, compromise of
the Rust process or shipped bundle, an already-compromised OS/user account, physical
access, swap/hibernation/backup forensics, traffic analysis, fingerprinting, or an
upstream supply-chain compromise. Runtime floors, dependency policy, signing, and rapid
updates mitigate some of these risks but do not remove them.

## Per-PR security checklist

- [ ] No raw tab gains Tauri IPC, a Wry IPC handler, a privileged initialization
      object, or a content-reachable custom protocol.
- [ ] Every new privileged command has an explicit caller policy, Rust-side input
      validation/bounds, and the smallest Tauri capability set.
- [ ] Every new navigation path uses the core and native scheme policy; no blocked URL
      is forwarded to the OS without a separate, user-gesture-aware broker.
- [ ] Page-derived strings and bytes stay tainted, bounded, and out of privileged HTML,
      command strings, filesystem paths, and in-process decoders.
- [ ] Profile context changes include cross-profile cookie/storage tests and document
      exactly which data remains shared in `meta.sqlite`.
- [ ] Incognito changes prove that session/history/favicon persistence is rejected and
      that privileged UI storage is non-persistent; tests account for engine-created
      temporary files and abnormal termination.
- [ ] Permission, download, popup, extension, content-blocking, and favicon code remains
      fail-closed unless its complete policy and native tests land together.
- [ ] Release devtools remain disabled; privileged navigation lock, CSP, response
      headers, incognito construction, retained fail-closed native delegates/handlers,
      mandatory hardening installation, and label-scoped projection delivery remain
      intact.
- [ ] Queue producers remain non-blocking, memory remains bounded, overload behavior is
      tested, and accepted work cannot reorder across lifecycle barriers.
- [ ] Mutating IPC reserves the process-local result ledger before FIFO admission;
      pending/processed dispositions remain queryable until main chrome acknowledges them,
      and no process-local operation result is described as cross-restart durable.
- [ ] View lifecycle changes preserve the 12/24/32 application watermarks, the separate
      48-resource native ceiling, visible-leaf protection, exact discard probes, and the
      rule that unsafe pages are not force-discarded to admit new work.
- [ ] Reentrant native work keeps typed normal/maintenance/observation/lifecycle/close/
      shutdown priority, keyed displacement rules, and a reserved non-droppable shutdown
      barrier.
- [ ] Linux WebKitGTK, macOS/Safari/WebKit, and Windows WebView2 capability/version floors
      remain aligned with supported packages and current security advisories; their
      review deadlines fail CI when stale.
- [ ] Profile-erasure changes preserve process-lifetime tombstones, exactly-once public
      completion, native in-flight state after a caller timeout, symlink-safe typed
      paths, and platform-specific post-clear verification. Do not call engine-only
      verification a complete profile deletion.
- [ ] Shutdown changes preserve command ordering, the durable snapshot/flush barrier,
      result-bearing main-thread dispatch, the eight-second outer application deadline
      around Windows' five-second process-group proof and bounded UDF removal, terminal
      post-teardown behavior, and fresh per-run private generations. Unproven older
      generations are never reused; only exact marked prior-boot data is reclaimed.
      Same-boot budget exhaustion remains recoverable by restarting Windows, and unknown
      objects require explicit manual remediation rather than inferred ownership.
- [ ] Dependency/source-policy and full frontend/build lockfile audits remain blocking CI
      gates; exact advisory exceptions stay justified, reviewed, and tied to removal work.
- [ ] This document is updated when a guarantee or accepted platform limitation
      changes.
