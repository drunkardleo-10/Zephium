# macOS trusted editing-unit proof

Measured 2026-09-09 on macOS 27.0 (26A5425a). This is an executable,
release-excluded proof using the existing semantic fixture server and owned
WKWebView adapter. Production admission, providers, accounts, and Windows are
unchanged. No global input, clipboard, private SPI, generic evaluation,
model-provided selector, or arbitrary model program is used.

## Decision

Separate **exact semantic Fill** from **trusted context interaction**. A DOM leaf
is a useful precondition and result witness, but it is not a security boundary
against the site that owns it. Framework reconciliation may legitimately replace
that leaf. A trusted interaction must instead carry its own policy/effect
authority for one owned context, run, profile, document/frame-origin scope, and
bounded operation; its intended logical editor/value remains a separately
verified postcondition. This does not widen an existing exact-leaf Fill permit.

The smallest useful native primitive is public
`insertText:replacementRange:` with a bounded string and the current-selection
sentinel (`NSNotFound`, zero length). It emits trusted editing events and updates
the fixture's delegated application model across leaf replacement. Unlike the
previous mouse/key responder experiment, this exact text primitive produced no
transient or sticky activation on this device. Thus trusted event production
and activation must be recorded separately. This measurement is not a universal
no-activation guarantee, nor a reason to admit arbitrary keys or mouse events.

The probe conservatively marks its context activation-tainted immediately before
native entry. It never clears that state based on page telemetry, elapsed time,
successful verification, blur, or cancellation. The only release here is owned
view teardown. An eventual product capability must persist that taint with the
native context across operations and navigation; a fresh action object cannot
reset it. Existing exact semantic execution stays unchanged.

## Implementation and measured behavior

`agentic_trusted_edit_probe.rs` is a child of the already release-excluded macOS
semantic probe. One closed environment enum selects a fixed local fixture row.
There is no native-input production entry point. The fixture grants local
context interaction explicitly, including its adversarial decoy; it does not
claim a semantic leaf lease. The existing ref/generation-bound semantic click
prepares the fixture's range once. This is a controlled application setup,
not yet a production selection primitive.

The fixture has a nested editable span plus a formatted, noneditable sibling.
Its delegated trusted-input listener reads the logical editing unit into an
application model and reconstructs the text leaf from that model. Verification
uses a later semantic snapshot of the reconstructed field and decoy, plus the
fixture's model/rerender/event witness. No old ref is rebased silently. These
are deterministic framework-like fixture results, not Notion acceptance.

| Fixed row | Trusted input effects | Intended model after rerender | Boundary result |
| --- | --- | --- | --- |
| `normal` | 1 | Replacement retained; original leaf replaced | Fixture effect verified |
| `retarget` in `beforeinput` | 1 | Replacement retained; decoy unchanged | This particular retarget did not redirect insertion |
| `retarget-before-native` | 1 to decoy | Original retained | Unverified, nonretryable |
| `retarget-text-input` | 1 to decoy | Original retained | Unverified, nonretryable |
| `cancel` in `beforeinput` | 0 | Original retained | No verified effect, nonretryable after dispatch |
| `navigation` in `beforeinput` | 1 | Replacement retained | Unauthorized external navigation denied; exact native URL retained |
| `takeover-before` | 0; dispatch not entered | Original retained | Revocation prevents dispatch |
| `takeover-after` | 1 despite immediate revocation | Replacement retained | Observation does not become action success; nonretryable |

Every row preserved the exact formatted sibling and its text. The two retarget
rows deliberately changed the same-origin decoy, demonstrating why a successful
native call or old exact selection must never imply exact-target success. Every
row reported no transient/sticky activation and no admitted popup. Normal input
produced both trusted `beforeinput` and `input`; textInput retarget occurred
before the intended unit received `beforeinput`.

The takeover rows exercise deterministic lease revocation around asynchronous
native entry, not a physical user taking over the production Work actor. The
post-entry result proves cancellation cannot retract an edit already queued in
WebKit. Neither cancellation nor a timeout/exception permits automatic retry.

## Authority and privileged surfaces

