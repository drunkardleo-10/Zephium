# macOS public native text-responder Fill boundary

Measured 2026-09-09 on macOS 27.0 (26A5425a). Release-excluded,
fixed-loopback, ephemeral profile. Production admission and Windows behavior
are unchanged. No account/provider, global keyboard event, clipboard, private
SPI, DOM-selector action, arbitrary page evaluation, commit, or push.

## Result: no public exact-target/selection authority

The owned WKWebView responds to the public NSTextInputClient method names,
including `insertText:replacementRange:`. This does **not** establish a usable
text-input lease. With the owned page as native first responder and an
independently observed exact DOM leaf selection, public readback gives:

```text
selectedRange = {NSNotFound, 0}
markedRange = {NSNotFound, 0}
hasMarkedText = false
attributedSubstringForProposedRange:{0,64} = nil
actualRange = untouched sentinel
```

Current [WebKit WebViewImpl source](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/mac/WebViewImpl.mm)
explicitly implements these synchronous getters as placeholders. This matches
the live runtime. `hasMarkedText == false` cannot certify absence of composition.
The selected-range and substring completion-handler alternatives are not declared
in the installed public AppKit headers. The probe does not invoke them.

Native insertion has no semantic ref/generation argument. Source review shows
asynchronous dispatch through the focused frame; numeric replacement ranges are
resolved against the editable root. A working range getter alone would still
not bind a later edit to the admitted leaf. Invalid range fallback, composition
confirmation, and focus/selection changes require an authority solution before
a write is safe. See [WebKit WebPage editing](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/WebProcess/WebPage/WebPage.cpp)
and [EditingRange conversion](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/Shared/EditingRange.cpp).

## Experiment

`agentic_responder_fill_probe.rs` is included only by the release-excluded semantic
probe. It has **no native insertion or marked-text mutation call**. A closed
environment enumeration selects flat, nested, retarget, or cross-leaf fixtures;
conflicting AX/relay variants refuse. The driver captures a fresh semantic ref
after presentation and performs the existing one-shot semantic button click.
The fixture prepares focus/selection itself: a controlled test precondition,
not an agent-owned selection lease. The observable click is never retried and
uses existing settlement verification. Subsequent native queries are read-only.

Each case concludes `fill=refused-before-native-dispatch`, zero native insert
dispatches. A separate executable report branch prevents measurement completion
from claiming successful Fill or model acceptance. Four semantic snapshots are
taken in one document epoch.

The page leaves selection in place, moves focus/selection to a same-text decoy,
or attempts a range spanning the nested leaf and its formatted noneditable
sibling. Fresh semantic witnesses distinguish the actual DOM endpoint results,
including engine normalization. Native readback is compared before and after;
no repeated read authorizes insertion.

The existing on-screen, inactive, non-key/non-main, mouse-ignoring presentation
guard restores hidden geometry/state and performs native teardown on all exits.
DOM focus on an editing ancestor is measured separately from exact leaf selection
and never grants ancestor mutation authority.

## Evidence limits

| Property | Evidence / limit |
| --- | --- |
| Exact leaf selection | Fixture verifies DOM endpoints; native readback cannot attest them. |
| Hostile focus retargeting | Same-text decoy receives focus/selection; native readback remains identical. |
| Cross-leaf selection | Fixture records actual endpoints or engine normalization; neither supplies native scope authority. |
| App/window focus | Native responder is owned page; app inactive, window non-key/non-main. |
| User activation / popup | Setup click shows inactive transient/sticky activation, popup denied, including settlement. This is not an insertion-event measurement. |
| Preservation | Semantic flat/nested/input/search values unchanged; fixture checks exact formatted sibling identity, decoy value, zero editing/composition events, and unchanged delegated model. |
| Composition | False public getter is a placeholder; reliable composition detection is unproven. No composition is started/confirmed. |
| Model/rerender acceptance | Native insertion unmeasured: authority failed first. Unchanged model preservation is not write acceptance. |
| Invalid range / beforeinput attacks | No native mutation issued. Source identifies unsafe fallback/retarget assumptions; runtime mutation behavior remains unqualified. |
| Retry/cancellation | Existing setup click is nonretryable after observation. No post-insertion cancellation or recovery guarantee is claimed. |

This establishes a public API boundary, not an inability of native text input
to edit a framework model. The route cannot safely authorize exact semantic Fill
under the tested constraints.

## Reproduction

All four native cases completed, refused before native insertion, and verified
hidden restoration/native teardown. Flat focus was the exact leaf; all nested
cases focused the editing ancestor. The cross-leaf range's requested DOM
endpoints were retained (`cross_leaf_normalized=false`). Every case reported
identical native readback before and after its challenge, with an inactive app,
non-key/non-main window, and exact owned-page responder.

The existing isolated Fill hostile regression passed: 22 snapshots, four epochs,
six nested context attacks refused nonretryably, delegated model retention,
page-origin/prototype/lifecycle checks, zero activation/popups/focus theft, and
zero retained views. Actual-runtime JS smoke and `git diff --check` passed.

`cargo xtask check-agentic-probe-boundary` did not pass in the shared dirty tree:
it reports `provider locate proposal drifted from bounded semantic core
Self::SurroundingText{..}=>Err(AgentBrowserToolContractError::Scope)`. This task
does not edit that provider-locate implementation or the boundary checker;
the failure is recorded, not bypassed or attributed to a successful release check.

Measured binary SHA-256:
`3b0335e5f0d9afa70f8d4f5b281f570c9c202b499ceb6530dbedead062a7c4d3`.

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
for responder_case in flat nested retarget cross-leaf; do
  env ZEPHIUM_LOCAL_RESPONDER_FILL_PROBE="$responder_case" target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
done
node eval/agentic-browsing/contenteditable-fill-smoke-v1.js
env ZEPHIUM_PAGE_WORLD_FILL_RELAY_PROBE=1 ZEPHIUM_PAGE_WORLD_FILL_RELAY_HOSTILE_PROBE=1 target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
git diff --check
```

Public contract: [Apple NSTextInputClient](https://developer.apple.com/documentation/appkit/nstextinputclient),
[Apple selectedRange](https://developer.apple.com/documentation/appkit/nstextinputclient/selectedrange()),
and the installed SDK `AppKit.framework/Headers/NSTextInputClient.h`.
