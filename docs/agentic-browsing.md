# Agentic browsing implementation specification

Status: accepted implementation program
Last reviewed: 2026-08-31

This document defines how Zephium proves and builds the browser-execution layer
used by Work. It is deliberately independent of the Work canvas, product
persistence, hosted backend, collaboration, and final visual design. The layer
must first demonstrate that agents can operate real pages quickly, safely,
efficiently, and recoverably across macOS and Windows.

This is target architecture, not a statement that the current tree implements
it. Existing guarantees remain those in `architecture.md` and
`security-model.md`. Where an experiment contradicts this specification, record
the evidence and change the smallest replaceable mechanism; do not weaken the
product or security invariant to preserve a proposed technique.

## 1. Outcome and proof standard

The outcome is not “JavaScript can click a button.” Zephium needs a
production-grade browser actor that can:

- operate difficult public and authenticated sites over many steps;
- use the user's authorized session without exposing secrets to a model;
- observe pages semantically without sending HTML or an unfiltered DOM;
- execute compact batches, verify real effects, and return small diffs;
- survive navigation, frames, shadow DOM, stale references, SPA updates,
  dialogs, downloads, failures, cancellation, suspension, and renderer death;
- run bounded work in parallel and support bounded sub-agents;
- pause instantly for a person and resume from observed page reality;
- preserve the current browser's extension and security boundaries;
- use no idle resources when no agent run exists;
- provide reproducible metrics rather than a polished anecdote.

The proof phase ends only with repeatable fixture, real-site, safety,
concurrency, performance, and resource evidence on both supported platforms.

## 2. Non-negotiable invariants

1. **No model-facing DOM, selector, JavaScript, CDP, or native-object API.** A
   model sees semantic observations and opaque references and emits bounded
   typed operations.
2. **No generic page-to-native bridge.** Agent instrumentation runs in an
   audited isolated content world/principal and can call only fixed internal
   recipes. Page scripts cannot invoke Zephium authority.
3. **No agent extensions.** Work-owned browser contexts never load user
   extensions or userscripts. Native blocking, cookie policy, and browser
   network policy still apply.
4. **Authority is deterministic.** The Rust policy layer enforces run scope,
   identity, origin, account, data sensitivity, destination, effect, budget,
   generation, and approval. A prompt is not a security boundary.
5. **Page content is hostile data.** Text, accessibility data, image text,
   structured data, tool responses, and model output never become instructions
   merely because they appear in context.
6. **Secrets are never observations.** Password values, cookies, tokens,
   authorization headers, credential-manager contents, hidden form data, and
   other secrets are redacted before serialization and never enter prompts,
   traces, screenshots by default, or persisted artifacts.
7. **Success is verified.** A requested event, returned HTTP status, or
   completed JavaScript expression is not proof of the intended effect.
8. **Ownership is separate from visibility.** Hiding, showing, focusing,
   suspending, taking over, or promoting a context does not change its identity
   implicitly.
9. **Human input wins.** A user never competes with an agent for focus, pointer,
   keyboard, navigation, or form state.
10. **Everything is bounded.** Contexts, workers, sub-agents, depth, payloads,
    text, images, frames, actions, retries, redirects, waits, logs, disk use,
    memory, CPU time, and spend have explicit ceilings and cancellation.

## 3. Context model

Agentic browsing needs browser identity that is not a disguised tab.

### 3.1 Context kinds

**Borrowed tab**

- is an existing user tab and remains in the tab strip and normal session;
- retains its normal extension behavior;
- can be automated only under an explicit revocable lease;
- pauses its run on any human interaction or conflicting browser command;
- is re-observed after every pause because all previous references are stale.

**Owned context**

- is created for a run and owned by that run or a delegated child;
- is not an `Item`, tab-strip entry, Today item, or normal session-restore tab;
- never appears through extension `chrome.tabs` inventory;
- is independently visible, focusable, suspendable, resumable, and destroyable;
- can be represented in Work without keeping a live page on the spatial
  surface;
- may be adopted explicitly into Browse, creating a normal tab under an
  auditable user action.

**Human sign-in handoff**

- is a paused, explicit flow used when an owned extension-free context cannot
  authenticate without a browser extension or user interaction;
- uses a temporary normal human-controlled context with normal extension
  behavior;
