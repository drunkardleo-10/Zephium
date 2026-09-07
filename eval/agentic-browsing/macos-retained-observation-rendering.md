# Retained observation rendering qualification

Status: deterministic/build checks and the actual-app native RAF qualification
pass (2026-09-07). This is not a model-driven multi-page qualification.

This slice puts presentation inside the shipping retained observation operation.
The existing release-excluded `macos-work-resource-probe` actual-app launcher now
uses that operation over its exact loopback RAF fixture, without acquiring the
old diagnostic rendering holder. It retains the same native page across two
different actor leases and rejects the first lease after revocation.

Positive evidence requires exact document/load/microtask/timer controls and the
RAF-gated semantic content, followed by an independent native inspection 400 ms
after each result was delivered. The page must be hidden, no observation or
presentation may remain pending, and every completed semantic invocation must
have a completed production presentation episode. Both lease stamps must retain
the original resource/view/document/isolated world. Original normal application
shutdown must prove all native resource classes and exact weak page/window/store
owners drained. The fixture listener is also retired. No API key, provider,
authenticated service, arbitrary site or script capability is used.

Deterministic coverage includes original semantic correlation/once-only result,
preparation through retirement cancellation, delayed presentation/semantic and
watchdog-return debt, original physical permit release, finite unclipped geometry,
each human foreground fact, source-document stamp invalidation, and mechanical
ordering/exclusion checks. The production and diagnostic engine graphs build;
the full diagnostic engine suite passes 574 tests and strict all-target Clippy.

Reproducible build uses Node 24.18.0 and pnpm 11.17.0:

```sh
pnpm -C desktop exec tauri build --debug --config tauri.work-rendering-probe.conf.json --features macos-work-resource-probe --bundles app --ci --no-sign
```

The isolated application identity is `app.zephium.work-rendering-probe`. Its
app-data directory must be fresh; previous diagnostic runs must be preserved,
not erased or silently restored. Commit the source and pin the executable and
complete bundle manifest before launch. The exact app needs user-owned foreground;
the qualifier does not activate it. Verify the bundle is unchanged after normal
exit. This witness cannot establish background rendering, site-wide readiness,
production browser interaction or a model-driven multi-page workflow.

## Reviewed actual-application result

One authorized launch used clean source
`644930ee619b104c449322136ed971f84230c001`, the previously absent isolated
application-data root, and the normal bundled Tauri startup/event/shutdown loop.
The source-pinned arm64 executable SHA-256 was
`4f8372bdb958efec1cfef2ba319e2c4c97b50be065c1162adf58be96302770ce`.
The canonical complete [bundle inventory](macos-retained-observation-rendering.bundle.json)
SHA-256 was `412f878e085073282af59abbfe6f26867d6fe73a9a36083e3d659acfe3d95e0f`.
Both hashes were unchanged after exit; no exact qualifier process remained.
The pre-launch inventory was checked at 18:10:13 UTC and post-exit inventory at
18:11:13 UTC. These are inventory times, not inferred launch duration.

The actual app admitted the existing foreground owner after 68 ms and two
checks. It did not use the diagnostic rendering holder. Both distinct actor
leases performed a shipping observation-owned presentation on the same native
page/document/isolated world, and completed these samples:

| Lease / sample | Elapsed since construction receipt (ms) | Nodes | Document/load/microtask/timer controls | RAF-gated content |
| --- | ---: | ---: | --- | --- |
| A / 0 | 518 | 7 | true | true |
| B / 1 | 1,810 | 7 | true | true |

These are cumulative witness sample times, not isolated rendering or snapshot
latencies. The second includes the deliberate post-delivery wait and lease
transition; it observes the retained document, not a newly navigated document.
Each result was withheld until the production presentation retired. Independent
native inspection 400 ms after each delivery found the page hidden, no pending
read/holder, and production presentation completions equal to native semantic
completions (1, then 2).

Both exact lease-ended receipts arrived. Core and native adapters rejected the
stale first lease. Native page/document/world identity stayed unchanged. Resource
core quiescence was true after destruction. The driver reported
`ResourceRetainedAcrossLeases` at 2,919 ms, with `cleanup_failure=None`,
`native_cohort_clean=true`, `human_ownership_preserved=true`, and
`fixture_clean=true`. Normal app exit returned 0 and the final join was:

```text
work-rendering-closure: qualified=true normal_shutdown_clean=true exact_native_weak_drain=Some(true)
```

The old diagnostic holder's native-failure trace was explicitly unavailable
(`available=false`); no clean failure-trace result is inferred from its `None`
fields. Positive closure instead joins the shipping observation's retirement
gate, independent native inspection, both original lease receipts, original
resource destruction, native cohort audit, exact weak page/window/store drain,
fixture retirement, and ordinary application shutdown.

The isolated diagnostic data is preserved. No provider call, credential access,
background-rendering claim, authenticated workflow, takeover test, Windows
qualification or real-site readiness/performance generalization is implied.
The next product-facing proof remains a retained multi-page open-objective run.
