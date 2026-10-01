# macOS owned native-input capability qualification

Measured 2026-09-09 on macOS 27.0 (26A5425a). This release-excluded
qualification uses the real `AgentOwnedView` constructor, ephemeral store,
navigation owner, isolated semantic runtime, and existing on-screen inactive,
non-key/non-main, mouse-ignoring presentation scope. It does not admit trusted
text in production or claim a general native-input capability.

## Decision

**No-go: display capture lacks a qualified owned denial, with one observed
native application activation and a pending request on fresh-process repeat.** Trusted text
without observed user activation is useful compatibility evidence, but it does
not establish that the native owner denies all browser/OS capabilities. The
current Wry permission handler covers media capture and motion; unrelated
capability paths cannot be inferred from `.with_permission_handler(Deny)`.

Follow-up [display-command qualification](macos-display-command-qualification.md)
distinguishes native/public-evaluation dispatch from the document-start isolated
command route. The real owned constructor, visible document and fixed
document-start `execCommand` trigger rejected display with `InvalidStateError`
without native activation. This is bounded mechanism evidence; it does not
change the activation-independent display-containment no-go above.

The focused executable is `agentic_owned_surface_probe.rs`, a child of the
already excluded semantic probe. One closed environment enum selects an embedded
local fixture. A fresh post-presentation semantic button click prepares the
fixture editor, then a single public `insertText:replacementRange:` call targets
the re-attested owned page/current selection. This is fixed fixture authority,
not a semantic Fill permit. Native exceptions and uncertain outcomes are
nonretryable; the context remains conservatively tainted until teardown.

## Measured rows

Initial 1.8-second native sampling produced:

| Row | Page result after one trusted input | Native result / interpretation |
| --- | --- | --- |
| `ui` | Popup denied; alert completed; confirm/prompt denied; file `click-returned`; close returned | No new app window, attached sheet, or modal; URL retained. File delegate reachability was not established. |
| `media` | Capture and display requests remained pending; fullscreen API unavailable | Explicit nonretryable failure, not denial or success. No app window/sheet/modal observed. |
| `passkey` | Fixed localhost assertion rejected `NotAllowedError`; bounded abort sent | No app window/sheet/modal. Rejection is not attributed to the owned permission handler. |
| `geolocation` | Denied | No app window/sheet/modal. Platform denial is not evidence of an owned geolocation callback. |

All four initial rows observed one trusted input, no transient/sticky activation, secure
localhost, exact intended editor value and native URL. Successful rows verified
hidden restoration and original native teardown. The media failure returned
through the same restoration/teardown path; it is not represented as qualification
success. The final diagnostic also separates `capture` and `display`, includes a
fixed one-second page-timer heartbeat, and samples for 3.5 seconds so a live page
with an unresolved media request is distinguishable from a paused renderer.

The final 3.5-second isolated matrix produced:

| Row | Final result | Native samples |
| --- | --- | --- |
| `ui` | Same denial results; file remains `click-returned`; timer fired | 571; no window/sheet/modal/focus change |
| `capture` | `NotAllowedError`; fullscreen unavailable; timer fired | 552; no window/sheet/modal/focus change |
| `display` | Native guard failed `application_became_active`; final page witness not captured | 13; no new in-process window/sheet/modal |
| `passkey` | `NotAllowedError`; abort sent; timer fired | 569; no window/sheet/modal/focus change |
| `geolocation` | Denied; timer fired | 563; no window/sheet/modal/focus change |
| `display`, new-process repeat | Pending, timer fired, trusted input, no transient/sticky activation | 566; no window/sheet/modal/focus change; nonretryable failure |

The native activation occurred during the isolated display request, but did not
repeat in the next fresh process. This is an observed lifecycle violation, not
a claim of a deterministic exploit or completed screen capture. Its failed
guard deliberately prevented a final semantic observation, so that particular
row has no measured page-activation witness. The other five final observations
report no page activation. Neither display run granted success or retried an
operation in its old context. Original owner teardown remains required before
the execution error can be returned; the focus-violation row does not claim
successful focus restoration.

