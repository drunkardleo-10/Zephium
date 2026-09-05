# Hidden macOS rendering readiness diagnostic

Status: release-excluded, provider-free measurement; no runtime policy change.

## Why this diagnostic exists

The separately recorded 2026-09-05 `commerce-product` attempt navigated exactly,
then correctly refused a complete bounded 17-node successor snapshot with none
of its three required facts. The retained output cannot identify those nodes
or establish a public-site root cause. See the [failed witness](m6-production-qualification.md).

Source inspection establishes two separate candidate seams:

- The native navigation gate settles an exact commitment, not load finish.
  The semantic channel becomes dispatchable at commit, and the immutable
  runtime accepts `interactive` or `complete` document readiness. Snapshot
  `Complete` describes the bounded capture, not future page stability.
- Read-only inspection of the [public product response](https://demo.vercel.store/product/acme-geometric-circles-t-shirt)
  found the product description inside a hidden streamed container. Its reveal
  script can queue that content behind `requestAnimationFrame`. The reviewed
  [pinned product template](https://github.com/vercel/commerce/blob/3761e52e60df9c6a316e067dbfd7032e494d3634/app/product/%5Bhandle%5D/page.tsx)
  wraps the description in a Suspense boundary with an empty fallback. This
  does not prove the deployed revision or the failed native page's state.

The production owned WKWebView remains hidden and attests the existing
`WKInactiveSchedulingPolicy::Throttle`. WebKit describes inactive-page RAF
stopping independently from timer throttling in its
[power guidance](https://webkit.org/blog/8970/how-web-content-can-affect-power-usage/).
[Apple's public scheduling policy](https://developer.apple.com/documentation/webkit/wkpreferences/inactiveschedulingpolicy-swift.enum)
distinguishes throttling from suspension; it does not itself promise that RAF
runs in a hidden view. Thus a load-complete check or an arbitrary delay is not
yet a justified fix. Neither projection changes nor relaxed commerce predicates
are justified by these observations.

## Closed experiment

Build the existing release-excluded semantic probe, then use its exact new mode:

```sh
cargo build --locked -p zephium-engine --features native-agentic-semantic-probe --bin macos-agentic-semantic-probe
target/debug/macos-agentic-semantic-probe --ci-hidden-rendering-readiness
```

The fixed loopback fixture reveals three separate initially hidden paragraphs
through a Promise microtask, one zero-delay timer, and one animation-frame
callback. Separate fixed markers report document-ready changes and the load
event. No external resource, keychain, provider, session, user input, page
permission or public-site dependency exists. The unchanged fixture server is
loopback-bound, budgeted, joined, and serves only fixed routes with a restrictive
CSP; the new page has no subresource or connection path.

The new mode reuses the exact production owned-view constructor, standard
1280×800 logical viewport, ephemeral profile, hidden/non-key/non-active focus
guard, semantic isolated-world channel and native policy/runtime attestation.
It admits only Observe and Navigate, not Act. The original exact navigation
operation supplies native-finish facts before and after each capture; no page
marker replaces that authority. The existing no-redirect navigation behavior
and semantic runtime remain unchanged.

At most eight semantic invocations sample offsets 0, 50, 100, 200, 400, 800,
1600 and 3200 ms from the committed-navigation return, under one five-second
measurement deadline. Each slot performs at most one exact invocation, with
fresh invocation/snapshot identities and unchanged initial capture limits.
This is a diagnostic sampling window, not a new production readiness retry,
fixed wait, or success predicate. The native run loop continues pumping its
existing bounded slices. A typed DocumentLoading receipt remains distinct from
an empty successful snapshot; other faults stop the diagnostic.

Successful samples require exact document/origin/frame joins, complete bounded
snapshots, no child boundary and unique public fixture markers in the expected
roles. Missing control markers do not prove RAF starvation. Negative RAF
classification requires converged native-finish/document-complete/load/
microtask/timer controls and completion of the whole 3200 ms window. Positive
classification can stop early once all controls and the RAF reveal are seen.
Missing RAF means only **not observed within this bounded window**, not never.

Content-free sample records and their disposition are returned only after the
same original policy/runtime registrations, page, window, ephemeral store and
fixture worker drain. A teardown fault returns failure, not a successful report.
No production scheduling preference, visibility, semantic program, provider
contract, Work predicate or default Browse path changes in this diagnostic.

## Proof boundary

Deterministic tests cover incomplete/contradictory controls, shortened negative
windows, wrong contexts/origins, truncated snapshots, frame boundaries,
duplicate markers and wrong-role/sensitive substitutes. Architecture mutations
guard sampling/deadline bounds, exact native-finish joins, teardown, the closed
CLI, independent fixture controls and absence of scheduling/script/provider
authority. Actual native results are recorded separately after a pinned run.

Even a confirmed hidden-view RAF stall on this fixture is not proof that a
particular scheduling change fixes the public commerce site. Any production
policy change requires separate design, lifecycle/resource review and native
qualification; this diagnostic never changes policy to force a result.