- transfers only the platform storage state explicitly supported by the
  context policy;
- destroys or retires the temporary handoff after success;
- recreates or refreshes the clean owned context and takes a new snapshot.

### 3.2 Identity port

Introduce an agent-browser port around a durable `ContextId`, owner, profile,
generation, navigation epoch, lifecycle state, visibility, suspension state,
and capability set. The port must express at least:

- create and close an owned context;
- borrow and release a tab lease;
- navigate and observe committed navigation;
- show for inspection or takeover without changing ownership;
- hide and return to a compact preview;
- suspend, resume, and recover after renderer loss;
- begin and end human control;
- explicitly adopt into Browse;
- enumerate resource/accounting state without exposing native objects.

The first adapter may map `ContextId` privately onto the existing `ItemId`
native lifecycle to reduce risk while extension work lands. It must not persist
an `Item`, project a tab, expose the mapping, or let callers depend on it. This
is an adapter, not the domain model. Replace the mapping before more Work
features accumulate on it.

All asynchronous results rejoin `ContextId`, owner, profile, context
generation, navigation epoch, frame generation, and run cancellation state.
A result missing any current join is stale and discarded.

### 3.3 Promotion and takeover

Selecting an owned context in Work presents the exact native page in a
dedicated non-overlapping page surface or split. It does not serialize and
recreate the page in another engine. When the person interacts, Zephium:

1. revokes the current input permit;
2. pauses dependent agent execution;
3. cancels queued actions and waits;
4. marks every semantic reference stale;
5. gives the person exclusive control;
6. on return, takes a complete fresh observation and resumes from actual state.

An explicit **Adopt as tab** action creates normal Browse identity and applies
the normal tab/extension lifecycle. Merely inspecting or taking over an owned
context does not adopt it.

## 4. Platform storage and extension isolation

The user's logged-in session is an important product advantage, but extension
isolation is a stronger invariant than exact storage equivalence. Platform
adapters state precisely what they can guarantee.

### 4.1 macOS

An owned `WKWebView` uses the exact selected profile's `WKWebsiteDataStore` but
a configuration with no `WKWebExtensionController` and no Zephium userscript or
extension principal. Context construction asserts the controller and extension
inventory are absent before navigation. The agent runtime uses a separate,
fixed `WKContentWorld`; user page-world code and extension worlds receive no
reference to it.

This should preserve cookies and normal site storage while excluding extension
execution. The native spike and hostile tests must prove the actual pinned
WebKit behavior; configuration naming alone is not evidence.

### 4.2 Windows

WebView2 extensions are profile-scoped. When the selected user profile has no
extensions, an owned context may use that exact profile after verifying the
inventory is empty at each creation.

When extensions exist, use a stable extension-free automation subprofile such
as `agent-<profile-id>` within the selected profile's WebView2 user-data
environment. Verify its extension inventory before every context and fail
closed if any extension is present. The subprofile is never silently replaced
with a default or temporary profile.

Before navigation, copy the bounded cookie set needed for the target origins
from the selected user profile into the automation subprofile through supported
cookie APIs, including HttpOnly cookies where the native API exposes them. A
sign-in handoff may repeat that one-way copy after the person completes
authentication. There is no automatic reverse synchronization.

Do not copy a WebView2 user-data directory, claim that cookies imply complete
origin storage, or synchronize local/session storage speculatively. Add a
specific origin-storage bridge only when a measured launch-critical site
cannot work otherwise, after a security review and live platform proof.

Runtime extension disabling is allowed only as an experiment. It cannot become
the production invariant unless the pinned WebView2 version proves that no
extension process, content script, background worker, or hook can execute for
the context.

### 4.3 Profile leasing

Every probe or production context names a profile explicitly. Test tooling
never falls back to the default profile.

Controlled raw-profile experiments acquire an exclusive lease and require the
normal Zephium process to be closed. Production-concurrency tests do not bypass
this rule by opening the raw profile twice; they exercise the adapter through a
running Zephium shell so ordinary Browse and agent contexts share the profile
machinery as designed.

Profile locations, cookies, page content, screenshots, and extracted data are
sensitive. They are not printed, included in CI artifacts, or committed.

## 5. Page runtime and trust boundary

