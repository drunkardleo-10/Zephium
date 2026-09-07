# Retained public multi-page objective

Status: implementation candidate; no live retained multi-page result yet.

This reuses the existing `macos-work-retained-product-probe` and shipping
`admit_retained_trusted_work` entry, original selected Ready profile, Engine,
Store, resource owner, scoped controller and provider loop. The earlier
[one-page product-entry evidence](macos-retained-product-workflow.md) is unchanged.

The public objective starts at `https://svelte.dev/docs/svelte/overview` and asks
Luna to investigate why a variable destructured from a reactive Svelte 5 state
object fails to update after mutation, recommend a correction, and identify
caveats. It supplies no destination or answer. The approved scope is
`https://svelte.dev/docs/svelte/`, Public/Anonymous/Read, at most two observed-link
hops. Queries, fragments, repeats, redirects, external origins and actions remain
refused. The model may finish early; a zero-hop answer is not multi-page proof.

The same retained resource/profile/lease must survive navigation. Before each
new model turn, old transcript/reference authority is retired, the exact native
terminal is independently core/policy-accounted, and a fresh successor observation
and account attestation must complete. Every observation uses the shipping
bounded native rendering episode and withholds results until presentation and
callback debt retire. No diagnostic rendering holder or legacy page is used.

Existing total limits remain eight model calls, 100,000 tokens, 100,000 micro-USD
and one 150-second absolute run deadline including credential lookup. The public,
release-excluded diagnostic selects `gpt-5.6-luna` and `store:true`; production
remains `store:false`. Source-mapped output uses the existing generic summary,
important-claims and caveats schema, 4,096 total value bytes. Only current-page
sources are supported. Human review of usefulness and claim support is separate
from mechanical acceptance; no page-specific predicate or expected answer passes
the run.

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-navigation-probe.conf.json --features macos-work-retained-product-probe --bundles app --ci --no-sign
```

Use pinned Node 24.18.0/pnpm 11.17.0, committed source, a pinned whole-bundle
inventory and executable, and a fresh isolated `app.zephium.work-navigation-probe`
data root (preserve previous runs). The user, not automation, focuses the exact
app. Keep original sanitized trace/Store and verify hashes after ordinary shutdown.

Required evidence includes model-selected navigation count and provider usage,
fresh observation counts, source-bound result after durable Succeeded ACK, healthy
retained idle resource before destruction, scoped worker drain, and original
Shell/global native shutdown. Inspect retained public requests/responses to judge
route independence and useful source fidelity. This does not qualify authenticated
work, writes, general cross-origin browsing, multi-agent work, human takeover,
repeat reliability, Windows, or rendering needed before native Finished.
