<div align="center">
  <img src="../../.github/assets/logo.png" width="112" height="112" alt="Zephium" />
  <h1>Zephium</h1>

  <p><strong>高速で、機能の揃ったブラウザ。<br />あなたとエージェントのために作り直した、仕事の環境。</strong></p>

  <p>
    <a href="https://zephium.app">Web サイト</a>
    ·
    <a href="https://github.com/zephium-browser/Zephium/releases/latest">ダウンロード</a>
    ·
    <a href="../README.md">ドキュメント</a>
    ·
    <a href="https://discord.gg/tyveTUyEp7">Discord</a>
  </p>

  <p>
    <a href="https://github.com/zephium-browser/Zephium/releases"><img src="https://img.shields.io/github/v/release/zephium-browser/Zephium?include_prereleases&label=release&color=blue" alt="最新リリース" /></a>
    <a href="../../LICENSE"><img src="https://img.shields.io/badge/license-MPL--2.0-blue" alt="ライセンス: MPL-2.0" /></a>
    <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey" alt="対応プラットフォーム: macOS と Windows" />
    <a href="https://discord.gg/tyveTUyEp7"><img src="https://img.shields.io/badge/Discord-5865F2?logo=discord&logoColor=white" alt="Discord" /></a>
    <a href="https://www.youtube.com/@crynta"><img src="https://img.shields.io/badge/YouTube-FF0000?logo=youtube&logoColor=white" alt="YouTube" /></a>
  </p>
</div>

<p align="center">
  <a href="../../README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a> |
  <a href="README.es.md">Español</a> |
  <a href="README.de.md">Deutsch</a> |
  <a href="README.fr.md">Français</a> |
  <strong>日本語</strong> |
  <a href="README.ko.md">한국어</a> |
  <a href="README.pt-BR.md">Português</a> |
  <a href="README.pl.md">Polski</a> |
  <a href="README.ru.md">Русский</a> |
  <a href="README.id.md">Bahasa Indonesia</a> |
  <a href="README.hi.md">हिन्दी</a>
</p>

<p align="center">
  <img src="../../.github/assets/browse.webp" alt="Browse モードの Zephium。サイドバーにタブが並び、zephium.app が開いている" width="960" />
</p>

<p align="center">
  <strong>Safari の効率。Brave の保護。Arc のデザイン。</strong><br />
  そして <strong>Work</strong>。エージェントが目の前で本物の仕事をこなすキャンバスです。
</p>

---

Zephium は、Rust で作られたオープンソースのブラウザです。OS 標準の Web エンジンを土台にしていて、macOS では Safari と同じ WebKit、Windows では WebView2 を使います。Chromium や Firefox のフォークではありません。広告とトラッキングをネイティブにブロックし、Chrome 拡張機能も動かせます。タスク、ノート、Web で過ごす時間も、ワンクリックで手元に開けます。ダウンロードサイズは約 30 MB です。

スイッチひとつで **Work** に切り替わります。仕事はもともとブラウザの中、つまりタブやログイン情報、履歴のある場所で行われています。Work は、どこか別の場所へ移るのではなく、エージェントのほうをそこへ連れてきます。しかも、その仕事ぶりを見届けられます。