Each page receives an immutable, versioned instrumentation program at document
creation in an engine-supported isolated world. Rust owns the program bytes and
the exact invocation vocabulary. The program does not accept arbitrary code,
selectors, or property paths from the model or page.

The runtime may:

- enumerate a bounded semantic view of a document and supported child frames;
- maintain internal stable node identities for diffing;
- resolve an opaque reference against the exact snapshot generation;
- read allowlisted semantics and geometry;
- execute fixed interaction recipes;
- observe bounded navigation, mutation, focus, dialog, and document state;
- return primitive, size-bounded encoded values.

It may not:

- expose a callable object to page script;
- forward arbitrary messages from page to Rust;
- evaluate model-provided JavaScript;
- serialize HTML, event listeners, framework objects, or secret values;
- reveal cookies, storage, headers, browser credentials, or extension state;
- grant page content a handle to another frame or native context.

On macOS, use WebKit content-world and frame APIs where the supported deployment
floor provides them. Activation-sensitive code must not use public
`WKWebView.evaluateJavaScript` or `callAsyncJavaScript`: current WebKit routes
both through `ForceUserGesture::Yes`, which contaminates transient and sticky
user-activation evidence. Install the immutable runtime as a `WKUserScript` in
a dedicated content world. Any native channel is registered only in that exact
world, accepts a closed versioned schema under hard size/generation/frame
bounds, and exposes neither native authority nor a callable function to page
world. The M1 fixture runtime uses one-way ready/result messages and no reply;
the production fixed-recipe invocation mechanism remains evidence-driven and
must not rely on private WebKit SPI. On Windows, isolated CDP worlds may be an
internal adapter mechanism, but CDP remains absent from domain and model
contracts. Every use is fixed, audited, generation-bound, and covered by
hostile tests.

## 6. Semantic observation

### 6.1 Snapshot contract

The model never receives HTML. A `Snapshot` is a bounded semantic tree or
linearized representation constructed from useful accessibility and document
semantics. It includes only what an agent needs to understand or act:

- role and accessible name;
- visible text at an appropriate granularity;
- interaction and form state;
- checked, selected, expanded, disabled, required, and invalid states;
- safe value summaries for non-sensitive controls;
- landmark, list, table, heading, dialog, and relationship structure;
- same-origin and supported cross-frame boundaries;
- geometry or viewport state only when action planning requires it;
- origin, frame, trust, sensitivity, and freshness labels.

It excludes style, hidden boilerplate, scripts, comments, raw attributes,
tracking values, duplicate navigation, and offscreen content until requested.
Password inputs and values that resemble credentials or tokens are represented
only as a redacted sensitive control.

The wire form is compact and deterministic, for example:

```text
@a1 heading "Repository settings" level=1
@a2 textbox "Repository name" value="zephium"
@a3 button "Save changes" effect=write
```

This example is illustrative, not a license to let text fields invent policy.
All labels remain hostile page data.

### 6.2 Opaque references

References such as `@a3` are opaque capabilities for one observed node. They
bind at least:

- context and owner;
- context generation;
- navigation epoch;
- frame identity and generation;
- snapshot generation;
- internal node identity;
- permitted operation class.

Before action, the runtime re-resolves the node and verifies connectedness,
frame, semantics, current geometry, occlusion where relevant, state, and action
compatibility. Navigation, takeover, suspension, renderer loss, or a material
replacement invalidates references. The model cannot manufacture CSS/XPath or
reuse a plausible reference from another snapshot.

### 6.3 Progressive observation and diffs

Start with the viewport, dialogs, active element, meaningful landmarks, and
interactive controls. Expand a region, subtree, table, frame, or surrounding
text only on request. The first snapshot target is normally 500–2,000 model
tokens rather than a 50–200k-token DOM or a multi-thousand-node raw
accessibility tree.

After an action, return a semantic diff against the last acknowledged snapshot:

- added, removed, changed, and moved semantic nodes;
- focus, URL, title, dialog, document, and loading changes;
- the evidence used to verify the intended effect;
- a bounded amount of surrounding context.

Internal stable IDs support diff computation but never become durable DOM
identity. If a confident diff cannot be formed, return a fresh snapshot rather
than an invented delta.

### 6.4 Frames and shadow DOM

Open shadow roots are traversed under the same bounds. Closed shadow roots are
not bypassed; use platform accessibility/native interaction or human takeover
when their public semantics are insufficient.

