# Retained observation rendering qualification

Status: deterministic/build checks pass; actual-app native qualification pending.

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
