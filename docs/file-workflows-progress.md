# File workflow implementation and qualification

Working branch: `feature/browser-file-workflows`, isolated at
`/private/tmp/zephium-browser-file-workflows`, based on `35308e16`.
The standalone work was committed as `8bc96fc4`. Current UI integration is
tracked in [the integration record](file-workflows-integration.md). This document
retains the standalone implementation and September 22 qualification evidence.

For current recovery/Windows changes, see [the hardening record](file-workflows-hardening.md).
The sections below preserve the earlier checkpoint evidence.

## Implemented on macOS

- Native macOS upload selection, bounded by exact view/document lifetime.
- Native WKDownload ownership independent of tab lifetime; explicit destination
  selection, profile-scoped preferences/history in the existing Store actor,
  progress, cancellation, Open/Reveal, and history removal.
- Private same-volume staging, exclusive publication, quarantine metadata and
  on-disk identity checks. Publication does not overwrite existing files.
- Store acknowledgements before native bytes are admitted; native cancellation,
  filesystem workers and Store callbacks participate in shutdown/profile erasure.
- Versioned typed IPC, real Downloads views and settings. Live updates use bounded
  native snapshots, rather than polling SQLite for every progress event.
- Interrupted journal entries and identity-checked staging recovery on history
  reads. Linux/Windows native download adapters are still outstanding.

## Live evidence on this Mac (2026-09-22)

- Real native picker opens and returns a selection (user-assisted first check).
- Picker-to-HTTP upload: exact 43 bytes including an embedded NUL received;
  SHA-256 `9afcbed43ee9f711a1f0e82908005cca1291eb57f6e86aed44769638aa50271c`.
- Generated blob download completed with the expected 35-byte text. Reveal
  selected that file in Finder; Open displayed the exact text in TextEdit.
- Attachment download: 32 bytes exactly matching `Zephium native download fixture\n`;
  SHA-256 `af816d943b8b840356cccd91cd2540b50d3c07d2f5b05c7fa128b77e55b4d559`.
  Downloads showed Completed; `com.apple.quarantine` was present (`0281;...`).
- Slow 32 MiB transfer: progress visible; cancellation at 31.4 MiB displayed
  Cancelled and removed the partial payload/staging directory.
- Another 32 MiB transfer: source tab closed at 8.4 MiB; transfer continued,
  completed, matched the expected fixture hash and retained quarantine metadata.
- Multiple selection uploaded both generated files (24 and 43 bytes), including
  the Unicode filename; directory upload delivered the nested 25-byte fixture.
- A cross-origin child frame selected and uploaded the exact 43-byte fixture;
  its picker named `http://localhost:61419`, the requesting frame's own origin.
- Cookie-authenticated `text/plain` attachment (29 bytes) and POST export (20
  bytes) completed with exact bytes and quarantine. Requests remained native.
- Redirected download preserved the original destination file and published
  `zephium-download (1).txt`, with exact bytes and quarantine.
- Native directory selection and Ask preference persisted across app restart;
  previous completed/cancelled downloads remained in history. The final rebuild
  was relaunched and its Downloads tool shows the direct fixture as Completed.
- A direct attachment in a fresh tab now opens a working Save dialog, writes
  exact bytes and leaves a normal New Tab. A subsequent ordinary navigation in
  that same tab renders correctly. Native policy cancellation is distinct from
  a controller failure and never fabricates a committed document.
- Native smoke found and fixed an Objective-C ivar ABI issue: a Rust ULID has
  16-byte alignment, so the delegate now owns it behind a pointer. A regression
  test checks the native ivar alignment bound.

The running bundle can lag subsequent source hardening. Final candidate tests
must be tied to the final build; this list is workflow evidence, not a release
certificate. Windows/Linux testing is deferred by the user to their machines.

## Startup asset failure and fix

The first download-enabled bundle failed its trusted UI startup deadline.
Bounded QA error logging identified failed static JS/CSS loads. Its startup
burst exceeded Wry's 32 in-flight custom-protocol limit. Translation data,
initial shared UI primitives, shared chrome helpers and the initial Svelte
runtime now have explicit shared chunks. Heavy feature implementations remain lazy. A build gate limits
browser/panel static JS+CSS graphs to 24 requests, reserving native capacity.
The fixed bundle starts and its browser HTML requests 23 startup assets.