Every frame is a separate trust and generation boundary. Same-origin frames
can expose the full bounded runtime. Cross-origin frames require an
engine-supported isolated injection path and are represented as explicit frame
boundaries. If a platform cannot observe a frame safely, Zephium reports the
limitation rather than pretending the elements do not exist.

## 7. Model-facing browser tools

The browser vocabulary is deliberately small:

- `navigate(url)`, `back()`, `forward()`, `reload()`;
- `snapshot(scope?)` and `locate(semantic_query, scope?)`;
- `act(actions[])` for a bounded sequence of `click`, `fill`, `select`,
  `press`, and `scroll` operations over observed references;
- `wait(condition, deadline)` for typed conditions;
- `read(scope?)` for bounded readable content with provenance;
- `extract(scope?, schema)` for validated structured data with provenance;
- `screenshot(scope?)` for an explicitly allowed bounded image;
- `show_for_human(reason)` and `resume_after_human()` through the supervisor.

There is no shipping `eval(js)`. A diagnostics-only evaluation facility may
exist behind a non-release Cargo feature in the probe. It is never wired to a
model, Work, remote input, or ordinary product build.

`extract` is a composite capability: the page runtime supplies bounded
semantic evidence, the model maps it to a schema where needed, and Rust
validates the schema, provenance, size, and sensitivity. It is not arbitrary
DOM evaluation.

### 7.1 Batched turn execution

Zephium is in-process and should exploit it. A normal model turn submits one
bounded action batch. The runtime executes actions sequentially while their
preconditions remain valid, waits for the declared observation condition,
verifies the effect, computes a diff, and returns once. It does not require a
model round-trip for `click`, then another for “wait,” then another for
“snapshot.”

Batching is capped and conditional. A navigation, dialog, unexpected origin,
generation change, failed precondition, new effect boundary, user input, or
meaningful unpredicted state stops the remainder. Speed never permits an action
to run against a guessed page.

## 8. Action pipeline

Every action passes the same pipeline:

```text
authorize -> resolve -> revalidate -> execute -> observe -> settle -> verify -> diff
```

1. **Authorize:** join current run and plan lease; check profile/account,
   origin, destination, data sensitivity, effect, budget, and user-control
   state.
2. **Resolve:** map an opaque reference inside its exact snapshot and frame.
3. **Revalidate:** prove node semantics, state, geometry, generation, and
   compatibility have not changed.
4. **Execute:** choose a measured platform backend and fixed semantic recipe.
5. **Observe:** gather navigation, focus, dialog, mutation, page state, and
   expected evidence.
6. **Settle:** wait for a typed condition under one absolute deadline.
7. **Verify:** prove the expected semantic effect or return uncertainty.
8. **Diff:** return only the relevant bounded change and next state.

Typed failures include at least stale reference, target changed, target
occluded, unsupported interaction, blocked origin, lease violation, needs
human, navigation replaced, renderer lost, timeout, verification failed,
cancelled, and resource exhausted. Retrying is a policy decision based on the
failure type and a new observation, never a generic loop.

### 8.1 Waiting and settling

“Network idle” is not a universal definition of settled state. A typed wait can
target committed navigation, document readiness, element state, URL/title,
dialog appearance/disappearance, semantic change, download decision, or a
bounded mutation-quiet interval. Every wait has one absolute deadline and is
cancellable. Continuous analytics traffic, animations, or an SPA socket cannot
hold a run indefinitely.

### 8.2 Input backends

The runtime can choose among:

- fixed DOM semantic recipes in the isolated world;
- supported engine-native input injection;
- operating-system accessibility activation;
- focused OS input only during explicit human-visible control;
- human takeover.

Backend choice is measured per action/site and returned as diagnostic metadata,
not exposed as a model choice. Zephium must never steal pointer or keyboard
focus from normal browsing to make an invisible owned context work.

## 9. Native-input spike comes first

Synthetic-event trust is the largest platform uncertainty and can change the
interaction design. Therefore a narrow native-input spike precedes the full
semantic runtime.

The spike builds only enough fixture instrumentation to identify one element,
obtain current geometry/state, invoke a candidate backend, and read trusted
fixture evidence. It compares:

