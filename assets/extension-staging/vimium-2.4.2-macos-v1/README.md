# Vimium 2.4.2 macOS staging catalog

This directory contains one immutable, non-production Zephium extension
catalog used to exercise the complete macOS product path. It is compiled only
when the explicit `staging-extension-catalog` feature is enabled.

Catalog revision 2 is active. Revision 1 and its exact manifest profile are
retained only as rollback/recovery authority so an existing staging repository
can authenticate and upgrade without lowering its monotonic high-water mark.

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

- catalog SHA-256: `b27962b45ecb1f85df1afbed626dc42a92c11230a8a7ac162a40511f1aa24b56`
- CRX3 SHA-256: `4bc71181804ec8aed8385bf2bd3db6df488c0b4e4bfffc20a7dc0ddeb131a9f6`
- archive SHA-256: `d78441192b493d8ca40ce17690e4a71bcdb024d02410ebaac33243c7329b381e`
- legal notice SHA-256: `bc52afe9916014c3fe1d9244b65a1c0293d1ccf7507af317b3b63c49a452dcbc`
