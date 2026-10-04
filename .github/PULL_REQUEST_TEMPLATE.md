<!-- The title becomes the squash commit and follows Conventional Commits, e.g. fix(omnibox): keep focus when suggestions open -->

## What

<!-- One or two sentences on the change. -->

## Why

<!-- The problem it solves. Link the issue or discussion, e.g. "Closes #42". -->

## How tested

<!-- The flows you actually exercised, and on which platform. "Checks pass" alone is not enough. -->

## Checklist

- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` are clean
- [ ] Relevant tests pass and new behavior is covered, including the deny path for anything security-sensitive
- [ ] `pnpm --dir frame check` is clean (frontend changes)
- [ ] Screenshots or a recording attached (visual changes)
- [ ] Docs updated where behavior or architecture changed
- [ ] No new telemetry, network path or heavy dependency without prior discussion
