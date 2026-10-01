# Logical title fields within inherited editing hosts

Measured 2026-09-09 on the same macOS WKWebView qualification machine. This
extends the release-excluded isolated command candidate, not shipping Fill.

## Real incompatibility and bounded model

The second authenticated Notion diagnostic reached Luna's exact approved title
Fill (4,020 input / 129 output tokens), then refused before mutation with
`nested_editable_ancestors`. The title still had one plain text child. Its
effective editable ancestor count was therefore not evidence of rich content
inside the selected field.

The old guard conflated the local logical field with the browser's editing host.
`isContentEditable` is inherited: several ancestor wrappers can all be editable
inside one host. The revised candidate retains the same limited logical unit:
one plain-text leaf, its immediate parent, and at most 31 explicitly noneditable
simple text siblings. It still refuses rich target children, unsupported
siblings, credential/protected contexts, multiline replacements and unbounded
inspection. It never selects, flattens or scans the larger editing host.

The existing ancestor capture is bounded to 32 levels and already retains exact
node identity and effective editability, checking visibility, document ownership,
credential/sensitivity and writable context. The candidate additionally captures
each ancestor's bounded declared `contenteditable` mode. Unknown modes refuse.
The exact spine and declared modes must remain consistent before insertion and
after framework reconciliation, including changes from `true` to
`plaintext-only` which preserve effective editability. An ancestor reparent or
protection change cannot silently become a verified continuation.

The command still consumes one opportunity for the document before focus, invokes
only fixed `insertText`, and emits an uncertain result after any possible effect.
A fresh logical value match remains `AppliedUnverifiedLogicalEditor`. Same-origin
page code can move selected DOM nodes; the spine is a locator/postcondition,
not a security boundary against that page.

## Native fixture correction

The local native fixture now places the original logical field inside an
explicit editable outer host and an inherited editable wrapper. An unrelated
rich field is a sibling elsewhere in the outer host.

The initial run kept listeners on the immediate logical container. The text
changed, but those listeners received zero events and the fixture's application
model remained unchanged. That run failed and is excluded from the passing
results. WebKit dispatches these editing events at the outer editing host. Moving
the fixed fixture's delegated listeners to that host made its model reflect
the edit. The final fixture explicitly checks `event.target === host`, performs
framework-style leaf reconstruction in a microtask, and reads the reconstructed
logical field. This fixes the fixture's model; it does not change the candidate
or synthesize additional page events.

## Actual owned/presented WKWebView results

Every row uses a fresh ephemeral owned view, the existing inactive visible
presentation, the actual isolated semantic Click/Fill channel, and the same
fixed replacement. There is no native responder insertion, public evaluation,
account or provider in these rows.

| Row | Before / input | Fresh model | Runtime terminal |
| --- | --- | --- | --- |
| Normal nested/inherited field | 1 / 1 trusted | Replacement retained after reconstruction | Logical editor matched, unverified |
| Cancel beforeinput | 1 / 0 | Original retained | Postcondition unverified |
| Replace selected leaf | 1 / 0 | Original retained | Postcondition unverified |
| Adopt leaf into same-origin child | 1 / 1 trusted | Intended model lost; original leaf in child | Postcondition unverified |
| Retarget focus to decoy | 1 / 1 trusted | Replacement retained; decoy unchanged | Logical editor matched, unverified |
| Change protected sibling | 1 / 1 trusted | Replacement retained; sibling mutation detected | Postcondition unverified |
| Change outer host declaration during beforeinput | 1 / 1 trusted | Replacement retained; declaration mutation detected | Postcondition unverified |

All seven rows passed. Each reported a visible document, no transient or sticky
activation, exact event host, unchanged unrelated rich field, unchanged decoy,
original URL, restored hidden placement and native teardown. The protected
sibling and ancestor declaration were unchanged except in their respective
deliberately hostile rows. Every result remained incapable of issuing a
successful Fill terminal or authorizing retry.

The 22 deterministic cases also pass. Added cases cover explicit/inherited
editable ancestors, preflight invalid declarations, declared-mode mutation
during focus (zero commands), declared-mode mutation during beforeinput,
ancestor reparenting, and refusal to reenter after a fresh observation. Existing
credential, readonly, rich/oversized, cancellation, retarget, adoption,
reentrancy, input and textarea cases retain their outcomes. Synthetic cases do
not establish event trust; the native rows do.
Both native-adapter program/diagnostic tests, the full agentic probe boundary
check (including unchanged production-runtime smoke and its negative control),
formatting and whitespace checks pass.

