# Contenteditable command: production admission assessment

Assessed 2026-09-09 against the current retained Work implementation and the
isolated-command qualification. This is an engineering decision, not an
additional browser compatibility result.

## Decision

The fixed isolated `insertText` command is a credible backend for the existing
Fill operation. The present candidate is not yet a complete production Fill:
it deliberately returns an uncertain terminal and consumes its only command
opportunity for the whole document. Neither limitation should be mistaken for
a fundamental WebView limitation. Do not replace these restrictions with a
successful terminal solely because the candidate observed matching text.

The next decisive experiment is **one exact approved Notion title change through
the existing retained Luna workflow, using the candidate and preserving its
uncertain terminal**, followed by an independent observation of the remote page
and manual restoration. It can be run before implementing logical-editor
identity or general repeated-command support. This answers whether the command
updates Notion's application state, rather than just the fixture DOM.

## What existing code establishes

- `build_owned_work_view` constructs a new hidden, unfocused WKWebView from the
  selected profile; it does not adopt the already human-controlled sign-in view.
  Profile cookies do not carry a DOM selection or a native composition session.
- `WorkObservationPresentation` presents that owned page in a surface which
  ignores mouse input and is checked to have neither key nor main capability.
  It captures and continuously checks the existing human foreground owner.
- The immutable document-start isolated runtime receives a closed Fill request
  through its correlated channel. Guarded delivery rechecks retained authority
  before the pending request is pulled. Public JavaScript evaluation and native
  responder insertion are not part of this command path.
- The retained Notion task already binds the authenticated account, owned
  context, exact origin, exact title transition, effect and ordering. Its first
  action can be executed independently of whether a second action is possible.
- The candidate already maps to `AppliedUnverified` and then `NeedsHuman`.
  That is an appropriate terminal for this experiment. No verified action,
  completed workflow, or remote persistence claim should be recorded.

This supports testing the fresh owned-document route. It does not qualify an
arbitrary previously human-controlled page, resumed IME composition, native
trusted clicks, or a document exposed to public evaluation. Future takeover
and resume must make that distinction explicit. A blanket requirement to solve
all those future states before the fresh-document experiment would not test
the current implementation more accurately.

## Exact experiment boundary

Build the usual retained Notion write desktop qualification with the additional
explicit Cargo feature `zephium-engine/native-agentic-semantic-probe`. The
normal `retained-product-qualification` feature does **not** enable this feature.
Set `ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=isolated-fill-normal` before process
construction, alongside the established Notion qualification configuration.
`agentic_semantic_program_probe::source` selects the candidate for installation
and attestation; the candidate itself does not restrict execution to the local
fixture URL. Do not run another qualification or change that environment value
within the same process.

