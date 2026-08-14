use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use zephium_extension_package::{
    CanonicalExtensionTreeIndex, PortableRelativePath, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILE_BYTES, MAX_EXTENSION_TREE_INDEX_BYTES,
};

#[derive(Serialize)]
struct TreeIndex<'a> {
    schema_version: u32,
    files: &'a [TreeFile],
}

#[derive(Serialize)]
struct TreeFile {
    path: String,
    length: u64,
    sha256: String,
}

#[derive(Default)]
struct Inventory {
    files: Vec<TreeFile>,
    collision_keys: BTreeSet<Box<str>>,
    entries: usize,
    bytes: u64,
}

pub(crate) struct GeneratedTreeIndex {
    pub(crate) bytes: Vec<u8>,
    pub(crate) parsed: CanonicalExtensionTreeIndex,
}

/// Emits a canonical closed-tree index for a diagnostic extension directory.
///
/// This is deliberately not release authentication. It gives native probes a
/// stable, fail-closed byte inventory after a separately authenticated package
/// has been extracted. Product ingestion must bind extraction to its package
/// authority rather than treating this caller-selected index as trust.
pub(crate) fn index_probe_tree(extension: &Path, output: &Path) -> Result<(), String> {
    let extension = canonical_extension_root(extension)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize index parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "tree-index output has no final component".to_owned())?;
    let output = parent.join(name);
    if output.exists() {
        return Err("tree-index output already exists".into());
    }
    if output.starts_with(&extension) {
        return Err("tree-index output may not be inside the indexed extension".into());
    }

    let generated = build_tree_index(&extension)?;

    let mut staged = tempfile::NamedTempFile::new_in(&parent)
        .map_err(|error| format!("cannot create tree-index stage: {error}"))?;
    staged
        .write_all(&generated.bytes)
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|error| format!("cannot durably stage tree index: {error}"))?;
    staged
        .persist_noclobber(&output)
        .map_err(|error| format!("cannot atomically publish tree index: {}", error.error))?;

    println!(
        "extension probe tree indexed: files={}; bytes={}; manifest_sha256={}; tree_sha256={}; index_sha256={}; product_authority=false",
        generated.parsed.files().len(),
        generated.parsed.total_bytes(),
        lower_hex(generated.parsed.manifest_sha256().as_bytes()),
        lower_hex(generated.parsed.tree_sha256().as_bytes()),
        lower_hex(generated.parsed.index_sha256().as_bytes()),
    );
    Ok(())
}