The native adapter is the existing owned, extension-free, ephemeral WKWebView.
Before presentation, full construction/profile attestation is repeated. Adjacent
to dispatch the probe rechecks the exact nonpersistent data-store object,
identifier absence, extension-controller absence, retained native page responder,
exact native URL, snapshot context join (which includes run/profile/document and
cancellation coordinates), five-second dispatch deadline, and the native
presentation/lifecycle guard. The initial and fresh observations use the existing
bounded isolated semantic runtime. One private, non-resettable dispatch state
marks entry and taint before the public native call. Native exceptions and
post-dispatch settlement/snapshot failures are classified nonretryably.

The presentation scope is honestly on-screen, inactive, non-key/non-main, and
ignores physical mouse input. Existing guard sampling detects focus/lifecycle
changes; restoration and original native teardown execute on all exits. This is
not hidden-view input qualification or a permission to steal foreground focus.

Source inspection of `platform/macos/agent_context.rs` and the vendored Wry
`wry_web_view_ui_delegate.rs` confirms the reused owner denies permissions,
downloads, popup construction, file selection, confirm/prompt, and page close,
and disables fullscreen, PiP, and autofill. Alert completion cannot construct a
dialog. The exact armed navigation controller denies unsolicited navigation.
Popup and attempted external navigation are live witnesses here; the remaining
privileged-surface statements are source evidence, not new activation-gated
physical tests. Fixture CSP denies external data fetches. No account or provider
is constructed.

## Production boundary and next decision

Do not promote this function into the existing `Fill` backend order. Its old
selection can become a decoy selection, and `textInput` can retarget native
editing. No amount of repeated public range readback repairs that atomicity.
The successful normal row establishes compatibility under a different authority
model, not exact-leaf safety.

The next implementation should introduce a separate, backend-owned trusted-text
interaction permit within the policy/effect pipeline. It must bind the existing
action/account/profile/context authority and fixed text before preparation,
declare potential activation independently from ordinary Fill, consume one
dispatch opportunity, and verify the fresh logical editor/value after possible
DOM replacement. The model must not select a backend or supply editor code.

Before production admission, the native owner must establish the full possible
focused-frame origin set (including dynamic/sandboxed frames), deny unauthorized
frame/navigation and privileged capability transfers while input is queued, and
qualify active composition behavior. Top-level URL plus owned WKWebView identity
alone does not prove the origin of the frame receiving text. A single fresh
main-frame DOM selection is not that proof either. Existing account/profile
authority must remain exact when this capability is threaded through Work.

That owner must also join real human takeover before every dispatch and retain
nonretryable uncertainty after entry. The probe's deterministic revocation test
is a concrete requirement for that integration, not proof the integration exists.
Once those boundaries are executable, one separately authorized real-framework
workflow can decide admission. No Notion/account write was performed here.

## Reproduction and focused checks

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
for trusted_case in normal retarget retarget-before-native retarget-text-input cancel navigation takeover-before takeover-after; do
  env ZEPHIUM_LOCAL_TRUSTED_EDIT_PROBE="$trusted_case" target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom || break
done
cargo test -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic revocation_and_uncertainty_never_regrant_or_clear_taint --lib
cargo test -p zephium-agentic --features probe-harness fixture_server --lib
env ZEPHIUM_PAGE_WORLD_FILL_RELAY_PROBE=1 ZEPHIUM_PAGE_WORLD_FILL_RELAY_HOSTILE_PROBE=1 target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
node eval/agentic-browsing/contenteditable-fill-smoke-v1.js
node eval/agentic-browsing/semantic-runtime-smoke-v1.js
git diff --check
```

All eight native rows, the one-shot revocation/uncertainty test, nine loopback
fixture-server tests, existing 22-snapshot hostile Fill regression, actual-runtime
semantic/contenteditable smoke, and whitespace checks passed. Listener tests
require loopback permission; the default sandbox refused their bind operations,
and the authorized run with loopback access passed. The diagnostic
module is not reachable from a production backend, and the semantic runtime's
program/digest was not modified by this experiment.

Measured binary SHA-256:
`e30e780447f610b2d5aabc0f88cf8623c24ed0ee6d457e7e131e5efb91ebb174`.
Native probe source SHA-256:
`74580888c153777e1f9681da4cca7a966f208b4638d00829fbe596cec91bc3cb`.
