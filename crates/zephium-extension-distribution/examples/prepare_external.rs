//! Inspect and prepare a real, original CRX through the product's external
//! package path. This does not install into Store or call a native runtime.
use std::{fs, io::Read, path::PathBuf};
use zephium_core::extensions::{ExtensionPackageRevision, ExtensionProvenanceSource};
use zephium_extension_acquisition::AcquiredExtensionArchive;
use zephium_extension_distribution::beta::{
    admit_external_source, BetaPreparationWorkspace, BetaRuntimeTarget,
};
use zephium_extension_package::{
    ChromiumExtensionId, MAX_CRX3_HEADER_BYTES, MAX_EXTENSION_ARCHIVE_BYTES,
};
use zephium_private_fs::LockedPrivateNamespace;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: prepare_external ORIGINAL.crx EXTENSION_ID PRIVATE_OUTPUT_DIRECTORY".into(),
        );
    }
    let path = PathBuf::from(&args[0]);
    let limit = MAX_EXTENSION_ARCHIVE_BYTES as u64 + MAX_CRX3_HEADER_BYTES as u64 + 12;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("CRX exceeds package bound".into());
    }
    let id = ChromiumExtensionId::parse(&args[1])?;
    let mut archive = AcquiredExtensionArchive::authenticate_upstream_crx3(&bytes, &id, None)?;
    let mut manifest = Vec::new();
    let mut receipts = Vec::with_capacity(archive.files().len());
    for i in 0..archive.files().len() {
        let receipt = if archive.files()[i].path().as_str() == "manifest.json" {
            archive.copy_file(i, &mut manifest)?
        } else {
            archive.copy_file(i, &mut std::io::sink())?
        };
        receipts.push(receipt);
    }
    let source = admit_external_source(
        archive.finish_tree(receipts)?,
        &manifest,
        BetaRuntimeTarget::MacosNative,
        ExtensionPackageRevision::INITIAL,
        None,
    )?;
    let mut workspace = BetaPreparationWorkspace::open(LockedPrivateNamespace::open_or_create(
        PathBuf::from(&args[2]),
    )?)?;
    let artifact = workspace.prepare_external(source, &bytes)?;
    let provenance = artifact.provenance(ExtensionProvenanceSource::ChromeWebStore)?;
    println!(
        "Authenticated/prepared {}: {} bytes retained; {} files; limitations: {:?}",
        id.as_str(),
        artifact.retained_bytes(),
        artifact.index().files().len(),
        artifact.limitations()?
    );
    println!(
        "Withheld optional APIs: {:?}; withheld site patterns: {}; external messaging restricted: {}",
        artifact.withheld_optional_permissions().iter().map(|name| name.as_str()).collect::<Vec<_>>(),
        artifact.withheld_optional_hosts().len(),
        artifact.external_messaging_withheld(),
    );
    drop(artifact);
    drop(workspace);
    let workspace = BetaPreparationWorkspace::open(LockedPrivateNamespace::open_or_create(
        PathBuf::from(&args[2]),
    )?)?;
    let restored = workspace.reopen_external_bound(
        &provenance,
        provenance.upstream(),
        BetaRuntimeTarget::MacosNative,
    )?;
    restored.verify()?;
    println!("Offline reconstruction passed; no signed backend policy or native runtime used.");
    Ok(())
}
