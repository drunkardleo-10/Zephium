//! Bookmarks: folders and links a person keeps, apart from the sidebar.
//! Pinned tabs stay live tabs; a bookmark is an address to come back to, and
//! an import from another browser lands here as one folder.

/// Bookmarks one profile may hold, folders included.
pub const MAX_BOOKMARKS: u32 = 50_000;
/// Folders nest at most this deep, counting the top level as one.
pub const MAX_DEPTH: usize = 32;
pub const MAX_TITLE_BYTES: usize = 512;
/// Rows one folder listing or search returns.
pub const MAX_LISTING: u32 = 5_000;
pub const MAX_SEARCH_RESULTS: u32 = 100;
pub const MAX_QUERY_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookmarkNode {
    pub id: i64,
    pub parent: Option<i64>,
    /// None for a folder.
    pub url: Option<String>,
    pub title: String,
    /// Direct children, for a folder.
    pub children: u32,
}

impl BookmarkNode {
    pub fn is_folder(&self) -> bool {
        self.url.is_none()
    }
}

/// One bookmark operation. Writes are validated again by the store, which
/// alone sees the tree they apply to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookmarkRequest {
    /// A folder's children in order; `None` is the top level.
    Children {
        parent: Option<i64>,
    },
    /// The folders from the top level down to `id`, `id` last.
    Path {
        id: i64,
    },
    Search {
        query: String,
    },
    /// `if_absent` returns the existing bookmark for the same address instead
    /// of adding a second one.
    AddLink {
        parent: Option<i64>,
        title: String,
        url: String,
        if_absent: bool,
    },
    AddFolder {
        parent: Option<i64>,
        title: String,
    },
    Rename {
        id: i64,
        title: String,
    },
    /// Moves `id` into `parent` at `index` among its new siblings, clamped.
    Move {
        id: i64,
        parent: Option<i64>,
        index: u32,
    },
    /// Removes `id` and, for a folder, everything in it.
    Remove {
        id: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookmarkReply {
    Nodes(Vec<BookmarkNode>),
    /// The bookmark added, or the existing one an `if_absent` add found.
    Added(i64),
    Done,
    Failed(BookmarkFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookmarkFailure {
    /// The bookmark or folder named no longer exists.
    Missing,
    /// The profile holds the maximum, or the folder is nested too deep.
    Full,
    /// A folder cannot move into itself or its own contents.
    Cycle,
    Invalid,
    Unavailable,
}

/// A title as stored: control characters removed, whitespace collapsed,
/// bounded on a character boundary.
pub fn clean_title(title: &str) -> String {
    let collapsed = title
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut end = collapsed.len().min(MAX_TITLE_BYTES);
    while !collapsed.is_char_boundary(end) {
        end -= 1;
    }
    collapsed[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_single_line_and_bounded() {
        assert_eq!(clean_title("  Read\n\tlater \u{7}now "), "Read later now");
        let long = "é".repeat(MAX_TITLE_BYTES);
        let cleaned = clean_title(&long);
        assert!(cleaned.len() <= MAX_TITLE_BYTES);
        assert!(cleaned.chars().all(|c| c == 'é'));
    }
}
