# Retained heterogeneous commerce objective

Status: composition prepared; **no live model run or native commerce success
claimed**. This is the next public workflow after the retained Luna Svelte
kernel qualification. It deliberately changes the website and information
shape without changing the agent kernel or adding a shop adapter.

## Objective and scope

Start at the real [LEGO US Architecture catalog](https://www.lego.com/en-us/themes/architecture).
The user is choosing one architecture-themed display set as an adult gift, with
a USD 150 item budget before tax/shipping and a 30 cm wide by 20 cm deep display
space (height unconstrained). Luna selects its own candidate from observed
links, investigates current product information, and returns a concise decision
brief with price, dimensions, relevant building/display qualities and caveats.
No product, product URL, price, dimensions, route or expected answer is supplied.
An honest mismatch or missing evidence is preferable to an invented match.

The public catalog's ordinary commerce interface adds catalog/navigation clutter,
product cards, dynamic controls, possible consent/location overlays, late content
and purchasing controls absent from a straightforward docs article. This is not
a fabricated benchmark storefront. Being a commerce site does not by itself
prove that a particular successful result depended on animation frames, client
hydration or an SPA transition; those remain separate observations, not a label
used to overstate this qualification.

A read-only HTTP preflight on 2026-09-08 returned 1,102,701 HTML bytes, 34 script
tags, `__NEXT_DATA__` and `/_next/` assets from that exact catalog URL, consistent
with a real Next.js storefront. Only counts/technology markers were retained;
the HTML, product answers and any search results are not injected into Luna.
This preflight is not a WKWebView hydration or foreground-rendering test.

Only `https://www.lego.com/en-us/` is authorized, anonymous/public/read-only, for
at most two **currently observed** navigation hops. The prefix covers the catalog
and product paths, not other locales/origins. The existing controller still
rejects guessed or rewritten URLs, query/fragment additions, repeats, redirects,
stale refs and out-of-scope navigation. Existing structural Snapshot, baseline
Read, Locate, Navigate and terminal Extract are the complete admitted workflow.
No effects are authorized: no consent/locale selection, filters/variants, sign-in,
cart, purchase, arbitrary JS or secondary HTTP/DOM extraction.

## Same product architecture

The commerce composition explicitly selects `DocumentQueryFinalization` for
construction and its trusted observed-link discovery manifest. Each successor
authorization carries that policy into its exact operation request. The model
must still select an exact current observed query-free URL. A native receipt may
finalize that request at a URL with one opaque nonempty query addition, with all
other raw URL bytes unchanged and the same native start/commit/finish lineage.
Both resource completion and agent policy independently check this relation.
Default navigation and the original Svelte successor workflow remain `Exact`;
the older `InitialQueryFinalization` policy cannot authorize a successor.

Requested targets remain the basis of model continuation, repeats and navigation
progress. Effective native URLs are separately bound to the committed receipts
and exposed as current-document metadata. When different, provider context also
includes the exact requested document URL. Initial metadata is joined to the
original retained construction receipt before any provider call. Resource read
bindings preserve both the original construction request and the request that
produced the current document. Existing source, ref, account, invocation, token,
URL and transcript ceilings continue to apply; an over-budget metadata envelope
refuses rather than dropping lineage or increasing a limit.

The new compile-time feature `macos-work-retained-commerce-probe` inherits
`macos-work-retained-product-probe`; the composition feature similarly inherits
`retained-product-qualification`. Only frozen objective constants are selected.
There is no new launcher, controller, page, Store, render holder or runtime loop.
The original Svelte objective remains byte-for-byte unchanged and selected when
the commerce feature is absent. Default Browse and production builds do not
enable either diagnostic objective; existing debug/macOS/configuration guards
continue to reject unsafe build combinations.

The actual `admit_retained_trusted_work` entry owns the same retained page/profile,
execution lease, document transitions, native callback/debt accounting, source
mapping, durable run and ordinary shutdown as before. Every initial and scoped
observation uses the shipping observation-owned rendering episode. Its semantic
result cannot reach model work until native presentation and callback retirement
finish; no qualification-only presentation is substituted. See the independent
[native rendering qualification](macos-retained-observation-rendering.md) for
the actual RAF and delayed-provider proof behind this inherited contract.

Budgets are unchanged: at most eight model calls within eight total policy
operations, 100,000 aggregate tokens, 100,000 micro-USD, one context, two
navigation hops and one 150-second absolute deadline
including credential lookup. Luna and explicit diagnostic `store:true` retain
the public requests for human inspection; production remains stateless. The
original 4,096-byte summary / important_claims / caveats value ceiling remains.
Only exact current-document source evidence can be mapped; catalog facts may
guide selection but must not silently become current-page citations later.

Retained commerce run twenty-fifth exposed a budget-contract mismatch: seven
model decisions and one navigation had consumed all eight policy operations,
although the decision guidance still promised a mapper call. Extract was
proposed but mapping admission failed before transport. The deterministic
controller reproduction reaches the same `Browser(Authority)` with eight
operations; the older budget regression used 24 and could not expose it.

Decision preparation now derives its remaining allowance from both the fixed
model-call ceiling and unreserved run/lease-node operations, before whole-input
counting. One operation remains for mapping; navigation is omitted when its
operation, next decision and mapping cannot fit. Native inspection/navigation
admission independently protects that reserve. Under the same eight-operation
manifest the corrected one-hop regression finishes with seven model calls
including mapping plus one navigation, with source mapping and clean closure.
No operation, call, token, cost, navigation or time ceiling was increased.
This is deterministic evidence; live useful-result qualification remains pending.

## Deterministic gate

The exact selected wrapper and original observer are tested, not just the bare
discovery task:

- Task and manifest freeze identical departure/origin/path/hop scope and public
  read-only effects. Query/fragment, other origin/locale, encoded path and repeat
  departure candidates remain denied; scope membership alone is not a link proof.
- Complete, scope-boundary and node-limited changing public observations remain
  inspectable without site-specific headings/answers/completeness predicates.
  Same-document inspection retains original account age; foreign resources and
  origins refuse. Cart controls in an observation confer no action capability.
- Current-source, shape-valid uncertainty can pass source verification; foreign
  sources cannot. No product recommendation or factual answer is an acceptance
  oracle. Model/inspection events alone cannot mark the observer accepted.
- Existing wrapper/observer tests cover progressive-tool forwarding, all effects
  refused, sticky failure, hop limits and exact usage accumulation under both
  selected objectives. Shared input construction is tested with durable and
  ephemeral profiles, without credential/native/provider activity.
- Feature mutation checks prevent detaching commerce from the existing isolated
  retained entry/public qualification and catch silently selecting the old docs
  objective or broadening the approved path.

Initial verification: all 16 composition unit tests passed with the commerce
feature; the original Svelte objective was independently compared byte-for-byte.
The doc-test tail then stalled with another parallel compiler. The coordinator
requested a clean stop of that tail and the waiting architecture, baseline and
desktop gates. Their intentional exit 143 is not a test failure or a completed
gate. Sequential integration verification remains required before live launch.

## Live handoff (not executed by this slice)

Build only after review with the pinned Node 24.18.0 / pnpm 11.17.0 toolchain:

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-navigation-probe.conf.json --features macos-work-retained-commerce-probe --bundles app --ci --no-sign
```

Pin committed source, whole-bundle inventory and executable; use a fresh empty
`app.zephium.work-navigation-probe` root, preserving old evidence. The same
ordinary app window requires the user's foreground focus; no automatic focus
or retry is added. Confirm the diagnostic selects `retained-commerce-gift-v1`,
`entry=admit_retained_trusted_work` and `rendering=observation_owned` before
interpreting results. Record actual route/tools, per-turn bytes/tokens/latency,
observations/completeness and exact durable/native closure. Preserve the failed
run too if a gate or capability blocks it.

Mechanical success means source-mapped, durably acknowledged completion after
at least one model-selected navigation, with the same healthy retained resource
and exact cleanup. **Usefulness is separate**: inspect the final chosen page and
each cited source; judge whether the brief identifies a candidate and correctly
states established fit/mismatch, current price/currency, availability uncertainty
and missing dimensions. A source-bound cookie notice or generic caveat is not a
useful purchase decision. Do not count it as a successful commerce workflow.

If consent/location redirects, hidden specification controls, virtualized content,
late readiness or current-page-only evidence prevent useful completion, document
the exact tool/input/state mismatch and return to that generic seam. Do not make
a per-site workaround, remove the gate, boost the limits or supply the answer.
This run alone cannot qualify purchases, signed-in tasks, cross-origin work,
all SPA behavior, cross-page evidence, repeated reliability or Windows.
