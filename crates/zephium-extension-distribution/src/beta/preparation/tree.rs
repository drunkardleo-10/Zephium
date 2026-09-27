use std::collections::BTreeMap;
use std::io::Read;

use sha2::{Digest, Sha256};
use zephium_extension_acquisition::{AcquiredExtensionArchive, AcquiredExtensionFileReceipt};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_ENTRIES,
};
use zephium_private_fs::{
    OpenedPrivateDirectory, PrivateDirectory, SealedPrivateDirectory, StreamingFileLength,
    TreeRemovalLimits,
};

use super::{
    bound, component, entry, storage, BetaArtifactPreparationError as Error, Blueprint,
    ProductAdmittedBetaSource,
};

// Borrow path components from the already bounded canonical index. A trie
// avoids quadratic directory scans and holds only depth-bounded live FDs.
#[derive(Default)]
struct Node<'a> {
    file: Option<usize>,
    children: BTreeMap<&'a str, Node<'a>>,
}

impl<'a> Node<'a> {
    fn new(index: &'a CanonicalExtensionTreeIndex) -> Result<Self, Error> {
        let mut root = Self::default();
        let mut entries = 0;
        for (slot, file) in index.files().iter().enumerate() {
            let mut node = &mut root;
            for name in file.path().as_str().split('/') {
                if node.file.is_some() {
                    return Err(Error::Integrity);
                }
                if !node.children.contains_key(name) {
                    entries += 1;
                    if entries > MAX_EXTENSION_TREE_ENTRIES {
                        return Err(Error::Integrity);
                    }
                }
                node = node.children.entry(name).or_default();
            }
            if node.file.replace(slot).is_some() || !node.children.is_empty() {
                return Err(Error::Integrity);
            }
        }
        Ok(root)
    }
}

pub(super) fn write(
    directory: PrivateDirectory,
    source: &ProductAdmittedBetaSource,
    archive: &mut AcquiredExtensionArchive<'_>,
    blueprint: &Blueprint,
) -> Result<(), Error> {
    let tree = Node::new(&blueprint.index)?;
    let mut receipts = Vec::with_capacity(archive.files().len());
    let slots: BTreeMap<String, usize> = archive
        .files()
        .iter()
        .enumerate()
        .map(|(slot, file)| (file.path().as_str().to_owned(), slot))
        .collect();
    write_node(directory, &tree, archive, blueprint, &slots, &mut receipts)?;
    let original = archive.finish_tree(receipts).map_err(|_| Error::Source)?;
    if original.index_bytes() != source.source.index_bytes()
        || original.original_crx_sha256() != source.upstream().original_crx_sha256()
        || original.developer_key_sha256() != source.source.developer_key_sha256()
    {
        return Err(Error::Source);
    }
    Ok(())
}

fn write_node(
    directory: PrivateDirectory,
    node: &Node<'_>,
    archive: &mut AcquiredExtensionArchive<'_>,
    blueprint: &Blueprint,
    slots: &BTreeMap<String, usize>,
    receipts: &mut Vec<AcquiredExtensionFileReceipt>,
) -> Result<SealedPrivateDirectory, Error> {
    for (name, child) in &node.children {
        let name = entry(name)?;
        if let Some(slot) = child.file {
            let expected = &blueprint.index.files()[slot];
            let path = expected.path().as_str();
            let input_slot = slots.get(path).copied();
            let replacement = if path == "manifest.json" {
                Some(blueprint.manifest_bytes.as_slice())
            } else {
                blueprint.replacements.get(path).map(Vec::as_slice)
            };
            if let Some(bytes) = replacement {
                if let Some(input_slot) = input_slot {
                    receipts.push(
                        archive
                            .copy_file(input_slot, &mut std::io::sink())
                            .map_err(|_| Error::Source)?,
                    );
                }
                directory
                    .write_new_entry_synced(
                        &name,
                        bytes,
                        bound(zephium_extension_package::MAX_EXTENSION_TREE_FILE_BYTES as usize)?,
                    )
                    .map_err(storage)?;
            } else {
                let input_slot = input_slot.ok_or(Error::Source)?;
                let length = StreamingFileLength::new(expected.length()).map_err(storage)?;
                let receipt = archive
                    .with_file_reader(input_slot, |reader| {
                        directory
                            .write_new_entry_from_reader(&name, reader, length)
                            .map_err(storage)
                    })
                    .map_err(|_| Error::Source)??
                    .1;
                receipts.push(receipt);
            }
            directory
                .seal_verified_entry_regular(&name)
                .map_err(storage)?
                .ok_or(Error::Integrity)?;
        } else {
            let nested = directory.create_new_entry_child(&name).map_err(storage)?;
            write_node(nested, child, archive, blueprint, slots, receipts)?;
        }
    }
    directory.seal().map_err(|_| Error::Storage)
}

