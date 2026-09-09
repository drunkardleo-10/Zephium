# Synchronous isolated editing-command experiment

Measured 2026-09-09 on macOS 27.0 (26A5425a), native WKWebView.
Result: **do not admit this recipe for general ref-bound contenteditable Fill**.

`editing-command-spike.swift` is an independent fixed local fixture, not linked
into the production semantic runtime. It captures the engine's `execCommand`
method in an isolated document-start WKContentWorld, selects precisely the
retained leaf's contents, then calls only `insertText` with fixed bounded plain
text. This smallest experiment tests a prerequisite for integration into the
digest-pinned runtime: whether engine editing preserves that exact leaf boundary.
The prerequisite failed, so the production runtime, digest, ref grammar, and
admission were not changed. There is no generic script, selector, HTML, clipboard,
page-world relay, global input, private SPI, account, or provider entry point.
The fixture's page script is adversarial/application test code, not an agent API.

## Decisive result

For a contenteditable span inside a contenteditable div, the exact selected
span was detached. WebKit inserted replacement markup under the ancestor,
including a copied sibling attribute. This happened without an adversarial event
handler and before the fixture's queued application rerender. The original
formatted sibling remained connected and intact, and the unrelated editor did
not change, but the retained target's identity and editable boundary did not
survive. Both trusted events targeted the editing ancestor. The fixture model
still held `Original` and the fresh leaf query found no target after rerender.
That fails exact leaf authority and framework-owned model retention for the
case that matters; a postcondition refusal cannot undo engine mutation safely.

The rich standalone root retained its `<b>` formatting immediately after the
command. This does not rescue nested-leaf identity or qualify arbitrary rich
structures. No automatic fallback or retry was added.

## Other measured observations

| Fixture | Observed result |
| --- | --- |
| Flat delegated model | Trusted `beforeinput` and `input`, type `insertText`, bounded data; model and text retained after queued rerender |
| Cancel `beforeinput` | Original text retained, no input event; command nevertheless returned `true` |
| Retarget selection/focus in `beforeinput` | Outer command edited the original leaf; other editor unchanged |
| Replace node in `beforeinput` | Old leaf detached by page handler; replacement unchanged, no input event; command returned `true` |
| Reenter command in `beforeinput` | Page-owned inner command edited other editor; outer command edited original leaf |
| Legacy `textInput` traps | No `textInput` event was emitted; those attack branches were not reached |
| Unknown command | Returned `false`, no events or text changes |
| Activation and popup attempt | `navigator.userActivation.isActive` false in all measured events and results; zero native popup delegate calls |
| Focus | Original sentinel DOM focus restored in every measured case |

The reentrant edit is explicitly a page-authored side effect: it does not prove
that the outer engine command redirected its own bounded text to another field.
The world boundary does not make the operation atomic against synchronous page
event listeners. An isolated caller gets control back only after those listeners
and engine editing finish. The command's boolean is not effect verification.

## Limits and stop condition

This is a WebKit prerequisite failure, not a full alternate-backend qualification.
It does not claim Chromium behavior, real Notion retention, generation/ref
admission coverage in this alternate route, complete selection restoration under
hostile focus handlers, full editing-host mutation attacks, user-input undo
semantics, or arbitrary rich-structure safety. Selection ranges are restored by
the recipe but their complete semantics were not independently measured. Existing
production stale/ref/generation guards are covered by the focused smoke, not by
this standalone command fixture. A Chromium success would not repair the measured
macOS exact-leaf failure. Per the stop condition, integration and broader
qualification stop here.

The fixture is hidden and ephemeral with an accessory application policy, never
made key or ordered on screen. It uses the public inactive scheduling policy
`.none` so the standalone hidden experiment runs; this does not alter production
scheduling. Initial runs without adequate diagnostics timed out; the final
recorded run completed with no script errors. The early error was the retention
observer dereferencing the now-missing nested leaf; it now records null.

## Reproduction and evidence

```sh
swiftc -module-cache-path /private/tmp/zephium-edit-command-swift-cache eval/agentic-browsing/editing-command-spike.swift -o /private/tmp/zephium-edit-command-spike
/private/tmp/zephium-edit-command-spike
node eval/agentic-browsing/contenteditable-fill-smoke-v1.js
git diff --check
```

Native execution needs macOS WebKit service access outside the filesystem
sandbox. The fixed fixture loads an in-memory document; its base URL is
`https://fixed.invalid/`, with no external subresources.

Full final output: `editing-command-wk-evidence.json`.
Source SHA-256: `08278b9d3615a2bb86de457e8aea7d15b1ebc2a0055bb396414cec18e89cfce7`.
Measured binary SHA-256: `bc746e5ae9ee2f2f235cc76c34873acafbdd2db0edf1b35fe35a057371c675f7`.
The contenteditable/semantic runtime smoke and whitespace check passed.

Primary background references (observations above come from the executable,
not inferred from the specifications):

- [WebKit Editor.cpp](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/editing/Editor.cpp)
- [Input Events Level 2](https://www.w3.org/TR/input-events-2/)
- [Editing command draft](https://w3c.github.io/editing/docs/execCommand/)
