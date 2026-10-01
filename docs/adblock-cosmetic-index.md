# Keep the generic cosmetic index out of the page

## Problem

Every main document receives the whole generic cosmetic index:
`CosmeticPolicy::generic_index` (about 487 KB of JSON) is embedded in the
script `document_style_script` evaluates, then `content_style.js` parses it
into a `Map` of token → selectors. Each tab pays for parsing that script and
keeps its own copy of the index in its renderer, although Rust already holds
one shared `Arc<str>`.

On Windows, protection-on pages load as fast or faster than protection-off
pages but never use less peak memory (Yahoo 349 → 365 MiB, GitHub 418 →
451 MiB; see [windows-protection-performance.md](windows-protection-performance.md)).
macOS has the same per-document cost. The product goal is that protection makes
pages lighter, not just faster.

## Goal

The page holds no index. It reports the class and id tokens it sees; the host
answers with the few selectors those tokens match. Brave keeps its index in
the browser process the same way. Expected result: renderer memory and
per-navigation parse time with protection on fall below protection off on
ad-heavy pages, and stay within noise on clean pages.

## Constraints

- Shared code for macOS and Windows. No new page-to-host bridge (no
  WebMessage, script message handler or host object): the host already
  evaluates scripts in the page and reads results, so it pulls tokens through
  that same channel. Keep the closed, protected page script and its token checks.
- No idle cost. Pull only around navigation milestones and while the tab is
  visible, with a short backoff after load (for example at delivery, load, then
  a few spaced pulls), and only when the page has new tokens. Hidden tabs pull
  nothing. Do not add a steady timer.
- Keep the current bounds: 2,048 generic selectors per document, sliced DOM
  discovery, `generichide`, per-selector exceptions, personal hides and the
  picker unchanged. Same-document navigations must not resend work.
- Matching on the host happens on the style worker, never the UI thread, and
  shares the one immutable index across all tabs and profiles.
- Measure first: confirm with an index-off build how much of the on/off memory
  gap the index explains before rewriting. If it is not the cause, report what is.

## Evidence

On both platforms, protection off vs on, matched builds, three or more
alternating runs: Yahoo, Bloomberg, a news article, GitHub, and one clean page.
Report load time, renderer/process-family memory (peak and settled),
CPU, and blocked counts; plus 10 tabs and a five-minute idle with 10 tabs.
Show that late-inserted ads (infinite scroll, delayed slots) are still hidden.
Keep raw runs under `target/`; commit a short results table in this file.
