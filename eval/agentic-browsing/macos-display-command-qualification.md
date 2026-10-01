# macOS display capture and isolated editing command

Measured 2026-09-09 on macOS 27.0 (26A5425a). This is fixed local,
release-excluded mechanism evidence. No production trusted-text admission,
permission delegation, native input route, account, or provider changed.

## Public API boundary

The public camera/microphone callback is not a display-capture denial hook.
WebKit's [279566@main change](https://commits.webkit.org/279566@main) deliberately
restored the default display picker when an embedder implements only the
camera/microphone callback. Current
[WKUIDelegate.h](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKUIDelegate.h)
has no public display permission callback. The corresponding display override
is private SPI. No public, per-owned-WKWebView, activation-independent display
denial suitable for these workflows was found; a pending promise is not denial.
The vendored Wry permission documentation now states this limitation precisely.

The public geolocation decision callback is new in macOS 27 and is absent from
the pinned bindings. A beta-only geolocation callback cannot resolve the
display or WebAuthn gaps. No unsupported private selector was added. Public
Lockdown Mode is also not a display/WebAuthn deny-all policy: WebKit's current
[preference definitions](https://github.com/WebKit/WebKit/blob/main/Source/WTF/Scripts/Preferences/UnifiedWebPreferences.yaml)
do not mark `ScreenCaptureEnabled` or `WebAuthenticationEnabled` for disablement
in Lockdown Mode. No production constructor was silently disabled or weakened.

## Distinct dispatch mechanisms

| Mechanism | Final local observation | Meaning |
| --- | --- | --- |
| Actual owned view, public `callAsyncJavaScript`, isolated `setTimeout(0)` then `execCommand('insertText')` | Two fresh processes failed `application_became_active`, sample 4; no new in-process window/sheet/modal | Public evaluation remains unsafe for this operation; does not establish document-start runtime behavior. |
| Actual owned view, visible document, document-start isolated script, captured `Document.prototype.execCommand` | Two fresh processes, 566/577 native samples: `hidden=false`; display `InvalidStateError`; one trusted input; no transient/sticky activation; heartbeat; restoration/teardown verified | Production-relevant no-evaluation mechanism did not reach display UI on this OS. |
| Standalone document-start isolated `WKUserScript`, timer, captured command | Final visible run: `hidden=false`, display `InvalidStateError`, trusted input, no transient/sticky activation, heartbeat; 693 native samples without a violation | Supporting mechanism evidence, separate from actual-owned attestation. |
| Same document-start command, only fixture display operation replaced with immediate `NotAllowedError` rejection | Trusted input; settled rejection and heartbeat; 713 native samples without a violation | Negative control rules out ordinary setup/insertion causing native activation in the corrected harness. |

The standalone probe never calls `evaluateJavaScript`, `callAsyncJavaScript`,
`insertText:`, or a private API. Its script is installed at document start in
a named isolated world. After DOM readiness and a fixed timer, a fixed synthetic
button click prepares the editor; one synchronous `execCommand('insertText')`
dispatches the page's input handler, which calls `getDisplayMedia` immediately.
The fixture stops any unexpectedly returned tracks and never serializes capture
data. It uses `loadHTMLString` with a localhost base URL and proves a secure
context; it is a mechanism fixture, not an actual-owned constructor or network
navigation attestation.

The actual-owned `display-document-start` case uses the real constructor and
existing honest inactive presentation scope. Only the
`native-agentic-semantic-probe` build with that exact fixed environment value
appends `agentic_owned_display_command_probe.js` to the isolated document-start
program. Script identity, sole-script inventory and exact source remain attested
against this fixed diagnostic source. The normal program/digest and normal
build's direct source equality check are unchanged. The snippet refuses all
but the fixed localhost fixture/hash, waits a bounded interval for the visible,
prepared editor, then executes exactly one command. The native qualifier itself
does not dispatch any insertion for that case. It requires an explicit
`hidden false active false sticky false` witness, exact final editor value,
settled rejection and heartbeat, retained URL, and original owner teardown.

Native initialization and monitoring match the existing qualifier's relevant
constraints: finish launching while activation is prohibited, switch to
Accessory before constructing the view, show without key/main admission, ignore
mouse input, and pump the run loop with bounded AppKit-defined event dispatch.
The native window baseline is
taken at the pre-command message after setup, as the owned qualifier does.
Messages are bounded to 1024 bytes; only the fixed fixture's closed facts are
printed. Native observations cover this process, not all OS processes or every
sub-sample UI transition.

Current [WKWebView implementation](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/Cocoa/WKWebView.mm)
sets `forceUserGesture:YES` for public evaluation and async evaluation.
That internal token is not equivalent to `navigator.userActivation`.
[MediaDevices::getDisplayMedia](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/Modules/mediastream/MediaDevices.cpp)
checks its internal user-gesture token before starting a native request and
rejects absent privilege with `InvalidStateError`. It can also return that
error for an inactive/hidden document, so the final probe includes a separate
`document.hidden` witness. This source supports the mechanism distinction;
it does not prove the shipped OS is byte-identical to WebKit main.

Initial standalone native-activation observations were **invalidated** by the
negative control: `NSApplication.run` caused startup activation, and setup could
create a native window before command dispatch. Those observations are not
capture evidence. Only the corrected launch/run-loop/dispatch-baseline runs in
the table above count. The actual-owned evaluation failures used the existing
qualified native guard and remain separate valid failures of that route.
Earlier standalone runs without AppKit-defined event dispatch also had an
unproven or hidden document; their `InvalidStateError` is not used to infer
gesture denial. The final standalone run and actual-owned case prove visibility.

The next candidate is a fixed editing recipe invoked through the existing
document-start isolated semantic channel. This evidence does not qualify
already-activated pages, passkeys, clipboard, payment, arbitrary commands,
contenteditable containment, or every OS version. Keeping a permission handler
that returns `Deny` cannot substitute for those qualifications.

## Reproduction

From the repository root:

```sh
swiftc -target arm64-apple-macosx14.0 -module-cache-path /private/tmp/zephium-display-command-module-cache eval/agentic-browsing/owned-display-command-spike.swift -o /private/tmp/zephium-owned-display-command-spike
/private/tmp/zephium-owned-display-command-spike --control
/private/tmp/zephium-owned-display-command-spike
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=display-command target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=display-document-start target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
```

`display-command-direct` is available as a fixed public-evaluation diagnostic
but was not run in this qualification. Swift compile, the Rust probe build,
and whitespace validation passed. `cargo xtask check-agentic-probe-boundary`
currently fails at the unrelated existing agent-effect canonical-manifest
revision check; this qualification does not claim that check passed.
Native execution required the existing
loopback/native-process sandbox exception. No commit or push.

Final standalone source SHA-256:
`5f519c2d65906ef8f5dfd24eea812f30724fa11dad9d784c1b045c19c7c547e8`.
Actual-owned document-start snippet SHA-256:
`228743d12de97984177f99ebf24c4034e0f1df6e0cc96068208864141ecf593a`.
Final Rust diagnostic binary SHA-256:
`6b20de89d816175622a6dd91180baaa43427618875b980b70aa4519af5404c92`.
