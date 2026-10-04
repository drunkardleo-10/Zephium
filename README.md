<div align="center">
  <img src=".github/assets/logo.png" width="112" height="112" alt="Zephium" />
  <h1>Zephium</h1>

  <p><strong>A browser-native work environment, rebuilt for you and your agents.</strong></p>

  <p>
    <a href="https://zephium.app">Website</a>
    ·
    <a href="https://github.com/zephium-browser/Zephium/releases/latest">Download</a>
    ·
    <a href="https://discord.gg/tyveTUyEp7">Discord</a>
  </p>

  <p>
    <a href="https://github.com/zephium-browser/Zephium/releases"><img src="https://img.shields.io/github/v/release/zephium-browser/Zephium?include_prereleases&label=release" alt="Latest release" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MPL--2.0-blue" alt="License: MPL-2.0" /></a>
    <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey" alt="Platforms: macOS and Windows" />
  </p>
</div>

---

Zephium is a fast, private, open-source browser built in Rust on your operating
system's own web engine: WKWebView on macOS and WebView2 on Windows. It is not a
fork of Chromium or Firefox. Alongside everyday browsing it has **Work**, a
canvas where you and your agents browse, compare and plan in the open, and keep
the results.

> [!NOTE]
> Zephium is in **beta**. It is ready to try and to give feedback on, and rough
> edges are expected. Please report what you find.

## Screenshots

<p align="center">
  <img src=".github/assets/browse.webp" alt="Zephium in Browse mode" width="900" />
</p>
<p align="center"><sub>Browse: your tabs in a quiet sidebar, the page in front.</sub></p>

<p align="center">
  <img src=".github/assets/work.webp" alt="Zephium in Work mode, with an agent comparing AWS, Vercel, Hetzner and Cloudflare" width="900" />
</p>
<p align="center"><sub>Work: an agent compares AWS, Vercel, Hetzner and Cloudflare on a canvas.</sub></p>

## Features

### Browse

- A native network blocker, on by default, turns away ads and trackers before a
  page can load them, using EasyList and EasyPrivacy, with a count for every
  site.
- Chrome extensions install straight from the Chrome Web Store.
- A launcher on `⌘ ⇧ Space` reaches tabs, tasks, notes, history and downloads
  from anywhere on your desktop.
- Tab sleeping keeps memory low when many tabs are open.
- Notes beside the page, as Markdown files you own.
- Tasks you write the way you would say them, landing on the right day.
- Activity shows where the day went, and Focus keeps distractions shut.
- A welcome flow imports what you already have from other browsers.

### Work

- One switch turns the browser into a canvas, where you describe an outcome in
  your own words.
- Agents work in parallel on live pages, in plain sight, and ask before they
  change anything.
- Agents read and edit the folders you grant, write and explain code, and run
  commands.
- Connect your tools through MCP servers and the command lines you already use,
  such as GitHub and Slack.
- Results stay on the canvas: tables, comparisons and plans, and tasks made
  from a plan.

### Privacy

- No telemetry. Zephium collects nothing about how you browse.
- Local first: history, tasks, notes and activity live on your device, and you
  never need an account to browse.
- Bring the model you prefer: Zephium's own AI, your own keys for OpenAI,
  Anthropic or Gemini, or a model that runs locally.

## Download

Built on the system WebView, so the download is about 30 MB.

| Platform | Architecture | Installer | Requires |
| -------- | ------------ | --------- | -------- |
| macOS | Apple Silicon | [`Zephium-macOS-arm64.dmg`](https://github.com/zephium-browser/Zephium/releases/latest/download/Zephium-macOS-arm64.dmg) | macOS Sonoma 14 or later |
| Windows | x64 | [`Zephium-Windows-x64-setup.exe`](https://github.com/zephium-browser/Zephium/releases/latest/download/Zephium-Windows-x64-setup.exe) | Windows 10 or 11 |

Windows builds are not code-signed yet, so SmartScreen may show "Windows
protected your PC". Choose **More info**, then **Run anyway**. Signing is on
the way.

Intel Macs, Linux, iOS and Android are coming. Zephium downloads updates in the
background and installs them when you choose **Relaunch to update**. All
releases are listed on the
[releases page](https://github.com/zephium-browser/Zephium/releases).

## Build from source

Prerequisites:

- [Rust](https://rustup.rs) through `rustup`. The toolchain pinned in
  [`rust-toolchain.toml`](rust-toolchain.toml) installs itself on first use.
- Node.js, at the version in [`.node-version`](.node-version).
- pnpm, through Corepack: `corepack enable`.
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your
  platform: Xcode Command Line Tools on macOS, or the Microsoft C++ Build Tools
  and WebView2 on Windows.

```sh
git clone https://github.com/zephium-browser/Zephium.git
cd Zephium
pnpm install --frozen-lockfile
pnpm dev
```

Development builds use their own `app.zephium.dev` profile, so they do not
touch the data of an installed Zephium. Linux is not supported yet. See
[CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## Architecture

- Untrusted pages run in raw native WebViews, apart from the privileged Svelte
  interface that draws the browser.
- A Rust core owns tabs, storage, the blocker, extensions and agents.
- Each profile has its own isolated website data and Zephium-owned history,
  session and favicon data.
- The network blocker is native and built on Brave's
  [adblock-rust](https://github.com/brave/adblock-rust).
- Narrow in-tree adapters for Tauri and Wry handle storage policy, WebView
  construction, callbacks and teardown.

Read more in [docs/architecture.md](docs/architecture.md) and
[docs/security-model.md](docs/security-model.md).

## Contributing

Zephium is maintained by one person with a clear product direction, so please
discuss larger changes before writing them. [CONTRIBUTING.md](CONTRIBUTING.md)
explains what gets merged and how. Questions and ideas are welcome on
[Discord](https://discord.gg/tyveTUyEp7).

## Security

Please report vulnerabilities privately, not in a public issue. See
[SECURITY.md](SECURITY.md).

## License

Zephium is licensed under the [Mozilla Public License 2.0](LICENSE).

The bundled EasyList and EasyPrivacy filter lists are used under
[CC BY-SA 3.0](assets/blocker-seed/v1/LICENSE-CC-BY-SA-3.0.txt); see
[the notice](assets/blocker-seed/v1/NOTICE) for attribution.

The Zephium name and logo are not licensed under the MPL.

## Acknowledgements

Zephium stands on [Tauri](https://tauri.app), [Wry](https://github.com/tauri-apps/wry),
Brave's [adblock-rust](https://github.com/brave/adblock-rust) and
[EasyList](https://easylist.to).
