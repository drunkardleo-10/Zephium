use zephium_extension_authority::{BundledCatalogAdmissionError, BundledPackageAuthority};
use zephium_extension_package::ExtensionReleaseCatalog;

pub(crate) trait CatalogAuthenticator {
    fn authenticate(&self, bytes: &[u8]) -> Result<ExtensionReleaseCatalog, ()>;
}

pub(crate) struct ProductCatalogAuthenticator {
    authority: BundledPackageAuthority,
}

impl ProductCatalogAuthenticator {
    pub(crate) fn new() -> Result<Self, BundledCatalogAdmissionError> {
        BundledPackageAuthority::product().map(|authority| Self { authority })
    }
}

impl CatalogAuthenticator for ProductCatalogAuthenticator {
    fn authenticate(&self, bytes: &[u8]) -> Result<ExtensionReleaseCatalog, ()> {
        self.authority
            .admit_acquired_catalog(bytes)
            .map(|catalog| catalog.catalog().clone())
            .map_err(|_| ())
    }
}

#[cfg(test)]
pub(crate) struct StructuralTestCatalogAuthenticator;

#[cfg(test)]
impl CatalogAuthenticator for StructuralTestCatalogAuthenticator {
    fn authenticate(&self, bytes: &[u8]) -> Result<ExtensionReleaseCatalog, ()> {
        ExtensionReleaseCatalog::parse_canonical(bytes).map_err(|_| ())
    }
}
