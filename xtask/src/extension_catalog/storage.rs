//! Crash-marked, no-replace filesystem publication primitives.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use zephium_extension_package::PortableRelativePath;

use super::INCOMPLETE_MARKER;

pub(crate) fn read_regular_bounded(
    path: &Path,
    max_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{label} must be one ordinary regular file"));
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!("{label} exceeds its byte ceiling"));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| format!("{label} length does not fit this process"))?;
    let mut bytes = Vec::with_capacity(capacity);
    File::open(path)
        .map_err(|error| format!("cannot open {label}: {error}"))?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(format!("{label} changed while being read"));
    }
    Ok(bytes)
}

pub(crate) fn write_new_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let relative = PortableRelativePath::parse(relative)
        .map_err(|error| format!("publication target is not portable: {error}"))?;
    let path = root.join(relative.as_str());
    let parent = path
        .parent()
        .ok_or_else(|| "publication target has no parent".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create publication directory: {error}"))?;
    restrict_directory(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("cannot create publication target: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write publication target: {error}"))
}

pub(crate) fn absent_output_path(output: &Path) -> Result<PathBuf, String> {
    if path_entry_exists(output)? {
        return Err("catalog publication output already exists".into());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize publication output parent: {error}"))?;
    let name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "catalog publication output has no final component".to_owned())?;
    let output = parent.join(name);
    if path_entry_exists(&output)? {
        return Err("catalog publication output already exists".into());
    }
    Ok(output)
}

pub(super) fn publish_no_replace(staging: tempfile::TempDir, output: &Path) -> Result<(), String> {
    publish_named_components_no_replace(
        staging,
        output,
        &["metadata", "targets", "product", "evidence"],
    )
}

pub(crate) fn publish_named_components_no_replace(
    staging: tempfile::TempDir,
    output: &Path,
    components: &[&str],
) -> Result<(), String> {
    create_restricted_directory(output)
        .map_err(|error| format!("cannot reserve catalog publication output: {error}"))?;
    let marker = output.join(INCOMPLETE_MARKER);
    write_marker(&marker)?;
    sync_directory(output)?;
    for &name in components {
        fs::rename(staging.path().join(name), output.join(name)).map_err(|error| {
            format!(
                "cannot publish catalog component {name}; incomplete output retained at {}: {error}",
                output.display()
            )
        })?;
    }
    sync_directory(output)?;
    fs::remove_file(&marker).map_err(|error| {
        format!(
            "cannot settle catalog publication marker at {}: {error}",
            output.display()
        )
    })?;
    sync_directory(output)?;
    sync_directory(
        output
            .parent()
            .ok_or_else(|| "catalog publication output has no parent".to_owned())?,
    )
}

fn write_marker(path: &Path) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot create publication marker: {error}"))?;
    file.write_all(b"incomplete\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot sync publication marker: {error}"))
}

fn path_entry_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot inspect catalog publication path: {error}")),
    }
}

fn create_restricted_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new().create(path)
    }
}

#[cfg(unix)]
pub(crate) fn restrict_directory(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot restrict publication directory: {error}"))
}

#[cfg(not(unix))]
pub(crate) fn restrict_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn sync_directory_tree(root: &Path) -> Result<(), String> {
    let mut directories = vec![root.to_owned()];
    let mut cursor = 0;
    while cursor < directories.len() {
        let directory = directories[cursor].clone();
        cursor += 1;
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("cannot enumerate publication directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot enumerate publication entry: {error}"))?;
            if entry
                .file_type()
                .map_err(|error| format!("cannot inspect publication entry: {error}"))?
                .is_dir()
            {
                directories.push(entry.path());
            }
        }
    }
    for directory in directories.iter().rev() {
        sync_directory(directory)?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn sync_directory_tree(_root: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync publication directory: {error}"))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}