Exact build and launch from the repository root (after the enrolled profile is
ready and the previous application process has exited):

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-navigation-probe.conf.json --features macos-work-retained-notion-write-probe,zephium-engine/native-agentic-semantic-probe --bundles app --ci --no-sign
env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=isolated-fill-normal ZEPHIUM_NOTION_QUALIFICATION_URL=https://app.notion.com/p/Zephium-Agent-Qualification-3d5656402d9a806aabb8c03c475ae5af 'target/debug/bundle/macos/Zephium Work Navigation Probe.app/Contents/MacOS/zephium-desktop'
```

The credential loader uses the existing Keychain item through `/usr/bin/security`;
no key is put in the launch command. Keep stdout and stderr together in the
private local run log so the `isolated-fill-terminal` diagnostic is preserved.
Record the candidate source, executable and bundle hashes for this explicitly
modified diagnostic build. Its result must not be attributed to the default
shipping runtime digest.

If the enrolled profile is absent, first build with
`--features macos-work-profile-enrollment` and otherwise the same bundling
arguments. That separate build has no automatic Work observer. Sign into the
disposable account there, verify the exact page title, then quit normally before
building the command candidate. Enrollment and execution features are mutually
exclusive. The isolated application data path is
`/Users/crynta/Library/Application Support/app.zephium.work-navigation-probe`.

Use the already authorized disposable page, verify its original title first,
and execute one run. Expected host outcome is uncertain after at most one
contenteditable command opportunity. A preflight refusal must be distinguished
from a command that was entered. The content-free runtime diagnostic records
this distinction, but a logical-editor match is still not persistence evidence.

Inspect the target independently after the run, preferably through a separate
authenticated view/reload. Record whether the temporary title persisted, whether
the original target's semantic identity survived, and whether restoration was
performed and independently observed. If the temporary title persisted, restore
it through the human interface. Do not restart the two-action automated task
against an uncertain title or use another command to conceal an uncertain first
outcome. The existing run record remains uncertain even if later inspection
establishes that the application accepted the write.

This experiment is not the final useful-work benchmark: its task intentionally
specifies the exact transition. It qualifies one missing browser editing
mechanism required by broader open-objective workflows.

## Minimal production integration after that result

If Notion accepts the command, choose the smallest result contract supported by
the actual observation:

1. If the original target survives, the current independent exact-ref Fill
   verification can establish the semantic postcondition. Retain that route;
   a replacement-identity abstraction is unnecessary for that result.
2. If the framework replaces the target, establish a bounded logical editor
   identity anchored to the original connected editor root, original document,
   supported structure and protected siblings. Carry it with the existing
   dispatched effect and verify it from a separate fresh observation. Do not
   reuse a detached leaf's ref for another node, match by label/value alone, or
   mint verification from the command's completion response.
3. Replace the document-wide command latch with one consumed opportunity per
   exact dispatched action. An uncertain attempt remains nonretryable. A second
   action requires its own fresh observation, newly admitted effect and current
   account/document/lease authority. The existing serialized runtime and Work
   action machinery should remain the authority owner.
4. Qualify update and restore in one fresh owned context, including framework
   reconciliation, cancellation/revocation, focus change, and teardown. Keep
   semantic verification distinct from independent remote persistence evidence.

If the command is refused by Notion, inspect the bounded structural/preflight
reason. If it executes but Notion does not retain the edit, preserve uncertainty
and examine that concrete incompatibility. Neither outcome justifies starting a
general native input subsystem by default.

## Evidence read

- `isolated-contenteditable-command-qualification.md`
- `macos-display-command-qualification.md`
- `trusted-document-command-spike.md`
- `crates/zephium-engine/src/platform/macos/agentic_isolated_fill_candidate.js`
- `crates/zephium-engine/src/platform/macos/agentic_semantic_program_probe.rs`
- `crates/zephium-engine/src/platform/macos/agent_context.rs`
- `crates/zephium-engine/src/platform/macos/work_observation_presentation.rs`
- `crates/zephium-engine/src/platform/macos/semantic_action.rs`
- `crates/zephium-agentic/src/semantic_verify.rs`
- `crates/zephium-work-composition/src/retained_notion_write_qualification.rs`

No provider call, authenticated write, or additional native compatibility run
was performed as part of this assessment. No shipping source was changed.

## First authenticated candidate refusal and diagnostic follow-up

The first retained Luna candidate run subsequently reached its one approved
title Fill and returned `UnsupportedInteraction` before command dispatch. Its
snapshot reported 87 nodes, 18 textboxes, one exact title and one eligible Fill
candidate; the title had one text child and an editable parent. The reported
model call used 3,988 input and 118 output tokens. No remote change was observed.
That snapshot does not reveal which additional candidate preflight guard failed.

The release-excluded candidate now distinguishes closed guard reasons such as
`nested_editable_ancestors`, `sibling_editability`, `sibling_tag`,
`root_visibility`, and an exception's fixed preflight stage. It includes no
DOM names, attributes, values, paths, selectors or exception messages. The probe
adapter accepts only the exact fixed reason list, writes
`isolated-fill-preflight: reason=... command_entered=false content=redacted`,
then normalizes the diagnostic to the existing `UnsupportedInteraction`
terminal before ordinary decoding. Unknown or malformed diagnostic strings
are not normalized. The guard conditions, mutation path, success semantics and
shipping runtime remain unchanged.

Rebuild and rerun with the exact command/environment above. Capture both output
streams. The reason identifies the concrete guard to investigate; absence of a
preflight reason means refusal occurred outside that candidate preflight (for
example in the unchanged outer runtime's Fill admission). Seventeen deterministic
candidate cases pass, including exact ancestor/sibling diagnostic refusal with
zero editing commands and zero input events. The unchanged production runtime
smoke also passes. This is diagnostic instrumentation, not relaxed admission.
The focused native-adapter diagnostic unit test, full
`check-agentic-probe-boundary`, formatting check and whitespace check pass.

The next authenticated run identified `nested_editable_ancestors` exactly.
[Nested editable spine qualification](nested-editable-spine-qualification.md)
records the resulting bounded candidate adjustment and seven passing native
rows. The build/launch settings above are unchanged; successful production Fill
and Notion persistence remain unqualified.

The inherited-spine candidate then reached the next preflight guard,
`sibling_tag`, with no command dispatched. Its sibling was already an element
explicitly marked noneditable; its tag was outside the original simple-text
sibling allowlist. The next diagnostic distinguishes only five fixed categories:
`sibling_tag_br`, `sibling_tag_wbr`, `sibling_tag_div`, `sibling_tag_p`, and
`sibling_tag_other`. No page-defined tag name, attribute or content is emitted.
All categories normalize to the same pre-mutation `UnsupportedInteraction`.
The accepted sibling set is unchanged pending that concrete observation.
Twenty-seven deterministic cases pass, including all five category refusals with
zero editing commands and zero input events.

The tag diagnostic subsequently identified `div`. The protected-div follow-up in
`nested-editable-spine-qualification.md` records its narrow text-only candidate
admission, 33 deterministic cases and 12 native rows. Its actual Notion child
structure has not yet been qualified; rich/nested protected siblings still
refuse. The build/launch settings and uncertain-result contract are unchanged.

### Authenticated postcondition diagnostic

The protected-div candidate entered the exact command on authenticated Notion,
then returned `AppliedUnverifiedPostcondition`. Independent viewer reload retained
the original title. This does not establish a successful local edit or persistence;
it also does not by itself identify a WebKit command or Notion reconciliation cause.

The next build adds closed, content-free postcondition reasons only. Output is:

```text
isolated-fill-postcondition: reason=<fixed reason> result=uncertain content=redacted
```

`value_mismatch` means a fresh bounded logical value was obtained but was not the
requested value. Other fixed reasons distinguish root identity/editability/
writability/visibility/sensitivity/credential/context, ancestor declarations,
editable child structure/count, protected sibling identity/editability/tag/
structure/text/sensitivity/count, or restoration/reconciliation exceptions.
The reason describes the first rejecting check, not an inferred causal diagnosis.
For example a replacement protected sibling may first fail `editable_child_count`
or `target_control`, rather than reaching the final `protected_count` check.
No actual value, page-defined tag, attribute, DOM, or exception text is emitted.

The release-excluded native adapter exact-allowlists these codes and normalizes
them to the original `AppliedUnverifiedPostcondition` before ordinary decoding.
Unknown or malformed suffixes remain invalid. Guard ordering, fixed command,
selection, one-attempt document latch, and shipping success semantics are unchanged.
Rebuild and rerun the same one-attempt authenticated command above; inspect this
reason before proposing another candidate adjustment. No retry is authorized by
the diagnostic.

Validation: 37 deterministic/adversarial cases pass with exact reasons, including
four added post-dispatch root/target changes; all three diagnostic adapter/source
tests, `check-agentic-probe-boundary`, formatting and whitespace checks pass.
This diagnostic-only revision has not yet run against authenticated Notion.
Candidate SHA-256: `316db067296d378ef89035a2588fdbd35af42fe30e166ebfb5babd3c394f5595`.
Shipping runtime remains
`b952b18e0166e67e56d4021c468ca5fcc05d35e9bf4d86267ce43ac76c2acae2`.

### Final command behavior diagnostic

The authenticated postcondition reason was `value_mismatch`, not a structural
rejection. The next revision adds one immediate, bounded logical read directly
after the fixed command returns, before focus/selection restoration. It records
only the command return category and value comparison category. The existing
fresh microtask observation remains unchanged. No listener, timer, observer,
additional edit, or value/content output is introduced.

Expected stderr for the same rebuilt authenticated run:

```text
isolated-fill-command: returned=true immediate=match content=redacted
isolated-fill-postcondition: reason=value_mismatch result=uncertain content=redacted
```

Those example categories are illustrative, not a prediction. `returned` is
exactly `true`, `false`, or `other`; `immediate` is exactly `match`, `mismatch`,
`guarded`, or `exception`. A match followed by mismatch proves the value changed
between the two observations. It does not alone distinguish restoration-triggered
handlers from framework reconciliation. Immediate mismatch does not exclude a
synchronous framework reversion inside the command. A command boolean is never
evidence of application, persistence, or authority.

The closed native normalizer strips this diagnostic suffix only from already
recognized uncertain outcomes; malformed suffixes remain invalid. Successful
logical comparison still cannot mint exact-ref success. No structural admission
was broadened. The original production program is unchanged.

All 39 deterministic/adversarial cases pass, including false-return/no-mutation
and immediate-match/microtask-reversion controls. All four focused Rust diagnostic
tests, boundary, format, and whitespace checks pass. Candidate SHA-256:
`f272e256b94cc6a3f8ca63e0b4afa158184780d5945106a80b2925dd9baa6d12`.

### Single-variable restoration discriminator

The authenticated command returned `true`, the immediate logical value matched,
and the microtask value mismatched. An independent reload of the prior equivalent
run retained the original title. This establishes failure of the complete tested
lifecycle recipe, not inherent incompatibility of document-bound insertion:
the recipe restored the prior focus/range synchronously between those observations.

The next candidate removes only that post-command restoration block. Pre-command
captures, focus/selection preparation, exact guards, immediate and microtask
observations, one-attempt latch, and uncertain outcomes remain unchanged. No
event-loop yield is added. This is restricted to the fresh owned diagnostic view,
which has no human selection/composition to preserve and remains owned until
terminal/teardown. It is not permission to change restoration behavior for a
human-controlled page. The native window presentation restoration still runs.

All 39 deterministic/adversarial rows pass, with explicit checks that range clear
occurs once (preparation only) and the diagnostic does not restore focus afterward.
All 12 actual owned/presented WKWebView rows pass: normal/retarget retain the
fixture model with uncertain logical outcomes; cancellation, leaf replacement/
adoption, protected sibling and ancestor attacks remain nonretryable failures;
rich/nested protected blocks refuse before mutation. Every row verifies hidden
native presentation restoration and teardown. These are local fixtures, not
authenticated Notion acceptance. All four focused Rust tests, boundary checks,
formatting, and whitespace checks also pass.

Use the same explicit probe build/environment for the next authenticated run.
If the value now survives, restoration was a material contributor under that
controlled comparison; persistence still requires independent reload. If it
does not, a single preparation-settle opportunity is a separate next decision,
not a reason to remove more structural guards or silently switch backends.

Candidate SHA-256:
`c5dc11e825bdd3d48e31fd962f059831090b5230d2ff91bb43228d2a4de77683`.
Native qualifier SHA-256:
`96be52da56b1d0a4608eb5fc0a46af64b491e1bdbe5079ada27d32411bd049a1`.

The alternative-path reassessment remains relevant, but does not supersede this
smaller discriminator: native responder tests have real frame/selection/composition
counterexamples and an unqualified display-capture boundary. The earlier AX
test only disproved direct Cocoa setters on the remote proxy, not all AX clients;
however [WebKit's AX value implementation](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/accessibility/AccessibilityRenderObject.cpp)
uses editing operations for editable fields, so AX identity alone does not establish
exact-leaf mutation confinement. Neither warrants an implicit backend fallback.
Explicit human takeover and a separately authorized [Notion API page update](https://developers.notion.com/reference/patch-page)
are usable product paths to develop, not evidence that browser editing succeeded.

### Owner-bounded save-window discriminator

The no-restoration authenticated run reached `returned=true`, `immediate=match`,
and `AppliedUnverifiedLogicalEditor`. This is a local microtask match, not verified
Fill or persisted remote state. The controller correctly reported zero verified
actions and NeedsHuman; an independent viewer later saw the original title.
Separately, ordinary visible Computer Use changed and restored the title across
independent reloads, establishing healthy account/editor/server behavior through
that interaction path.

The retained host currently retires presentation immediately when the native
uncertain terminal arrives. The controller does not enter its requested normal
mutation-quiet settlement after that failure. Closing/hiding this page may abort
or throttle a deferred autosave. That is a concrete lifecycle confound, not proof
that autosave was in fact scheduled or aborted.

The next diagnostic leaves the existing page presented for three seconds after
an AppliedUnverified terminal before delivering that same terminal. Enable it
only on the same rebuilt release-excluded app with both exact environment values:

```sh
ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=isolated-fill-normal
ZEPHIUM_LOCAL_NOTION_SAVE_WINDOW_PROBE=1
```

It uses the existing host-owned 50ms wake and action lease; it creates no timer,
JS callback, observation, edit, credential, or additional provider request. The
normal profile/tombstone/document/deadline/human checks run on every wake and
native presentation is additionally polled. Revocation/lifecycle failure exits
retention immediately. It never extends the original action deadline; a full
three-second window plus a 100ms retirement margin must fit, or the diagnostic
reports `insufficient_deadline` and preserves ordinary behavior. Existing wake
and cleanup limits remain unchanged. It cannot grant success or retry, and is
compiled only for macOS with `native-agentic-semantic-probe`.

Expected content-free stderr:

```text
isolated-fill-save-window: phase=started duration_ms=3000 mutation=none terminal=unchanged content=redacted
isolated-fill-save-window: phase=completed mutation=none terminal=unchanged content=redacted
```

`interrupted` is not a completed save window. Even `completed` proves only the
bounded lifecycle window, not DOM retention, server receipt, or persistence.
The independent fresh-profile viewer must then check which title the server
returns. If the temporary title persists, premature retirement contributed. If
not, later framework rejection and a different save/commit requirement remain
possible; this is not a reason to automatically repeat the write.

Validation before that authenticated run: three focused save-window unit/source
tests pass; the full agentic release-boundary gate (including Work-resource
guards and unchanged shipping JS smoke), formatting, and whitespace checks pass.
No actual retained-host save-window execution has yet been claimed. The command
candidate itself is unchanged from the 12-row native qualification above.
Save-window helper SHA-256:
`62156c1b7ac8181b4e6eb40bf1fdec3644a7ed2056b67efbf61fe0de355a83ed`.

### Guarded preparation/command join

The authenticated three-second save window completed under live presentation,
but independent reload still returned the original title. Visible ordinary input
had persisted faster. This rules out that specific bounded retirement delay as
a sufficient remedy; it does not identify whether the editor later reverted or
never scheduled a model update. The next controlled variable is allowing focus
and selection preparation to settle before insertion.

The release-excluded candidate now supports a native-owned two-phase join:

1. The original admitted action prepares its exact private target and range.
   Its one-attempt document latch is already spent; preparation is not retryable.
2. Through the existing attested isolated reply channel, it issues one fixed
   preparation message. The native channel retains that reply alongside the
   original pending action, authority closure, and completion. No second action
   envelope, model request, selector, or alternate target is created.
3. The existing retained host wake checks the original lease, deadline, profile,
   document, human fence, and presentation. Only after at least 100ms may the
   native channel recheck the original authority closure and release its one
   fixed continuation response. No independent timer or delayed mutating JS
   callback is introduced.
4. The same original private closure revalidates the exact ref/descriptor,
   editable spine, selection endpoints, visibility, geometry, and hit target
   before the single command. It does not repair a changed selection or adopt a
   replacement target. Failure remains uncertain, with no automatic retry.
5. The existing immediate/microtask diagnostics and uncertain terminal remain.
   Successful local comparison is not action verification or persistence.

Duplicate preparation, a result sent before continuation, missing guarded action,
revocation, deadline expiry, navigation, renderer loss, and retirement cannot
release a pending insertion. Cancellation drains the pending native reply and
original completion. A continuation already handed to WebKit still carries the
ordinary nonretryable queued-effect uncertainty; this does not claim an ability
to retract a delivered native reply. The real qualifier uses a guarded exact
owned-view loop too, not an unconditional native release.

The 100ms interval is a rendering/event-loop opportunity, not proof that an
arbitrary application has finished all async preparation. There is no readiness
claim beyond the fresh checks. Two native fixtures use actual `selectionchange`
events during that interval to move the range or change an editable ancestor;
both changes are observed and insertion refuses before any editing event.

Authenticated launch adds one exact switch, preserving the prior save window:

```sh
ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=isolated-fill-normal
ZEPHIUM_LOCAL_ISOLATED_FILL_PREPARATION_PROBE=1
ZEPHIUM_LOCAL_NOTION_SAVE_WINDOW_PROBE=1
```

Expected additional content-free stderr before the existing command/terminal:

```text
isolated-fill-preparation: phase=waiting minimum_settle_ms=100 mutation=not_yet_dispatched content=redacted
isolated-fill-preparation: phase=released minimum_settle_ms=100 authority=rechecked mutation=not_yet_dispatched content=redacted
```

All 47 deterministic candidate cases and seven focused native-channel/source
tests pass. Tests include busy/reentrant admission, denied continuation, changed
selection/identity/credential/readonly/spine/occlusion, exact single release,
interval enforcement, revocation, matching action timeout, document replacement,
renderer loss, retirement, duplicate preparation, and premature result refusal.
Fourteen actual owned/presented WKWebView rows pass, including two preparation-gap
attacks with zero beforeinput/input events and preserved values; all restore
native presentation and complete teardown. The full boundary gate, formatting,
and whitespace checks pass. Authenticated Notion acceptance/persistence remains
unmeasured for this new preparation variant; production admission remains absent.

Final rebuilt binary was rerun through all fourteen rows successfully. SHA-256
identifiers for that evidence:

- candidate JS: `2088b575b6891456f2e63b7ab4c1323472df119952364ffebf63395c525c36da`
- native channel helper: `0bfa0929529a83e8bc8b64a6626574fe5fa40c8ee8a02160ac5c7d55ec45ed79`
- owned fixture: `37c3546d4be24891e5eac8643348dbc4a6523dc39a14b108335d2ec596d02e4b`
- native qualifier binary: `bdcf786ec326c294e5a569196f7314ca595ef3648545184acb15f37039540e63`
- unchanged shipping semantic runtime: `b952b18e0166e67e56d4021c468ca5fcc05d35e9bf4d86267ce43ac76c2acae2`

### URL invalidation after preparation release: outcome ownership

The authenticated prepared run logged `waiting` and `released`, then resource
`ContextLost`. Native URL evidence showed a changed path with unchanged
origin/port/query/fragment. No command diagnostic or save-window terminal was
collected. Independent reload returned the original remote title.

This does **not** establish that insertion never ran. Current macOS URL KVO
handling calls `WorkDocumentNavigation::location_changed`, which changes Ready
to Refused on any non-exact URL. The observer invalidates shared health and calls
`semantic.cancel()`. Cancellation consumes the original pending invocation and
settles its owner as Cancelled immediately. Any later isolated result finds no
pending invocation, so its command/postcondition diagnostic is never decoded.
The retained host also cancels when `ready()` or the document stamp changes.
A deterministic native-channel regression reproduces this exact ordering after
the preparation continuation has been released: one cancellation completion,
zero decoded results, no second completion, and no new admission. Existing debt
is not silently dropped; the original runtime evidence is replaced by the
uncertain cancellation terminal.

Neither URL component equality nor the absence of a navigation callback so far
proves same-document identity. KVO and navigation events are independently
delivered. Do not fix this by accepting same-origin paths, updating the frozen
document, delaying all health invalidation, or preserving stale action guards.

The smallest production design is **result-only draining**, separate from
execution authority. It requires a coherent change across all current owners,
not a KVO exception:

1. A handed-off action may transition to a closed Draining state on URL
   invalidation. New actions, reads, page pulls and any still-held preparation
   continuation are refused immediately. No authority is renewed. An action
   merely queued in native is cancelled without draining.
2. Preserve the one original invocation/completion receiver and its original
   deadline. The same view/world/frame attestation and one outstanding result
   join remain required. Receipt is evidence/debt ownership, not permission to
   operate the changed page. A result is never promoted to verified success
   after document authority has been invalidated.
3. Repeated host cancellation must not consume that receiver early. The host
   must use its existing bounded wake to force exact-attempt timeout at the
   original deadline; no new timer or worker is needed. Failed health remains
   visible and the core already supports settling an original action terminal
   with `is_current=false`.
4. Native full-document events, renderer loss, transport failure, explicit
   teardown, or deadline expiry force one uncertain settlement and close the
   receiver. They must never allow a replacement document to inherit it. No
   extended save-window presentation is authorized merely by result draining.
5. Audit/debt/retirement predicates must recognize Draining as pending, not idle
   or reusable. Destruction cannot report native drainage while this original
   callback is still owned. The current Failed-state invariant explicitly
   forbids pending receivers; it must be changed together with cancellation,
   host timeout and retirement behavior, not bypassed.

The ownership design is now implemented in the production host/runtime path;
the logical-editor command itself remains release-excluded. Deterministic
runtime and host coverage includes URL revocation before and after the terminal,
the runtime-terminal/queued-host-callback gap, exact attempt identity, repeated
URL callbacks, cancellation, full-document replacement, renderer loss, malformed
terminals, timeout and bounded presentation retirement. The observer closes
semantic authority before invoking any failure callback that can progress the
host. Boundary negative controls reject restoring host-terminal prerequisites,
widening the passive interval, extending the original deadline, removing
cancellation, or substituting a non-exact runtime witness.

### Authenticated retained-write and persistence result (2026-09-09)

The first bundled run after result-only draining collected the exact terminal
instead of replacing it with `ContextLost`, but retired presentation immediately.
Its save window was interrupted and an independent clean-profile reload returned
the original title. A preparation-only authenticated control produced no URL
drift, associating the path change with the command or its page reactions.

The initial passive-lifetime implementation also interrupted immediately. Its
release-excluded diagnostics exposed a concrete classification mismatch: the
runtime retained only generic `AppliedUnverified`, while the fixed Notion
candidate returned `AppliedUnverifiedLogicalEditor`. The native mapper later
collapsed that specific fault to `AppliedUnverified`, so the diagnostic save
window started even though the production lifetime witness was absent. Runtime
eligibility now uses that same closed native fault mapper. The URL observer was
also ordered so semantic revocation precedes failure notification, removing a
separate reentrancy hazard.

The corrected actual-application run used `gpt-5.6-luna`. It consumed 3,957
input tokens and 124 output tokens, cost 781 microUSD under the priced ceiling,
and completed its model call in 4,396 ms. Luna proposed one exact Act. Native
preparation logged `waiting` then `released`; the command returned an immediate
logical match and the exact `AppliedUnverifiedLogicalEditor` terminal. Passive
lifetime then reported `eligible=true retained=true cancelled=false`, and the
full three-second window completed. A path-only same-origin URL change still
revoked document authority. The controller conservatively ended in Recovery /
`NeedsHuman`, with one proposed and active action, zero verified actions, and no
source-mapping or durable-terminal claim.

After the application exited, the mutated profile was archived and an untouched
logged-in profile was restored. Navigating independently to the original page ID
returned `Zephium Agent Qualification — Write Probe` and Notion's corresponding
updated canonical slug. This is independent server-state evidence that the
retained agent write persisted. The page was then restored manually to exactly
`Zephium Agent Qualification`; a later UI/server refresh showed the original
title and canonical slug again.

This qualifies the bounded lifecycle mechanism for the measured authenticated
Notion title transition. It does not turn elapsed time, a DOM match, or a route
change into a generic persistence oracle, and it does not admit the command into
shipping builds by itself. Product verification still needs a source-specific
fresh-state witness before it can report success or continue a multi-action plan.
Full-document transitions, takeover, cancellation, renderer loss and deadline
expiry continue to terminate passive lifetime without retry.
