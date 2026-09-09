# macOS private isolated contenteditable Fill

Measured 2026-09-09 in hidden ephemeral WKWebViews. No account/provider used.

## Withdrawn relays and reproduced security failure

The Notion attribute relay failed with `AppliedUnverifiedRelayCommandGone`.
A fixed local negative control reproduces a reconciler erasing COMMAND before
the consumer can reread it. That control does not identify Notion's own remover.

The subsequent event relay was insecure, despite passing compatibility tests:
a page-visible fixed event name and correlation ID did not provide admission
authority. A real WKWebView negative run failed
`page_origin_fill_unauthorized`: page JS invoked Fill without any admitted
controller operation. The captured-payload retarget variant was independently
reproduced in the actual page-world program by audit; the original native trap
did not independently demonstrate that variant. Neither relay is retained.

## Correction

Native registration installs/attests exactly one immutable isolated-world
script. Fill executes privately inside its admitted action closure, using the
exact private ref/descriptor and captured native value/text setters and
InputEvents. No page-world mutating script, request listener, token, callback,
transport attributes, or completion timer remains.

Before mutation, the complete private descriptor, connected identity, writable
state, supported plain-text shape and credential metadata are revalidated
after beforeinput. Cancellation/revalidation/mutation failures remain
AppliedUnverified. A captured getter throwing after the setter is explicitly
AppliedUnverifiedPostcondition rather than Internal/Transport.

Native provenance is FixedSemanticRecipe + connected writable form readiness.
The core accepts that closed combination; independent adjacent and fresh
exact-value verification still determine success.

## Current evidence

- Shared-runtime smoke passes text/search/textarea/contenteditable values,
  untrusted event contracts, native primitive capture, password/read-only
  boundaries, beforeinput cancellation/target/markup changes, page rewrites,
  credential repurposing and post-mutation getter exceptions.
- Runtime tests: 10/10. Fixed-backend readiness contract test: passed.
- Hostile WKWebView: 22 snapshots/four epochs; clean: 18/four. Page-origin request and captured-
  payload retarget traps leave unrelated fields unchanged; the actual admitted
  editable Fill succeeds with the stripping observer present.
- Main-world setter/getter/InputEvent/dispatch poisoning, reparent/type and
  credential changes, recovery and exact page-handler receipt pass.
- Zero user activation, admitted popups, focus theft or retained views.

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
env ZEPHIUM_PAGE_WORLD_FILL_RELAY_PROBE=1 ZEPHIUM_PAGE_WORLD_FILL_RELAY_HOSTILE_PROBE=1 target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom
node eval/agentic-browsing/semantic-runtime-smoke-v1.js
```

The historical environment-variable names only choose fixed fixture variants;
they no longer enable a relay or change the production Fill backend.
The old focused smoke command now delegates to the actual isolated-runtime
suite. Obsolete delayed-relay tests were removed with their implementation.

Shared runtime: 112,425 bytes, within the existing 111 KiB ceiling.
SHA-256: `b952b18e0166e67e56d4021c468ca5fcc05d35e9bf4d86267ce43ac76c2acae2`.

Measured native binary SHA-256:
`9dd3ba2e8bb1d230fcc4e575ad4d125fd71fe6a43e5535b200298b9e0faa3456`.

This proves the isolated WKWebView route and fixture authority boundaries, not
Notion's editing compatibility, autosave persistence, Windows native action
parity or generic rich-text editing. No uncertain profile was retried.

## Fill support diagnosis

Seven complete fresh Notion observations retained the exact original title but
never advertised Fill. Waiting did not establish compatibility. Accessibility
"settable" is not sufficient proof that the fixed recipe can safely replace
text without erasing markup.

The isolated runtime now emits one optional numeric `fs` per textbox/searchbox.
Rust decodes it into a closed `SemanticFillSupport` enum, validates consistency
with the role and advertised Fill operation, and retains it only as host
diagnostic state. It does not enter provider projection or effect authority.
The excluded Notion witness prints only the exact matched title's enum, never
DOM, tag strings, attributes, text, or credentials.

Codes distinguish supported controls, missing explicit editable attributes,
native non-editability, unsupported host tags, editable parents, child-count
limits, element children, other non-text children, native read failures,
readonly/disabled controls, and unsupported native input types. Direct text
children remain required; nested context support is described below.
The following instrumented observation identified `EditableAncestor`; its
incomplete structural explanation is addressed below. Adding these diagnostics
used only local fixtures.

Verification: all twelve JS reason branches, Rust closed-decoder consistency
and identical provider projection, and ten immutable-runtime/hash tests pass.
The real hostile WKWebView run passes the supported/missing-attribute/tag/
editable-parent/child-limit/element-child/comment/readonly/disabled/input-type
reason matrix and the existing 15-snapshot/four-epoch Fill security regression.
Native-read exceptions and native non-editability are deterministic-double
coverage, not claimed as native fixture reproductions. A CSS-readonly attempt
was discarded because the fixture CSP had blocked that style, so it established
no CSS/native-readonly behavior. The local fixture now permits inline styles
only to place its diagnostic panel without shifting existing test controls;
production policies are unchanged.

### Editable-parent finding and required structural evidence

The instrumented live witness reported `EditableAncestor` on every fresh
observation, with zero provider calls/actions and a healthy retained profile.
That check precedes the direct-child restriction: it establishes an editable
parent, not a text-only target or the sole incompatibility.

The runtime therefore now attaches a separate optional host-only `es` tuple:
direct-child count (129 means at least 129), kind bits for the first at most 128
children (text/element/other), and immediate editable-parent status. It scans
at most 128 direct children, not arbitrary descendant trees. Rust validates the
bounded shape and its consistency with support/operations; provider projection
is unchanged. The exact matched Notion title prints only these counts/booleans.

The subsequent live structural observation established one text child, no
element/other children, and an editable parent on all seven fresh observations.
No provider request or mutation occurred during that diagnostic.

### Exact nested text-only leaf recipe

Supported nested leaves now retain a private editing-context witness in the
existing generation-bound identity entry, captured only while observing—not
while refreshing an action descriptor. The ancestor path through the current
document is capped at the existing 32-node traversal depth. Native identity,
root, connectedness and editability must match; hidden/inert/readonly/disabled,
sensitive and credential ancestor contexts refuse. These checks run before
dispatch, after beforeinput and after mutation. Mutation remains confined to
the exact explicit-editable leaf with at most 128 direct text children.
Parents/siblings are never promoted to targets; no selection, execCommand,
page-world transport or generic rich-text behavior is introduced.

The JS regression covers 21 combinations of predispatch/beforeinput/postinput
timing and unchanged/moved/relabeled/protected/credential/editability/rich-child
contexts, including sibling structure retention. Post-observable failures are
applied-unverified. The WK fixture additionally exercises a delegated editor
handler, application model and microtask rerender plus six beforeinput attacks.
Both actual WK variants passed, including eight expected nonretryable
beforeinput revalidation failures on hostile (six nested plus two preexisting)
and six on clean. The fixture exposed and fixed a projection mismatch: an
editable role=group ancestor had been allowed to collect sibling text as a field
value. Only textbox/searchbox nodes now collect editable values; ancestors do
not acquire field authority. Temporary per-node library diagnostics were removed.
Live Notion editing and
server persistence remain separate qualification; fixture success is not that proof.
