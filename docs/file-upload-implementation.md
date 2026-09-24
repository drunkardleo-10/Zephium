# Native file uploads: first implementation slice

Branch: `feature/browser-file-workflows`, based on UI commit `35308e16`.
Implementation is isolated in `/private/tmp/zephium-browser-file-workflows`;
the original UI worktree and its uncommitted edits are untouched.

## Implemented

macOS foreground human tabs now open a native, window-attached NSOpenPanel when
WebKit requests file selection. The panel displays the canonical origin of the
initiating frame. Single-file, multiple-file and directory modes follow the
native request parameters. Selected NSURLs return directly to the original
WebKit completion; Rust does not read file contents or pass paths through IPC.
There is no persistent grant and no upload-history subsystem.

- `vendor/wry/src/file_upload.rs` defines the native request and a single-use,
  non-Send responder that cancels on drop. The handler is opt-in at construction;
  the default cancels without reading page-controlled metadata.
- `crates/zephium-engine/src/host/file_uploads.rs` owns the panel reservation,
  origin label, exact request identity and document/view validation. At most one
  picker is admitted on the main thread, with no queue or idle polling.
- Raw human-view construction installs the handler. Native navigation start,
  renderer exit, layout exclusion and physical-view teardown cancel pending
  selection. Settlement also rejects a failed intervening navigation attempt,
  a revoked view, loss of visibility or a changed host window.
- Existing privileged and agent-only WebViews do not opt in. The subsequent
  macOS download and file-drop implementation is documented in
  [file workflows](file-workflows-progress.md). Local-file viewing is out of scope.

## Known scope and release gates

The pinned public WKOpenPanelParameters binding exposes multiple/directory
selection but no `accept` filter metadata. The picker therefore does not filter
files using the HTML accept hint; it does not extract page DOM or inject a
privileged script to invent such a contract. Native WebKit retains responsibility
for user activation and the original input/frame association.

Selections are bounded to 1024 native entries. Directory expansion and the
actual HTTP upload belong to WebKit/the website; this is not a whole-file Rust
buffering path. Directory mode, cross-origin frames, cloud-provider files and
native cancellation under navigation/close/crash need interactive qualification.
Linux file selection remains denied. Windows keeps its existing engine-owned
picker; neither platform has been live-tested in this slice.

Downloads use an owned native WKDownload, not the URL-only legacy callback;
see the current [implementation and qualification record](file-workflows-progress.md).

## Reproduce the actual-product check

The fixture creates only fixed harmless files and serves a tokenized loopback
URL. Its upload endpoint bounds the request, compares SHA-256 and byte lengths
against those generated files, reports pass/fail, and discards the body. It does
not serve arbitrary local files or log incoming filenames/content.

```sh
python3 scripts/qualification/file_workflows.py --files /private/tmp/zephium-upload-fixtures
pnpm -C frame build
cd desktop
./node_modules/.bin/tauri build --debug --bundles app --features file-workflows-qa \
  --config tauri.files-qa.conf.json \
  --config '{"build":{"beforeBuildCommand":null}}' --no-sign
```

The debug-only `file-workflows-qa` feature requires the exact isolated
`app.zephium.files-qa` identity, bundled frontend and matching window title.
It cannot be combined with the other QA/staging identities. The QA feature
does not enable the upload implementation: ordinary macOS builds use the same
production construction path.

Open the resulting `target/debug/bundle/macos/Zephium Files QA.app` and navigate
to the printed URL. For each test, reset the form, select generated files and
submit. The report must say `ok: true` and match the intended count/byte hashes.
Use the fixture's delayed navigation button to navigate with a picker open;
then verify that a fresh picker still works. Check cancel, multiple selection,
directory selection and the embedded cross-origin frame separately. A file
input showing a selected filename proves selection, not upload transport.

## Evidence

The current evidence, including actual picker-to-HTTP verification and native
download tests, is maintained in [file workflows](file-workflows-progress.md).
Static checks do not replace signed release qualification or platform testing.