pub(crate) fn verify_closed_tree(
    extension: &Path,
    index: &Path,
) -> Result<(PathBuf, CanonicalExtensionTreeIndex), String> {
    let extension = canonical_extension_root(extension)?;
    let index_metadata = fs::symlink_metadata(index)
        .map_err(|error| format!("cannot inspect extension tree index: {error}"))?;
    if !index_metadata.is_file() || index_metadata.file_type().is_symlink() {
        return Err("extension tree index must be one ordinary file".into());
    }
    let index = index
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize extension tree index: {error}"))?;
    if index.starts_with(&extension) {
        return Err("extension tree index must be outside the indexed tree".into());
    }
    let metadata = fs::metadata(&index)
        .map_err(|error| format!("cannot inspect extension tree index: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_EXTENSION_TREE_INDEX_BYTES as u64 {
        return Err("extension tree index is not a bounded regular file".into());
    }
    let expected_bytes = read_exact_file(&index, metadata.len(), "tree index")?;
    let expected = CanonicalExtensionTreeIndex::parse_canonical(&expected_bytes)
        .map_err(|error| format!("extension tree index is invalid: {error}"))?;
    let observed = build_tree_index(&extension)?;
    if observed.bytes != expected_bytes {
        return Err("extension tree does not match its exact canonical index".into());
    }
    Ok((extension, expected))
}

pub(crate) fn build_tree_index(extension: &Path) -> Result<GeneratedTreeIndex, String> {
    let mut inventory = Inventory::default();
    collect(extension, Path::new(""), 0, &mut inventory)?;
    inventory
        .files
        .sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let bytes = serde_json::to_vec(&TreeIndex {
        schema_version: 1,
        files: &inventory.files,
    })
    .map_err(|error| format!("cannot serialize canonical tree index: {error}"))?;
    let parsed = CanonicalExtensionTreeIndex::parse_canonical(&bytes)
        .map_err(|error| format!("generated tree index is invalid: {error}"))?;
    Ok(GeneratedTreeIndex { bytes, parsed })
}

fn canonical_extension_root(extension: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(extension)
        .map_err(|error| format!("cannot inspect extension root: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("extension root must be one ordinary directory".into());
    }
    extension
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize extension root: {error}"))
}

fn collect(
    root: &Path,
    relative: &Path,
    depth: usize,
    inventory: &mut Inventory,
) -> Result<(), String> {
    if depth > 32 {
        return Err("extension tree exceeds the path-depth ceiling".into());
    }
    let mut entries = fs::read_dir(root.join(relative))
        .map_err(|error| format!("cannot enumerate extension directory: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot enumerate extension entry: {error}"))?;
    entries.sort_unstable_by_key(|entry| entry.file_name());

    for entry in entries {
        inventory.entries = inventory
            .entries
            .checked_add(1)
            .ok_or_else(|| "extension entry accounting overflowed".to_owned())?;
        if inventory.entries > MAX_EXTENSION_TREE_ENTRIES {
            return Err("extension tree exceeds the entry ceiling".into());
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "extension tree contains a non-UTF-8 path".to_owned())?;
        let child = relative.join(name);
        let path = encode_relative_path(&child)?;
        let portable = PortableRelativePath::parse(&path)
            .map_err(|error| format!("extension path {path:?} is not portable: {error}"))?;
        if !inventory.collision_keys.insert(portable.collision_key()) {
            return Err(format!(
                "extension tree contains a cross-platform path collision: {path}"
            ));
        }

        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("cannot inspect extension entry {path}: {error}"))?;
        if metadata.is_dir() {
            collect(root, &child, depth + 1, inventory)?;
            continue;
        }
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "extension entry {path} is not an ordinary file or directory"
            ));
        }
        if metadata.len() > MAX_EXTENSION_TREE_FILE_BYTES {
            return Err(format!(
                "extension file {path} exceeds the per-file byte ceiling"
            ));
        }
        inventory.bytes = inventory
            .bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "extension byte accounting overflowed".to_owned())?;
        if inventory.bytes > MAX_EXTENSION_TREE_BYTES {
            return Err("extension tree exceeds the aggregate byte ceiling".into());
        }
        let bytes = read_exact_file(&entry.path(), metadata.len(), &path)?;
        inventory.files.push(TreeFile {
            path,
            length: metadata.len(),
            sha256: lower_hex(&Sha256::digest(&bytes)),
        });
    }
    Ok(())
}

fn read_exact_file(path: &Path, expected: u64, description: &str) -> Result<Vec<u8>, String> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open extension file {description}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect open extension file {description}: {error}"))?;
    if !metadata.is_file() || metadata.len() != expected {
        return Err(format!(
            "extension file {description} changed during indexing"
        ));
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(expected)
            .map_err(|_| format!("extension file {description} does not fit this process"))?,
    );
    file.take(expected + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read extension file {description}: {error}"))?;
    if bytes.len() as u64 != expected {
        return Err(format!(
            "extension file {description} changed during indexing"
        ));
    }
    Ok(bytes)
}

fn encode_relative_path(path: &Path) -> Result<String, String> {
    let mut encoded = String::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err("extension tree contains a non-relative path".into());
        };
        let component = component
            .to_str()
            .ok_or_else(|| "extension tree contains a non-UTF-8 path".to_owned())?;
        if !encoded.is_empty() {
            encoded.push('/');
        }
        encoded.push_str(component);
    }
    Ok(encoded)
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}
