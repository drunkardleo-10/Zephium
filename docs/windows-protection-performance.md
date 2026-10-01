# Windows protection results

Measured 2026-09-30 / 2026-10-01 on an Intel i3-1115G4 (2 cores / 4 threads),
12 GB RAM, Windows 11 build 26200, Balanced power, WebView2 154.0.4258.48.
Raw runs, screenshots and binary hashes stay local under `target/`.

Protection ships **on by default** on Windows, as on macOS.

## Behavior (Zephium Protection QA app)

- Three clicks in a row on a strict-CSP page (`script-src 'none'; style-src
  'none'`, Trusted Types) hid three elements; Undo restored only the newest,
  Done kept the rest, and both survived restarts.
- The site switch reloads the page; the fixture's ad script arrives only while
  paused and is blocked again after resuming.
- The new-tab counter counts successful blocks only, ignores paused loads and
  persists across restarts.

## Request callback

Blocking reuses one empty `403` response per view instead of creating one per
request. Fixture of 100 ad + 100 useful scripts, five alternating trials; every
ad blocked before reaching the server, every useful script ran.

| Median across trials | Before | After |
| --- | ---: | ---: |
| Blocked callback p50 / p95 | 10.3 / 33.4 µs | 6.2 / 24.0 µs |
| Allowed callback p50 | 7.4 µs | 7.5 µs |

On real pages the full callback stays under ~70 µs p50 and ~125 µs p95. Paused
views skip the matcher (p50 0.1 µs). This excludes WebView2's own cross-process
dispatch, which the callback cannot see.

## Real pages, protection off / on

Native adapter harness (not the full browser), one resident view, fresh
profiles, three alternating trials, medians. Both arms hold the compiled policy,
so the difference is interception plus page work.

| Page | Load, ms | Peak private, MiB | CPU, s | Blocks |
| --- | --- | ---: | ---: | ---: |
| Clean local page | 414 / 560 (ranges overlap) | 171 / 174 | 2.48 / 2.41 | 0 |
| Yahoo | 1,264 / 1,026 | 349 / 365 | 7.83 / 7.48 | 14 |
| Bloomberg | 6,167 / 6,124 | 973 / 976 | 16.3 / 17.1 | 16–20 |
| Guardian article | 862 / 791 | 233 / 235 | 3.84 / 3.86 | 11 |
| GitHub repo | 1,909 / 2,365 (ranges overlap) | 418 / 451 | 6.72 / 6.88 | 3 |

Ad-heavy pages load as fast or faster; peak memory is never lower. The likely
cause is the per-document generic cosmetic index (about 487 KB per page, shared
with macOS); moving it out of the page is the next optimization.

With 10 and 30 resident views all share one immutable policy; 30 views handled
3,000 blocked + 3,000 allowed requests with no errors or budget exhaustion.
Five-minute idle with ten views: 1.0–1.5 CPU s whether protection is on or off.

## Browser efficiency

| Change | Before | After |
| --- | ---: | ---: |
| Discard safety probe, 100k-element page | 16.6 ms | 0.4 ms |
| Removed shadow roots kept alive (20 removed) | 20 | 0 |
| Nested cosmetic mutation scan steps | 12,352 | 160 |
| Release app idle, minimized new tab, private memory | 367 MiB | 352 MiB |

The idle saving comes from loading the panel document on first use.

## Coverage gaps

- WebSockets produce no `WebResourceRequested` event and cannot be blocked.
- Shared and service worker requests are not filtered. Covering them needs one
  environment-wide owner; registering them on every view duplicates work.
- Navigations and worker scripts arrive as `Document`/`Other`, so rules typed
  for scripts or subdocuments do not apply to them, and there is no reliable
  initiating-frame URL for party rules.

## Not yet measured

Full-browser foreground load and startup with restored sessions, wakeups and
energy (ETW), low-memory pressure and suspend/resume stress, and native
private-window interaction.

## Reproduction

The qualifier is an ignored test in
`crates/zephium-engine/src/platform/windows/content_filter_qualification.rs`.
Build it with `cargo test --release -p zephium-engine --lib --no-run`, then:

```powershell
./scripts/qualification/windows-protection.ps1 -Binary <zephium_engine-*.exe> `
  -Site yahoo -Mode on -Tabs 1 -OutputDirectory target/protection-evidence/<run>
```

Sites: `fixture`, `clean`, `csp`, `coverage`, `yahoo`, `bloomberg`, `news`,
`github`, `google`. Modes: `off`, `on`, `paused`. Tabs: 1, 10, 30.
`scripts/qualification/protection-ui-fixture.cjs` serves the QA app fixture.
For whole-browser numbers, build with
`pnpm -C desktop exec tauri build --no-bundle --config tauri.performance.windows.conf.json`
and sample with `scripts/qualification/windows-browser-resources.ps1`.
