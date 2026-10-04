# WebView2 security review: October 2, 2026

The reviewed floor and latest recommendation are now **154.0.4258.53**,
published October 1. Review again by October 9 inclusive; CI expires at
2026-10-10T00:00:00Z. This deliberately excludes every earlier 152/153/154
build; there is no compatibility exception for a superseded security patch.

## Vendor evidence

[Microsoft's security release notes](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-relnotes-security)
identify security updates after the previous 152.0.4191.66 floor in
153.0.4234.32, .46 and .48, followed by 154.0.4258.37, .48 and .53.
The September 18/24/29 and October 1 entries say CVE details will follow;
this review does not invent identifiers or claim a WebView2-specific CVE matrix.
No newer outstanding Stable fix was identified in those notes at review time.

[Stable release notes](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-relnote-stable-channel)
confirm 154.0.4258.53 as the October 1 Stable update. Extended Stable Edge 152
is a different servicing line, not an exception to Zephium's Evergreen floor.

[Microsoft's WebView2 runtime notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/)
explain its shared binaries and updates with Edge. Edge publication alone is
not artifact evidence: the [Microsoft Update Catalog query](https://www.catalog.update.microsoft.com/Search.aspx?q=Microsoft%20WebView2%20Runtime%20154.0.4258.53)
returned these exact WebView2 packages, each dated October 1:

| Architecture | Runtime build | Catalog size (bytes) |
| --- | --- | ---: |
| ARM64 | 154.0.4258.53 | 220957008 |
| x86 | 154.0.4258.53 | 193083728 |
| x64 | 154.0.4258.53 | 217117008 |

[Microsoft's API compatibility contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/versioning#forward-compatibility-of-apis)
preserves released APIs in later runtimes. Zephium still independently requires
native capability, identity, confinement and provenance checks at startup;
version arithmetic does not substitute for those checks. Stable majors above
154 retain the unreviewed-runtime advisory. Beta, Dev and Canary remain denied.

## Machine evidence and qualification boundary

At the beginning of this review, the machine-wide EdgeUpdate registration
reported **154.0.4258.48**. Its matching msedgewebview2.exe had a valid Microsoft
Authenticode signature. Older 153.0.4234.48 and 154.0.4258.37 executables also
remained installed. Registration and files are inventory evidence, not proof
of the version loaded into an already-running browser process.

Runtime .48 is below the reviewed floor and now yields an update-recommended advisory rather than a startup refusal. The signed Microsoft standalone installer
updated the machine to **154.0.4258.53** through normal Windows administrator
approval. After restarting the QA app, its child processes loaded
`Microsoft/EdgeWebView/Application/154.0.4258.53/msedgewebview2.exe`.
No loader override or floor exception was used.

The policy tests cover the exact floor, .52 immediately below it, previews,
malformed versions, major 155, pre-publication time, and exclusive review expiry.
The July 14 historical pending-fix guard remains intact. No macOS, WebKitGTK or
advisory-exception date was changed by this Windows review.

This document records vendor and local inventory review only. Packaged
hostile/native qualification and a second maintainer's review remain required
before signing a release.

## Recheck: October 4, 2026

The security release notes still list **154.0.4258.53** (October 1) as the
newest Stable security update, and the Stable release notes still list it as
the newest Stable 154 build. Neither page acknowledges an outstanding
Chromium fix that Stable lacks. The Microsoft Update Catalog query above still
returns the ARM64, x86 and x64 WebView2 154.0.4258.53 packages. The floor and
recommendation are unchanged; the review now runs through October 11
inclusive, and CI expires at 2026-10-12T00:00:00Z. This recheck was
vendor-source only: it did not reinstall or re-inventory a Windows machine.
