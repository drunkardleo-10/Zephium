use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zephium_extension_package::{
    ChromiumExtensionId, PortableRelativePath, VerifiedCrx3Package, MAX_CRX3_HEADER_BYTES,
    MAX_EXTENSION_ARCHIVE_BYTES, MAX_EXTENSION_RELATIVE_PATH_DEPTH, MAX_EXTENSION_TREE_BYTES,
    MAX_EXTENSION_TREE_ENTRIES, MAX_EXTENSION_TREE_FILES, MAX_EXTENSION_TREE_FILE_BYTES,
};

const CRX3_PREFIX_BYTES: u64 = 12;

pub(crate) fn check(path: &Path, expected_id: &str) -> Result<(), String> {
    let bytes = read_crx(path)?;
    let expected_id = parse_expected_id(expected_id)?;
    let package = VerifiedCrx3Package::parse_and_verify(&bytes, Some(&expected_id))
        .map_err(|error| error.to_string())?;
    print_authentication(&bytes, &package);
    Ok(())
}

/// Authenticates and safely materializes one CRX into a fresh diagnostic tree.
///
/// This is intentionally an offline probe utility, not product admission. The
/// resulting bytes still require a catalog release envelope, canonical tree
/// receipt, and live lease before they can acquire product authority.
pub(crate) fn materialize_probe(
    path: &Path,
    expected_id: &str,
    output: &Path,
) -> Result<(), String> {
    let bytes = read_crx(path)?;
    let expected_id = parse_expected_id(expected_id)?;
    let package = VerifiedCrx3Package::parse_and_verify(&bytes, Some(&expected_id))
        .map_err(|error| error.to_string())?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "materialization output has no final component".to_owned())?;
    let output = parent.join(name);
    if output.exists() {
        return Err("materialization output already exists".into());
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(package.archive_bytes()))
        .map_err(|error| format!("authenticated CRX payload is not a supported ZIP: {error}"))?;
    let plans = preflight_archive(&mut archive)?;
    fs::create_dir(&output)
        .map_err(|error| format!("cannot atomically reserve materialization output: {error}"))?;
    restrict_directory(&output)?;
    let incomplete = output.join(".zephium-incomplete");
    if let Err(error) = (|| {
        let marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&incomplete)
            .map_err(|error| format!("cannot create materialization marker: {error}"))?;
        marker
            .sync_all()
            .map_err(|error| format!("cannot sync materialization marker: {error}"))?;
        extract_archive(&mut archive, &plans, &output)?;
        sync_directory(&output)?;
        fs::remove_file(&incomplete)
            .map_err(|error| format!("cannot retire materialization marker: {error}"))?;
        sync_directory(&output)
    })() {
        let _ = fs::remove_dir_all(&output);
        return Err(error);
    }
    sync_directory(&parent)?;

    print_authentication(&bytes, &package);
    let file_count = plans
        .iter()
        .filter(|plan| plan.kind == EntryKind::File)
        .count();
    let total_bytes = plans
        .iter()
        .filter(|plan| plan.kind == EntryKind::File)
        .map(|plan| plan.length)
        .sum::<u64>();
    println!(
        "CRX3 probe materialization passed: files={file_count}; bytes={total_bytes}; product_authority=false"
    );
    Ok(())
}

fn read_crx(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect archive: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("archive must be one ordinary regular file".into());
    }
    let max_bytes = MAX_EXTENSION_ARCHIVE_BYTES
        .checked_add(MAX_CRX3_HEADER_BYTES as u64)
        .and_then(|value| value.checked_add(CRX3_PREFIX_BYTES))
        .ok_or_else(|| "CRX3 byte ceiling overflowed".to_owned())?;
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!(
            "archive uses {} bytes; maximum is {max_bytes}",
            metadata.len()
        ));
    }

    let capacity = usize::try_from(metadata.len())
        .map_err(|_| "archive length does not fit this process".to_owned())?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(|error| format!("cannot open archive: {error}"))?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read archive: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err("archive changed while being read".into());
    }
    Ok(bytes)
}

