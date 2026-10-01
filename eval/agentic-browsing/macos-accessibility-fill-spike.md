# macOS in-process accessibility Fill spike

Measured 2026-09-09. Release-excluded, fixed loopback fixture, ephemeral profile.
No account/provider request, global AX root, CGEvent, private SPI, page evaluation,
selector, production admission change, commit or push.

## Result: Cocoa AX-value route blocked before mutation

The parent supplied the real Notion result: the isolated synthetic edit reached
native execution but fresh verification reported OutcomeNotObserved; reopening
showed the original title. This spike does not retry that retired profile.

The hidden initial AX hit returned nil. An honest on-screen/non-key cross-check
also initially returned nil. Current WebKit source explains why this is not an
ordinary hit test: WKWebView enables accessibility asynchronously and returns its
remote web-area child, ignoring the point. The probe therefore samples that
actual readiness condition at most four times (0/125/250/375 ms intersample
delays, with the native lifecycle/deadline checked), then invokes the public
hit test on the exact owned remote child. A delay alone never proves readiness.

Final native evidence:

```text
ax-fill-object-boundary: cf_proxy=false remote_proxy=true content=redacted
ax-fill-public-methods: supported=00000000 content=redacted
macos-agentic-semantic-probe: failed; fixture_variant=standard; stage=ax_public_legacy_unavailable
```

The returned object is an NSAccessibilityRemoteUIElement proxy, not the local
WebCore accessibility field wrapper. All eight modern public selectors were
absent: role, value, label, frame, enabled, protected-content, selector allowance,
and value setter. The public legacy attribute getter/setter surface was also
unavailable. The probe refused before a setter; **zero AX edits occurred**.
Original refusal survived the fixture's fail-closed restoration and native
teardown checks. No permission was requested to bridge this into global AX.

This does not prove that all scoped AX architectures are impossible. It proves
that directly invoking Cocoa accessibility setters on the owned WK hit result
does not reach a writable field on this runtime. The WebCore setter cited below
lives across the WebContent-process boundary; its existence is not evidence that
the application can invoke it through these public in-process proxy methods.

## Implemented experiment, not a production backend

`agentic_accessibility_fill_probe.rs` is included only by the release-excluded
semantic probe. It gates any potential setter on exact semantic textbox/value/
name/geometry, public sensitivity and Fill support; native AX role/value/label/
frame/enabled/protected/settable state; fresh frame/context continuity; and
retained AX identity. Unsupported/missing evidence refuses. It has no generic
target-selector or script entry point. The legacy compatibility branch likewise
requires an explicitly false protected-content value, not missing metadata.

The local fixture includes flat and nested editor application-model witnesses.
The nested witness preserves the exact formatted sibling and rerenders its
model-owned leaf after the delegated event. These AX branches were **not reached**;
no AX application persistence, native input-event behavior, adversarial mutation
behavior, or full cancellation behavior is claimed. Synchronous public native
calls also cannot be preempted mid-call: any future dispatched setter must remain
nonretryable on uncertain return/deadline/teardown, not become an automatic retry.

Honest presentation reuses the existing no-key/no-main/no-activation guard,
ignores mouse input and restores original hidden geometry/state on every exit.
It is an explicit local experiment, not a new product presentation policy.

## Next route to assess: native text responder

WKWebView implements public `insertText:replacementRange:`. This is a credible
separate engine-editing experiment, not yet a qualified solution. It acts on the
native focused editor/selection, not an opaque semantic ref. The next proof must
establish a private exact-leaf focus/selection lease, map the native replacement
range without granting ancestor/sibling authority, and challenge focus/selection
retargeting around beforeinput and asynchronous native dispatch. Never substitute
select-all on the ancestor or global keyboard events. Demonstrate application
model/rerender retention, fresh independent effect verification, no app/window
focus theft, and actual user-activation/popup behavior before production use.

The current WebKit AX proxy result does not justify weakening those invariants.
If exact range/target continuity cannot be proven, report that boundary rather
than turn a value write into unrestricted editor input.

## Reproduction and regression

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
env ZEPHIUM_LOCAL_AX_FILL_PROBE=1 target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
```

Measured binary SHA-256:
`6b315de914f0c760802cf1066741dca46f78741f87d296716f0b61c79ebe6851`.

The unchanged isolated Fill hostile regression passed afterward: 22 snapshots,
four epochs, all six nested-context attacks nonretryable, original controls and
page-origin/prototype/lifecycle checks intact, no activation/popups/focus theft/
retained views. Actual-runtime JS smoke and `git diff --check` passed.

## Primary references

- [Apple NSAccessibilityProtocol](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol): public getter/setter contract and selector allowance.
- [WebKit WebViewImpl](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/mac/WebViewImpl.mm): accessibilityHitTest, asynchronous accessibility enablement, remote web-area proxy.
- [WebKit Mac accessibility wrapper](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/accessibility/mac/WebAccessibilityObjectWrapperMac.mm): public legacy attribute setter schedules AXValue onto its backing object; not a locally available application wrapper.
- [Apple insertText:replacementRange:](https://developer.apple.com/documentation/appkit/nstextinputclient/inserttext(_:replacementrange:)): text-input entry point and replacement range.
- [WebKit WKWebViewMac](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/API/mac/WKWebViewMac.mm): public native text-input methods delegate into WebKit.