## Reproduction and next step

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
for command_case in normal cancel replace adopt retarget protected ancestor; do
  env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=isolated-fill-$command_case target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom || break
done
node eval/agentic-browsing/isolated-contenteditable-command-smoke-v1.js
cargo xtask check-agentic-probe-boundary
```

The next authenticated Notion diagnostic uses the unchanged build and launch
settings in `contenteditable-production-admission.md`. Only the observed
ancestor restriction has been addressed. The next result can still identify a
different unsupported local field structure. Do not infer Notion acceptance,
remote persistence, repeated-command support or production admission from this
fixture result.

Measured SHA-256 values:

- Candidate source: `30e89ea5334358c16ba857d400207504944af3e7da691e39002e31728d4d7fb5`
- Fixture: `79bb8a08ec8390f0c5d1868d24295c5eea45a59ddb6a3be065ffc57448e63090`
- Native qualifier executable: `2b101fe9718d18509ebe8e1ffe4d44ebb57f19aec61fb182b3db5a5018c9d058`
- Unchanged shipping runtime: `b952b18e0166e67e56d4021c468ca5fcc05d35e9bf4d86267ce43ac76c2acae2`

No authenticated service was accessed in this local qualification. No production
backend or successful-result contract changed.

## Protected text-only div follow-up

The next authenticated diagnostic identified the noneditable sibling's tag as
exactly `div`, with zero command dispatch. That established the tag, not its
remaining child structure or application semantics. The candidate now admits a
div only under the same bounded public text-only protected-sibling contract.
No other tag was added. The independent selected leaf remains the only range
passed to the editing command. Rich/nested div children still refuse; block
layout does not grant an operation on the protected sibling.

The protected sibling's original element identity, tag, explicit noneditability,
text-only structure and exact bounded text remain postconditions. Public
sensitivity is now rechecked after dispatch as well as before it. A page changing
those properties yields an uncertain terminal even if the title value matches.
The 33 deterministic cases pass, with exact failure-code assertions for hostile
postconditions rather than merely accepting any uncertain diagnostic.

The actual owned/presented WKWebView fixture now uses a noneditable block div
instead of the formatted inline sibling. All seven preceding cases pass again
with that block layout. Five additional native cases also pass:

| Protected div case | Before / input | Fresh model | Runtime result |
| --- | --- | --- | --- |
| Existing rich child | 0 / 0 | Original retained | Unsupported; sibling text structure |
| Existing nested div | 0 / 0 | Original retained | Unsupported; sibling text structure |
| Replace sibling with an equivalent new div during beforeinput | 1 / 1 trusted | Replacement retained; identity changed | Postcondition unverified |
| Make sibling editable during beforeinput | 1 / 1 trusted | Replacement retained; editability changed | Postcondition unverified |
| Insert nested structure while preserving sibling text | 1 / 1 trusted | Replacement retained; structure changed | Postcondition unverified |

Together these are 12 passing native rows. The existing `protected` row now
verifies block-div text mutation. Normal and focus-retarget rows retain matching
logical-editor diagnostics but never successful Fill terminals. All rows retain
the unrelated rich field, original URL, and decoy, with no observed activation,
verified hidden restoration and native teardown. The fixture keeps a replaced
protected element replaced during reconstruction; it does not silently restore
the old element and conceal the identity mutation.

Reproduce the added native rows with the same command and fixed values
`isolated-fill-div-rich`, `isolated-fill-div-nested`, `isolated-fill-div-identity`,
`isolated-fill-div-editable`, and `isolated-fill-div-structure`. Authenticated
diagnostic build and launch settings are unchanged. Actual Notion child structure
and persistence remain to be observed; this evidence admits only the bounded
candidate adjustment.

Follow-up measured SHA-256 values:

- Candidate source: `e69c89b6027e950ea3f69d4f5c2cfbbad13c8f844b942738cd70c3b44f7b46c4`
- Fixture: `536a18c917cf2b2935fc65d0a224a7d42a80c830f0cbcd2f8b95be7ca4522303`
- Native qualifier executable: `4b89dea3f9218426ba231d63ffdef6f9c99b84ab699399075f889b3a21bf324c`
- Shipping runtime remains unchanged at the hash above.
