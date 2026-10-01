# Fixed isolated contenteditable command qualification

This initial candidate record is followed by
[nested editable spine qualification](nested-editable-spine-qualification.md),
which supersedes its single-editable-ancestor restriction and records the newer
candidate/fixture hashes. The initial measurements below remain historical.

Measured 2026-09-09 on macOS 27.0 (26A5425a), using the actual owned-view constructor and its
inactive, visible, non-key/non-main presentation scope. This is a gated candidate,
not production admission or Notion acceptance. No account, provider, remote
website, native responder dispatch, public evaluation, or page-world relay ran.

## Mechanism and admission result

`agentic_isolated_fill_candidate.js` is inserted into the production semantic
runtime's private lexical scope only by the release-excluded
`agentic_semantic_program_probe.rs`. The six exact `isolated-fill-*` fixture
values select this fixed source at construction. Installation and attestation
both use that identical combined source; it remains one immutable isolated
document-start script. The existing closed ref/generation/descriptor action
grammar and message channel deliver the operation. There is no model-selected
backend, new callable entry point, editor program, selector, token or page bridge.
Normal builds continue to install and attest the original byte-identical program.

The recipe accepts a direct plain-text host, or one plain-text leaf directly
inside one editable root with at most 31 explicitly noneditable simple formatted
siblings. It bounds DOM/text inspection, rejects multiline/tab replacement for
this candidate, and preserves the shipping input/textarea setter path. It
captures native range/selection/focus and document `execCommand` primitives,
checks exact refs and descriptors, writable/credential/context state and exact
selected contents, then invokes only `insertText` with the admitted bounded value.
Focus state alone may change during its own preparation; all other descriptor
fields stay exact. Range bounds use WebKit's inherited `AbstractRange` getters.
The synthetic DOM model was corrected to reflect that inheritance after the
first native run exposed the missing getter capture.

One document-local opportunity is consumed **before focus**, which itself can
run page handlers. Any failure thereafter is nonretryable. The code has no
fallback, second command, synthetic repair or reset of that consumed state.
It restores bounded prior focus/selection where those nodes still belong to the
original document, then crosses one microtask boundary to observe framework
reconciliation. Fresh logical text is read from the connected root, excluding
the retained noneditable siblings; stale/detached leaf text is not a witness.
Root/ancestor identity and protection, bounded supported child structure, and
sibling identity/tag/text/editability are checked again.

These locators/postconditions are not authority against same-origin page code.
The actual authority must remain the exact task, account, profile, origin,
owned context/document lease and bounded effect. A matching logical value returns
`applied_unverified_logical_editor`, never `ok`. The decoder accepts only that
closed content-free diagnostic; macOS maps it to `AppliedUnverified`. The
native qualifier exercises the existing host settlement and requires it to
reject success even when the fresh logical value matches. A logical text match
does not imply persisted external application state or authorize retry.

## Actual owned-view observations

Every row uses a fresh ephemeral owned view and the fixed runtime's real
semantic Click/Fill channel. The click prepares the local editor; no native
input or evaluation call performs insertion. Fresh semantic snapshots verify
the framework-reconstructed editor/model and decoy after the runtime terminal.

| Fixed row | beforeinput / input | Fresh model | Runtime diagnostic |
| --- | --- | --- | --- |
| normal | 1 / 1 trusted | Replacement retained after leaf reconstruction | Logical editor matched, unverified |
| cancel | 1 / 0 | Original retained | Postcondition unverified |
| replace selected leaf | 1 / 0 | Original retained | Postcondition unverified |
| adopt selected leaf into same-origin initial child | 1 / 1 trusted | Intended model lost; original leaf belongs to child | Postcondition unverified |
| retarget focus to decoy in beforeinput | 1 / 1 trusted | Replacement retained; decoy unchanged | Logical editor matched, unverified |
| page changes protected sibling in beforeinput | 1 / 1 trusted | Replacement retained; sibling mutation detected | Postcondition unverified |

All six rows passed. Each final witness reported a visible document and no
transient/sticky activation. Original URL, decoy, hidden restoration and original
native teardown were verified. Protected siblings retained their identity/text
except in the deliberate page-authored mutation row. Child ownership in this
new fixture is isolated semantic/page-model evidence; the independent native
child-frame event attribution is in `trusted-document-command-spike.md`.

This run did not attempt privileged sinks, prior human composition, takeover,
or already-activated retained documents. It does not turn the separate display
qualification into a general no-activation or privileged-surface-denial claim.
The existing retained channel's revocation-before-pull test separately checks
that authority revoked while waiting prevents delivery of the recipe.

## Remaining production blocker

The current host's successful Fill verification is exact-ref based. It has no
admitted result join for a reconstructed logical editor under the existing
effect/account/lease authority, and the supported retained lifecycle's prior
activation/composition history is not qualified for this command. This candidate
therefore remains excluded from shipping source and deliberately cannot issue
an action-success terminal. Neither standalone compatibility evidence nor a
fresh text match repairs those missing joins. No dormant native trusted-text
qualification or approval was silently granted.

## Reproduction

