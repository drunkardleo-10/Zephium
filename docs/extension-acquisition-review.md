# Extension acquisition: review brief

Reviewed September 11, 2026. This records technical evidence and questions for
qualified counsel; it is not a legal opinion or proof that Zephium must obtain
a bespoke Google agreement. Public acquisition eligibility remains unresolved.

## What the public evidence establishes

- Google documents `https://clients2.google.com/service/update2/crx` as the
  Chrome Web Store update URL. Chromium's current source uses that endpoint.
  This establishes a real installation/update mechanism, not permission for
  every third-party use of it.
  [Chrome installation documentation](https://developer.chrome.com/docs/extensions/how-to/distribute/install-extensions)
  [Chromium source](https://chromium.googlesource.com/chromium/src/+/main/extensions/common/extension_urls.cc)
- The documented Chrome Web Store API manages publishers' own items. Its scope
  does not by itself answer whether a browser may use the separate update
  service. Registering as an extension developer is not a browser-distribution
  agreement.
  [API documentation](https://developer.chrome.com/docs/webstore/api)
  [Developer agreement](https://developer.chrome.com/docs/webstore/program-policies/terms)
- The store-linked consumer terms, updated January 27, 2025, describe use in
  connection with Chrome (1.2), restrict non-provided interfaces and automated
  access (3.3), address copying (3.5), and restrict modification unless otherwise
  permitted (3.7). Their application to the exact flow below needs review.
  [Store consumer terms](https://ssl.gstatic.com/chrome/webstore/intl/en-US/gallery_tos.html)
- The developer agreement's default user license is scoped to use in connection
  with Chrome, while allowing a publisher's separate EULA to govern instead
  (5.2). Store access and rights in an extension are separate questions.
  [Developer agreement](https://developer.chrome.com/docs/webstore/program-policies/terms)
- Edge and Brave document Chrome Web Store installation. Orion is a relevant
  WebKit-based example with Chrome/Firefox extension support. These demonstrate
  existing product routes; their public documentation does not establish their
  contractual arrangements or authorize Zephium's particular implementation.
  [Edge](https://support.microsoft.com/en-us/edge/why-are-some-of-my-extensions-missing-on-the-new-microsoft-edge)
  [Brave](https://support.brave.com/hc/en-us/articles/360017909112-How-can-I-add-extensions-to-Brave)
  [Orion](https://help.kagi.com/orion/why-orion/orion-vs-safari.html)

## Exact proposed Zephium flow to review

This is the intended public flow, not a claim that the product integration is
complete or enabled:

1. A user selects an extension and explicitly requests installation.
2. Zephium obtains the original package directly from its approved source.
   Zephium's metadata backend does not download, proxy or mirror packages.
3. Zephium authenticates the CRX publisher and original bytes, checks current
   signed compatibility/revocation policy, and obtains explicit user grants.
4. On the user's device, Zephium extracts the package. For the implemented Beta
   subset, it inserts the authenticated publisher's manifest key only when
   missing; otherwise the manifest bytes are preserved. Other compatibility
   transformations would need separate review. Original signatures are never
   described as authenticating the transformed output.
5. The selected platform runtime loads the retained output (WKWebExtension on
   macOS, WebView2's native extension facility on Windows).
6. Updates repeat authentication, compatibility and permission checks. Proposed
   source requests carry necessary extension identifiers, without a Zephium
   account or unique client identifier. This does not make source requests
   anonymous to Google or the publisher.

Zephium's own catalog, UI, metadata copying, trademarks, bundled packages and
any redistribution must be assessed separately from this direct-download flow.

## Questions for a focused software-licensing / service-terms review

Provide the company's jurisdiction and intended launch markets with this brief.
Request a written assessment distinguishing these operations:

- Does direct, user-requested installation through Google's published extension
  update mechanism qualify as use of a provided interface for Zephium?
- How do the automated-access and Chrome-use clauses apply to scheduled updates
  and to a browser using native WebViews rather than the full Chromium browser?
- Can the intended flow rely on existing terms, or is clarification / additional
  permission from Google needed? What precise change would remove an obstacle?
- What rights are needed for extraction, missing-key insertion, and any later
  code or manifest transformation? Distinguish runtime interoperability, local
  modification, and distribution of modified copies.
- What reusable acceptance rules can cover permissively licensed extensions,
  copyleft extensions and proprietary EULAs, rather than assuming an individual
  commercial agreement is required for every extension?
- What notices, source-availability obligations and branding restrictions apply
  if Zephium builds or redistributes a package itself?

Do not infer that an open-source extension license authorizes use of Google's
servers. Conversely, do not infer that store terms eliminate independently
obtained rights to publisher-hosted source or release artifacts. Whether a
particular restriction applies or is enforceable is a legal question.

## Available product routes

| Route | Main benefit | Work / uncertainty |
| --- | --- | --- |
| Direct Chrome Web Store integration | Broad familiar catalog | Exact protocol, service terms, publisher rights and update availability need review |
| Direct publisher release downloads | Can operate without Google's distribution service | Approved origins, artifact authentication, licenses and dependable updates; incomplete catalog coverage |
| Builds from appropriately licensed source | Control over adaptations and delivery | Reproducible builds, signing under the correct identity, compliance and maintenance burden |
| User-provided packages | Useful advanced / developer entry point | Still needs authentication and permission checks; manual import is not a universal rights exemption |

For a concrete example, Dark Reader's repository provides build instructions
and an MIT license permitting modification and distribution subject to its
notice condition. This is evidence for a source-build route, not a statement
that every store binary, bundled dependency or trademark has identical terms,
and not a claim that the current Beta subset runs Dark Reader correctly.
[Source and build instructions](https://github.com/darkreader/darkreader)
[License](https://github.com/darkreader/darkreader/blob/main/LICENSE)

Other extension stores need their own acquisition and licensing review; they
are not automatically unrestricted replacements.

## Recommended next steps

Keep Chrome Web Store integration as an intended provider while resolving the
specific review questions. Do not promise universal store installation before
that route and representative real extensions are qualified. Continue the
provider-independent runtime and lifecycle implementation.

In parallel, identify a small set of important extensions with direct publisher
artifacts or suitable source licenses. Record each authoritative source, exact
artifact identity, permitted adaptations and update route. This provides a
concrete fallback but should not be presented as equivalent to full store
coverage. Unsigned ZIP releases need an appropriate authentication adapter;
they must not be silently treated as publisher-authenticated CRX files.

If review identifies a need for Google's clarification, the question to send is:

> We are building Zephium, an independent desktop browser using WKWebExtension
> on macOS and WebView2 on Windows. Users would explicitly select extensions,
> download original packages directly from Google's documented update service,
> and receive subsequent updates through that service. We would not crawl or
> mirror the store. Packages would be verified and extracted locally; where
> required and permitted, a local manifest adaptation would retain the original
> publisher identity. Is this flow supported under the existing terms, and are
> additional permissions or integration requirements applicable?

No inquiry has been sent. A developer account, a successful HTTP response, or
another browser's implementation should not be recorded as legal clearance.
