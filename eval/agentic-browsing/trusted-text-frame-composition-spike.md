# Trusted text: frame and composition counterexamples

Measured 2026-09-09 on macOS 27.0 (26A5425a). This fixed standalone Swift
proof is release-excluded and has no provider, account, network resource, global
input, clipboard, evaluation API, or production entry point. It creates one
ephemeral WKWebView at a time, never shows or activates its window, and tears
down each view before the next row. It is a platform counterexample, not an
attestation of the retained Work adapter or a new hidden-rendering guarantee.

## Results

| Row | Native result | Admission implication |
| --- | --- | --- |
| Main-frame control | One trusted input replaces the selected input | Confirms this fixed native call reached its selected control |
| Dynamic blank, child navigation denied | The initial `about:blank` child receives the trusted insertion despite a denied child navigation callback; main value remains unchanged | Denying child navigations does not establish exact main-document input confinement |
| Sandboxed srcdoc allowed | Native frame origin has empty protocol/host; isolated origin is `null`; child script runs but its focus attempt does not focus its editor; no input effect | An opaque child is observable; this row does **not** prove cross-origin insertion or safe origin confinement |
| Sandboxed srcdoc denied | Child navigation is denied; parent focuses the iframe element; no input effect | A denied navigation is not an action-success witness |
| Existing native composition | `setMarkedText` produces trusted composition events; synchronous `hasMarkedText` returns false before insertion; insertion deletes the marked text and commits `insertFromComposition` | The synchronous native getter cannot certify composition absence |

All five executable assertions passed. Each final native responder remained
the exact WKWebView, its URL remained `https://fixed.invalid/`, and application
activation, window visibility/key/main state, and sampled event transient/sticky
activation were false. These are measured states, not a universal activation
guarantee. The all-frame observer is an immutable isolated document-start
script. Its native message frame information is recorded separately from
bounded fixture telemetry; page code has no native-dispatch message or bridge.
Host timers own the single insertion for each row. The composition row adds
one explicit native marked-text preparation call before that insertion.

The dynamic child inherits the authorized main origin. This result disproves
an exact **document/frame** claim, not an origin escape. The parent's fixed page
script writes the initial child's editor and selects it; the child navigation
callback then denies `about:blank`, but that denial does not destroy its initial
document. Same-origin initial documents therefore belong in any conservative
set of possible recipients. The sandbox rows do not complete qualification of
opaque, remote, or dynamically navigated frame recipients.

The composition evidence joins `insertCompositionText` telemetry received
**before** native insertion with `deleteCompositionText`, `insertFromComposition`,
and `compositionend` received afterward. This is not a conclusion from a
synthetic `CompositionEvent`. The native getter's false result is recorded
adjacent to insertion, after a 400 ms interval following marked-text entry.

## Source evidence and decision

Current upstream [WebViewImpl](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/mac/WebViewImpl.mm)
explicitly retains synchronous text-input getters for spurious calls:
`hasMarkedText()` returns false and `markedRange()` returns an absent range.
Its asynchronous counterpart asks the Web process. This explains the measured
false-negative getter, but upstream source is not a pinned-binary attestation.
No private async WebKit selector is introduced by this proof.

Keep native trusted text out of ordinary exact-target Fill. A native URL,
exact WKWebView responder, denied child navigation, and a false synchronous
marked-text getter do not establish the required recipient and composition
authority. Repeating those observations cannot repair an asynchronous input
dispatch gap.

The retained Work action host already tracks one in-flight action, exact lease
and document stamp, a human-input fence, terminal delivery debt, and quarantine
on post-dispatch cancellation (`host/work_resource_action.rs` and
`work_resource_action_port.rs`). Those are useful integration points, but their
semantic-runtime terminal must not be fabricated from the return of void
`insertText`. A queued edit may outlive cancellation. Human control must not be
declared exclusive until that native debt is safely retired; unresolved debt
requires scoped quarantine or native-view teardown. A later observation of
the intended value cannot erase revocation or authorize a retry.

Before widening native authority, investigate a fixed isolated synchronous
document editing command under logical-editor authority. The prior editing
command/fence failures disproved exact leaf identity; they did not decide this
different document-bound interaction contract. Upstream
[Document](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/dom/Document.cpp)
resolves its editor through the receiver document's frame, and
[EditorCommand](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/editing/EditorCommand.cpp)
passes that frame's document to text insertion. That is a reason to test
cross-frame/reentrant retargeting, not a production claim or proof of real
framework acceptance.

## Reproduction

```sh
swiftc -module-cache-path /private/tmp/zephium-trusted-text-swift-cache \
  eval/agentic-browsing/trusted-text-frame-composition-spike.swift \
  -o /private/tmp/zephium-trusted-text-frame-composition-spike
/private/tmp/zephium-trusted-text-frame-composition-spike
```

The run needs access to local macOS WebKit services. The sandboxed attempt
could not obtain those services; the authorized local-service run passed.
The probe fails nonzero if any required row, recipient, event, phase, or native
state differs. A 25-second process deadline bounds the complete five-row run.

Full raw per-frame event evidence and source/binary SHA-256 values are in
`trusted-text-frame-composition-evidence.json`. No production source or runtime
digest was changed for this experiment.
