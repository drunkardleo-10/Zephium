# Document editing command under logical-editor authority

Measured 2026-09-09 on macOS 27.0 (26A5425a). This standalone release-excluded
proof revisits fixed isolated `document.execCommand('insertText')` under a
different contract from exact-leaf Fill: the leaf locates one approved logical
editor, and a fresh model/value observation verifies its intended outcome.
The fixture grants its fixed local adversarial operations explicitly. This
does not widen a production Fill permit or permit model-provided programs.

## Outcome

The command updates a framework-like nested logical editor correctly, including
trusted delegated `input`, leaf replacement, and reconstruction from the model.
Changing focus/selection into a child frame does not redirect this command's
bounded replacement. However, **adopting the selected leaf into that child
during `beforeinput` redirects the command's own replacement to the child**.
Native `WKScriptMessage.frameInfo` identifies the replacement's trusted `input`
as a child-frame event; its data is the outer command's fixed replacement.
The original logical editor's model becomes empty, so its postcondition fails.

This disproves exact document/frame confinement even for the document-receiver
command. The destination in this proof inherits the same origin; an origin
escape is not established. Same-origin DOM adoption is enough to invalidate
the narrower claim. A broader origin-scoped context capability would require
an explicit different authority contract and a proof of all DOM-adoptable
destinations, including any effective-origin relaxation. It is not admitted
here.

| Row | Logical model after rerender | Child effect | Interpretation |
| --- | --- | --- | --- |
| Normal nested editor | Replacement retained | None | Framework-like acceptance |
| Same-document focus retarget in `beforeinput` | Replacement retained | None | Original selection used |
| Cross-frame focus retarget in `beforeinput` | Replacement retained | None | Original document selection used |
| Cross-frame `textInput` trap | Replacement retained | None | No `textInput` event was emitted; trap was not reached |
| Child focused before command | Replacement retained | None | Receiver document's selection used |
| Reentrant child command | Replacement retained | Distinct `page reentrant` text | Page-authored inner command; outer replacement stays in intended unit |
| Adopt selected leaf into child during `beforeinput` | Empty | Outer `document replacement` inserted into adopted leaf | Exact document/frame confinement fails |
| Cancel `beforeinput` | Original retained | None | Command returns true without input effect |
| Replace selected leaf during `beforeinput` | Original retained | None | Command returns true without intended input effect |

All nine rows preserve the original formatted noneditable sibling and decoy.
The successful six rows replace the old text leaf and reconstruct a fresh one
from the delegated model. The fixture reads the current logical unit, excluding
its exact formatted sibling; it does not read the detached old leaf. This
corrects the old proof's unsuitable model witness for this broader contract.
The successful rows prove a relevant mechanism, **not Notion acceptance**.
No Notion code, account, provider, authentication, or remote site was used.

The native output distinguishes page-authored reentrant insertion from the
outer command's bounded value using separate fixed data, row identity, and
native frame metadata. The adoption counterexample performs no page-authored
inner edit: its handler only adopts/selects the original leaf in the child.
The outer command supplies the child edit. Native child navigation denial
remains active throughout; all nine initial `about:blank` child navigation
attempts are denied without destroying their initial documents.

## Why source inspection was insufficient

Upstream [Document.cpp](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/dom/Document.cpp)
resolves `execCommand` using the receiver document's frame and rejects a frame
that no longer owns that document. Upstream
[EditorCommand.cpp](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/editing/EditorCommand.cpp)
passes the frame document to `TypingCommand::insertText`. Those entry checks
explain why ordinary focus retargeting did not redirect insertion. They do not
freeze the selected DOM nodes' owning document while synchronous page listeners
run. The physical adoption row provides the missing counterexample.

Do not repair this by suppressing page events, patching page prototypes,
reinstalling editable-ancestor fences, or treating command `true` as proof.
Those approaches either change the framework contract or fail after page
listeners have already observed the operation. Any entered command remains
nonretryable under uncertainty; a failed logical postcondition does not retract
its side effects.

## Scope and reproduction

The immutable isolated document-start script invokes only the captured native
`Document.prototype.execCommand` with fixed `insertText` and fixed text. It
has no native-dispatch page bridge, arbitrary evaluation interface, generic
selector/program input, global input, clipboard, private SPI, or production
call site. A second immutable isolated observer records bounded all-frame
events. Page scripts implement only the closed local fixture/application
behaviors. The hidden ephemeral view never activates the accessory application
or becomes visible/key/main. In this measurement all observed editing events
were trusted, sampled transient/sticky activation was false, and native popup
creation count was zero. This does not qualify production rendering behavior.

```sh
swiftc -module-cache-path /private/tmp/zephium-trusted-text-swift-cache \
  eval/agentic-browsing/trusted-document-command-spike.swift \
  -o /private/tmp/zephium-trusted-document-command-spike
/private/tmp/zephium-trusted-document-command-spike
```

The complete run has a 15-second deadline and needs local macOS WebKit service
access. It asserts the fixed case inventory, model and fresh value, sibling and
decoy retention, event counts, the two distinct child input witnesses, trust,
activation, and native URL/lifecycle. Raw evidence plus source and binary
SHA-256 values are in `trusted-document-command-evidence.json`. Production
sources, semantic runtime digest, and admission remain unchanged.