fn parse_expected_id(expected_id: &str) -> Result<ChromiumExtensionId, String> {
    ChromiumExtensionId::parse(expected_id)
        .map_err(|error| format!("expected extension id is invalid: {error}"))
}

fn print_authentication(bytes: &[u8], package: &VerifiedCrx3Package<'_>) {
    let package_sha256: [u8; 32] = Sha256::digest(bytes).into();
    let developer_key_sha256 = package.developer_key_sha256().bytes();
    println!(
        "CRX3 package authentication passed: extension_id={}; crx_sha256={}; developer_key_sha256={}; proofs={}; archive_bytes={}",
        package.extension_id(),
        lower_hex(&package_sha256),
        lower_hex(&developer_key_sha256),
        package.signature_proof_count(),
        package.archive_bytes().len(),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Directory,
    File,
}

#[derive(Debug)]
struct EntryPlan {
    path: PathBuf,
    kind: EntryKind,
    length: u64,
}

fn preflight_archive(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<Vec<EntryPlan>, String> {
    if archive.is_empty() || archive.len() > MAX_EXTENSION_TREE_ENTRIES {
        return Err(format!(
            "ZIP entry count {} is outside the supported range",
            archive.len()
        ));
    }
    let mut plans = Vec::with_capacity(archive.len());
    let mut archive_paths = BTreeSet::new();
    let mut portable_shapes = BTreeMap::new();
    let mut files = 0_usize;
    let mut bytes = 0_u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect ZIP entry {index}: {error}"))?;
        if entry.encrypted() {
            return Err(format!("ZIP entry {index} is encrypted"));
        }
        let raw_name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| format!("ZIP entry {index} has a non-UTF-8 name"))?;
        let kind = if entry.is_dir() {
            EntryKind::Directory
        } else if entry.is_file() && !entry.is_symlink() {
            EntryKind::File
        } else {
            return Err(format!(
                "ZIP entry {raw_name:?} is not an ordinary file or directory"
            ));
        };
        let normalized = match kind {
            EntryKind::Directory => raw_name.trim_end_matches('/'),
            EntryKind::File => raw_name,
        };
        if normalized.is_empty()
            || normalized.contains('\\')
            || normalized.as_bytes().contains(&0)
            || (kind == EntryKind::File && normalized != raw_name)
        {
            return Err(format!("ZIP entry {raw_name:?} has a non-canonical path"));
        }
        let portable = PortableRelativePath::parse(normalized)
            .map_err(|error| format!("ZIP entry {raw_name:?} is not portable: {error}"))?;
        if portable.as_str() == ".zephium-incomplete" {
            return Err("ZIP uses the reserved materialization marker path".into());
        }
        if portable.as_str().split('/').count() > MAX_EXTENSION_RELATIVE_PATH_DEPTH {
            return Err(format!(
                "ZIP entry {raw_name:?} exceeds the path-depth ceiling"
            ));
        }
        if !archive_paths.insert(portable.as_str().to_owned()) {
            return Err(format!("ZIP contains a duplicate path: {normalized}"));
        }
        register_shape(&portable, kind, &mut portable_shapes)?;

        let length = entry.size();
        if kind == EntryKind::File {
            files = files
                .checked_add(1)
                .ok_or_else(|| "ZIP file accounting overflowed".to_owned())?;
            if files > MAX_EXTENSION_TREE_FILES {
                return Err("ZIP exceeds the extension file-count ceiling".into());
            }
            if length > MAX_EXTENSION_TREE_FILE_BYTES {
                return Err(format!(
                    "ZIP entry {normalized} exceeds the per-file ceiling"
                ));
            }
            bytes = bytes
                .checked_add(length)
                .ok_or_else(|| "ZIP byte accounting overflowed".to_owned())?;
            if bytes > MAX_EXTENSION_TREE_BYTES {
                return Err("ZIP exceeds the aggregate extension-tree ceiling".into());
            }
        } else if length != 0 {
            return Err(format!("ZIP directory {normalized} has a non-zero payload"));
        }
        plans.push(EntryPlan {
            path: PathBuf::from(portable.as_str()),
            kind,
            length,
        });
    }
    Ok(plans)
}