## Concrete owner gaps and evidence limits

The checked-in UIDelegate explicitly cancels file selection, confirm/prompt and
popup construction without a handler. Alert completion does not construct UI.
The existing owned constructor disables fullscreen, PiP and autofill, ignores
page close, rejects downloads without metadata, and routes navigation through
its exact gate. These are source-backed policies; a popup/file call denied
before its delegate due to missing activation does not exercise that policy.
Existing trusted-edit navigation evidence remains in
`macos-trusted-editing-unit.md`; this probe does not repeat its navigation row.

The vendored `PermissionKind::DisplayCapture` is not routed by the macOS
UIDelegate; its media callback maps camera, microphone and their combination.
The enum's existence and the owned deny-all closure therefore do not establish
display-capture denial. This is the first concrete capability blocker to resolve
before broadening native input. Do not paper over it by increasing a wait or
treating a promise left pending as denial.

Two further upstream source findings prevent treating the handler as complete:

- The public geolocation decision callback is available on macOS 27.0, but is
  absent from the vendored delegate. The local denial could come from WebKit or
  OS policy and does not establish a Zephium-owned denial.
  The installed Xcode SDK header also lacks this newly published callback;
  runtime/SDK availability must be handled explicitly before using it.
  [WebKit WKUIDelegate header](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKUIDelegate.h).
- A missing WebAuthn UI callback produces `Unavailable` in WebKit's delegate
  adapter. In the compatible authenticator path, only `DidNotPresent` prevents
  subsequent discovery; `Unavailable` is not a deny decision. The modern path
  can use a separate presenter. Thus autofill-off and no private authentication
  callback do not prove authentication containment.
  [UIDelegate](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/Cocoa/UIDelegate.mm),
  [AuthenticatorManager](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/WebAuthentication/AuthenticatorManager.cpp).

These upstream sources explain possible paths; they are not a claim that the
installed OS WebKit is byte-identical or that a real credential escaped. The
passkey fixture uses a fixed nonexistent localhost credential ID, never
discoverable credentials, registration, a real account, or a provider. It logs
only a closed error name and abort status. Media success would stop all tracks
and fail qualification. Geolocation values are never read or serialized.

Clipboard read/write and payment initiation were deliberately not executed:
the clipboard contains unrelated user data and a successful write would mutate
it; no merchant/payment context is authorized. Their owner-enforced denial
remains unqualified. The fixture makes no external fetches, has `connect-src
'none'`, and never emits raw exception messages, credential/device/location
data, arbitrary page code, or model-selected scripts.

Native samples inspect this process's `NSApplication.windows`, attached sheets,
modal window, URL and existing lifecycle/focus guard at every pump. This does
not inventory UI belonging to another OS process, certify that a sub-sample
transient UI never existed, or prove denial in an already activated context.
No activation-bearing mouse/key positive control was added. Failure to reach a
surface is not containment; unavailable APIs, pending requests and platform
denials must retain those distinct meanings.

## Reproduction

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=ui target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=capture target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=display target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=passkey target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=geolocation target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
```

Loopback/native process access requires the same sandbox exception as the
existing qualifiers. The default sandbox returned `fixture_start`; the
authorized native runs reached the owned constructor. No commit or push.

The final native binary built successfully. Embedded fixture JavaScript passed
a syntax check, and `git diff --check` passed. No production runtime or delegate
was changed. `fixture_server` remains under `probe-harness`; the native child
remains under `native-agentic-semantic-probe`.

Final measured binary SHA-256:
`ad2ce84e704292a54fe60ffe42953daf98c176067097cf5cc010460f12741ca0`.
Native probe source SHA-256:
`cb032e41b48069ee9e458f8f6ea9228b4a5382a20da725c71c923338166138b8`.
Embedded fixture SHA-256:
`149449ae958328a634ca3ce0b9db48b33b856ed9e8566bace4ebd69d55a34710`.
