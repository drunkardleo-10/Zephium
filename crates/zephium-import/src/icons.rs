//! Site icons another browser already holds, so imported bookmarks and history
//! wear their real marks without Zephium asking any site for them. The bytes
//! come back as the source stored them; decoding stays with the caller's
//! bounded decoder.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::{snapshot, ImportError};

/// Icons read from one source. The profile's favicon store keeps 1024, so an
/// import never crowds out the icons Zephium fetched itself.
pub const MAX_ICONS: usize = 512;
/// Larger stored images are skipped rather than decoded.
pub const MAX_ICON_SOURCE_BYTES: usize = 256 * 1024;
/// The raster every icon is fitted into; a stored size nearest it is chosen.
const WANTED_SIDE: i64 = 32;
/// Firefox records a vector icon with this width; the decoder takes rasters only.
const FIREFOX_VECTOR_WIDTH: i64 = 65_535;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIcon {
    /// `https://host[:port]`, as the favicon store keys it.
    pub origin: String,
    pub bytes: Vec<u8>,
}

/// The HTTPS origin of a page address, the only kind the favicon store serves.
pub fn https_origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    match parsed.origin() {
        origin @ url::Origin::Tuple(..) => Some(origin.ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

/// Lower is better: the smallest stored side at or above the raster, then the
/// largest below it.
fn fit(width: i64) -> i64 {
    if width >= WANTED_SIDE {
        width - WANTED_SIDE
    } else {
        1_000 + (WANTED_SIDE - width)
    }
}

/// Picks one image per wanted origin from `(page_url, image_id, width)` rows,
/// in the order the origins were asked for.
fn choose(rows: impl Iterator<Item = (String, i64, i64)>, wanted: &[String]) -> Vec<(String, i64)> {
    let asked: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let mut best: HashMap<String, (i64, i64)> = HashMap::new();
    for (page, id, width) in rows {
        let Some(origin) = https_origin(&page) else {
            continue;
        };
        if !asked.contains(origin.as_str()) {
            continue;
        }
        let score = fit(width);
        match best.get(&origin) {
            Some((_, held)) if *held <= score => {}
            _ => {
                best.insert(origin, (id, score));
            }
        }
    }
    wanted
        .iter()
        .filter_map(|origin| best.get(origin).map(|(id, _)| (origin.clone(), *id)))
        .collect()
}

fn read(
    path: &Path,
    rows_sql: &str,
    blob_sql: &str,
    wanted: &[String],
) -> Result<Vec<SourceIcon>, ImportError> {
    let wanted = &wanted[..wanted.len().min(MAX_ICONS)];
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let snapshot = snapshot::open(path)?;
    let conn = &snapshot.conn;
    // Sizes first: only the one image chosen per origin is ever read.
    let chosen = {
        let mut statement = conn.prepare(rows_sql)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        choose(rows.filter_map(Result::ok), wanted)
    };
    let mut blob = conn.prepare(blob_sql)?;
    let mut icons = Vec::with_capacity(chosen.len());
    for (origin, id) in chosen {
        let bytes: Option<Vec<u8>> = blob.query_row([id], |row| row.get(0)).ok();
        if let Some(bytes) =
            bytes.filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_ICON_SOURCE_BYTES)
        {
            icons.push(SourceIcon { origin, bytes });
        }
    }
    Ok(icons)
}

/// Chromium's `Favicons` database, shared by Chrome, Arc, Brave and Edge.
pub(crate) fn chromium(profile: &Path, wanted: &[String]) -> Result<Vec<SourceIcon>, ImportError> {
    read(
        &profile.join("Favicons"),
        "SELECT m.page_url, b.id, b.width FROM icon_mapping m
         JOIN favicon_bitmaps b ON b.icon_id = m.icon_id
         WHERE length(b.image_data) > 0",
        "SELECT image_data FROM favicon_bitmaps WHERE id = ?1",
        wanted,
    )
}

/// Firefox's `favicons.sqlite`, shared by Zen.
pub(crate) fn firefox(profile: &Path, wanted: &[String]) -> Result<Vec<SourceIcon>, ImportError> {
    read(
        &profile.join("favicons.sqlite"),
        &format!(
            "SELECT p.page_url, i.id, i.width FROM moz_pages_w_icons p
             JOIN moz_icons_to_pages t ON t.page_id = p.id
             JOIN moz_icons i ON i.id = t.icon_id
             WHERE i.width <> {FIREFOX_VECTOR_WIDTH} AND length(i.data) > 0"
        ),
        "SELECT data FROM moz_icons WHERE id = ?1",
        wanted,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn origins(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn only_https_page_origins_are_keyed() {
        assert_eq!(
            https_origin("https://Open.Spotify.com/album/1?x=1").as_deref(),
            Some("https://open.spotify.com")
        );
        assert_eq!(
            https_origin("https://example.com:8443/").as_deref(),
            Some("https://example.com:8443")
        );
        assert_eq!(https_origin("http://example.com/"), None);
        assert_eq!(https_origin("chrome://settings"), None);
    }

    #[test]
    fn the_stored_size_nearest_the_raster_wins_in_asked_order() {
        let rows = [
            ("https://a.example/one", 1, 16),
            ("https://a.example/two", 2, 64),
            ("https://a.example/three", 3, 32),
            ("https://b.example/", 4, 16),
            ("https://c.example/", 5, 32),
            ("http://a.example/", 6, 32),
        ]
        .into_iter()
        .map(|(page, id, width)| (page.to_owned(), id, width));
        assert_eq!(
            choose(rows, &origins(&["https://b.example", "https://a.example"])),
            [
                ("https://b.example".to_owned(), 4),
                ("https://a.example".to_owned(), 3)
            ]
        );
    }

    #[test]
    fn chromium_icons_are_read_for_wanted_origins_only() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("Favicons")).unwrap();
        conn.execute_batch(
            "CREATE TABLE icon_mapping(id INTEGER PRIMARY KEY, page_url TEXT, icon_id INTEGER);
             CREATE TABLE favicon_bitmaps(id INTEGER PRIMARY KEY, icon_id INTEGER, width INTEGER, image_data BLOB);
             INSERT INTO icon_mapping(page_url, icon_id) VALUES
               ('https://x.com/home', 1), ('https://news.example/', 2);
             INSERT INTO favicon_bitmaps(icon_id, width, image_data) VALUES
               (1, 16, x'01'), (1, 32, x'0202'), (2, 32, x'03');",
        )
        .unwrap();
        drop(conn);
        let icons = chromium(dir.path(), &origins(&["https://x.com"])).unwrap();
        assert_eq!(
            icons,
            [SourceIcon {
                origin: "https://x.com".into(),
                bytes: vec![2, 2]
            }]
        );
    }

    #[test]
    fn firefox_vector_icons_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("favicons.sqlite")).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE moz_pages_w_icons(id INTEGER PRIMARY KEY, page_url TEXT);
             CREATE TABLE moz_icons_to_pages(page_id INTEGER, icon_id INTEGER);
             CREATE TABLE moz_icons(id INTEGER PRIMARY KEY, width INTEGER, data BLOB);
             INSERT INTO moz_pages_w_icons VALUES (1, 'https://spotify.example/');
             INSERT INTO moz_icons VALUES (1, {FIREFOX_VECTOR_WIDTH}, x'0a'), (2, 16, x'0b');
             INSERT INTO moz_icons_to_pages VALUES (1, 1), (1, 2);"
        ))
        .unwrap();
        drop(conn);
        let icons = firefox(dir.path(), &origins(&["https://spotify.example"])).unwrap();
        assert_eq!(icons.len(), 1);
        assert_eq!(icons[0].bytes, vec![0x0b]);
    }
}