- fixed DOM activation/input recipes and their `isTrusted` behavior;
- macOS accessibility and native event routes supported by the pinned WebKit;
- WebView2 controller/composition/CDP input routes supported by the pinned
  runtime and current Wry integration;
- visible focused OS input as a human-control baseline.

Fixtures cover buttons, links, text, contenteditable, select, pointer/mouse/key
listeners, transient user activation, popup requests, clipboard-gated behavior,
drag, iframe, and a minimal shadow-DOM case. Tests record event trust, event
sequence, focus, activation lifetime, target verification, focus theft, and
background/hidden behavior.

No conclusion is generalized from `isTrusted` alone. Real sites may reject
automation through many mechanisms. The spike determines which backends are
available and safe, then the early real-site slice verifies their practical
coverage before the semantic runtime is allowed to harden around them.

If background native input is unavailable on a platform, the production
fallback is fixed semantic DOM recipes for ordinary operations and explicit
human takeover for transient-activation or unsupported controls. It is never
silent foreground focus theft.

## 10. Prompt injection, data flow, and effect safety

Boundary markers around page text are useful model context but are not the
security solution. Zephium enforces source-to-sink policy outside the model.

Every observed value carries:

- source kind and origin/service;
- profile/account and run;
- trust label;
- sensitivity label;
- capture time and freshness;
- provenance references.

Every proposed operation declares:

- destination origin/service/account;
- read, local-write, external-write, communication, purchase, destructive, or
  other effect class;
- data fields transferred and their sensitivity;
- relation to an approved plan node;
- expected result and verification evidence;
- cost and resource impact where relevant.

Deterministic policy rejects unapproved source/destination flows, secrets,
cross-profile data, new accounts/origins/effects, and stale or ambiguous
authority. A model cannot approve its own escalation. A page telling the agent
to ignore instructions, reveal data, install software, send a message, or call
another tool remains untrusted content.

The semantic audit stores plan/effect/approval transitions, resource identity,
typed results, and provenance. It does not store hidden chain-of-thought, raw
DOM, secret values, or indiscriminate page contents.

### 10.1 Downloads, uploads, dialogs, and external schemes

Downloads, file selection/uploads, clipboard, print, popup, permission,
external-application schemes, notifications, credential requests, and OS
dialogs are distinct capabilities. They do not inherit permission from a
generic click. Each needs a typed effect contract, bounded destination/source,
platform adapter, verification, and plan scope. Until implemented and gated,
the browser actor returns `Unsupported` or `NeedsHuman`.

## 11. Agent supervisor

The Rust supervisor owns a run tree rather than allowing models to create
unbounded loops. A parent can delegate a scoped branch with explicit objective,
inputs, origin/data/effect limits, token/spend/time budget, context budget, and
expected artifact. A child cannot widen any inherited capability.

Initial defaults, configurable only within hard product ceilings:

- four executing agents;
- eight total live run nodes;
- delegation depth two;
- four hot native browser contexts;
- additional contexts queued or snapshot-then-suspended;
- per-origin external-write serialization unless a workflow proves safe
  concurrency;
- one cancellation tree from Work/run to every model stream, page wait, tool,
  worker, and persistence operation.

These are starting budgets, not promises. Resource measurements can lower or
carefully raise them by platform/hardware class. The absolute native-view
ceiling already enforced by the browser remains authoritative.

OpenAI and Anthropic are the first proof providers so browser reliability is
not accidentally tuned to one model. Provider adapters implement the same
stream, structured-output, tool-call, usage, cancellation, retry-after, and
error contracts. Local and hosted transports attach later without changing the
browser vocabulary.

The supervisor records concise semantic progress—responsibility, active
resource, operation class, state, result, and blocker—not hidden model
reasoning or raw tool chatter.

## 12. Probe and test harness

Before Work UI integration, create a non-shipping native probe using the same
engine, profile, blocker, context, policy, and lifecycle adapters intended for
the product. It is not an Electron/Playwright/CDP substitute.

The probe includes:

- a small Rust controller with a versioned JSONL or similarly deterministic
  protocol;
- platform-native owned and borrowed context adapters;
- a local fixture server with deterministic hostile and normal pages;
- an inspector showing semantic snapshots, diffs, references, backend,
  generations, resource state, and typed errors;
