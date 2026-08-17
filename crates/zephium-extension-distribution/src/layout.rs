use std::fmt::Write as _;

use url::Url;
use zephium_core::extensions::{
    ExtensionArchiveDigest, ExtensionPackageKey, ExtensionPackageRevision,
};

pub(crate) const CATALOG_TARGET: &str = "catalog-v1.json";

pub(crate) fn catalog_url(metadata_base: &Url) -> Result<Url, ()> {
    metadata_base.join(CATALOG_TARGET).map_err(|_| ())
}

pub(crate) fn crx3_url(
    targets_base: &Url,
    package_key: ExtensionPackageKey,
    revision: ExtensionPackageRevision,
    archive_digest: ExtensionArchiveDigest,
) -> Result<Url, ()> {
    let mut target = String::with_capacity(64 + 1 + 20 + 1 + 64 + 10);
    target.push_str("crx3/");
    append_lower_hex(&mut target, package_key.as_bytes())?;
    target.push('/');
    write!(&mut target, "{}", revision.get()).map_err(|_| ())?;
    target.push('/');
    append_lower_hex(&mut target, archive_digest.as_bytes())?;
    target.push_str(".crx3");
    targets_base.join(&target).map_err(|_| ())
}

pub(crate) fn legal_notice_url(targets_base: &Url, sha256: &[u8; 32]) -> Result<Url, ()> {
    let mut target = String::with_capacity(6 + 64 + 7);
    target.push_str("legal/");
    append_lower_hex(&mut target, sha256)?;
    target.push_str(".notice");
    targets_base.join(&target).map_err(|_| ())
}

fn append_lower_hex(output: &mut String, bytes: &[u8]) -> Result<(), ()> {
    for byte in bytes {
        write!(output, "{byte:02x}").map_err(|_| ())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_derived_only_from_fixed_width_authenticated_identities() {
        let base = Url::parse("https://extensions.example/stable/targets/").unwrap();
        let crx = crx3_url(
            &base,
            ExtensionPackageKey::from_bytes([0xab; 32]),
            ExtensionPackageRevision::new(17).unwrap(),
            ExtensionArchiveDigest::from_bytes([0xcd; 32]),
        )
        .unwrap();
        assert_eq!(
            crx.as_str(),
            concat!(
                "https://extensions.example/stable/targets/crx3/",
                "abababababababababababababababababababababababababababababababab/17/",
                "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd.crx3"
            )
        );
        assert_eq!(
            legal_notice_url(&base, &[0xef; 32]).unwrap().as_str(),
            concat!(
                "https://extensions.example/stable/targets/legal/",
                "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef.notice"
            )
        );
    }
}