pub(super) fn verify(
    directory: &SealedPrivateDirectory,
    index: &CanonicalExtensionTreeIndex,
) -> Result<(), Error> {
    verify_node(directory, &Node::new(index)?, index)
}

fn verify_node(
    directory: &SealedPrivateDirectory,
    node: &Node<'_>,
    index: &CanonicalExtensionTreeIndex,
) -> Result<(), Error> {
    let names = directory
        .list_entry_names(node.children.len().max(1))
        .map_err(storage)?;
    if names
        .iter()
        .map(|name| name.as_str())
        .ne(node.children.keys().copied())
    {
        return Err(Error::Integrity);
    }
    for name in names {
        let child = node.children.get(name.as_str()).ok_or(Error::Integrity)?;
        if let Some(slot) = child.file {
            let file = &index.files()[slot];
            let (length, digest) = directory
                .with_bounded_entry_regular_reader(
                    &name,
                    bound((file.length() as usize).max(1))?,
                    hash_reader,
                )
                .map_err(storage)?
                .ok_or(Error::Integrity)??;
            if length != file.length() || digest != file.sha256() {
                return Err(Error::Integrity);
            }
        } else {
            verify_node(
                &directory.open_sealed_entry_child(&name).map_err(storage)?,
                child,
                index,
            )?;
        }
    }
    Ok(())
}

// Keep this scratch buffer out of every recursive directory frame.
#[inline(never)]
pub(super) fn hash_reader(reader: &mut dyn Read) -> Result<(u64, [u8; 32]), Error> {
    let mut buffer = [0; 64 * 1024];
    let mut digest = Sha256::new();
    let mut length = 0u64;
    loop {
        let count = reader.read(&mut buffer).map_err(|_| Error::Integrity)?;
        if count == 0 {
            break;
        }
        length = length.checked_add(count as u64).ok_or(Error::Integrity)?;
        digest.update(&buffer[..count]);
    }
    Ok((length, digest.finalize().into()))
}

pub(super) fn remove_artifact(parent: &PrivateDirectory, name: &str) -> Result<(), Error> {
    let opened = parent
        .open_private_child_any_mode(&component(name)?)
        .map_err(storage)?;
    let names = match &opened {
        OpenedPrivateDirectory::Writable(directory) => directory.list_entry_names(4),
        OpenedPrivateDirectory::Sealed(directory) => directory.list_entry_names(4),
    }
    .map_err(storage)?;
    if names.iter().any(|name| {
        !matches!(
            name.as_str(),
            "extension" | "original.crx" | "tree-index.json" | "evidence.json"
        )
    }) {
        return Err(Error::Integrity);
    }
    let directory = match opened {
        OpenedPrivateDirectory::Writable(directory) => directory,
        OpenedPrivateDirectory::Sealed(directory) => {
            directory.unseal().map_err(|_| Error::Storage)?
        }
    };
    // Remove the extension subtree separately so a maximum-depth portable
    // package stays within the existing private-fs depth ceiling.
    if names.iter().any(|name| name.as_str() == "extension") {
        let tree = directory
            .open_private_child_any_mode(&component("extension")?)
            .map_err(storage)?;
        tree.remove_tree_bounded(
            TreeRemovalLimits::new(
                MAX_EXTENSION_TREE_ENTRIES,
                MAX_EXTENSION_RELATIVE_PATH_DEPTH,
            )
            .map_err(storage)?,
        )
        .map_err(|_| Error::Storage)?;
    }
    for name in ["original.crx", "tree-index.json", "evidence.json"] {
        directory
            .remove_verified_regular(&component(name)?)
            .map_err(storage)?;
    }
    directory.remove_empty().map_err(|_| Error::Storage)?;
    Ok(())
}
