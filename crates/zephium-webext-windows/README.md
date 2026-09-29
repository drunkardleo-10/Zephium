# Windows extension lab

Native extension qualification only. No product crate depends on this crate. The empty
default feature set exposes no runtime; the binary requires `lab` and refuses
release builds. Wry's existing extension startup boundary is unchanged.

Run from the repository root as a normal, **non-elevated Windows user**:

```powershell
cargo build --locked -p zephium-webext-windows --features lab --bin webext-lab-windows
powershell -NoProfile -ExecutionPolicy RemoteSigned -File crates\zephium-webext-windows\run-probe.ps1
```

`-ExecutionPolicy RemoteSigned` applies only to this PowerShell process; it does
not change the machine or user execution policy. A previous failed command makes
`$LASTEXITCODE` nonzero, so rerun the command directly rather than wrapping it in
a condition on that old exit code.
The runner expects the five signed packages identified by the macOS suite in
`target/webext-suite/<id>.crx`. The runner does not download or silently replace
them. Native messaging and removal-residue checks are not implemented in this
initial, priority-ordered probe.

Results are saved under `target/webext-windows-probe/<timestamp>/`: machine
metadata, exact scenarios, JSONL observations, stderr, popup screenshots and
process-family CSV samples. The default resource pass has two 60-second
baselines (extensions disabled/enabled, three tabs each), then 600 seconds with
Bitwarden, Dark Reader and Grammarly on three tabs. `-SkipResources` runs only
capabilities. `-IdleSeconds` changes the measured duration and is recorded.
The additional access/action scenario checks revocation and a 22-line diagnostic
action-call observer; the worker scenario leaves the diagnostic worker without
messages for 45 seconds and then wakes it through a new content script.

Lab host windows are hidden unless a scenario requests a visible human/popup window.
The resource measurements describe hidden controllers,
not foreground UI performance. There is no synthetic user activation: a
host-opened popup or script `.click()` must not be reported as a real toolbar
action click. A missing tab mapping or gesture path is a finding to report,
not permission to implement a replacement `chrome.tabs` or action dispatcher.

## Direct scenarios

```powershell
target\debug\webext-lab-windows.exe crates\zephium-webext-windows\scenarios\diagnostic.json target\new-disposable-lab-directory
```

The data directory must not already exist. It is retained for evidence, never
deleted automatically. The lab authenticates the actual user data folder,
profile name, non-private mode and reused COM environment in Wry's startup
gate. Its extension-enabled environment is created only within this process,
against fresh owned storage. No browser profiles are opened.

A scenario is an array of steps:

- `load`: signed `.crx` or an unpacked lab fixture. CRX3 signatures and an ID-shaped
  filename are validated by `zephium-webext`; extraction uses its existing
  bounds/path checks. Only the signed public key is added to the unpacked
  manifest to preserve identity. Runtime ID must match. No compatibility code
  is injected into real extensions.
- `view`: create a named hidden view, optionally in another `profile`. Default
  profile is `LabHuman`; `in` selects a view for subsequent operations and
  defaults to `tab1`.
- `navigate`, `popup`, `sleep` (milliseconds), `list`, `eval` (async function body),
  `cdp` with optional `params`, and `screenshot` (a filename within lab data).
- `$ID`, `$ORIGIN` and `$DENIED` expand in navigation/evaluation to the last loaded
  extension ID, a private loopback fixture at `127.0.0.1`, and the same fixture
  addressed as `localhost`. The diagnostic extension has only the first origin
  granted initially; localhost is optional.
- An initial `extensions_enabled: false` selects an extension-free baseline.

`--prepare <crx> <NEW-directory>` validates/extracts a package without starting
WebView2. Package records include version, manifest version and SHA-256.

## QA follow-up qualifiers

- `run-browser-surfaces.ps1` checks Grammarly's native OAuth-window adoption,
  native extension context-menu entries, and 1Password's empty runtime popup
  and separate welcome document. It reports failed native action commands as
  findings. No account is used and no extension API is replaced. Its DOM/CDP
  clicks are diagnostic, not physical user activation. `adopt_new_windows` is
  a lab-only scenario flag for up to four HTTP(S) native child windows; default
  scenarios continue denying all new-window requests.
- `run-access-transition.ps1` replaces a keyed test extension from a different,
  narrowed directory without removing its native ID. It asserts local storage
  survival, the native manifest change, and allowed/denied content injection.
- `run-management-resources.ps1` compares 1/3/5 cumulative store extensions with
  persistent per-extension pages and one shared page parked on `about:blank`.
  Each fresh profile idles for 120 seconds; use `-Counts 1 -Modes shared` for one
  case. It reports sample gaps. No worker debugger is attached during idle.
- `fixtures/storage` is a keyed, account-free extension for the actual QA UI.
  Its popup saves and reads a visible `chrome.storage.local` marker. Install as
  an unpacked folder, change site access, and reopen its popup to inspect data.

`ok` in a step record means the harness operation returned, **not extension
compatibility**. Inspect CDP `exceptionDetails`, returned error objects and
actual page effects. A `complete` record means the scenario finished, not that
all steps passed. Startup is bounded to 45 seconds; individual asynchronous
calls have a 20-second deadline. The runner also bounds each child process.

The diagnostic fixture reports native API results. It does not replace tab,
permission or action APIs. Selected-profile and `agent-lab` views reproduce
relevant profile-sharing topologies; they do not execute Zephium's production
Work adapter. Report topology evidence separately from product qualification.

CDP is used only for deliberate diagnostic operations. Resource scenarios do
not attach to service workers or evaluate scripts during idle sampling. Memory
is recorded per process; shared process costs cannot be assigned exactly to an
individual extension. Preserve raw CSVs and report warm-up and late-run trends.
Summarize the process-family CSVs with:

```powershell
node crates\zephium-webext-windows\summarize-resources.mjs target\webext-windows-probe\<timestamp>
```

Private bytes are the main memory comparison. Summed working sets double-count
shared pages. CPU is cumulative process time converted to a percentage of one
core; it includes the lab's message pump and fixture server. Short-lived processes
between samples are not captured. These are exploratory measurements, not a
foreground browser performance benchmark.

After collecting and reviewing evidence, append findings to
`docs/windows-extensions-handoff.md` and stop. Do not continue to step 2.
