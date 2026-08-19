# Vimium 2.4.2 macOS staging catalog

This directory contains one immutable, non-production Zephium extension
catalog used to exercise the complete macOS product path. It is compiled only
when the explicit `staging-extension-catalog` feature is enabled.

Catalog revision 6 is active. Revisions 4 and 5 and their exact manifest
profiles are retained only as ordered rollback/recovery authority so current
staging repositories can authenticate the upgrade without lowering their
monotonic high-water marks. Revision 4 keeps the options control as a genuine
link to the exact options-page URL: ordinary activation remains on the sealed
native broker, while WebKit accessibility activation retains native link
semantics. Revisions 5 and 6 intentionally reuse those exact package bytes
while advancing catalog/install authority to gate live runtime-generation
action invalidation without conflating metadata freshness with a process
restart.

On the exercised WebKit runtime, Computer Use `AXPress` still dismisses the
popup without delivering DOM activation or native link navigation. Keyboard
activation is live-gated, and the browser-owned Extensions Center now exposes
an independently authorized Settings action. Direct assistive activation
inside extension-owned popup content remains unclaimed until a physical
VoiceOver gate proves that path.

The package is Vimium 2.4.2 at upstream revision
`eb737abdd65b070c05ef06d39f1d78751aa7f738`, distributed under MIT. The
catalog, transformed manifest/tree, CRX3 package, legal notice, compatibility
profile, and review evidence were produced by Zephium's deterministic release
tools. The public developer key is part of the signed CRX3 package. No private
key, credential, mutable endpoint, source checkout, or ambient path is stored
here.

The feature uses an in-process fixed-object transport with the same catalog,
CRX3, legal, repository, admission, and activation pipeline as network-backed
distribution. Ordinary builds do not compile these bytes or construct the
extension distribution worker.

Exact primary identities:

- catalog SHA-256: `d15bb0a138379049d933c531b5225ec11c11a216d5d094e8d9c67212fce47638`
- CRX3 SHA-256: `5bf9a4d8916959fca6b441b1ded47fa8dd615dffa2d1fb80314e5f9f4ec3187b`
- archive SHA-256: `0154d07f8378f4e0396d1083941854dc141090680bb58394260ca5bd799e9598`
- release tree SHA-256: `6b0960e703616c920c75a7b3994a4f63c2313234fc3886a764cbdf04d86a2e68`
- legal notice SHA-256: `bc52afe9916014c3fe1d9244b65a1c0293d1ccf7507af317b3b63c49a452dcbc`