> [!NOTE]
> Zephium は **ベータ版** です。日常的に使えますし、フィードバックも歓迎していますが、粗いところが見つかるかもしれません。見つけた点は[ぜひ報告してください](https://github.com/zephium-browser/Zephium/issues)。

## Work

求める結果を、ふだんの言葉で伝えてください。旅行の計画、ベンダーの比較、システムの設計、バグの修正など。あとは Work がキャンバス上で引き受け、すべてのステップが見えます。

<p align="center">
  <img src="../../.github/assets/work.webp" alt="Work モードの Zephium。エージェントが AWS、Vercel、Hetzner、Cloudflare を比較し、リファレンスアーキテクチャを描いている" width="960" />
</p>
<p align="center"><sub>AI SaaS 向けのホスティングの比較を頼むと、Work は料金ページを読み、構成を提案し、アーキテクチャ図を描きます。</sub></p>

- **すでにあるものから始まります。** Work はあなたについて知っていることを思い出し、役に立つときは履歴を検索し、その仕事に合ったスキルを読み込みます。各ステップは、実行されるたびにキャンバスに現れます。
- **ヘルパーが並行して、オープンに作業します。** 旅行なら、ひとりが物件を探し、別のひとりがビザを調べ、もうひとりが航空券を比べます。ヘルパーはライブのページを閲覧し、その様子は見ることができます。どの情報源も、キャンバスの横のペインで開けます。
- **結果はキャンバスに残ります。** 比較、表、グラフ、図、計画、コード、ドキュメントが、読みやすい位置に並びます。流れていくチャットの中に埋もれることはありません。
- **計画が、あなたの一日になります。** ワンクリックで、計画の各ステップが、その Work にリンクしたタスクになります。残しておきたいものは、ノートとして保存できます。
- **ブラウザの外にも手が届きます。** Work は、許可したフォルダを読み書きし、コマンドを実行し、大きなコーディング作業は Claude Code や Codex に任せます。Linear、Notion、Sentry、Stripe、Figma などの MCP サーバーや、`gh` のようなコマンドラインツールにも接続できます。
- **主導権はあなたにあります。** 投稿、マージ、変更を伴う操作は、あなたの OK を待ちます。
- **好きなモデルを使えます。** Anthropic、OpenAI、Google Gemini、DeepSeek、OpenRouter は自分のキーで利用でき、Ollama、LM Studio、OpenAI 互換のサーバー経由でローカルモデルも動かせます。キーはシステムのキーチェーンに保存されます。

Work には 25 のスキルが付属しています。旅行の計画、リサーチ、比較して選ぶ、今日の計画、バグの修正、週次ステータスなどです。自分でスキルを書くこともできます。

## Browse

### 速くて軽い

- macOS ではネイティブの WebKit、Windows では WebView2 を使うので、ページは OS がすでに最新に保っているエンジンで動きます。
- 使っていないタブはスリープし、戻ると復帰するので、タブをたくさん開いてもメモリ使用量は低く抑えられます。
- ネイティブの広告・トラッカーブロッカーが、デフォルトでオンです。Brave の [adblock-rust](https://github.com/brave/adblock-rust) を土台に、EasyList と EasyPrivacy を使っています。リクエストはページが発行する前に止められ、それ以外にページ上で隠したいものは、クリックひとつで非表示にできます。

### 住み続けられるデザイン

- 落ち着いたサイドバーに縦型のタブ。スペース、プロファイル、ピン留めタブ、フォルダ、分割表示に対応しています。
- macOS 26 では Liquid Glass、Windows 11 では Mica。
- `⌘ ⇧ Space`（Windows では `Ctrl Shift Space`）のランチャーから、デスクトップのどこからでもタブ、履歴、ノート、コマンドにアクセスできます。そこで入力した内容は、そのままタスクにできます。
- キーボードショートカットは、すべて変更できます。

### 拡張機能

Chrome ウェブストアから、そのまま拡張機能をインストールできます。人気の 20 本は Zephium で問題なく動くことを確認済みで、拡張機能マネージャーからワンクリックで入れられます。1Password、Bitwarden、Grammarly、DeepL、Dark Reader、Vimium、SponsorBlock、Raindrop.io、Notion Web Clipper、Refined GitHub などが含まれます。

### すべてを持ち込めます

ウェルカムフローでは、Chrome、Safari、Arc、Zen、Firefox、Brave、Edge からインポートできます。

## 内蔵機能

<table>
  <tr>
    <td width="33%" align="center"><img src="../../.github/assets/tasks.webp" alt="Tasks: ステータス、期限、リスト、優先度、リンクされたページを持つタスク" /></td>
    <td width="33%" align="center"><img src="../../.github/assets/time.webp" alt="Time: 今日は Web で 42 分、フォーカスタイマー、サイトごとの時間" /></td>
    <td width="33%" align="center"><img src="../../.github/assets/notes.webp" alt="Notes: ページの横に開いた、見出し、リスト、コードを含む Markdown ノート" /></td>
  </tr>
  <tr>
    <td valign="top"><strong>Tasks</strong><br /><sub>話しかけるように書けます。たとえば「明日の 3 時にアンナに電話」。リスト、優先度、期限、サブタスクに対応し、開いていたページもリンクされたまま残ります。</sub></td>
    <td valign="top"><strong>Time</strong><br /><sub>Web での時間がどこに使われているかを確認できます。集計はこのデバイス上だけで行われます。Focus ラウンドを始めると、選んだサイトは休憩まで閉じたままになります。</sub></td>
    <td valign="top"><strong>Notes</strong><br /><sub>ページの横にノートを置けます。ノートはどれも、あなた自身のものとして残る Markdown ファイルです。ほかのアプリでの編集は、Zephium にも反映されます。</sub></td>
  </tr>
</table>

## デフォルトでプライベート

- **テレメトリはありません。** Zephium には解析機能がなく、閲覧の仕方に関する情報は一切収集しません。
- **アカウントは不要です。** 履歴、タスク、ノート、メモリ、時間のデータは、あなたのデバイスに保存されます。
- **接続は少なく、すべて明らかです。** 訪問するサイトのほかに Zephium が接続するのは、アプリのアップデート、フィルターリストの更新、検索候補、拡張機能のインストール、そしてあなたが選んだ AI プロバイダーと MCP サーバーだけです。
- **プライベートウィンドウは、閉じると何も残しません。**

## ダウンロード

| プラットフォーム | アーキテクチャ | インストーラー | 動作環境 |
| -------- | ------------ | --------- | -------- |
| macOS | Apple Silicon | [`Zephium-macOS-arm64.dmg`](https://github.com/zephium-browser/Zephium/releases/latest/download/Zephium-macOS-arm64.dmg) | macOS Sonoma 14 以降 |
| Windows | x64 | [`Zephium-Windows-x64-setup.exe`](https://github.com/zephium-browser/Zephium/releases/latest/download/Zephium-Windows-x64-setup.exe) | Windows 10 または 11 |

Windows 版にはまだコード署名がないため、SmartScreen に「Windows によって PC が保護されました」と表示されることがあります。その場合は **詳細情報** を選び、**実行** をクリックしてください。署名は準備中です。

Zephium はアップデートをバックグラウンドでダウンロードし、**Relaunch to update** を選ぶとインストールします。すべてのリリースは[リリースページ](https://github.com/zephium-browser/Zephium/releases)にあります。

## 今後の予定

- Linux と Intel Mac への対応。
- コード署名つきの Windows 版。
- オプションの Zephium Cloud。Work 向けのホスト型 AI で、試せる無料プランもあります。
- 翻訳など、重い拡張機能の代わりになる内蔵機能の追加。

## ソースからのビルド

前提条件:

- `rustup` 経由の [Rust](https://rustup.rs)。[`rust-toolchain.toml`](../../rust-toolchain.toml) で固定されたツールチェーンは、初回の使用時に自動でインストールされます。
- Node.js。バージョンは [`.node-version`](../../.node-version) のとおりです。
- pnpm。Corepack 経由で有効にします: `corepack enable`。
- お使いのプラットフォーム向けの [Tauri の前提条件](https://v2.tauri.app/start/prerequisites/)。macOS では Xcode Command Line Tools、Windows では Microsoft C++ Build Tools と WebView2 です。

```sh
git clone https://github.com/zephium-browser/Zephium.git
cd Zephium
pnpm install --frozen-lockfile
pnpm dev
```

開発ビルドは専用の `app.zephium.dev` プロファイルを使うので、インストール済みの Zephium のデータには影響しません。Linux にはまだ対応していません。プルリクエストを出す前に、[CONTRIBUTING.md](../../CONTRIBUTING.md) をお読みください。

## アーキテクチャ

- 信頼できないページは、ブラウザを描画する特権付きの Svelte インターフェースとは別に、素のネイティブ WebView の中で動きます。
- Rust 製のコアが、タブ、ストレージ、ブロッカー、拡張機能、エージェントを管理します。
- プロファイルごとに、独立した Web サイトのデータと、Zephium が管理する履歴、セッション、ファビコンのデータを持ちます。
- ネットワークブロッカーはネイティブで、Brave の [adblock-rust](https://github.com/brave/adblock-rust) を土台に作られています。
- Tauri と Wry 向けの、リポジトリ内にある小さなアダプターが、ストレージポリシー、WebView の構築、コールバック、破棄を担当します。

詳しくは [docs/architecture.md](../architecture.md) と [docs/security-model.md](../security-model.md) をご覧ください。

## コントリビュート

Zephium は、明確なプロダクトの方向性を持つ一人がメンテナンスしています。大きな変更は、書き始める前にご相談ください。何がどのようにマージされるかは、[CONTRIBUTING.md](../../CONTRIBUTING.md) にまとめてあります。質問やアイデアは、[Discord](https://discord.gg/tyveTUyEp7) で歓迎しています。

## セキュリティ

脆弱性は、公開の issue ではなく、非公開で報告してください。[SECURITY.md](../../SECURITY.md) をご覧ください。

## ライセンス

Zephium は [Mozilla Public License 2.0](../../LICENSE) のもとでライセンスされています。

同梱の EasyList と EasyPrivacy のフィルターリストは、[CC BY-SA 3.0](../../assets/blocker-seed/v1/LICENSE-CC-BY-SA-3.0.txt) のもとで使用しています。帰属表示については[告知](../../assets/blocker-seed/v1/NOTICE)をご覧ください。

Zephium の名称とロゴは、MPL のライセンス対象ではありません。

## 謝辞

Zephium は、[Tauri](https://tauri.app)、[Wry](https://github.com/tauri-apps/wry)、Brave の [adblock-rust](https://github.com/brave/adblock-rust)、[EasyList](https://easylist.to) の上に成り立っています。