Measured complete static graphs after regrouping:

| Graph | JS bytes | CSS bytes |
| --- | ---: | ---: |
| Browser | 331334 | 91477 |
| Panel | 184285 | 31265 |
| Download library | 180342 | 8977 |

The former placeholder library's CSS budget is now 9000 bytes, accounting for
its real controls/progress styles and the shared initial UI CSS chunk. Startup
budgets were not raised. Native admission was not relaxed.

## Automated qualification

- Core: 364 tests; app: 350; engine: 381; Store: 281 with 2 explicitly ignored.
- Desktop: 105 tests pass, 2 explicitly ignored; generated bindings match Rust.
- Vendored Wry: 69 tests, including upload completion ownership, bounded attachment
  classification and exact-identity native cancellation without a false commit.
- Frame: typecheck, lint, styles, formatting, dependency boundaries and 192 unit
  tests pass. All 85 existing WebKit component tests and the new Downloads
  transition/action test pass. The emitted production bundle and CSS gate pass.
- Targeted Rust and standalone Wry all-targets Clippy pass with warnings denied. Windows core/engine
  cross-compilation passes (existing vendored platform dead-code warnings remain).
- Native adapter lock/provenance gate passes. The blocker-service graph remains
  free of network/update dependencies; the existing desktop search HTTPS client
  is checked independently. ResourceCall's boxed payload leaves its wire shape
  unchanged and resolves the baseline enum-size lint failure.
- Full `cargo xtask ci` stops at the extension acquisition source-inventory gate:
  `desktop/src/foreground_rendering_probe.rs` resolves
  `desktop/foreground_probe_admission.rs` outside its scanned inventory. The exact
  failure was reproduced on the unchanged `35308e16` snapshot. Full CI is not green.

## Remaining release gates and limitations

- Windows/Linux native downloads are not implemented. Windows retains its native
  website picker; Linux file selection is still denied. Both need implementation
  and real-platform qualification before a cross-platform readiness claim.
- File drag/drop is implemented with bounded native fallback, but actual Finder
  dragging, cloud-provider selections and hostile-page teardown stress remain
  unqualified. Upload cancellation ownership has unit evidence; the delayed page
  timer cannot prove navigation during an AppKit sheet because WebKit may suspend
  page script while that sheet is open.
- Journal recovery has identity/substitution and Store restart tests; forced-crash,
  low-disk, storage-failure and profile-erasure fault injection still need a native
  release qualification pass. Cleanup runs when history is read. Transfers do not
  automatically resume after restart.
- macOS quarantine is implemented and verified; this is not malware scanning.
  Open/Reveal reject missing or changed recorded files. `accept` hints are not
  exposed by the pinned public native picker parameters and are not fabricated.
- Signed/notarized release-artifact qualification remains outstanding. This QA
  bundle is debug-only, unsigned and isolated from the user's normal profile.
- Local-file viewing and Cmd+O were explicitly excluded by the user. Nothing has
  been pushed or applied to the separate UI worktree. The original file-workflow
  checkpoint is committed and is being integrated on a separate branch.

## Repeatable local fixture

Run `python3 scripts/qualification/file_workflows.py --files
/private/tmp/zephium-upload-fixtures` and use the newly printed URL. Tokens are
hexadecimal to avoid the previous trailing-hyphen copy error. The server serves
only fixed generated fixtures, bounds and discards uploaded bytes, and reports
exact byte/hash matches. It has no arbitrary filesystem-serving endpoint.

Current live server (only while its process remains running):
`http://127.0.0.1:61419/b22a26b5c06a12c23ce5943e9d47f635ba31`.
The QA bundle is `target/debug/bundle/macos/Zephium Files QA.app`; its profile is
`app.zephium.files-qa`, and selected download destination is
`/private/tmp/zephium-download-fixtures`. Ask before saving is enabled again.