- a minimal show/takeover/resume path;
- repeatable task scripts and a machine-readable result bundle;
- secure provider/secret injection with redacted output;
- release-feature exclusion so diagnostics/eval/eval-JS cannot ship.

Test fixtures and expected outputs are versioned. Real profiles, keys, cookies,
screenshots, site content, and result payloads remain local ignored data. CI
runs only deterministic fixtures and synthetic resource tests unless a
separately secured environment supplies authorized test accounts.

## 13. Evaluation program

### 13.1 Deterministic fixtures

The fixture suite is exhaustive and cheap. It covers:

- document and SPA navigation, redirects, history, replacement, and reload;
- forms, validation, select, contenteditable, check/radio, keyboard, scrolling,
  and focus;
- dynamic replacement, stale references, virtualized lists, infinite scroll,
  dialogs, and continuously mutating pages;
- same-origin and cross-origin frames, open/closed shadow DOM, and unsupported
  boundaries;
- popup, download, upload, permission, clipboard, and external-scheme denial;
- prompt-injection text and exfiltration attempts;
- hidden, occluded, detached, overlapped, and malicious lookalike controls;
- navigation races, renderer termination, suspension, cancellation, takeover,
  and recovery;
- payload, node, depth, image, action, redirect, wait, and resource ceilings;
- empty-extension inventory and profile/account joins;
- no focus theft and no generic bridge from page worlds.

Fixture correctness is 100%: no flaky pass budget, false success, safety
violation, or uncontrolled side effect.

### 13.2 Initial real-site matrix

The first matrix is intentionally small enough to run repeatedly while still
covering distinct failure classes. Use only owned/authorized test accounts and
reversible sandbox data.

| Category | Sites | Example bounded work |
| --- | --- | --- |
| Public discovery | YouTube, Airbnb | search/filter, inspect results, extract cited facts |
| Authenticated read | Gmail test mailbox, Notion test workspace | locate/read seeded records without writing |
| Reversible write | GitHub test repository, Linear test workspace | create/update/delete designated test artifacts |

Define two scripted tasks per site. Run all twelve three times on one primary
platform/provider pair: 36 runs. Then run one representative task per site on
the other platform with the second provider: six cross-check runs. Re-run
targeted failures after fixes, and expand sites or repetitions only when
numbers are marginal, failures cluster, or an unrepresented control class is
found.

This is a proof-stage matrix, not the final release compatibility claim. The
release matrix grows from measured failure classes and launch-critical user
workflows rather than an arbitrary hundreds-of-runs grid.

The primary pair is chosen from the available supported hardware and provider
credentials and recorded in a versioned eval manifest. The cross-check swaps
both OS and provider. A smaller set also swaps only provider or only OS when a
failure must be attributed correctly.

### 13.3 Concurrent production configuration

The controlled matrix may use an exclusive profile with the normal browser
closed. That is not the production condition. A separate qualification runs:

- normal Browse tabs and major enabled extensions;
- visible user navigation and media activity;
- multiple owned agent contexts, including suspension/resume;
- a borrowed-tab lease and human revocation;
- blocker and profile operations;
- agent model streaming and semantic projection.

It measures correctness, profile access, focus isolation, UI latency, media
stability, native view/process pressure, CPU, memory, GPU/compositor behavior,
wakeups, battery/energy impact, and cancellation. Agent work must not corrupt
or noticeably destabilize ordinary browsing.

### 13.4 Endurance and fault injection

Run multi-hour loops with bounded create/navigate/suspend/resume/destroy,
renderer termination, network loss, model disconnect, rate limit, cancellation,
disk pressure, shutdown, and restart. Validate no leaked native view, process,
task, cookie bridge, profile lease, model stream, secret, or temporary artifact.
Faults settle as typed recoverable or terminal states; they do not hang.

## 14. Metrics and gates

Record at action and run granularity:

- task and action success, verification result, and typed failure reason;
- site, control class, platform, engine/runtime, model/provider, and input
  backend;
- snapshot/diff nodes, characters, estimated provider tokens, and redactions;
- browser action, settle, model, queue, and total wall-clock latency;
- stale references, replans, typed retries, takeovers, and human time;
- prompt/completion tokens, cached tokens, and provider-reported cost;
- live/hot/suspended contexts and native processes;
- CPU time, peak/incremental RSS, GPU/compositor measures available from the
  platform, wakeups, energy impact, disk, and network bytes.

