# Grammarly compatibility inspection — September 11, 2026

Grammarly is currently blocked before native installation. This inspection
does not establish a working editing workflow or qualify Grammarly for the
Tested with Zephium list. No sign-in, personal text, or native QA instance was
used, and no publisher resource was modified.

## Original package evidence

The original package was downloaded directly from Google's HTTPS CRX update
service for the [official store listing](https://chromewebstore.google.com/detail/grammarly-ai-writing-assi/kbfnbcaeplbcioakkpcpgfkobkghlhen).

- Extension ID: `kbfnbcaeplbcioakkpcpgfkobkghlhen`.
- Manifest version: 3; package version: `14.1328.0`.
- Original CRX3 bytes: 18,267,171.
- SHA-256: `377f9caf1a5d53d33c705e79c1cd05a2495f3a6997ee027ffea26277efb56092`.
- ZIP entries: 1,009; declared expanded bytes: 49,088,154.
- Diagnostic input: `/private/tmp/zephium-grammarly-20260911/original.crx3`.
  This temporary path is local evidence, not a committed fixture or hosted package.

`cargo xtask check-crx3 --archive <input> --expected-id
kbfnbcaeplbcioakkpcpgfkobkghlhen` passed its initial
`VerifiedCrx3Package::parse_and_verify` call, then failed acquired-archive
preflight with `acquired extension ZIP contains an invalid path`. The command
exited 1. Thus the signature/ID check succeeded; complete acquired-package
acceptance did not. The manifest and script observations below are read-only
inspection of bytes inside that signed archive, not an admitted runtime artifact.

## First blocker: portable resource paths

The actual resource `src/js/сlickableCard.common.chunk.js` starts its filename
with Cyrillic small es (`U+0441`), not ASCII `c`. The upstream `Grammarly.js` and
`Grammarly-gDocs.js` scripts reference that exact chunk name. Deleting it or
renaming it without rewriting its references would break upstream behavior.

`PortableRelativePath::parse` explicitly rejects non-ASCII paths. Its collision
key and complete archive/tree inventory contract use ASCII case folding across
platforms. This is a deliberate unsupported package shape, not evidence of a
bad publisher signature. Four image filenames containing `@2x` are allowed by
the existing grammar and are not this failure's cause.

Unicode support needs a coherent archive, canonical-tree, resource-URL and
filesystem collision/normalization design, including macOS and Windows aliases.
Simply removing the ASCII check would violate the current contract. No such
relaxation or package-specific rename was made.

## Additional requirements visible in the original manifest and scripts

These requirements remain after the archive blocker. They were compared with
the current compiled external compatibility policy; they were not reached in a
successful product admission run.

| Requirement | Current evidence |
| --- | --- |
| Required `cookies` | Unsupported by admission. Background code uses real cookie reads, writes, removal and availability checks; an empty-success shim would not implement this behavior. |
| Required `identity` | Unsupported by admission. Background code uses `getRedirectURL` and noninteractive `launchWebAuthFlow`; it also contains a Safari callback fallback. The fallback alone does not establish authenticated Zephium behavior. |
| Required `sidePanel` and `side_panel` page | Unsupported by admission. Background code calls `chrome.sidePanel.open` and detects whether the API is present. This does not by itself authorize discarding a required permission. |
| Required `notifications` | The existing brokered compiler has an explicit degraded adapter, but Grammarly does not satisfy the current brokered selection/subset merely by declaring it. |
| Managed storage schema | Manifest names `src/schema.json`; the current external policy rejects this declaration. Existing managed-storage compiler assets are not proof that the original manifest is admitted. |
| `externally_connectable` | Declares HTTPS Grammarly subdomains; remains unmodeled authority in the closed manifest parser. |
| Content-script `exclude_globs` | Present on several script groups and explicitly unsupported by the current compatibility policy. Dropping these exclusions would alter where code runs. |
| Optional permissions | `nativeMessaging` and `clipboardRead`; these must remain denied unless real implementations and grants exist. |

The manifest also includes one `MAIN` world script for Office pages, ordinary
isolated scripts, a classic worker, HTTP(S) hosts, and a strict extension-page
CSP. Their presence is not the first failure and should not be replaced with
guessed package adaptations.

## Verification and next qualification

`cargo test -p zephium-extension-package relative_path --lib` passed all five
existing tests, including traversal/alias rejection and the property that
accepted paths are ASCII and preserved exactly. No production source changed.

A complete implementation needs the portable-path design first, followed by
explicit permission/runtime work for the requirements above. Then install the
unchanged original package through the normal browser-owned store action and
verify a synthetic textarea/contenteditable editing workflow, background and
popup behavior, restart, disable and removal. Authentication flows need their
own explicit authorization and account-independent testing where possible.
Physical Windows qualification remains separate. This inspection alone does
not justify adding Grammarly to the tested list or broadening admission.
