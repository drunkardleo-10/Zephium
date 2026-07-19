# Vendored `tauri-runtime-wry` provenance

This directory is `tauri-runtime-wry` 2.11.3 from Tauri revision
`6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a`:

<https://github.com/tauri-apps/tauri/commit/6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a>

It is a narrow runtime adapter fork, not an unmodified crates.io package. Its
local invariants are:

- asynchronous `CreateWindow` and `CreateWebview` requests return only after
  the event-loop handler has either committed the native wrapper to its
  registry or returned the exact construction error;
- a missing or reentrantly removed parent fails with `WindowNotFound`, so no
  detached logical handle is fabricated for absent native state;
- failed embedded-WebView construction removes the provisional Tao window-ID
  mapping and the newly admitted context label reference;
- retained Web contexts and Linux custom-protocol registration markers are not
  described as rolled back: those native/context effects may outlive a failed
  builder stage and remain available for explicit teardown or retry; and
- GTK composition lookup is fallible and a dropped getter receiver cannot
  abort the browser process.

The standalone manifest patches Wry to `../wry`, ensuring fork CI exercises
the same native adapter as Zephium. Cargo ignores this nested patch table when
the repository workspace is the build root; the root manifest owns that build.
The standalone lockfile is intentional and must be reviewed alongside the
workspace lockfile.

Follow [`REBASE.md`](REBASE.md) for every update. Passing source-level tests is
not a substitute for packaged native startup and failure-injection tests on all
supported platforms.
