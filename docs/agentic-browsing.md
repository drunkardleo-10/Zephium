# Agentic browsing implementation specification

Status: accepted implementation program
Last reviewed: 2026-09-06

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

### 1.1 Current evidence boundary

The current implementation proves a substantial **bounded execution kernel**:
native owned contexts, compact semantic observations, epoch-bound references,
fixed native interaction recipes, deterministic policy admission, fresh
postcondition observations, provider accounting, audit, cancellation, and
clean teardown. Real Luna runs have exercised short exact routes on macOS.

That evidence must not be described as a general Work agent yet. The live
qualifiers still use trusted task adapters that provide route, completion,
effect, account, extraction, and acceptance facts. They prove guarded execution
of a prescribed workflow; they do not prove that a model can discover a route,
decide that evidence is sufficient, or finish an unfamiliar objective. The
current whole-context teardown is also a qualification lifetime, not the final
persistent Work lifetime.

The implementation therefore separates these three things explicitly:

- **authority:** an immutable user/product-approved capability envelope for
  accounts, origins, data classes, effects, budgets, approvals, and resources;
- **intent and progress:** a bounded, persistent, revisable plan that a model
  may propose but that never grants authority merely by existing;
- **evidence:** native or service observations with an explicit verification
  level, provenance, freshness, and uncertainty.

Rust enforces authority and evidence joins. A model chooses within the admitted
space; it does not classify its own permissions or manufacture proof of a
business effect. Trusted workflow adapters remain valuable optimizations for
known services, not the universal authoring model.

### 1.2 Immediate implementation order

The next production sequence is deliberately outcome-driven:

1. qualify rendering and input behavior in the real macOS application
   lifecycle, including honest foreground/background constraints;
2. split persistent Work-owned browser resources from revocable run-owned
   execution leases and scoped resource quarantine;
3. add the capability envelope, persistent plan/progress state, evidence
   levels, and conservative source/account sensitivity floors;
4. build one generic open-objective runner whose expected answer and route are
   hidden from the actor;
5. prove one unfamiliar multi-page Luna workflow with retained evidence and
   same-context human takeover;
6. expand from measured failures into a small heterogeneous site matrix,
   authenticated reads, reversible writes, Windows parity, concurrent
   Browse/Work, and resource/endurance qualification.

Tool breadth is not a milestone by itself. Add deterministic Rust capabilities
when successful workflows or classified failures show that they reduce turns,
tokens, latency, unsafe ambiguity, or human intervention.

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

- is created for and owned by a persistent Work browser resource;
- is not an `Item`, tab-strip entry, Today item, or normal session-restore tab;
- never appears through extension `chrome.tabs` inventory;
- is independently visible, focusable, suspendable, resumable, and destroyable;
- can be represented in Work without keeping a live page on the spatial
  surface;
- may be adopted explicitly into Browse, creating a normal tab under an
  auditable user action.

An actor or delegated child operates an owned context only through a revocable
execution lease. Pausing, completing, cancelling, or replacing a run drains and
revokes that lease without implicitly destroying the useful page. Human
takeover retains the exact context while invalidating every old agent reference
and action permit. Uncertain teardown quarantines the affected context; it does
not block unrelated Work resources unless the fault is proven engine-global.

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

Introduce an agent-browser port around a durable `ContextId`, Work-resource
owner, profile,
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

Version 1 qualification contexts use one domain-selected 1280-by-800
logical/CSS pixel viewport. The construction request derives that closed value
from the owned source; neither a model nor a page can choose an arbitrary
layout size.
Each platform adapter sets the native child bounds explicitly, retains the
expected viewport, and fails construction or later attestation if the native
frame diverges. Borrowed tabs and human sign-in handoffs retain their ordinary
presentation owner's viewport. This logical layout contract is distinct from
physical screenshot dimensions under device scale and does not replace the
named-device CPU, memory, GPU, or energy qualification.

The viewport sentence above describes the current qualification adapter. In
the product lifetime the viewport belongs to the Work browser resource and its
current presentation/rendering lease, not permanently to one actor run.

### 3.3 Independent lifecycle axes

The initial [persistent Work-resource and execution-lease boundary](agent-work-resources.md)
now expresses separate resource identity, scoped lease drain and quarantine in
the functional core. macOS now supports the narrow frozen-document/initial-read
native slice, with one actual-application witness proving that the same native
page survives two distinct execution leases under the independently qualified
foreground rendering holder. The existing common controller now has a read-only
retained-resource backing with scoped worker closure and source-bound non-durable
results, proven through deterministic private-application/loopback fixtures.
General persistent Work execution, navigation,
product presentation and human takeover remain unqualified; the existing
general workflow qualifiers still use their original run-owned/all-zero lifetime.

Do not encode browser execution as a hidden/headless Boolean. These states are
independent and policy-controlled:

- **presentation:** compact card/preview, dedicated page surface, or no current
  user presentation;
- **rendering opportunity:** whether the platform is currently allowed and able
  to advance layout, animation, hydration, and visibility-dependent content;
- **human input authority:** none, inspect-only, or exclusive human takeover;
- **agent execution authority:** the exact active lease and its admitted
  operations;
- **resource state:** live, throttled, queued, suspended, renderer-lost, or
  quarantined.

On macOS, a permanently hidden `WKWebView` is not assumed to have full rendering
liveness. If a page needs a display-backed rendering opportunity, the scheduler
must acquire an explicit bounded lease in the normal Zephium application
lifecycle. It must never activate Zephium, take key/main-window status, or
compete for input merely to keep an agent running. If the application is
inactive and supported public APIs cannot satisfy the lease, local execution
pauses or defers honestly. No private WebKit SPI or website-specific animation
shim may turn that limitation into a false success claim.

### 3.4 Promotion and takeover

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
authentication. There is no automatic reverse synchronization. The complete
native attempt, including cleanup proof after a partial write, uses one
trusted-shell absolute deadline under a fixed product ceiling. Correlation
retains that exact request/deadline window; native adapters cannot widen or
replace it.

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

The exact profile lease also binds whether the selected profile is durable or
ephemeral. A native adapter may verify that class against the authoritative
profile registry, but it may not infer a persistence class from `ProfileId` or
substitute another storage partition.

Controlled raw-profile experiments acquire an exclusive lease and require the
normal Zephium process to be closed. Production-concurrency tests do not bypass
this rule by opening the raw profile twice; they exercise the adapter through a
running Zephium shell so ordinary Browse and agent contexts share the profile
machinery as designed.

Profile locations, cookies, page content, screenshots, and extracted data are
sensitive. They are not printed, included in CI artifacts, or committed.

## 5. Page runtime and trust boundary

Each supported page receives an immutable, versioned instrumentation program
in an engine-supported isolated world. Prefer document creation when the engine
provides a world-specific registration that preserves the required isolation.
If a document-start mechanism silently widens world authority, install lazily
after commit into a freshly created, native-proven least-authority world before
the first semantic operation. Rust owns the program bytes and the exact
invocation vocabulary. The program does not accept arbitrary code, selectors,
or property paths from the model or page.

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
must not rely on private WebKit SPI. Its native state machine returns the typed
`NotReady` refusal for a loading document; it does not encode that ordinary
lifecycle state as an invariant abort.

The macOS owned-agent-view constructor additionally installs one immutable,
main-frame-only page-world compatibility shim for bounded Fill operations on
text/search inputs and textareas. This is a narrow compatibility exception,
not a general page-world execution surface: it is never installed in Browse or
borrowed tabs, has no native bridge, selectors, arbitrary code input, user
activation route, storage/network authority, or cross-document authority. The
shim's command and terminal attributes are untrusted transport hints and can
never authorize success. The isolated runtime first binds the exact target and
private input, the shim independently revalidates connected control identity,
writability, supported type, and bounded credential metadata before and after
`beforeinput`, and only a complete adjacent isolated-world snapshot proving the
exact post-value can authorize success. Once `beforeinput` has been observed,
any cancellation, mutation, ambiguous terminal, or later exception is treated
as applied-but-unverified and requires human judgment; it is never blindly
retried.

