//! Isolated public-package workflow testing without touching the ordinary QA
//! session. This module is absent from builds without external-extensions-qa.
use std::{ffi::OsStr, io, path::PathBuf};

pub(super) fn select(default: PathBuf, mode: Option<&OsStr>) -> io::Result<PathBuf> {
    match mode {
        None => Ok(default),
        Some(mode) if mode == "1" => Ok(default.join("isolated-compatibility-v1")),
        Some(mode) if mode == "2" => Ok(default.join("isolated-offscreen-v2")),
        Some(mode) if mode == "3" => Ok(default.join("isolated-bitwarden-v2")),
        Some(_) => Err(io::Error::other(
            "ZEPHIUM_EXTERNAL_QA_ISOLATED must be unset, 1, 2, or 3",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolation_is_a_fixed_child_not_an_arbitrary_data_path() {
        let root = PathBuf::from("qa-data");
        assert_eq!(select(root.clone(), None).unwrap(), root);
        assert_eq!(
            select(root.clone(), Some(OsStr::new("1"))).unwrap(),
            root.join("isolated-compatibility-v1")
        );
        assert_eq!(
            select(root.clone(), Some(OsStr::new("2"))).unwrap(),
            root.join("isolated-offscreen-v2")
        );
        assert_eq!(
            select(root.clone(), Some(OsStr::new("3"))).unwrap(),
            root.join("isolated-bitwarden-v2")
        );
        assert!(select(root, Some(OsStr::new("../other-profile"))).is_err());
    }
}