Initial qualification gates are:

- 100% deterministic fixture correctness;
- zero security/scope violations and zero false-success reports;
- every real-site scripted run reaches a correct terminal result, with human
  takeover permitted and recorded; aggregate completion remains at least 99%
  as the suite expands, and any outright failed workflow blocks the proof;
- median initial snapshot at or below 2,000 model tokens and median action diff
  at or below 200, with explicit justified exceptions for requested long
  documents;
- every action failure is typed and bounded; no blind retry or indefinite wait;
- no pointer/keyboard focus theft from Browse;
- no leaked context, task, process, profile lease, or secret in endurance runs;
- no unreviewed regression against the committed Browse resource and input
  latency baseline.

Autonomous completion rate, takeover frequency, per-site action success,
latency, and cost are scored prominently but not gamed into a false binary
gate. Human takeover is part of the reliability design; excessive takeover on
ordinary controls still triggers a backend or semantic-runtime improvement.

Before setting absolute CPU/RAM/GPU/battery budgets, capture a repeatable Browse
baseline on named macOS and Windows hardware. At the native-input spike and
each later milestone, commit the reviewed acceptable deltas to the eval
manifest. Do not invent numbers without data, and do not ship a regression
because no number was written in advance.

## 15. Implementation sequence

Each milestone ends with code, tests, measurements, updated evidence, and
coherent reviewable commits. A later milestone does not paper over a failed
earlier gate.

### Milestone 0 — Ground truth and harness skeleton

- map current `ItemId`, profile, extension, engine, suspension, blocker, stage,
  shutdown, and resource-limit paths on both platforms;
- record the exact pinned Wry/WebKit/WebView2 capabilities and gaps from code
  and primary documentation;
- define versioned probe protocol, fixture server, result schema, redaction,
  and eval manifest;
- capture Browse startup/idle/concurrent-use baselines;
- add build guards proving probe-only facilities cannot ship.

### Milestone 1 — Native-input risk spike

- implement the narrow fixtures and candidate platform backends from section
  9 before building the complete semantic tree;
- measure trust, focus, activation, hidden/background behavior, and teardown;
- run a minimal slice on one difficult real site per platform;
- select the initial backend order and document unsupported interactions;
- stop and revise the interaction strategy if safe background operation is not
  viable.

### Milestone 2 — Context identity and lifecycle

- add the `ContextId` port and initial private native adapter;
- enforce owned/borrowed/adopted identity and visibility separation;
- implement platform extension-free construction and inventory assertions;
- implement explicit profile selection, leasing, Windows cookie bridge, and
  sign-in handoff skeleton;
- integrate suspension, resource accounting, renderer loss, cancellation,
  takeover, and shutdown;
- prove zero tab/session/extension projection for owned contexts.

### Milestone 3 — Semantic runtime

- install the fixed isolated-world runtime with size and generation limits;
- implement snapshot filtering, progressive scopes, frames, open shadow roots,
  secret redaction, opaque references, internal stable IDs, and diffs;
- implement deterministic compact encoding and Rust decoding/validation;
- add hostile-page tests for spoofing, collisions, stale nodes, and bridge
  escape;
- meet initial token and latency budgets on fixtures before real-site breadth.

### Milestone 4 — Action, settle, and verification

- implement the complete action pipeline and selected input backends;
- add bounded action batches and typed wait conditions;
- implement read, extract, and screenshot contracts with provenance and
  sensitivity rules;
- add typed error taxonomy, no-blind-retry policy, navigation/dialog handling,
  and effect verification;
- qualify fixtures and the six-site matrix, iterating by failure class.

### Milestone 5 — Policy and supervisor

- add run manifests, domain/account/data/effect/cost scopes, source-to-sink
  checks, plan lease consumption, and `NeedsHuman` transitions;
- add bounded run tree, sub-agent delegation, per-origin write serialization,
  context scheduling, cancellation tree, and semantic progress;
- add two model providers through one typed adapter contract;
- test malicious pages, malicious model output, compromised tool results,
  cancellation races, budget exhaustion, and provider failure.

### Milestone 6 — Production qualification

- complete deterministic, real-site, concurrent-use, endurance, and fault
  suites;
- profile hot paths and remove unnecessary allocation, serialization, native
  views, page work, model tokens, wakeups, and copies;
