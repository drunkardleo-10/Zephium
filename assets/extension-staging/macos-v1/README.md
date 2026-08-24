# macOS mixed extension staging catalog

This directory contains one immutable, non-production Zephium catalog used to
exercise the complete macOS product path with more than one extension and more
than one native runtime profile. It is compiled only when the explicit
`staging-extension-catalog` feature is enabled. Ordinary and release builds do
not compile these bytes or construct the distribution worker.

Catalog revision 9 is active. Revisions 7 and 8 are retained as the two ordered
rollback generations required by the repository high-water contract. Revision
7 kept the exact Vimium revision-6 package while adding Dark Reader revision 1,
so the migration tested catalog-cohort growth without conflating it with a
Vimium package update. Revision 8 keeps both exact package byte streams and
advances Dark Reader to package revision 2 so the newly observed background
lifecycle degradation is published monotonically rather than changing an
already admitted revision-7 profile in place. Revision 9 reuses both packages'
exact CRX3 objects, legal notices, manifests, compatibility receipts, and
transformed trees. It advances only Dark Reader's update-line identity from 2
to 3 so the `alarms` declaration can move from compatible to degraded through
the normal new-degradation consent transaction after the long-duration native
gate proved live delivery but no persistence across exact context reload. The
new declaration review is separately retained under `evidence/`, so the old
profile remains an immutable rollback generation.

The active cohort is:

- Vimium 2.4.2, upstream revision
  `eb737abdd65b070c05ef06d39f1d78751aa7f738`, using the exact reviewed
  `macos.wkwebextension-brokered.v1` profile.
- Dark Reader 4.9.129, upstream revision
  `c2a707302a39b8047543712e9c582bac07835d34`, using the package-neutral
  `macos.wkwebextension.v1` transform and native runtime.

Both packages are distributed under MIT and carry their exact legal notices.
The catalog, transformed manifests and trees, CRX3 packages, compatibility
receipts, and review evidence were produced by Zephium's deterministic release
tools. Public developer keys are embedded in the signed packages. No private
key, credential, mutable endpoint, source checkout, or ambient path is stored
here.

Dark Reader's reviewed result is
`usable-with-about-srcdoc-frame-degradation`: top-level, dynamic-style, and
same-origin-frame theming plus action-popup execution pass on the exercised
WebKit runtime, while an `about:srcdoc` descendant remains unthemed. Its absent
`fontSettings` namespace and Chromium-version equivalence are also disclosed as
degraded. Unloading and reconstructing its native context for re-enable, and
reconstructing it for the revision-7 to revision-8 package update, cause WebKit
to emit another `runtime.onInstalled` event with reason `install`, so the stock
extension opens its help tab again even with the deterministic
`webkit-extension` origin; restart-only rehydration does not. The background
lifecycle remains degraded. A 0.5-minute native alarm fires while the context
is loaded, but an alarm armed immediately before unload is absent after the
same context reloads; alarm-backed work that assumes Chrome restart persistence
is therefore also disclosed as degraded. WebKit's own runtime test expects the install event
after every unload/load; keeping the context loaded would violate disabled-state
ownership, the public unsupported-API set does not hide this event member, and
attempting to replace it makes the background fail. Closing the gap therefore
requires a future host-controlled lifecycle API or a deeper compatibility
runtime, not tab suppression or a dormant-context cache. This staging membership
is not a public compatibility or distribution claim.

The feature uses a five-object in-process transport with the same catalog,
CRX3, legal, repository, admission, runtime-selection, and activation pipeline
as network-backed distribution. The transport accepts only the exact catalog,
two package objects, and two legal objects named below.

Exact active identities:

- catalog SHA-256: `8462dad95e46ed68bf03760607a293c99afd45f9b7b0a9cb98942ba5c858f0bd`
- catalog inventory SHA-256: `ce107b0ecf2c3bf1fe11f67894b69abc1a9e1e8fb7297a90aa18a12bee0b459f`
- Vimium CRX3 SHA-256: `5bf9a4d8916959fca6b441b1ded47fa8dd615dffa2d1fb80314e5f9f4ec3187b`
- Vimium archive SHA-256: `0154d07f8378f4e0396d1083941854dc141090680bb58394260ca5bd799e9598`
- Vimium release tree SHA-256: `6b0960e703616c920c75a7b3994a4f63c2313234fc3886a764cbdf04d86a2e68`
- Vimium legal notice SHA-256: `bc52afe9916014c3fe1d9244b65a1c0293d1ccf7507af317b3b63c49a452dcbc`
- Dark Reader official MV3 ZIP SHA-256: `20e7993eee8015f7db18748eea366616dfd05ec477efb7be6ae52d2b221b0a64`
- Dark Reader CRX3 SHA-256: `523516c9550b15fd2cd97ce722348313da32deccb777caae4dc0a127e13d2c1a`
- Dark Reader archive SHA-256: `2d39a45d9a1514b214efee202c8e8276b1e8fc61461f33ee46b8ce20f65cfeb0`
- Dark Reader release tree SHA-256: `ac9611842b883e1c100b2eb0a28e8351783a33ecff319ec3b39dba3b9fc8cb07`
- Dark Reader legal notice SHA-256: `f0a5f835174494f8981b2cbb1a34054d4f887a5c865318650d6a17afe1c7850e`
