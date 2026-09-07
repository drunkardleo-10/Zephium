# Shipping retained product-entry witness

Status: first live v1 attempt failed safely; corrected v2 contract is prepared,
with no subsequent live qualification claimed.

`macos-work-retained-product-probe` invokes the shipping
`zephium_desktop::admit_retained_trusted_work` entry in the actual installed
application, with its original Engine/Store and selected Ready profile. It
reuses the isolated `app.zephium.work-navigation-probe` bundle/configuration
and bounded foreground/profile/credential observer, not the old retained
rendering probe owner or a raw controller constructor. It is mutually exclusive
with the discovery and rendering witnesses and rejected in optimized builds.

The model-decided one-page objective asks Luna to read `https://agent-browser.dev/`
and produce a source-backed technical brief useful to Zephium's team, selecting
important claims, architecture, capabilities, caveats and missing information.
The v2 schema is a generic `summary` plus `important_claims` and `caveats` lists,
with independent collection/item source bindings. The total value ceiling is
still 4,096 UTF-8 bytes (640 + 8*320 + 4*224). There is no expected answer,
heading, route or factual validator. Public/Anonymous/Read authority and the
existing 8-call, 100,000-token, 100,000-micro-USD, 150-second absolute ceilings
remain. No navigation, actions, subtree capture, rendering lease or successor
admission is added. The homepage result cannot establish general JS/RAF readiness.

Only this explicitly public diagnostic opts into `gpt-5.6-luna` `store:true`
for owner dashboard inspection. Production/BYOK constructors still select
`store:false`. Never use a private or signed-in profile. Preserve any previous
isolated app data intact; the launcher requires a fresh empty root.

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-navigation-probe.conf.json --features macos-work-retained-product-probe --bundles app --ci --no-sign
```

Commit before launch; pin executable SHA-256 and canonical whole-bundle manifest
with `bundle-manifest-v1.mjs`, preserve content-redacted logs and original Store,
and verify the bundle is unchanged after normal exit. The user must focus the
actual isolated app window within its existing 15-second foreground gate; the
driver cannot take focus or gain native rendering authority from that check.

Required evidence: actual native snapshot node/text-byte/frame/completeness
counts, every bounded event and settled provider token/cost accounting, source-
bound ModelMapped result after durable Succeeded ACK, exact original retained
resource phase/health/idle state before destruction, observer worker join and
normal Shell/global native clean shutdown. Inspect all retained provider calls
to judge page coverage, usefulness and factual support; local logs intentionally
omit page and answer text. Dashboard retention is not proof of completed review.
Store integrity, terminal debt and matching accounting are checked after exit.

No UI, takeover, pointer arbitration, retained navigation, second-run resume or
restart durability is claimed. A failed or incomplete page is evidence, not a
reason to add readiness markers, scripted answers or weaker admission.

## First live attempt and concrete contract correction

The unchanged `e9dfb34` app ran once after an earlier pre-credential foreground
deferral. Its exact evidence and original stopped Store are preserved at
`/private/tmp/zephium-retained-product.aA4Vjt`; nothing in that run was rewritten.
Foreground admission was immediate, and the same Ready profile was checked
before and after credential lookup. The original native observation contained
82 nodes, one Complete main frame, zero boundaries and 2,495 text bytes. Luna
chose Read then Extract: three completed calls, 14,526 input and 980 output
tokens, 4,099 micro-USD under the existing PricedCeiling accounting. The original
resource remained Retained, healthy, idle and reusable before clean normal
shutdown. Store integrity passed; its one immutable terminal record is Failed,
debt NONE, revision3, with no artifact, nine audit events and one delivery.

The owner inspected retained response
`resp_0cac7e6b4fedcc40006a9ee1deff9887d2b3e97be155ca55a6` and judged the public
brief useful, but its decoded 2,495-byte text included paragraph newlines.
The original `SemanticText` contract correctly rejects those as
`Browser(Extraction(Extraction(InvalidText)))`. Its four declared source IDs
were ordered and present; additional inline markers were plain text, not
independent source-array citations, and were not the rejection cause.

Two bounded corrections follow: the retained projection now reports the exact
original ClosedUnsuccessfully cause without changing clean Failed closure into
Uncertain; the generic mapper contract advertises single-line printable values
in both instructions and trusted ZEXTRACT input. The constrained provider schema
also excludes C0/C1 controls as a generation aid; full Unicode/secret/source and
UTF-8 byte validation remain in Rust, with no normalization or blind retry.
OpenAI documents string `pattern` support for this non-fine-tuned structured-
output path in its [Structured Outputs guide](https://developers.openai.com/api/docs/guides/structured-outputs).
The diagnostic representation uses individually cited statements rather than
one paragraph transcript. Native readiness, authority and budgets are unchanged.

Offline regressions reproduce the equivalent multiline rejection, accept a
separate valid single-line/structured output, reject multiline list items and
missing source IDs, pin provider schema/contract constraints, and exercise held
Failed CAS plus original retained worker/native closure. They do not qualify a
new provider run; a newly pinned, separately authorized v2 run is warranted.

Validation: application `work-execution-probe` library 434 passed / one existing
child-process helper ignored; controller `probe-harness` 46 passed; agentic
`probe-harness provider-transport` 602 passed; retained composition 10 passed.
Strict all-target Clippy passed for agentic, application and composition, as did
desktop diagnostic library Clippy under the required isolated Tauri config,
shipping `macos-work` release library checking, formatting, and the controller,
runtime and agentic-probe architecture gates (including semantic smoke and its
negative control). No GUI, provider retry, or evidence-root mutation was used.