- audit all native/unsafe code, isolated-world boundaries, profiles, secrets,
  logs, diagnostics, and release feature graphs;
- prove clean shutdown, recovery, migration compatibility, and no unused-agent
  overhead;
- publish a concise reproducible engineering report and investor/demo metrics;
- expose the stable ports required by Work without implementing Work UI here.

## 16. Commit and review policy

Commit the work as production software:

- one coherent contract, adapter, vertical behavior, or evidence-backed fix per
  commit;
- include its tests, generated bindings, migrations, and relevant docs in the
  same commit;
- keep platform implementations separate when that makes review and rollback
  safer, but do not split a few mechanical lines into noise commits;
- do not batch the entire context runtime, semantic engine, policy system, and
  evaluation harness into one or two commits;
- record benchmark/eval manifests without sensitive results;
- never commit credentials, profiles, screenshots, page contents, raw traces,
  or provider responses;
- keep the tree buildable and its applicable gates green at each milestone
  boundary;
- preserve unrelated work and coordinate before changing extension-owned
  seams that may be evolving concurrently.

Use an ADR only for a genuinely expensive-to-reverse change under the criteria
in `product-system.md`. An experiment result and updated subsystem spec are
normally sufficient.

## 17. Explicit non-goals for this phase

Do not build during the isolated proof:

- the Work spatial UI or design system;
- production Work persistence, tasks, notes, memory, or knowledge;
- the hosted AI service or cloud browser execution;
- mobile, collaboration, teams, or synchronization;
- arbitrary JavaScript, CSS selectors, XPath, raw DOM, or a model-facing CDP;
- full Chrome extension compatibility for owned agent contexts;
- unsafe profile-directory cloning or broad origin-storage synchronization;
- a universal CAPTCHA/2FA bypass;
- a replacement browser engine;
- polished demo behavior that bypasses the production adapter or policy path.

The probe may be visually plain. Its architecture, bounds, security, lifecycle,
tests, and evidence may not be prototype-grade.

## 18. Decisions that remain evidence-driven

The following mechanisms are intentionally not frozen before the spikes:

- platform input-backend order per control class;
- exact isolated-world injection mechanism permitted by the pinned Wry forks;
- compact snapshot encoding details;
- snapshot mutation-quiet windows and site-specific settle hints;
- default hot-context and worker budgets by hardware class;
- whether `@xyflow/svelte` or any Work UI library is suitable (outside this
  phase);
- any per-origin storage bridge beyond cookies on Windows.

Changing one of these from measured evidence does not change the product
direction. Changing an invariant, identity boundary, security guarantee, or
model-facing bounded-tool principle requires explicit review and usually an
ADR.

## 19. Primary references

Research implementation details against current pinned code and primary
sources rather than copying another browser agent's architecture:

- [Vercel agent-browser](https://github.com/vercel-labs/agent-browser) is a
  useful CDP/CLI comparison and performance reference, not Zephium's runtime.
- [WebKit `WKContentWorld`](https://developer.apple.com/documentation/webkit/wkcontentworld)
  defines the native content-world primitive used on supported macOS versions.
- [WebKit JavaScript evaluation in a frame and content world](https://developer.apple.com/documentation/webkit/wkwebview/evaluatejavascript%28_%3Ain%3Ain%3Acompletionhandler%3A%29)
  documents the public evaluation surface; WebKit source and M1 device evidence
  show that it is unsuitable for activation-sensitive execution because it
  forces a user gesture.
- [WebKit content-world-scoped script-message handlers](https://developer.apple.com/documentation/webkit/wkusercontentcontroller/add%28_%3Acontentworld%3Aname%3A%29)
  define the supported page-world-isolated observation channel used by the M1
  fixture runtime.
- [WebView2 profiles](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/multi-profile-support)
  describe profile-scoped browser data and extension behavior.
- [WebView2 cookie management](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/cookies)
  defines supported cookie transfer primitives.
- [WebView2 frame APIs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/frames)
  define platform frame observation constraints.
- [OWASP LLM prompt-injection prevention](https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html)
  summarizes the threat class; Zephium's deterministic source-to-sink policy
  remains the actual product boundary.

Primary documentation does not substitute for device tests against the exact
runtime Zephium ships.
