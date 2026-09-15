# Agentic browsing for Work: implementation brief

Status: handoff to the browsing implementation agent
Written: 2026-09-15, from the Work-mode integration branch at `7d3d88af`

You are the lead engineer for the browser-execution layer that Zephium's Work
mode runs on. Work mode is being built in parallel on the same branch by the
product/Work engineer. This brief tells you what Work needs from you, what
exists today and where it stops, the contract you deliver, the order to build
it in, and the rules that never move. Read `docs/product-system.md` (§3.2, §4.5,
§4.7, §4.8, §6, §8.1, §10, §11) and `docs/agentic-browsing.md` in full before
writing code. The accepted spec stays the spec; this brief prioritizes it for
the product that now exists on top of it.

## 1. What we are building and the bar

Zephium is a WebView browser with two environments. Browse is a normal browser.
Work is a spatial canvas where a Rust-supervised agent does real work on real
websites while the user watches, steers, and takes over. Every AI browser we
have evaluated is slow, brittle, token-hungry, and blind: one page at a time,
DOM dumps to the model, a JS `eval` behind a prompt, no honest verification.
We are building the opposite, and it has to be the best browser actor that
exists:

- **Powerful.** Anything a person does in a browser: search inside a site,
  open results, paginate, read product pages, compare, add to cart, fill long
  multi-step forms, work in GitHub, Gmail, Slack, Notion, Google Docs and
  Sheets, LinkedIn, with or without a connector, signed in when the user says
  so.
- **Fast.** Batched turns, no model round trip for mechanics, bounded waits
  that settle as soon as the page settles, parallel workers on independent
  pages.
- **Cheap.** Semantic observations of 500 to 2,000 tokens, diffs of about 200,
  structured extraction instead of prose, no repeated page transcripts.
- **Stable.** Typed failures, verified effects, no blind retries, recovery
  from navigation, SPA churn, dialogs, renderer loss, cancellation.
- **Visible.** The user can watch each agent page on the Work canvas, open it,
  and take it over; the agent keeps working on the others.
- **Efficient.** Zero idle cost with no run; WebKit throttling under
  occlusion; frozen frames during canvas gestures; no per-frame native
  repositioning.
- **Secure and production-grade.** The invariants in
  `docs/agentic-browsing.md` §2 hold without exception. Page content is
  hostile data, secrets are never observations, effects are verified,
  authority is deterministic Rust policy.

## 2. Product context you must design for

### 2.1 Two situations

1. **On the page.** The user is on a page in Browse or in the Work pane and
   asks the AI to do something there. The actor works in that exact page
   under a revocable lease, pauses on any human input, re-observes after.
2. **On the canvas (the core).** A Work has several pages the agent or the
   user opened. The primary agent and bounded sub-agents work across them at
   once. Each page is a live card on the canvas. The user sees what happens in
   each, can open one in the pane, can interrupt one, and the run continues
   on the rest.

### 2.2 The proving objective (M6 in the Work plan)

"Find the best three GPUs for local LLM inference on Amazon, compare them,
then add the one I choose to my cart."

1. The objective is sent. No plan window. The Work agent loop
   (`crates/zephium-app/src/work_agent.rs`) runs provider-native web search
   first; sources land on the canvas as cards.
2. If search evidence is insufficient, the loop spawns a browsing worker on
   amazon.com: search inside the site, open the top results, extract each
   product as structured data: title, price with currency and observed time,
   main image URL, rating, key specs (VRAM, memory type, bandwidth, TDP),
   product URL, availability.
3. Work renders three product cards with picture and price, a comparison
   matrix, and a chart with a stated basis.
4. The user presses Choose on one card and sends "add this to my cart".
5. The loop proposes an act; the user approves inline; a worker opens the
   product page in a Work-owned page that shares the profile's signed-in
   session, presses the add-to-cart control, verifies the cart count or the
   cart page, and returns a finding with the cart page as source. Checkout,
   purchase, payment and sign-in are refused.
6. While the worker acts, its page is a live card on the canvas.

Everything in this brief is judged against that objective first, then against
the site list in §2.1.

### 2.3 Site skills

Popular sites get short, versioned, origin-keyed playbooks ("skills"):
canonical search URL shape, how results are structured, product page fields,
the add-to-cart control and its verification, pagination, what never to
click, known interstitials. Skills are data, not code, not authority. They
are disclosed to models as guidance and used by Rust to pick recipes and
verifications. Amazon first, then GitHub, Notion, Google Docs/Sheets, Gmail,
Slack, LinkedIn. The Work engineer owns the registry format in `zephium-core`
and the turn disclosure; you consume skills inside the browsing worker.