```sh
cargo build -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic --bin macos-agentic-semantic-probe
for command_case in normal cancel replace adopt retarget protected; do
  env ZEPHIUM_LOCAL_OWNED_SURFACE_PROBE=isolated-fill-$command_case target/debug/macos-agentic-semantic-probe --ci-hidden-fixed-dom || break
done
node eval/agentic-browsing/isolated-contenteditable-command-smoke-v1.js
cargo test -p zephium-agentic semantic_runtime --lib
cargo test -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic semantic_runtime --lib
cargo test -p zephium-engine --features native-agentic-semantic-probe,native-agentic-work-lifetime-diagnostic content_free_fill_diagnostics --lib
cargo test -p xtask isolated_command_program
cargo xtask check-agentic-probe-boundary
```

The native run requires the existing loopback/macOS WebKit service sandbox
exception. Fifteen deterministic command/control rows and the unchanged full
semantic-runtime smoke passed. Deterministic cases cover credential/read-only/
rich/oversized preflight refusal, focus repurposing, engine exceptions,
reentrancy, fresh logical reconstruction, adoption/retargeting, and refusal to
reenter after a fresh observation. Their simulated events are not evidence of
browser event trust; the native rows provide that evidence.

The ten functional-core runtime tests, eight macOS runtime tests (including the
retained pre-pull revocation check), content-free host fault mapping test, exact
diagnostic-gate mutation test, full `check-agentic-probe-boundary`, and
`git diff --check` passed. All six native rows also passed again after the final
bounded-text hardening and exact fixture-witness assertions.

Measured candidate SHA-256:
`3dc0cd49726ce7b4bd1374e7a1965321b214a89b1950d9efddf4c0d0b822447d`.
Unchanged shipping runtime SHA-256:
`b952b18e0166e67e56d4021c468ca5fcc05d35e9bf4d86267ce43ac76c2acae2`.
Binary used for the six successful native rows:
`6d109d4b803133ad0dcb750553d3aae3bfc8de40c101734b0dd01fe7c920a2ce`.

## Preparation-only negative control (2026-09-09)

With an existing `isolated-fill-*` diagnostic selection, set
`ZEPHIUM_LOCAL_ISOLATED_FILL_PREPARATION_ONLY_PROBE=1` to omit insertion.
This implies the existing guarded 100ms native preparation barrier, even without
`ZEPHIUM_LOCAL_ISOLATED_FILL_PREPARATION_PROBE=1`. The immutable installed
candidate follows the same focus/range preparation and post-release target,
selection, ancestor, visibility and geometry revalidation, then returns the exact
closed terminal `E2:applied_unverified_preparation_only`. Native normalization
records `phase=revalidated command_entered=false result=uncertain content=redacted`
and retains the ordinary non-success `E2:applied_unverified` result. Waiting and
release logs also record `command_entered=false`, before any insertion is possible.
Input/textarea fallback is refused in this mode. The document opportunity remains
consumed, including after a new observation; the internal `commandEntered` latch
means preparation opportunity consumed, not evidence that execCommand ran.

This remains within the release-excluded diagnostic program and original source
attestation. Exact URL authority and ContextLost handling are unchanged. Focus
and selection may themselves run page handlers or alter the URL: the control does
not promise a side-effect-free page, success, saved content, or persistence.
If URL invalidation wins before collection, the fixed terminal may remain unseen;
the earlier phase logs establish only that insertion had not occurred at those
phases. A future authenticated run is needed to determine the URL cause. No live
authenticated account was run for this implementation.

The deterministic smoke additionally runs all eight preparation cases and two
synthetic fallback controls with this mode enabled, checking zero commands/events,
unchanged fixture text, consumed retry opportunity, and preservation of each
post-barrier refusal. All 57 cases passed, as did the full agentic probe boundary
command, the preparation-only boundary unit test, eight native diagnostic unit
tests, and the diagnostic engine `cargo check` build.

### Authenticated Luna preparation-only result (2026-09-09)

The subsequent authenticated qualification used `gpt-5.6-luna`: 3,988 input
tokens, 121 output tokens, and a cost of 247 microUSD. It issued one exact Act.
Preparation logged both waiting and released phases; post-barrier revalidation
then logged the preparation-only result with `command_entered=false`. The
terminal was `AppliedUnverified`. The unchanged three-second save window
completed, and the final outcome was `NeedsHuman`. There was no URL, context, or
resource failure. Page content and account identifiers remain redacted.

Preparation alone did not cause route drift in this authenticated control. In
comparison with the preceding insertion run, the prior path-only drift is
command-associated. This establishes an association with insertion or its page
reactions, without identifying the specific downstream handler. It does not
establish successful editing or persistence, and does not relax URL authority.

The subsequent result-drain and passive-lifetime implementation is recorded in
[contenteditable production admission](contenteditable-production-admission.md).
After exact fault-classification and callback-order fixes, a bundled Luna run
retained rendering for the full bounded three-second interval while semantic
authority remained revoked. An independent clean-profile reload returned the
temporary title and updated Notion slug, establishing remote persistence for
that measured transition. The controller still correctly reported `NeedsHuman`
because it had no source-specific fresh-state persistence witness.
