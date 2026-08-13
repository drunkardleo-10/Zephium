//! Closed resource-tree verification shared by external native probes.
//!
//! A caller-selected tree index is diagnostic evidence only. Product runtime
//! authority still requires a catalog-authenticated materialization receipt.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::path::{Component, Path};

use sha2::{Digest, Sha256};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_ENTRIES,
};

pub(super) fn verify_closed_tree(
    root: &Path,
    index: &CanonicalExtensionTreeIndex,
    description: &str,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect {description} extension root: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{description} extension root is not an ordinary directory"
        ));
    }
    let mut actual = BTreeSet::new();
    let mut entries = 0_usize;
    collect_tree_paths(
        root,
        Path::new(""),
        0,
        &mut entries,
        &mut actual,
        description,
    )?;
    let expected = index
        .files()
        .iter()
        .map(|file| file.path().as_str().to_owned())
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(format!(
            "{description} extension tree differs from its closed index"
        ));
    }
    for expected in index.files() {
        let path = root.join(expected.path().as_str());
        let bytes = read_bounded_file(
            &path,
            expected.length(),
            expected.path().as_str(),
            description,
        )?;
        if bytes.len() as u64 != expected.length()
            || Sha256::digest(&bytes).as_slice() != expected.sha256()
        {
            return Err(format!(
                "{description} extension file differs from its closed index: {}",
                expected.path().as_str()
            ));
        }
    }
    Ok(())
}

fn collect_tree_paths(
    root: &Path,
    relative: &Path,
    depth: usize,
    entries: &mut usize,
    files: &mut BTreeSet<String>,
    description: &str,
) -> Result<(), String> {
    if depth > MAX_EXTENSION_RELATIVE_PATH_DEPTH {
        return Err(format!(
            "{description} extension tree exceeds the path-depth ceiling"
        ));
    }
    let mut children = fs::read_dir(root.join(relative))
        .map_err(|error| format!("cannot enumerate {description} extension tree: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read {description} extension-tree entry: {error}"))?;
    children.sort_unstable_by_key(|entry| entry.file_name());
    for entry in children {
        *entries = entries
            .checked_add(1)
            .ok_or_else(|| format!("{description} extension entry accounting overflowed"))?;
        if *entries > MAX_EXTENSION_TREE_ENTRIES {
            return Err(format!(
                "{description} extension tree exceeds the entry ceiling"
            ));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("{description} extension tree contains a non-UTF-8 path"))?;
        let child = relative.join(name);
        let encoded = encode_relative_path(&child, description)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!("cannot inspect {description} extension-tree entry {encoded}: {error}")
        })?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            collect_tree_paths(root, &child, depth + 1, entries, files, description)?;
        } else if metadata.is_file() && !metadata.file_type().is_symlink() {
            files.insert(encoded);
        } else {
            return Err(format!(
                "{description} extension tree contains a special entry"
            ));
        }
    }
    Ok(())
}

fn encode_relative_path(path: &Path, description: &str) -> Result<String, String> {
    let mut encoded = String::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err(format!(
                "{description} extension tree contains a non-relative path"
            ));
        };
        let component = component
            .to_str()
            .ok_or_else(|| format!("{description} extension tree contains a non-UTF-8 path"))?;
        if !encoded.is_empty() {
            encoded.push('/');
        }
        encoded.push_str(component);
    }
    Ok(encoded)
}

fn read_bounded_file(
    path: &Path,
    max_bytes: u64,
    relative: &str,
    description: &str,
) -> Result<Vec<u8>, String> {
    let path_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {description} file {relative}: {error}"))?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(format!(
            "{description} file {relative} is not an ordinary file"
        ));
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("cannot open {description} file {relative}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open {description} file {relative}: {error}"))?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(format!(
            "{description} file {relative} is not a bounded regular file"
        ));
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| format!("{description} file {relative} does not fit this process"))?,
    );
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {description} file {relative}: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(format!(
            "{description} file {relative} changed while being read"
        ));
    }
    Ok(bytes)
}
