# Editable-ancestor fence follow-up: no-go

Measured 2026-09-09 on the same macOS 27.0 (26A5425a) WKWebView as the
unfenced experiment. This is a separate fixed local, isolated document-start
fixture. Production code, runtime digest, and admission remain unchanged.

The narrow recipe retains the exact nested span and its single editable parent,
saves that parent's literal `contenteditable` attribute, sets it to `false`,
selects only the span's contents, synchronously runs engine `insertText`, and
restores the original attribute. The leaf itself remains explicitly editable.
No additional ancestors, siblings, or fields are fenced. The fixture uses only
fixed targets/text, a hidden ephemeral view and public WebKit APIs.

## Measured result

| Case | Synchronous result | After observer/application microtasks |
| --- | --- | --- |
| Unfenced control | Original leaf detached, events target ancestor | Leaf absent, model `Original` |
| Benign fence | Same leaf retained with `Bounded text`; both trusted events target leaf and bubble to delegated parent listener | Leaf/model retain `Bounded text`; observer sees both attribute mutations |
| Reopen ancestor during trusted `beforeinput` | Leaf detached; `beforeinput` targets leaf but `input` targets ancestor | Leaf absent, model `Original` |
| Observer reacts to temporary fence | Same leaf initially retains `Bounded text`; trusted events update model | Observer sees both attribute records and restores model/text to `Original` |

All final ancestor attributes exactly equal the original literal `true` in
these fixtures. That restoration does not erase mutation records: benign and
observer-revert cases report old values `true` then `false`, while each record's
current value is already restored to `true`. Observers therefore see the
temporary state transition even if they run after the synchronous recipe returns.

The reopening handler only sets the ancestor back to `true`; it does not remove
or replace the leaf. The subsequent leaf removal is the engine command's
normalization. This is the decisive boundary failure: the fixed isolated caller
cannot revalidate its fence between the engine's trusted `beforeinput` listener
dispatch and the ensuing mutation. Exact attribute restoration afterward is
too late to preserve the target's identity. The observer-revert fixture separately
demonstrates an asynchronous framework reaction to the fence, not a claim that
all real frameworks react that way.

The original sibling stayed connected and unchanged, the other field was
unchanged, sentinel DOM focus was restored, activation remained false, and the
native popup delegate recorded zero calls in all cases. Command return was `true`
even for the failed identity/model cases.

## Decision and limits

Do not integrate this fence. Benign success does not establish bounded authority
against synchronous page listeners, and final attribute equality does not make
the ancestor mutation invisible to frameworks. No attempts were made to suppress
page observers/events, add page-world interception, expand the fence, or make
the hostile cases pass. Stop at this concrete failure.

Only the measured one-ancestor case and original attribute value `true` were
tested. Multiple ancestors, lexical attribute variants, arbitrary rich content,
full exception restoration, ref-generation integration, Chromium, real providers,
and production scheduling are not qualified by this fixture.

## Reproduction

```sh
swiftc -module-cache-path /private/tmp/zephium-edit-command-swift-cache eval/agentic-browsing/editing-command-fence-spike.swift -o /private/tmp/zephium-edit-command-fence-spike
/private/tmp/zephium-edit-command-fence-spike
node eval/agentic-browsing/contenteditable-fill-smoke-v1.js
git diff --check
```

Execution needs local macOS WebKit service access. The view never becomes key
or visible and uses the same public inactive scheduling setting as the original
standalone experiment. No external resources or accounts are accessed.

Full evidence: `editing-command-fence-wk-evidence.json`.
Source SHA-256: `723732e84f7077d62b55c9df5751621bae70a4f7d96b441cd7deee69ae5055a0`.
Binary SHA-256: `b2f50da93aed072944df1f5b48d806a182ff005d25c0c5553688d84b5bb73bda`.
Focused semantic/contenteditable smoke and whitespace checks passed afterward.