fn register_shape(
    path: &PortableRelativePath,
    terminal: EntryKind,
    shapes: &mut BTreeMap<Box<str>, (Box<str>, EntryKind)>,
) -> Result<(), String> {
    let parts = path.as_str().split('/').collect::<Vec<_>>();
    let mut prefix = String::new();
    for (index, part) in parts.iter().enumerate() {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        let portable = PortableRelativePath::parse(&prefix)
            .map_err(|error| format!("ZIP path prefix is not portable: {error}"))?;
        let kind = if index + 1 == parts.len() {
            terminal
        } else {
            EntryKind::Directory
        };
        let key = portable.collision_key();
        match shapes.get(key.as_ref()) {
            Some((existing_path, existing_kind))
                if existing_path.as_ref() != portable.as_str() || *existing_kind != kind =>
            {
                return Err(format!(
                    "ZIP path {path:?} has a cross-platform or file/directory collision"
                ));
            }
            Some(_) => {}
            None => {
                shapes.insert(key, (portable.as_str().into(), kind));
            }
        }
    }
    Ok(())
}

fn extract_archive(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    plans: &[EntryPlan],
    root: &Path,
) -> Result<(), String> {
    for (index, plan) in plans.iter().enumerate() {
        let destination = root.join(&plan.path);
        match plan.kind {
            EntryKind::Directory => {
                fs::create_dir_all(&destination).map_err(|error| {
                    format!(
                        "cannot create materialized directory {:?}: {error}",
                        plan.path
                    )
                })?;
                restrict_directory(&destination)?;
            }
            EntryKind::File => {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!("cannot create materialized parent {:?}: {error}", plan.path)
                    })?;
                    restrict_directory(parent)?;
                }
                let mut entry = archive
                    .by_index(index)
                    .map_err(|error| format!("cannot reopen ZIP entry {index}: {error}"))?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)
                    .map_err(|error| {
                        format!("cannot create materialized file {:?}: {error}", plan.path)
                    })?;
                let copied = std::io::copy(&mut entry, &mut output).map_err(|error| {
                    format!("cannot extract materialized file {:?}: {error}", plan.path)
                })?;
                if copied != plan.length {
                    return Err(format!(
                        "ZIP entry {:?} changed length while extracting",
                        plan.path
                    ));
                }
                output
                    .flush()
                    .and_then(|()| output.sync_all())
                    .map_err(|error| {
                        format!(
                            "cannot durably write materialized file {:?}: {error}",
                            plan.path
                        )
                    })?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot restrict materialization directory: {error}"))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync materialization directory: {error}"))
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            for (name, bytes) in entries {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn preflight_accepts_one_bounded_portable_tree() {
        let bytes = archive(&[("manifest.json", b"{}"), ("assets/icon.png", b"png")]);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let plans = preflight_archive(&mut archive).unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].path, Path::new("manifest.json"));
        assert_eq!(plans[1].path, Path::new("assets/icon.png"));
    }

    #[test]
    fn preflight_rejects_cross_platform_case_collision() {
        let bytes = archive(&[("Assets/one.js", b"1"), ("assets/two.js", b"2")]);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(preflight_archive(&mut archive)
            .unwrap_err()
            .contains("cross-platform"));
    }

    #[test]
    fn preflight_rejects_reserved_incomplete_marker() {
        let bytes = archive(&[(".zephium-incomplete", b"not-a-marker")]);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(preflight_archive(&mut archive)
            .unwrap_err()
            .contains("reserved materialization marker"));
    }
}