## 3. What exists today and exactly where it stops

Read `docs/agentic-browsing.md` §1.1 and
`eval/agentic-browsing/m6-production-qualification.md` ("Remaining release
blockers") for the program's own accounting. The relevant facts for Work:

**Crates.** `zephium-agentic` (manifests, policy, semantic runtime contracts,
`AgentBrowserPort` at `src/context_port.rs:1003`, `SemanticActionKind` at
`src/semantic_action.rs:149`, extraction schemas at `src/semantic_extract.rs`),
`zephium-agent-runtime` (runtime worker), `zephium-agent-controller` (22k
lines: `AgentBrowserSession`, `AgentWorkController`, `AgentWorkDiscoveryTask`,
`AgentWorkFormTask`, retained resources, `AgentBrowserModel::{Terra, Luna}` at
`src/terra.rs:112`), `zephium-engine` (macOS owned WKWebView contexts in
`src/platform/macos/agent_context.rs`, presentation in
`work_observation_presentation.rs`, `semantic_screenshot.rs`, work resource
ports), `zephium-work-composition` (the adapter Work calls:
`durable_runtime.rs` `run_agent_step` at line 130 and `compile_step` at 406,
`open_objective.rs`, `account_scope.rs`), `zephium-terra-macos-probe` (headless
live qualification binary).

**What the kernel proves.** Owned hidden contexts, one immutable isolated-world
runtime, compact semantic snapshots with opaque refs, `locate`, `read`,
`extract` (text fields only: `try_text`, `try_text_list`), fixed native
recipes Click, Fill, Select on the retained view, fresh postcondition
verification, policy accounting, audit, clean teardown, WebKit `Throttle`
scheduling attested at construction (`agent_context.rs:566`, `:791`). Live
Luna runs completed short prescribed routes and one Notion title change and
restore.

**How Work uses it now.** The agent loop's `read` and `discover` steps compile
to a `PublicReadWorkInvocation` with `AgentNavigationDiscovery::try_new_public_web`
(read: 1 hop; discover: bing start, up to 8 hops), a single text field
`output_0` of 4,096 bytes, `max_model_calls: 16`, one context, anonymous
account. The result becomes a "Notes from {host}" document artifact with
citation links. The loop runs native steps one at a time; searches run in
batches of the worker cap.

**Where it stops (each is a blocker for §2.2).**

1. Extraction reaching Work is prose. The schema layer has text, text list,
   boolean, and unsigned fields; Work's step compiles only one 4 KB text
   field, and there are no money, decimal with unit, URL, image, enum, date,
   or object-array fields anywhere.
2. One context, one page at a time, one action per model turn, at most eight
   actions per session. No parallel workers, no batched action turns.
3. Navigation is link-following only: a requested URL must appear as a public
   link in the acknowledged observation and stay inside the approved path
   subtree. No click-to-navigate, no in-site search submission, no
   pagination, no cross-origin discovery. Discovery starts from bing.
4. Actions that execute are snapshot-verifiable Click, Fill, Select on
   trusted task contracts. `Press`, `Scroll`, batch results, and wait kinds
   exist as contracts in `zephium-agentic` but have no production path
   through the controller, native recipes, or composition. Every workflow
   needs a hand-written task adapter with exact field and value
   postconditions.
5. Model catalog is closed to GPT-5.6 Terra and Luna. Work's grant names a
   model (`WorkAgentGrantV1.model`); the worker must run on the eligible model
   the grant names, with a stated fallback when it cannot.
6. Pages are hidden, non-key, ignores-mouse surfaces used only for a bounded
   observation episode. There is no way to present an agent page inside the
   Work window at a canvas rect while it stays agent-owned.
7. Intervention: sign-in walls and challenges stop the run with a reason
   (`intervention` on `WorkBrowserOutcome`). There is no resume from a fresh
   observation after takeover; continuation is a new approval.
8. No effect class for add-to-cart or any remote action outside the single
   reversible field update. No approval binding for a proposed action.
9. Discovery calls carry no site knowledge; a run on Amazon has no idea what
   an Amazon result page or product page looks like.
10. Windows has no owned-context composition; all live evidence is macOS.

## 4. The contract you deliver to Work

Work talks to browsing through one Rust port in `zephium-work-composition`.
Today that is `run_agent_step(shell, probe, WorkAgentBrowseRequest, settings)
-> WorkBrowserOutcome`. Replace it with a worker port whose shape is below.
Names are proposals; the semantics are requirements. Keep it typed, bounded,
`Send`, and free of provider or engine types in its public surface.

```rust
pub struct WorkBrowseTask {
    pub kind: WorkBrowseTaskKind,
    pub session: WorkBrowseSession,        // Anonymous | Profile { attested_account }
    pub model: WorkBrowseModel,            // from the grant; eligibility checked here
    pub skills: Vec<WorkSiteSkill>,        // origin-keyed playbooks, data only
    pub budget: WorkBrowseBudget,          // model calls, tokens, cost, deadline, hops, actions
    pub presentation: WorkBrowsePresentation, // Hidden | Placed { rect } (live card)
}

pub enum WorkBrowseTaskKind {
    /// Open one URL, extract a schema, follow at most `hops` on-page links.
    Read { url, schema: WorkExtractionSchema, hops: u8 },
    /// Start from a search (provider results or a site's own search) and
    /// collect up to `n` items matching the schema across result pages.
    Collect { start: WorkBrowseStart, schema, items: u8, hops: u8 },
    /// Perform an approved effect on one page and verify it.
    Act { url, effect: WorkBrowseEffect, verify: WorkEffectVerification },
}

pub enum WorkBrowseEffect { AddToCart { product_url }, FillForm { fields }, ... } // closed, versioned

pub enum WorkBrowseEvent {
    Page { page: WorkPageHandle, url, title },        // a live page exists; Work places it
    Progress { page, note: WorkBrowseNote },          // closed enum, no page text
    Extracted { page, items: Vec<WorkExtracted> },    // schema-validated, cited
    Verified { page, effect, evidence: WorkEvidenceRef },
    NeedsHuman { page, reason: WorkInterventionReason },
    Done { usage: WorkUsage, status: WorkBrowseStatus },
}
```

Requirements on the port:

- **Streaming.** Work commits each event as a durable step while the worker
  runs. The worker never buffers a whole run to the end.
- **Structured extraction.** Schema fields: text, text list, number with unit,
  money with currency, url, image url, enum, date, boolean, and an
  object-array (rows) of those. Every value carries a citation to the exact
  observation fragment. Rust validates types, bounds, secret screening, and
  provenance; models never emit free HTML or selectors.
- **Page handles and presentation.** A worker's page is addressable. Work can
  ask for it to be placed at a rect inside the Work window (a live card),
  hidden, snapshotted (bounded PNG, existing `semantic_screenshot`), or handed
  to the user (takeover: automation revoked and drained first, then input).
  Placement never routes input to the page and never changes ownership.
- **Parallelism.** N tasks run at once under a cap the caller passes
  (`WorkAgentLimits.max_workers`, today 4). Per-origin writes serialize.
  Anonymous tasks use fresh cookie stores; profile tasks share the profile's
  session and are limited to one writer per origin.
- **Model.** The task names the model. Provider transport reuses
  `zephium-agentic` planner transport (OpenAI Responses, structured outputs).
  Anthropic is a second adapter behind the same contract when its checkpoint
  and accounting seam exist; do not claim parity before that.
- **Skills.** Guidance goes into the worker's instructions; recipe selection
  and verification use the skill's structured hints; page data never
  overrides them.
- **Intervention.** CAPTCHA, 2FA, sign-in, unsupported control: `NeedsHuman`
  with a closed reason; the page stays alive; after takeover the run resumes
  only from a fresh observation and Work's fresh admission.
- **Diagnostics.** Closed facts only. No page text, model text, URLs with
  query strings, or provider bodies in logs. Raw bodies only under the
  release-excluded `probe-harness` retention path.

The Work engineer wires this port into `work_agent.rs` steps (`Read`,
`Discover` becoming `Collect`, and a new `Act` step with an inline approval)
and into canvas live-page cards. You do not touch `zephium-core/src/work`,
`zephium-store`, `zephium-app/src/work_*`, `zephium-ipc`, `desktop/src/work_*`,
or `frame/`; if the port needs a change there, write the exact change you need
in your report and the Work engineer lands it.

## 5. Build order

Each milestone ends green on the gates in §8, with live evidence from the
probe, and coherent commits. Do not start a later milestone to cover a failed
earlier gate.

### B1. Structured extraction and the worker port (unblocks product cards)

- Extend `semantic_extract.rs` schemas with the field kinds in §4; keep the
  `ZEXTRACT1` guard binding and provenance rules; add the OpenAI strict schema
  projection and Rust re-validation for each kind; money and number carry
  unit or currency and are refused when the cited fragment lacks them.
- Image URLs: the semantic runtime already reads anchor hrefs through a
  captured native getter; add the same bounded, credential-free read for
  `img` sources and `srcset` candidates, capped per node, so an extraction
  can cite them.
- Implement the port with `Read` first. Replace `run_agent_step`'s text-only
  compile path with a schema-driven one. Keep the existing text schema as a
  degenerate case so current Work runs keep working.
- Live proof: the probe's `--live-agent-read-work` mode returns a typed
  product record with price, currency, and image URL from one public product
  page.

### B2. Site-driven collection (unblocks Amazon search → three products)

- `Collect`: a start (`SiteSearch { origin, query }` from a skill's search URL
  shape, or `Urls`), result-page parsing guided by the skill, opening the top
  N results, per-item extraction, pagination up to the hop budget.
- Navigation beyond link following: submitting a site's own search form
  (typed fill and press Enter or click search) under a read-only lease,
  clicking result links, back navigation, typed waits for committed
  navigation and document readiness. Keep the manifest's origin and path
  scope; add a skill-declared allowed path set instead of a single prefix.
- Batched turns: one model turn proposes a bounded sequence; Rust executes
  sequentially, stops on navigation, dialog, origin change, or failed
  precondition, and returns one diff.
- Add `press` and `scroll` recipes to the isolated runtime with the same
  revalidation and postcondition proof as Fill.
- Skills: consume `WorkSiteSkill` (schema owned by Work; agree the shape in
  your first report) for amazon.com; add fixtures that replay recorded
  amazon result and product page semantics offline.
- Live proof: probe mode that runs `Collect` for a GPU query on amazon.com
  anonymously and returns three typed products in one run, with token and
  latency numbers per page.

### B3. Parallel workers and live pages (unblocks the canvas)

- Worker pool under the caller's cap; per-origin write serialization;
  independent budgets and deadlines per task; cancellation tree; clean
  teardown proof for N contexts.
- Presentation: a page can be placed at a rect inside the Work window
  (`WorkBrowsePresentation::Placed`). Reuse the existing owned-view rules
  (non-key, non-main, no mouse, no responder routing) and the Work pane
  geometry path (`crates/zephium-app/src/shell/work_pane.rs`,
  `desktop/src/lib.rs` `work_pane_set_rect`) rather than inventing a second
  geometry channel. Rect updates are coalesced; during a canvas gesture the
  frame shows a snapshot and the native view is hidden or left in place, then
  repositioned once on gesture end. Measure and record WebKit throttling when
  the page is occluded or hidden.
- Human takeover of a placed page: revoke and drain automation, then hand
  input to the pane; the run continues on other pages; the taken-over page
  returns to the worker only through a fresh observation and admission.
- Live proof: two anonymous `Read` tasks in parallel with both pages placed
  and visible; a takeover mid-run; measurements of CPU, RSS, and wakeups idle
  versus two workers versus two placed pages.

### B4. Act with verified effects (unblocks add to cart)

- `Act` with a closed `WorkBrowseEffect` vocabulary; first `AddToCart`.
  The worker opens the product page in a profile-session page, locates the
  add-to-cart control via skill hints and semantics, clicks under a
  policy-authorized effect, and verifies by a fresh observation of the cart
  count or the cart page. Checkout, buy-now, payment, address, sign-in,
  account settings, and any control the skill marks never-click are refused
  at the recipe and policy level, not by prompt.
- `FillForm`: multi-field forms with typed phases (extend `AgentWorkFormTask`
  rather than replacing it), including selects, checkboxes, radios, date
  inputs, multi-step wizards with next buttons, and file inputs refused
  unless explicitly granted. Verification per phase from fresh state.
- Approval binding: the effect the worker executes is exactly the one Work
  approved (product URL, origin, effect class); anything else is a refusal.
- Live proof: add a product to a cart on amazon.com with the user's
  signed-in profile in the isolated app build, with the page placed on the
  canvas, then remove it by hand. Record the run and its accounting.

### B5. Breadth and hardening

- Skills and fixtures for GitHub, Notion, Google Docs and Sheets, Gmail,
  Slack, LinkedIn; one live route each that a connector could not do
  (for example a form that only exists in the web UI).
- Anthropic adapter behind the same contract when its seam is complete.
- The six-site matrix, endurance, fault injection, and the release blockers
  list in the M6 record, worked from measured failures.

## 6. Performance and resource rules

- Initial snapshot at or below 2,000 model tokens median; action diff at or
  below 200; extraction envelope stays within its 112 KiB conservative cap.
- No model round trip for mechanics. Waits are typed and settle early.
- No page transcript is resent; only the current observation and a bounded
  checkpoint.
- One provider request in flight per worker; N workers under the cap.
- Zero idle cost: no timers, threads, contexts, or provider connections when
  no run exists. Prove it with the existing zero-resource proof path.
- Placed pages: geometry coalesced per frame at most, snapshot during
  gestures, WebKit throttling honored under occlusion, no wakeups from hidden
  agent pages beyond the throttled policy.
- Every milestone records tokens, cost, wall-clock per page, contexts, RSS,
  CPU, and wakeups in the eval record. Do not invent numbers.

## 7. Invariants that do not move

`docs/agentic-browsing.md` §2, restated for this work:

1. No model-facing DOM, selector, JavaScript, CDP, or native object.
2. No page-to-native bridge beyond the fixed isolated-world channel.
3. No extensions or userscripts in owned contexts.
4. Rust policy decides authority; prompts and skills are not boundaries.
5. Page content, skill text, and model output are hostile data.
6. Secrets are never observations, logs, screenshots, or artifacts.
7. Success is verified from fresh observation; a click is not a result.
8. Ownership is separate from visibility; placing a page changes nothing
   about who owns it.
9. Human input wins; automation is revoked and drained before a takeover.
10. Everything is bounded and cancellable.

Never weaken a production limit to make a test pass. Never dump provider
bodies or page content outside the release-excluded probe retention path.

## 8. Working agreement

- Repo `/Users/crynta/Dev/Zephium`. Work in a git worktree on a branch off
  `work-mode-integration` (for example `agentic-browsing-v2`). Never reset,
  rebase, force-push, or stash. Coherent local commits with conventional
  prefixes and a scope (`feat(agentic):`, `feat(controller):`,
  `feat(engine):`, `feat(composition):`, `fix(...)`, `docs(...)`). No AI
  attribution trailers in commit messages.
- Crates you own: `zephium-agentic`, `zephium-agent-runtime`,
  `zephium-agent-controller`, `zephium-agent-model-catalog`,
  `zephium-agent-provider-transport`, `zephium-engine` agent ports and macOS
  agent context, `zephium-work-composition`, `zephium-terra-macos-probe`.
  Crates you do not edit: `zephium-core/src/work`, `zephium-store`,
  `zephium-app/src/work_*`, `zephium-ipc`, `desktop/src/work_*`, `frame/`.
  Ask through your report for changes there.
- Node 24.19.0 from
  `/Users/crynta/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin`
  if you touch anything under `frame/`; system Node is not allowed. Never
  hand-edit generated bindings.
- Gates before every commit that touches Rust:

```sh
cargo test -p zephium-agentic
cargo test -p zephium-agent-runtime
cargo test -p zephium-agent-controller --features provider-transport
cargo test -p zephium-work-composition
cargo test -p zephium-engine
cargo test -p zephium-app
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

  Clippy on the probe crate with `durable-runtime` fails inside
  `zephium-engine` probe features today; that is pre-existing and not a gate.
  Some `zephium-app` and `zephium-store` tests are timing-sensitive under
  parallel load; rerun a failure alone before treating it as real.
- Live qualification:

```sh
cargo build -p zephium-terra-macos-probe --features durable-runtime
./target/debug/macos-terra-agentic-probe --live-agent-work
./target/debug/macos-terra-agentic-probe --live-agent-read-work
```

  The OpenAI development credential is in the macOS Keychain; a Keychain
  prompt appears for each freshly built probe binary and the user allows it.
  Proof JSON lands under `target/work-runtime-proof/`. Add probe modes for
  each milestone's proof. The app build for signed-in runs is
  `cd desktop && pnpm exec tauri build --debug --config
  tauri.work-integration.conf.json --features work-integration-qa --bundles
  app`.
- Accounts: the user provides test accounts and is present for Keychain
  prompts and sign-in. Consequential external writes (anything beyond add to
  cart on a test account, or any message, purchase, or publish) need the
  user's explicit approval per run.
- Reports: terse. What was built, what was measured, what is blocked, and the
  exact port or Work-side change you need. Update
  `eval/agentic-browsing/m6-production-qualification.md` with each live run
  and `docs/agentic-browsing.md` when a mechanism changes; do not write
  additional documents unless asked.

## 9. First deliverable

Before B1 code: a short design note in your first report with the final port
types from §4, the `WorkSiteSkill` shape you want to consume, the extraction
field kinds and their validation rules, and the list of engine and runtime
seams you will touch. The Work engineer signs off on the port and the skill
shape, then both sides build against them.