On Windows, isolated CDP worlds may be an
internal adapter mechanism, but CDP remains absent from domain and model
contracts. Do not use a named
`Page.addScriptToEvaluateOnNewDocument` world: current Blink creates that world
with universal access even when the public protocol surface does not disclose
the widening. Instead, create a new unpredictable world name for every
authorized document load with `grantUniveralAccess: false`, join its exact
root-frame isolated execution-context event and system-unique context id, and
install the fixed runtime lazily through a fixed `Runtime.callFunctionOn`
before its first invocation. Blink also caches inspector worlds by frame and
name, so reusing a name across documents is forbidden. These requirements are
grounded in the current primary
[Blink inspector implementation](https://chromium.googlesource.com/chromium/src/third_party/+/34509812c5711b92703c48b0970f587c2fd442d3/blink/renderer/core/inspector/inspector_page_agent.cc#1041).
Every use is fixed, audited, generation-bound, and covered by hostile tests.
The three parameter-free Windows control commands are constructed directly
from one immutable `"{}"` payload whose size is checked against the control
ceiling at compile time. This avoids a temporary JSON value and a fallible
constructor followed by an invariant abort; the only payload allocation is
the owned string required at the native command boundary.
The release-excluded M1 Windows fixture adapter uses only fixed
`Runtime.evaluate` expressions with `userGesture: false` for bounded fixture
observation and fixed semantic recipes. It issues only one CDP request at a
time and waits for its completion before the next because WebView2 explicitly
permits CDP methods to be processed out of dispatch order. The dispatch
boundary accepts a closed three-variant method enum, not a method string. Its
raw COM completion scans a fixed UTF-16 and UTF-8 ceiling before allocating;
the webview2-com helper's eager unbounded string conversion is not used. This
main-world fixture mechanism is not the production semantic-world
implementation and is never available for arbitrary pages, selectors, or
model input.

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

The bounded runtime preserves content-derived names when a control contains
other semantic nodes: for example, a commerce link containing a heading and a
price paragraph retains a name as well as the separately addressable children.
This uses only retained ancestry during the existing traversal, charges each
copy to the same field/global text ceilings, and preserves explicit author-name
precedence. Hidden descendants, editable/value boundaries and secret/sensitive
classification cannot acquire a new ancestor-name disclosure path. This is a
bounded semantic projection, not a claim of complete platform accessibility-name
algorithm conformance; alternate text and other unsupported naming cases still
need separate measured coverage.

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

`locate` is a bounded deterministic search over one exact acknowledged
semantic observation, not a page query. Its normalized hostile-text query is
capped at 1,024 bytes and 16 distinct terms, rejects secret-like and invisible
control text, and can search only the initial observation or an exact
region/subtree/table/observed-frame reference. Binding rejoins the committed
observation fingerprint and the complete current native frame cohort. The
matcher examines at most the already-bounded observation, withholds secret
nodes before matching, retains at most 32 ranked results (eight normally), and
returns only opaque references, closed roles/match classes, sensitivity/trust,
actionability, and truthful truncation counts. It exposes no selector, raw
document surface, script, regex, native handle, or new action authority.

The result is encoded as deterministic `ZLOC1` content without repeating the
query or any matched page string. Encoding is capped at 8 KiB and 2,048 exact
tokens. Only an exact prior tool-only `locate` stop may bind it to a strictly
newer same-plan call. OpenAI and Anthropic receive their matching fixed
function/tool result through the bounded stateless transcript; the complete
immutable body must pass the pinned local whole-input counter before policy
reservation, or use the OpenAI-only provider-exact path described below. Policy
rejoins the exact committed observation cohort and proves
every returned reference was already disclosed. Committing delivery therefore
adds no origin, account, sensitivity, trust, or reference taint; it returns a
content-free receipt and retains the same observation acknowledgement for a
later bounded turn. Refusal or pre-commit cancellation releases the reservation.
`read` result continuation follows the same one-shot correlation rule but may
carry bounded page strings. A read result records the exact full-observation
fingerprint in addition to its context, generation, capture time, provenance,
omissions, and content guard. Only the matching prior tool-only `read` stop may
bind its token-admitted `ZREAD2` bytes to a newer same-plan call. The full
OpenAI or Anthropic replay must retain the exact semantic-delivery revision.
The exact-local path requires an exact (`ExactLocal` or authenticated
`ProviderExact`) latest-result measurement and an `ExactLocal` whole-input
count before policy mutation. The OpenAI-only provider-exact path instead
reserves the already-serialized complete replay by its UTF-8 byte length before
the authenticated count request; an Anthropic provider estimate is never
accepted as exact admission. Policy proves the exact committed baseline cohort
and every returned reference/origin, then reuses that cohort unchanged. This
admits truthful empty reads, preserves the conservative prior
sensitivity and trust, and adds no origin, account, reference, or taint-cohort
growth. Its delivery receipt remains deliberately weaker than an observation
acknowledgement; the move-only bound request carries the prior acknowledgement
separately so a successful read result can retain stateless continuation
without allowing a standalone read to manufacture diff or expansion authority.
The current tool-continuation seam accepts only `scope: initial` projected from
that already-acknowledged initial observation. The original requested scope is
retained in the sealed correlation and checked independently; an expansion
request cannot be answered by baseline data or coincident ref ordinals. A
nonterminal expanded observation needs a separate delivery/host contract rather
than silently rebasing through a read receipt. Refusal and cancellation release
the reservation. An explicit frozen `with_baseline_read` provider configuration
adds only this initial-scope tool to the restricted capability profiles; no
other profile capability or continuation budget changes.

`extract` now consumes that bounded read through a distinct, terminal mapping
turn. Only the exact prior tool-only `extract` correlation selecting the same
trusted schema ID may bind a strictly newer same-plan call. Deterministic
`ZEXTRACT1` input carries trusted, closed schema field declarations followed by
the exact hostile `ZREAD2` evidence; one guard binds the full schema definition,
read, observation fingerprint, context, generation, and capture time. The
complete immutable replay must either receive an `ExactLocal` pinned
provider/model/tokenizer count, or enter the OpenAI-only conservative-reserve
then authenticated-exact-count path, before model generation. Policy rejoins
the unchanged committed baseline taint without adding origins, references,
authority, or a new observation acknowledgement.

A trusted extraction schema may carry a nonempty, duplicate-free closed set of
semantic source roles. Its default is all roles. Selection removes readable
fields only after the same bounded native capture and sensitivity/secret
checks; it is not a DOM selector, capture expansion, native call, or permission.
Nondefault `ZREAD2` headers name the canonical `selected_roles`; excluded
otherwise-readable fields report `role_selection`, separately from
`source_incomplete` and privacy/byte/item omissions. The added metadata consumes
the same combined encoding budget. Both schema and read guards bind the exact
selection even when two role sets happen to produce identical fragments.
Encoding and output admission reject mismatched schema/read selections; only
fragments in the exact delivered projection can be cited. Default all-role
model bytes and baseline-read behavior are unchanged.

The mapping call exposes no browser tools. OpenAI uses strict Responses
`text.format` JSON Schema and Anthropic uses stable Messages
`output_config.format`, following their current
[structured-output](https://developers.openai.com/api/docs/guides/structured-outputs)
[contracts](https://platform.claude.com/docs/en/build-with-claude/structured-outputs).
Both receive one fixed universal envelope; product schema field names, page
content, and values remain only in bounded request message content. This avoids
turning Anthropic's documented 24-hour compiled-schema cache into a page- or
workspace-schema store. The Anthropic projection removes its unsupported
numeric, length, and cardinality constraints, while the original Rust schema
remains authoritative as its guidance requires.

A purpose-bound collector retains at most 64 KiB from the exact provider call,
poisons and clears itself on a wrong-call or tool event, and admits output only
after an exact `Completed` terminal with matching text-byte accounting and no
tool output. Rust then reapplies schema identity and definition, ordered field
semantics, value/aggregate bounds, secret screening, sensitivity, and exact
`@rN` provenance. Refusal, incomplete output, cancellation, byte mismatch, or
schema/read substitution fails closed. Extraction proof is content-free and
terminal: it cannot seed a diff, another browser-tool turn, or browser
authority. No live provider request was made for this implementation evidence.

The diff path mechanically rejects locate, read, extract, and screenshot
correlations, so none of those result classes can be substituted with a page
delta.

After an action, return a semantic diff against the last acknowledged snapshot:

- added, removed, changed, and moved semantic nodes;
- focus, URL, title, dialog, document, and loading changes;
- the evidence used to verify the intended effect;
- a bounded amount of surrounding context.

Diff admission is valid only when that exact baseline was already committed to
model delivery. The model payload and terminal delivery proof bind the exact
baseline/current fingerprint pair, not merely their public observation and
generation coordinates. The policy derives current opaque-reference authority
by retiring old references and installing add/rebase references over the exact
committed baseline; it never accepts a caller-supplied replacement inventory.

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

The complete native capability registry may be richer than the tools visible
in one model turn. A deterministic router selects a small profile from the
objective phase, admitted envelope, current resource state, and prior typed
failure. The model does not receive fifty schemas merely because Zephium can
perform fifty operations. Tool discovery is bounded and cannot grant a
capability absent from the original envelope.

Prefer typed compound operations and Rust-selected execution recipes over
making the model coordinate incidental mechanics. For example, a form action
normally states its target, value, and desired observable outcome; Rust derives
the compatible input recipe, settle strategy, and mandatory verification from
the control semantics and policy. Explicit model-proposed waits or effect
claims are hints to validate, not authority. Add specialized table, filter,
sort, search, or service operations only when evidence shows that they improve
verified outcomes versus the generic semantic path.

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
hold a run indefinitely. The functional core exposes the earliest exact wake:
the current quiet boundary for mutation-quiet waits and the absolute deadline
for every other wait. The shell owns one cancel-or-replace timer per admitted
action; it does not poll.

The native-to-settle transition consumes the complete non-cloneable execution
outcome and rechecks it against the exact prepared action. Only an `Applied`
terminal can mint a settlement tracker, using the trusted native completion
time as the settle start. A typed execution failure, contract violation, action
substitution, or deadline overflow returns the retained policy authority and
creates no tracker, timer, worker, or retry path.

Shipping code cannot split that authority through the raw preparation helper
or destructure an execution outcome, execution-admission refusal,
native-to-settlement refusal, settlement terminal, or settlement-admission
refusal. Those escape hatches are crate-private. The bounded execution and
settlement coordinators are the only public admission owners, and run policy
consumes each complete refusal directly. The refusal itself selects the closed
action failure: exact native failures remain exact, missing fresh geometry maps
to target change, capacity/context contention maps to resource exhaustion, and
contract, shutdown, replay, or internal routing violations fail as backend
refusal. A caller therefore cannot relabel one failed attempt before charging
or batch terminalization. Pre-verification refusal charging retains no page
evidence, creates no retry authority, and allocates no evidence box.

Pending settlement is retained by one zero-idle single-owner coordinator. Its
empty vector allocates nothing; it admits at most four applied actions and one
per logical context. Each accepted fact or borrowed snapshot consumes a
move-only routing reservation and returns either one replacement reservation
with the exact next wake or the complete terminal authority for verification.
A premature wake, replaced schedule, wrong attempt, malformed snapshot, or
cross-request reservation cannot release retained debt. Shutdown seals new
pending admission while preserving exact terminal drain. This is bounded timer
planning only: it creates no timer, callback, task, native object, snapshot
copy, or retry loop. Pending state exposes neither mutable tracker access nor a
public destructuring path; only the coordinator-minted terminal owner releases
the tracker and policy authority for independent verification or typed failure.

Independent verification consumes that terminal owner and one borrowed
evidence value exactly once. The raw tracker verifier is crate-private. Success
keeps the exact policy authority, execution attribution, terminal settlement,
and opaque proof together; refusal destroys the proof opportunity and returns
the same authority plus the trusted monotonic observation instant and a closed
verification error for one failed policy settlement. That content-free clock
flows through batch terminalization so qualification can measure failed proof
attempts without inventing a completion time. Neither path retains borrowed
evidence or creates a verification retry.

Snapshot-derived evidence crosses sibling-crate composition only through the
agentic core's opaque preparation helper. Exact form values used to verify a
fill remain borrowed behind that envelope; their constructor is crate-private.
Navigation, dialog, and scroll contracts return a distinct content-free
`NonSnapshotEvidenceRequired` refusal so the controller must obtain the
appropriate independently sampled native evidence rather than misclassifying
the request as an action-policy denial or substituting a semantic snapshot.

Policy accounting then consumes the joined verified or refused terminal rather
than accepting separately supplied authority and proof. A verified charge
returns the immutable effect receipt still joined to execution attribution,
terminal settlement, and the opaque proof needed by result finalization. A
verification refusal maps exactly once to the closed action-failure taxonomy
and returns the failed receipt with content-free timing/error state. The raw
verified-effect settlement method is crate-private; pre-verification native or
settlement failures retain their distinct typed failure path.

Action-result finalization consumes the policy-accounted verified owner, not a
loose proof. It validates the action, acknowledged baseline, proof observation,
current context, and monotonic capture bounds while the exact current
observation is still borrowed. Only after every join passes does it consume the
owner and compute a bounded diff or fresh-snapshot fallback. Refusal returns
both the charged proof owner and the complete current observation, so an
authority mistake cannot discard post-action state. The loose finalizer exists
only under `cfg(test)`.

Sequential batch aggregation consumes that accounted result directly; a loose
`SemanticActionResult` is not a shipping admission surface. Before consuming
the owner it rechecks exact batch order, action guard, receipt effect, attempt,
proof, terminal settlement, and the execution-to-settlement clock join. Every
refusal returns the complete accounted result without mutating the batch.
Successful admission retains only the latest bounded state plus at most eight
content-free summaries containing the immutable receipt, native attribution,
and settlement counters/timing. Each summary has a compile-time 512-byte
ceiling, so the entire fixed completion inventory is at most 4 KiB. This adds
no idle work and no unbounded state.

A failed batch terminal likewise consumes a non-cloneable policy-accounted
failure bound to the exact prepared next action; callers cannot terminate a
shipping batch from a loose failure enum. The immutable effect receipt carries
the private action guard, and admission rejoins it with batch order, effect,
attempt uniqueness, and the exact failed settlement before consuming the
owner. A pre-verification failure allocates no evidence box. A verification
refusal retains one bounded box only until terminalization, where its native
attribution, terminal settlement counters/timing, and closed verification
error become one content-free summary under a compile-time 512-byte ceiling.
Any refusal returns the unchanged batch execution and the exact failed owner,
so an authority mismatch cannot discard either. The terminal discards prior
page state because an unverified attempt may have changed the page, adds no
retry surface, and adds no idle work.

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

Qualification rejoins each claimed effect to a case-specific event on the
exact intended fixture target. A Windows HWND or diagnostics-only CDP route
can qualify only when that exact effect event is browser-reported as trusted;
an aggregate trusted-event count or an unrelated trusted event cannot promote
the route. The serialized target-focus bit must equal the retained target focus
event before it can refine the coarse native focus owner. A backend classified
as non-dispatching must retain no event, target, focus, activation, navigation,
popup, or clipboard effect. Popup and clipboard results additionally rejoin
their dedicated native request and closed gate fields; a generic `Unsupported`
label cannot stand in for evidence that the intended control was reached. A
qualified Windows row also requires exactly one Browser-kind process plus a
joint nonzero bounded derived helper-process count and aggregate resident-
working-set sample before and after the action. The adapter obtains each count
from one Environment8 process snapshot, rejects zero, duplicate, or more than
64 total process IDs, opens only query-limited non-inheritable handles, closes
each through an owned guard, and checked-sums `WorkingSetSize` under a 1 TiB
evidence ceiling. It holds those handles while re-reading and exactly rejoining
the sorted Environment8 PID/kind, total-count, and helper-count cohort, closing the
process-exit/PID-reuse and kind-substitution gaps. `GetProcessInfos` excludes
crashpad, so the resident sum covers the reported API cohort and is not a
whole-process-family or resource-budget measurement. Any unavailable, changed,
or partial sample rejects the row; the content-free review aggregate retains
both maxima without emitting process IDs. The required raw Win32 bindings are
an exact-pinned optional dependency activated only by the release-excluded
input or semantic probe features. The same sampler qualifies the seven-mode
Windows semantic-runtime harness before mode-specific work and after semantic
work drains; protocol and review schema v5 require both observations and expose
only process-count and resident-byte maxima. Until those seven records are
captured and reviewed on physical Windows, this is cross-compile and producer-
integrity evidence rather than a device behavior or resource-budget claim.
The checked-in physical workflow additionally requires an explicitly
acknowledged authorized Windows device, a clean exact Git revision, an empty
fixed ignored directory, the x86-64 MSVC host toolchain, create-new records,
and source continuity through the separate debugger/review phase. Every Cargo
operation in both phases is offline; dependencies must be acquired before
collection authority begins. Before the manual handoff it resolves Cargo's
active target directory, hashes the direct semantic-probe executable before
collection, requires the final build to retain that digest, and records it in
a create-new SHA-256 stamp. Review rehashes that same direct file before
accepting the debugger record. It cannot
invoke visible-focused input or launch the debugger-only case; those
authorities remain outside the script.

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

The source/account establishes a conservative sensitivity floor before
field-level classification. Authenticated mail, documents, dashboards, and
account pages are not treated as public merely because a field lacks a known
`autocomplete` token. Field detection may raise sensitivity; it cannot lower
the source floor. Unknown authenticated sources default to private/sensitive
until a trusted adapter proves a narrower class.

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
DOM, secret values, or indiscriminate page contents. Each current-progress
snapshot is accepted only from the exact supervisor incarnation and canonical
manifest revision originally joined to the ledger; matching public identities
cannot substitute a different scope revision.

Evidence records distinguish at least:

- **native-observed:** a fresh bounded browser observation established the
  stated page fact;
- **service-confirmed:** a trusted API or adapter independently established the
  remote effect or durable state;
- **model-interpreted:** a model mapped cited source evidence into a claim;
- **user-accepted:** a person explicitly accepted or supplied the fact;
- **needs-review:** provenance exists but the requested conclusion or effect is
  not established strongly enough.

Citation provenance does not turn interpretation into fact, and a page saying
“saved” does not prove a remote write. Product UI and completion logic retain
these distinctions instead of flattening them into one success Boolean.

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

Input accounting is an explicit immutable call mode rather than an inferred
provider default. `ExactLocal` requires pinned exact (`ExactLocal` or
authenticated `ProviderExact`) semantic/objective measurements and an
`ExactLocal` complete structured-request measurement before dispatch. The OpenAI Responses
mode `ProviderExactAfterConservativeReservation` first encodes the immutable
request, records the newest semantic projection as `Conservative`, and reserves
the complete serialized request's UTF-8 byte length as a conservative input
ceiling. Policy commits that already-authorized disclosure before transport
sends the canonical token-relevant projection to `/v1/responses/input_tokens`.
Only an authenticated `ProviderExact` count bound to the exact call, request
digest, projection, counting-contract revision, pricing range, and original
reservation can unlock generation. A count above the reservation, malformed or
failed count response, cancellation, or protocol mismatch cannot dispatch the
model request and settles exact-zero generation usage; the conservative policy
ceiling remains held until terminal settlement. Provider-exact standard-rate
calls are capped at 272,000 input tokens so a future long-context pricing tier
cannot be charged under the wrong schedule. Anthropic remains exact-local until
it offers a qualifying exact authenticated contract.

The BYOK HTTPS shell uses private exact provider endpoints, provider-bound
zeroizing credential owners, redirect refusal, identity-only response content
encoding, bounded deadlines, and at most four simultaneous attempts. The only
non-production endpoint seam is diagnostic: it exists only when provider
transport is combined with the release-forbidden `probe-harness` feature, and
optimized builds refuse that feature graph. It accepts only explicit nonzero-
port `http://127.0.0.1` source strings whose serialization is already
canonical, whose path contains only nonempty ASCII-unreserved segments, and
which have no percent escapes, dot segments, duplicate/trailing slashes, user
information, query, or fragment. Validation happens before URL parsing can
erase an ambiguous source spelling. The OpenAI input-count path is derived
from the admitted Responses path, so a probe cannot redirect counting to
separate authority.
The agentic `probe-harness` feature remains dependency-free and does not enable
provider transport by itself; the optional transport façade provides the
explicit combined diagnostic feature. The library-owned authentication
`HeaderValue` is constructed only at dispatch and
marked sensitive. Pinned `http` copies that value into ordinary `Bytes` and
does not zeroize it on drop, so the zeroization guarantee applies to
Zephium-owned credential and construction buffers, not unavoidable HTTP-stack
or wire copies. An already-sticky run or shutdown cancellation is checked
before that header or request is materialized, then checked again after request
construction and immediately before the send future can be polled. Admission
acquires a transport slot before semantic input commits; after commit, send
errors and cancellation are terminal outcomes rather than proof that no
provider work occurred. The final run-cancellation and transport-shutdown
checks are linearized with the synchronous semantic-disclosure commit: each
cancellation authority owns a zero-worker commit gate, cancellation takes that
gate before becoming sticky, and admission holds both gates only across policy
commit and transport-slot commitment. A cancellation that wins cannot race a
later disclosure; a commit that wins is already post-commit authority and must
be terminally settled. A poisoned gate becomes sticky cancellation in
unwind-capable builds. HTTP/2 ingress uses explicit protocol-standard
65,535-byte initial stream and connection receive windows with adaptive growth
disabled, plus a 16,384-byte maximum frame. These are flow-control and frame
bounds, not a claim about TLS, socket, or whole-process allocation. The HTTP
client applies a 64 KiB HTTP/2 header-list
limit before decode and rechecks the decoded field list with checked
name/value/per-field-overhead accounting before status, retry-hint,
content-type, or body processing. That second protocol-neutral check also
covers an HTTP/1 fallback, whose receive-buffer ceiling is not exposed by the
pinned reqwest builder; pinned Hyper can allocate up to its larger internal
HTTP/1 receive ceiling before the decoded check. Before streaming a successful
response, the transport
also rejects a duplicate, non-canonical, or over-budget `Content-Length`;
absence remains valid for SSE, and incremental decoding still enforces the
same exact per-call wire ceiling. The HTTP client disables even library-default
protocol-NACK retries; any future retry requires a new call identity and policy
admission. A committed attempt proven not to have polled the HTTP send future
settles exact zero provider usage and cost while retaining its already-committed
semantic taint. Once send may have been polled, a terminal result without
trustworthy provider usage charges the complete token and cost reservation and
records that charge as a conservative ceiling. Provider-reported usage retains
the exact fixed provider, model, tokenizer revision, and billing class in a
move-only pricing requirement. The request also binds a nonzero trusted pricing
revision and inclusive input-price range before disclosure. OpenAI requests
require the default service tier; Anthropic requests require standard-only
capacity and global inference. The stream decoder must attest those actual
classes before provider usage can reach pricing. Policy authority cannot be
extracted or a completed call settled until the matching trusted pricing
schedule supplies cost.

The normalized stream-batch consumer is an external imperative boundary. In
unwind-capable development and evaluation builds, a consumer panic is caught,
the shared transport is permanently sealed and cancelled, and the attempt
returns its move-only policy authority as a non-retryable integration failure
with conservative post-dispatch accounting. This prevents one callback panic
from stranding the model reservation or escaping a provider task. Optimized
desktop builds retain `panic = "abort"`, so no recoverable release-panic claim
is made.

An idle provider-transport snapshot is observational only because admission
may still reopen it. Process shutdown first applies the transport's sticky
cancellation/admission seal, settles every retained attempt and its separate
policy authority, and then requests a constructor-closed shutdown proof from
an exact sealed, zero-active snapshot. The nonblocking proof remains valid
across shared transport clones because no operation can clear the seal; it owns
no task, timer, provider content, credential, or settlement authority and does
not substitute for provider usage, policy, audit, or native-resource drain.
The reusable wait seam applies that seal synchronously before returning its
future, registers its drain notification before every state sample, and waits
only for the exact last slot or the caller's absolute monotonic deadline.
Dropping the unpolled or pending future therefore leaves admission closed, and
releases its sole bounded wait slot for a corrected retry. A concurrent second
wait is refused, and no polling worker or periodic timer exists when shutdown
is not being driven.

The fixed OpenAI and Anthropic builders accept token-admitted full-observation
or bounded-read types. Committed admission retains a cloneable content-free
proof beside move-only usage authority: a full observation can seed later diff
computation, while a standalone read receipt cannot. A bound `read` result may
retain only the exact earlier acknowledgement already present in its
move-only transcript; the receipt itself still cannot mint or expose one. If
the shell loses this optional proof it must request a fresh snapshot; it cannot
recover by retaining or reconstructing semantic strings or request bytes.

Extraction is not sent through either ordinary observation/read request or the
generic diff continuation. Its exact `extract` tool result is replayed into a
separate tool-free constrained-output request using the fixed universal
extraction envelope above. The response binding is returned beside the
move-only transport input, starts retention only after exact input commitment,
and is destroyed on refusal/cancellation. A committed extraction receipt can
validate only that exact schema/read mapping output and is never eligible for a
continuation seed.

A compact diff is never sent as a standalone stateless provider turn because
the model would not have the acknowledged baseline it modifies. Provider diff
delivery requires a separate bounded continuation/tool-result contract that
retains exact prior-turn correlation without logging or persisting provider
responses. The provider-neutral seam shares the already-admitted objective
allocation and moves the initial compact semantic allocation into a private,
move-only structured transcript; it never retains a raw request or response.
Seed eligibility is limited to the normal 32 KiB initial-observation envelope,
and the transcript has a 256 KiB / eight completed-turn ceiling before the
adapter must rebase with a fresh full observation. Beside that bounded content,
it retains exact committed baseline, provider/model/tokenizer/pricing
configuration, model-call lineage, and terminal tool correlation. Failures,
cancellation, non-tool stops, mixed text/reasoning output, and multi-tool stops
destroy it. It can bind only a strictly newer same-plan call and the exact
token-admitted diff extending that baseline. Binding first reserves the one
eventual vector slot, then holds the newly completed turn in a structurally
nonempty carrier beside the prior transcript. Provider encoders therefore
receive an infallible latest correlation without indexing or a release-aborting
invariant assertion; after serialization, the carrier moves that turn into the
already-reserved slot without copying content or allocating. Fixed
provider-specific stateless codecs turn that bound value into a move-only
draft: OpenAI replays the
two original user inputs, each exact tool-only `function_call` output item, and
its matching `function_call_output` under `store: false`; Anthropic replays the
original user message followed by adjacent assistant `tool_use` and user
`tool_result` messages with matching ids. Both retain the immutable
instructions, closed tools, disabled parallel calls, model/output bounds,
service class, and streaming mode. They accept no generic payload, remote
conversation id, metadata, reasoning/thinking block, selector, DOM, or script.
The draft retains the bounded structured transcript, exact diff-delivery
authority, and serialized-body byte accounting without initially holding policy
or transport-commit authority. A synchronous provider-specific local counter
may inspect only that exact fixed body and must return an `ExactLocal`
measurement for the pinned model/tokenizer. JSON-byte tokenization is not a
valid implementation. Alternatively, an OpenAI provider-exact call reserves
the complete already-serialized body by UTF-8 byte length, commits the same
policy-authorized disclosure, and requires the exact bound count before
generation. `ProviderEstimate` is rejected, and Anthropic has no provider-exact
path. Both modes validate the semantic delivery revision, fixed output bound,
pricing range, exact diff, baseline taint, account/context, call, manifest,
lease, and node before transport can progress. The resulting prepared diff uses
the same bounded transport as full observations and reads.

Diff disclosure authority is split from the compact content before a provider
request can retain it. The stateless draft keeps the redacted, content-free
authority inseparable from the exact bounded transcript and body. Exact local
counting grants no disclosure authority; successful policy admission joins the
same authority to transport. Refusal or cancellation drops it, and only an
exact transport commit mints the diff receipt, advances policy taint, and makes
the current acknowledgement plus bounded transcript eligible for one later
tool-only continuation. Provider-exact counting changes accounting quality, not
semantic authority or continuation eligibility.

Viewport screenshots use a separate one-shot visual continuation. Only an
exact prior `screenshot` tool-only stop can bind the canonical PNG, and it must
retain the same committed observation/generation/context, provider
configuration, manifest, lease, and plan node while advancing to a strictly
newer model call. A private delivery digest covers the exact source
fingerprint, complete context join, capture time, validated dimensions/metrics,
and every canonical PNG byte. OpenAI receives the matching
`function_call_output` with one high-detail PNG data URL; Anthropic receives the
matching adjacent `tool_result` with one base64 PNG block and
`transformations.oversized_image:error`, preventing silent coordinate-changing
resize. These are fixed projections of the current
[OpenAI Responses API](https://developers.openai.com/api/reference/resources/responses/methods/create)
and [Anthropic image contract](https://platform.claude.com/docs/en/build-with-claude/vision-coordinates),
not generic multimodal or computer-use bridges.

The model-facing visual seam caps canonical PNG at 1,300,000 bytes and retained
private text at 64 KiB inside the existing 2 MiB serialized request ceiling.
Base64 is allocated once at checked exact capacity. The complete immutable
multimodal body must either receive an `ExactLocal` count from the pinned
provider/model/tokenizer counter or, for OpenAI only, fit the conservative
whole-body reservation and authenticated provider-exact count sequence. The
standard 272,000-input ceiling rejects oversized provider-exact image requests
before dispatch rather than silently selecting long-context pricing. The fixed
model instruction treats pixels as hostile page data and grants them no opaque
reference or action authority. On exact transport commitment, every observed
frame origin gains conservative `Sensitive` / `UntrustedPage` taint with zero
references. Refusal or pre-commit cancellation releases the reservation, while
a commit returns only a content-free receipt and destroys image replay
eligibility. Production remains disabled until the reviewed visual counters,
model limits, provider data handling, and live qualification exist. In
particular, OpenAI's
[data controls](https://platform.openai.com/docs/models/default-usage-policies-by-endpoint#image-and-file-inputs)
document image scanning and a flagged-content retention exception even under
Zero Data Retention or Modified Abuse Monitoring.

This is intentionally stricter than assuming an errored `send` transmitted no
bytes.

Trusted schedules price uncached input, cached-read input, cache-write input,
and inclusive output as four disjoint categories in integer micro-USD per
million tokens. They use checked arithmetic and one upward rounding after the
sum. Reasoning stays inside inclusive output. A catalog identity, range, or
arithmetic refusal returns the move-only settlement without invoking policy;
there is no raw-cost settlement escape. Successful catalog pricing is recorded
as a `PricedCeiling`, distinct from exact zero and from charging the complete
reservation when all usage is unknowable. Production model/rate entries still
require an explicit reviewed product catalog.

The supervisor records concise semantic progress—responsibility, active
resource, operation class, state, result, and blocker—not hidden model
reasoning or raw tool chatter. Active model calls, model receipts, effect
permits, active effects, effect receipts, and human transitions retain a
private canonical manifest-revision guard. Progress accepts them only when
both the public manifest identity and that guard match the supervisor
topology; reusing an identity for different scope, budget, or lifetime facts
cannot contaminate the progress projection or its downstream semantic audit.

Run accounting is an explicitly constructed, run-local functional reducer,
not application telemetry. It accepts only exact policy-derived model/effect
receipts for the queued supervisor's canonical manifest revision, rejects
replay while allowing concurrent out-of-order settlement, and independently
rechecks run and node operation/token/cost budgets. It exposes content-free
totals by opaque plan responsibility, closed effect/proof/failure class, usage
accounting class, and at most eight exact pricing-schedule digests. It retains
no site, origin, model/tokenizer label, rate table, prompt, response, page data,
credential, or timing source and owns no port, persistence, thread, task, or
clock. If the reducer is not constructed, it has zero runtime state or work.

Run progress measurement is a separate explicitly constructed streaming
functional reducer. It accepts only canonical semantic audit events sealed to
the exact manifest revision and supervisor, rejects event/time replay and
invalid node or operation sequences without partial mutation, and refuses a
supervisor terminal while that node still has an observed active model call or
effect instead of silently discarding the open duration. Its transactional
vector mutations carry the already-validated operation row, and cancellation
finalization carries the matched cancelling variant; neither path re-matches a
mutable projection through a process-terminating invariant branch. The reducer
retains at most the manifest topology, four active model calls, four active
effects, and one cancellation identity per topology node. It derives only
observed initial queue, model, effect, `NeedsHuman`, and root queued-to-terminal
durations from the audit event's trusted monotonic timestamp, plus closed
human-pause counts, distinct takeover cancellations, and the root outcome.
Unobserved and still open durations are `None`, never synthetic zeroes. It owns
no clock, telemetry or persistence port, task, worker, channel, browser
context, or native resource; if it is not constructed, it has zero runtime
state or work.

Work also supports one task-authored exact same-origin document hop followed
by a cited initial-scope extraction. Admission freezes the destination and
schema, excludes actions/history/subtree/baseline-read combinations, and
requires an independently observed departure predicate before the model's
exact `navigate` proposal is eligible. A distinct policy permit reserves one
operation; the registry revokes the old document refs before native dispatch.
The native exact-target gate admits no redirects, including a same-URL
redirect, and the policy accepts only its original next-document operation
and exact committed destination. Failure, callback loss, or audit ambiguity
cannot become task success or silently release the original owner.
Product proposal publication happens before navigation policy/registry
mutation. The original policy and native-operation owners are then stored
before dispatch; the active audit projection is recorded synchronously after
the port's dispatch decision and before processing any callback. This is not
a durable pre-dispatch audit guarantee. A fallible audit record therefore
retains a real callback obligation or an explicit refusal, not a never-issued
navigation. Once policy accounts an exact terminal, a later journal refusal
retains that receipt as audit debt without pretending a native reservation is
still pending; it grants neither further provider work nor clean closure.
An explicit synchronous refusal is retained with its original operation before
any fallible audit or settlement clock read. Cleanup can account that same
known refusal once time is valid without reminting authority or dispatching
again. Unavailable or regressed time retains the refusal and policy reservation;
it cannot become a successful navigation or a clean audit/time claim.

After a commit, Work requires a fresh successor initial observation, an
independent arrival predicate, and a newly sampled same-account attestation
whose exact context and observation time join the native terminal. The old
provider transcript and tool correlations are retired; only the already
admitted trusted objective continues with the new bounded observation. The
original policy, taints, model-call and operation counters, deadline,
credential/transport, audit, and lifecycle owners remain unchanged. Navigation
does not reset a budget or manufacture an action/effect sample. Its separate
metric receipt joins both original policy accounting and canonical progress
through an opaque full-authority fingerprint; audit records retain their
existing fixed 128-byte size. Completion still requires exact destination
citations, task acceptance, and the usual durable/native/provider closure.
This first slice is not redirect, cross-origin, authenticated-session, SPA
history, multi-source synthesis, or unrestricted multi-page qualification.

Work also admits an optional finite same-origin route on a
plan node: one exact departure and at most two distinct ordered destinations.
This is immutable manifest authority, not an increased navigation counter.
All URL bytes, their order, and the owning node extend the manifest fingerprint;
nodes without a route retain the historical fingerprint and one-hop limit.
The second permit requires the first exact committed receipt under the same
lease, node, document successor and account, with a successor-time account
sample. A cancelled or failed checkpoint cannot be retried or skipped. Two
fixed receipt/progress slots preserve exact ordered audit/accounting closure;
the audit record remains 128 bytes and original run ceilings do not change.
Controller admission binds the task's entire route to the selected manifest
node and its departure to the actual initial context target; semantic
observations do not attest a complete source URL. The legacy one-hop target
and an explicit route are mutually exclusive. Each fresh intermediate
observation must independently satisfy the trusted task's arrival and
departure predicates before the next fixed destination becomes eligible.
Only the final checkpoint admits terminal extraction. The controller retires
each prior transcript, refreshes account authority and revokes old refs on
each hop under the same session and original model-call/operation/deadline
owners. Before navigation it reserves room within the unchanged eight-call
ceiling for all remaining route proposals plus terminal extraction/mapping.
Deterministic three-document fixtures cover ordered admission, independent
phase refusal, old-document replay retirement, second-hop native/account/
audit/clock/backpressure debt and exact original terminal closure. A real
three-document Luna witness remains separate; this contract alone makes no
additional live-site claim.

Explicit routes also supply one bounded, host-authored provider checkpoint:
version, completed/total hops and the exact next already-approved destination,
or null after the final commit. The policy derives it from the immutable node
route and its ordered committed native receipts, not task counters, model
memory or page text. A private binding retains the manifest revision, lease,
node, document, account scope and exact terminal identities. Every supported
same-document continuation revalidates that binding before new input
reservation. A fresh account sample may change its identity/time but cannot
substitute a document, scope or receipt prefix.

On the current whole-request-counted OpenAI path the checkpoint is a separate
developer message; the original objective and untrusted semantic observation
remain separate user items. Locate and extraction replay retain only the
current checkpoint. Navigation retires it with the old transcript, then
rebuilds it after the independent arrival predicate and fresh account sample.
The checkpoint is descriptive guidance, not a permit, task-success proof or
citable page evidence. Exact next-target equality, all task phases and policy
guards remain mandatory even when a model ignores that guidance. The serialized
message is included in the existing request digest, exact count projection and
transcript/token ceilings, and rejects existing secret-shaped value patterns
before reservation. No default/no-route wire bytes or ceilings change. Legacy
fixed-count observation/read builders and screenshot paths refuse explicit
routes; Anthropic also refuses routed continuation encoding until an equivalent
trusted, completely accounted adapter is implemented. This is not a claim of
local-model or second-provider route qualification.

The release-excluded `react-route` qualifier freezes Quick Start → Your First
Component → Importing and Exporting Components in both its manifest and task.
The middle page requires separate exact public headings for arrival (`Your
First Component`) and departure (`Components: UI building blocks`); a heading
on a menu link, duplicate, missing predicate, old observation, or skipped
document cannot advance it. Terminal extraction requires one exact current
`Importing and Exporting Components` heading citation. It uses only initial
observations and the unchanged heading-only extraction projection, takes at
most three cached anonymous-scope samples in the fresh isolated ephemeral
profile, and admits no actions/read/subtree/redirect/cross-origin/auth claims.
Its deterministic guards and build are distinct from a separately recorded
live Luna outcome; the mere presence of this qualifier is not qualification.

The release-excluded `commerce-product` qualifier reuses an explicit one-hop
manifest/task route from the Vercel demo catalog to one frozen product URL.
Departure requires a unique exact public price-bearing product Link accessible
name. Arrival requires three distinct unique current sources: product Heading
accessible name, displayed-price Paragraph visible text, and material-description
Paragraph visible text. All three values are task-authored source-pinned
constants, not model-inferred acceptance criteria. Both checkpoints require a
complete bounded initial snapshot with no frame boundary; no subtree, role
fallback, recapture, ceiling increase, action, redirect or cross-origin scope is
admitted. Heading/Paragraph extraction selection is immutable at admission and
still reports intentional role omission separately from source incompleteness.
Each returned field must exactly copy its one independently verified arrival
reference in the same current document and isolated anonymous account sample.
The owned-result check repeats exact field/value/role/public-source checks; it
does not replace the current-document task gate. This tests a commercial
multi-field read, not availability, variant selection, cart, checkout or
cross-document synthesis. Public-source inspection and deterministic tests are
not native/provider evidence; the ledger records those boundaries separately.

The release-excluded `react-navigation` public qualifier fixes Quick Start as
departure and `/learn/your-first-component` as the only successor, then requires
one exact `Your First Component` heading citation. It runs in the existing
new ephemeral profile, takes at most one cached anonymous-scope sample per
document, and makes no login-detection or authenticated-session claim. Its
unit guards are deterministic; any actual Luna outcome is recorded separately
in the M6 qualification ledger, not implied by the qualifier's existence.

The default semantic core does not convert validated internal identity or
static operation assumptions back through process-terminating constructors.
Opaque node keys move their existing nonzero representation directly into the
native target recipe; operation allowlists propagate a typed decode refusal if
their construction is ever invalid. The release source gate mutation-tests
both carries and the structurally nonempty provider continuation above. This
narrows avoidable exposure to the workspace `panic = "abort"` policy without
claiming that allocation failure or defects outside these guarded seams are
recoverable.

Production builds of the agentic functional core, provider transport, and all
sixteen dedicated engine agentic modules additionally deny direct Clippy
`unwrap`, `panic`, and `unreachable` findings. The release source gate pins the
complete current module inventory and mutation-tests both the lint contract
and the fixed Windows control-command construction. This is a narrow rule
against explicit invariant-abort mechanisms in owned code; it is not proof
against allocation failure, bounds-check panics, dependency defects, or every
possible Rust panic source.

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

### 13.2 Held-out open-objective qualification

The open-objective gate proves that the model can make the decisions that
task-authored qualifiers otherwise make for it. The actor receives only:

- the user's bounded natural-language objective;
- the immutable capability envelope and resource ceilings;
- the initial Work-owned browser resource and its current observation; and
- the small model-facing capability profile admitted for the current phase.

The route and answer are withheld from the actor. Trusted host policy supplies
only the approved origin/path scope, maximum hop allowance, operation budgets,
and extraction schema; it cannot turn page content into authority. A successful
run must independently choose what to inspect, follow at least one previously
observed navigation opportunity, recognize when the current document contains
sufficient evidence, and request a source-bound extraction. Mechanical
completion verifies provenance and lifecycle. A separate human review judges
factual correctness and usefulness; a model's declaration of success is never
the verdict.

The first actual-app objective is now qualified on macOS at source `e792f6f`.
Starting at React's public Learn index, Luna discovered an unknown two-hop route,
selected the relevant documentation page, and returned the correct functional
updater solution with current-page evidence. The run used the actual bundled
application, ordinary Work controller, native WKWebView, provider adapter,
durable Store, audit path, and normal shutdown. Full identities, metrics,
retained response IDs, manual judgment, and scoped limits are recorded in the
[qualification evidence](../eval/agentic-browsing/macos-open-objective.md).

Subsequent objectives add deterministic loopback tasks with hidden ground truth,
same-origin SPA transitions, cross-origin scope boundaries, multiple pages,
contradictory sources, insufficient evidence, and a required human-takeover
branch. At least one successful page and its evidence must remain owned by Work
after the actor lease ends, proving that run completion and resource destruction
are not the same lifetime.

Every run records a content-free decision trace: offered capability profile,
chosen capability, observation/evidence identifiers, verification level,
typed refusal or recovery class, turns, tokens, latency, cost, and resource
closure. It does not retain hidden answers, page content, provider payloads, or
credentials in committed evidence.

### 13.3 Evaluation portfolio

No single browser benchmark is the release gate. Zephium uses complementary
evidence layers:

1. deterministic hostile fixtures for protocol, policy, lifecycle, safety, and
   exact failure diagnosis;
2. held-out open objectives for model planning, evidence selection, stopping,
   uncertainty, and recovery;
3. a small heterogeneous live-site matrix for current-web compatibility;
4. reproducible self-hosted tasks from maintained BrowserGym/WebArena-family
   environments where their task semantics match Zephium capabilities;
5. a sampled live benchmark such as Online-Mind2Web for external validity and
   a sampled WebVoyager-compatible run only for market comparability; and
6. owned authenticated accounts with reversible read/write workflows, followed
   by long-running real Work scenarios and human takeover.

Live benchmarks drift, contain ambiguous or time-dependent tasks, encounter
anti-automation defenses, and often use model judges. Report the exact task
revision, exclusions, environment, model, provider settings, pass definition,
repetitions, intervention policy, and confidence interval. Never optimize the
runtime or prompt solely for one leaderboard, and never let benchmark-specific
helpers enter the production capability surface.

### 13.4 Initial real-site matrix

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

### 13.5 Concurrent production configuration

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

### 13.6 Endurance and fault injection

Run multi-hour loops with bounded create/navigate/suspend/resume/destroy,
renderer termination, network loss, model disconnect, rate limit, cancellation,
disk pressure, shutdown, and restart. Validate no leaked native view, process,
task, cookie bridge, profile lease, model stream, secret, or temporary artifact.
Faults settle as typed recoverable or terminal states; they do not hang.

Process shutdown first permanently seals and exactly drains the process context
registry, profile leases, cookie transfers, native-action execution,
post-action settlement, and screenshot capture. Screenshot admission has the
same retain-and-drain seal as the other native coordinators: sealing refuses new
captures but cannot discard an accepted capture's terminal obligation. Only a
consuming join over that complete sealed, empty cohort may begin the native
port barrier. The distinct audit admitted with the atomic seal is never
substituted by an earlier ordinary audit. If it does not prove all nine native
resource counts are zero, the shell may issue only strictly newer, read-only
audits under the process deadline and an eight-attempt hard ceiling. Reporting
clean process teardown requires the resulting constructor-closed zero proof in
addition to, not instead of, run cancellation, durable audit drain, policy
settlement, and the application shutdown barrier. A failed proof may not skip
best-effort engine cleanup, but that path is terminally unclean.

The native port treats both outer main-loop dispatch and later execution of
each queued context or screenshot task as unwind boundaries. In unwind-capable
development and evaluation builds, a panic is caught, permanently seals
admission, reports only one content-free fatal invariant, and releases the
exact logical and physical permits. If synchronous dispatch already executed
the task, its scheduled terminal obligation remains authoritative. A poisoned
outer slot is recovered only to apply that same permanent seal. Optimized
desktop builds retain the workspace-wide `panic = "abort"` policy, so a panic
is process-terminal and cannot unwind through native callback or shutdown
code; the recoverable path is not claimed for release binaries.

The reusable imperative driver for this terminal native phase consumes the
coordinator only after its lossless admission has accepted the sealed cohort;
the caller retains every logical owner when admission refuses. The driver also
takes an existing native port, a shell-minted first audit identity, and a
caller-owned native-event source. It rechecks the absolute deadline
before native dispatch, accepts only the exact audit event class expected by
the current coordinator stage, and mints retry identities with checked
addition. Nonzero or refused audits use fixed exponential waits beginning at
100 ms and capped at one second; the coordinator still owns the eight-attempt
ceiling. The driver creates no worker, timer, channel, page, or native object,
and its diagnostics cannot project event payloads. The concrete application
runtime remains responsible for settling higher run/provider/policy/audit
owners and for supplying the real event source.

The stable application boundary is a move-only, consuming agent-browser
lifecycle. Its terminal `Clean` result necessarily carries that native zero
proof and additionally attests complete run cancellation, provider settlement,
durable audit delivery, mutable-policy settlement, and logical-owner drain.
Failure consumes the lifecycle and is terminal; the application may continue
best-effort Store and engine cleanup, but it cannot report a clean shutdown.
The feature-gated application Shell can own this lifecycle without learning
its internal runtime vocabulary. Every worker-construction or actor-handoff
failure returns both the agent and extension owners losslessly. The shared
spawn path carries both unique owners directly through diverging failure
branches and the single successful handoff; it uses no temporary optional
owner or release-path invariant abort, and its function-local Clippy contract
denies `unwrap`/`expect`, `panic`, and `unreachable`. After the retryable Store
durability preflight succeeds, the Shell consumes agent
shutdown first, then extension shutdown and the terminal Store barrier, and
finally requests engine/blocker teardown under the same absolute deadline. An
unclean or panicking agent lifecycle cannot skip those later best-effort
barriers or produce `Clean`. The entire seam remains absent from the ordinary
desktop dependency graph until a concrete runtime is integrated.

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

The run-local receipt reducer supplies only the exact accounting subset it can
prove from policy receipts. The independent audit-derived progress reducer
supplies observed duration sample counts, sums, and maxima for initial queue,
model, effect, and human waits, together with root elapsed time and closed
completion/takeover counts. A separate optional exact batch-terminal reducer
supplies closed batch/action outcomes, backend counts, settlement-event counts,
and fixed content-free histograms for native execution, settlement,
revalidation-to-settlement, and ordered revalidation-to-verification timing.
It rejects manifest-revision, plan-node, effect, attempt, batch, shape, and
operation-budget mismatches before logical mutation; its copyable snapshot is
capped at 1 KiB and it owns no clock, telemetry, persistence, task, or browser
resource. The reducer is constructed and fed explicitly, so unused agentic
browsing pays no reducer allocation or execution cost. These reducers retain
no raw action samples or site/platform/resource labels, so exact medians,
percentiles, reviewed per-site distributions, native/process counts, and
machine resource measures remain qualification-harness inputs. Committed
provider input now carries a separate fixed, content-free qualification value
with the exact serialized request bytes, the existing closed semantic or
screenshot encoding stats, the newest semantic-payload token count when one
exists, and the whole structured-replay measurement when either the trusted
local counter ran or the provider-exact path established its conservative
reservation. The authenticated count replaces only that structured measurement
with `ProviderExact`; it never relabels the semantic metric. It is inaccessible
before disclosure commit, capped at 64 bytes, and owns no content, tokenizer
label, sink, clock, task, or browser resource. An exact-local accounting-mode
commit can mint an at-most-192-byte copyable metric receipt immediately even
when an exact semantic measurement came from the provider. A provider-exact commit
cannot: while its structured count remains `Conservative`, the public attempt
returns no receipt, so a stale provisional sample cannot consume the call id in
the replay-protected reducer. Authenticated counting exposes the updated
`ProviderExact` receipt on the counted typestate and terminal result. If counting
fails, the terminal result instead seals the original conservative disclosure as
the one final receipt while generation usage remains exact zero. Every receipt is
bound to the private canonical manifest revision, call, lease, and node. A
separate optional run-local reducer consumes only final receipts, rejects
revision/node mismatch, call replay, contradictory closed shapes, arithmetic
overflow, and run or node operation-budget excess before logical mutation,
then aggregates exact bytes, lines, source-shape/redaction counts, and measured
token totals/quality counts across the six closed input classes. Its copyable
snapshot is capped at 1 KiB; it retains only a bounded sorted call-identity
index and canonical plan-node counts, never raw samples or content. The
qualification harness must still retain reviewed samples when medians or
percentiles are required, and an absent token measurement is never represented
as zero.

Terminal reporting may explicitly construct a separate, at-most-192-byte
metric-closure value. It rejoins all four reducers to the exact private
manifest revision and supervisor, requires an unsealed terminal supervisor
with no live node, execution slot, wait, cancellation drain, or browser-context
assignment, and matches the audit-derived root outcome and complete activated-
node count. It then compares the exact sorted model-call identities between
receipt accounting and committed inputs, and the exact sorted effect and
native-attempt identities between receipt accounting and action terminals;
model/effect duration sample counts and checked node, input-kind, action,
settlement, backend, and fixed-histogram partitions must also agree. The value
is point-in-time descriptive evidence only. It grants no authority, owns no
runtime or telemetry seam, and does not by itself prove that mutable policy,
native resources, sites, providers, devices, or qualification-harness samples
are settled or production-qualified.

Mutable run policy has a separate consuming settlement. It accepts only that
exact metric closure and accounting reducer, rejoins their private manifest
revision and supervisor, refuses a sealed policy, and requires zero pending
model calls, effects, origin writes, and reserved operations/tokens/cost. It
then checks run-wide and every canonical plan-node consumed operation,
model-token, and cost total against receipt accounting. The same operation
consumes an exact shutdown-sealed audit ledger only when it is unambiguous,
quiescent, and its durable commit count covers every closure event. Success
destroys both mutable owners and returns an at-most-256-byte copyable,
non-authorizing settlement; every refusal returns the complete move-only policy
and audit ledger for cleanup, drain, or a corrected retry, so pending authority
cannot be discarded as a false terminal. This closes mutable policy and local
durable audit delivery only. It does not prove native-resource, site, provider,
device, or production qualification.

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
  offline physical-result qualification, and eval manifest;
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

The current reusable locate/act controller vertical and its explicit host
obligations are documented in [Bounded semantic browser session](agent-browser-session.md).
The [M6 engineering record](../eval/agentic-browsing/m6-production-qualification.md)
separately reports real-site evidence and remaining shipping integration gaps.

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
- Work tasks, notes, memory, or knowledge; the subsequently authorized
  [application admission/recovery foundation](agent-work-persistence.md) stores
  only bounded content-free facts, not executable session or content persistence.
  Its opt-in [trusted macOS composition](agent-work-composition.md) accepts an
  already-approved typed task contract, not UI/model-authored authority. Explicit
  [sequential native lifetimes](agent-work-lifetimes.md) require exact completed
  predecessor proof and fresh input; no parallelism or hidden replay is implied;
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
- [WebView2 `ICoreWebView2Environment8::GetProcessInfos`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment8#getprocessinfos)
  defines the user-data-folder process snapshot and process-ID surface used by
  both release-excluded Windows qualifiers' shared bounded resource sampler;
  the contract explicitly omits the crashpad process.
- [Win32 `GetProcessMemoryInfo`](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo)
  defines the query-limited process-handle and working-set measurement used for
  aggregate resident bytes. Partial or inaccessible samples fail qualification.
- [WebView2 `CallDevToolsProtocolMethod`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#calldevtoolsprotocolmethod)
  documents asynchronous completion and the fact that dispatched methods may
  be processed out of order; the M1 adapter therefore permits one in-flight
  fixed command.
- [WebView2 composition hosting](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis#rendering-webview2-using-composition)
  ties `SendMouseInput` and `SendPointerInput` to a composition controller.
  Pinned Wry owns an ordinary controller, so the adapter reports composition
  input as unsupported instead of casting across controller kinds.
- [Win32 `SendMessageTimeoutW`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw)
  invokes the procedure of the exact target HWND. The M1 HWND candidate
  therefore resolves and validates the direct WebView document child that
  pinned Wry itself uses for focus instead of sending to Wry's container.
- [Win32 keyboard-message flags](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-keydown)
  together with [`GetKeyboardLayout`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getkeyboardlayout)
  and [`MapVirtualKeyExW`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-mapvirtualkeyexw)
  define the target thread's input-locale binding plus the scan-code,
  extended-key, previous-state, and transition fields. The M1 HWND encoder
  preserves those fields, refuses an unknown mapping or mid-row locale change,
  and applies the same scan metadata to its fixed character message.
- [Win32 `ShowWindow`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow)
  defines `SW_SHOWNOACTIVATE`; the M1 adapter additionally verifies host
  foreground/activation and owned-subtree keyboard focus before accepting a
  hidden or background baseline.
- [Windows reparse-point operations](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-operations)
  define `FILE_ATTRIBUTE_REPARSE_POINT` as the filesystem-level check for a
  file or directory with an associated reparse point. The release-excluded
  Windows physical runners and offline reviewers reject that attribute for
  evidence directories and records, covering junctions and mount points in
  addition to ordinary symbolic links. Rust's Windows
  [`MetadataExt::file_attributes`](https://doc.rust-lang.org/stable/std/os/windows/fs/trait.MetadataExt.html#tymethod.file_attributes)
  exposes the exact attribute field used by that check.
- [Chrome DevTools Protocol Input](https://chromedevtools.github.io/devtools-protocol/tot/Input/)
  defines the fixed diagnostic mouse and key command coordinates and fields.
- [Chrome DevTools Protocol `Runtime.evaluate`](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/#method-evaluate)
  defines the explicit `userGesture` control used for M1 fixture observation.
- [OWASP LLM prompt-injection prevention](https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html)
  summarizes the threat class; Zephium's deterministic source-to-sink policy
  remains the actual product boundary.
- [OpenAI Responses API](https://platform.openai.com/docs/api-reference/responses/create)
  defines the first fixed request, bearer-authentication, streaming, and usage
  surface; Zephium sets `store: false` and accepts only its own closed tools.
- [Anthropic Messages API](https://docs.anthropic.com/en/api/messages)
  defines the second fixed request, `x-api-key` and version headers, streaming,
  and usage surface used by the provider-neutral adapter.

Primary documentation does not substitute for device tests against the exact
runtime Zephium ships.
