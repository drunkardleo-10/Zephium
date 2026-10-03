//! Bounded per-profile favicon persistence and raster validation.

use super::*;

use zephium_core::icon::{validated_rgba32, RGBA32_BYTES, RGBA32_MIME};

const MAX_CONTENT_TYPE_BYTES: usize = 128;
/// Rows a profile keeps; the oldest beyond this are dropped on save.
const MAX_STORED_FAVICONS: i64 = 1024;
/// At most half of them may come from one import.
const MAX_IMPORTED_FAVICONS: usize = 512;

impl Hub {
    pub(crate) fn favicon_age(&mut self, profile: ProfileId, origin: &str) -> Option<i64> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
        {
            return None;
        }
        let now = now_secs();
        self.profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT fetched_at FROM favicons WHERE origin = ?1",
                [origin],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
            .map(|at| now.saturating_sub(at))
    }

    pub(crate) fn save_favicon(
        &mut self,
        profile: ProfileId,
        origin: &str,
        _content_type: Option<&str>,
        bytes: &[u8],
    ) {
        if self.recovery_required.is_some() {
            return;
        }
        let Some(content_type) = validated_favicon(origin, bytes) else {
            return;
        };
        if !self.registry.contains(&profile) || self.degraded_profiles.contains(&profile) {
            return;
        }
        let now = now_secs();
        let result = self.profile_conn(profile).and_then(|conn| {
            let tx = conn.transaction()?;
            tx.prepare_cached(
                "INSERT INTO favicons(origin, content_type, icon, fetched_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(origin) DO UPDATE SET content_type = ?2, icon = ?3, fetched_at = ?4",
            )?
            .execute(params![origin, content_type, bytes, now])?;
            // Rows from before the fixed-raster format can never be read back
            // and would otherwise hold retention slots forever.
            tx.execute(
                "DELETE FROM favicons WHERE length(icon) <> ?1",
                [RGBA32_BYTES as i64],
            )?;
            tx.execute(
                "DELETE FROM favicons WHERE origin IN (
                     SELECT origin FROM favicons
                     ORDER BY fetched_at DESC, origin
                     LIMIT -1 OFFSET ?1
                 )",
                [MAX_STORED_FAVICONS],
            )?;
            tx.commit()
        });
        if let Err(e) = result {
            eprintln!("store: save_favicon failed: {e}");
        }
    }

    /// Icons another browser held. An origin with a row keeps it, and only
    /// free rows are filled, so this never replaces or evicts an icon Zephium
    /// fetched itself.
    pub(crate) fn import_favicons(
        &mut self,
        profile: ProfileId,
        icons: &[(String, Vec<u8>)],
    ) -> Option<u32> {
        if self.recovery_required.is_some()
            || !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
        {
            return None;
        }
        let now = now_secs();
        let result = self.profile_conn(profile).and_then(|conn| {
            let tx = conn.transaction()?;
            let held: i64 = tx.query_row("SELECT count(*) FROM favicons", [], |row| row.get(0))?;
            // Only free rows are filled, so retention never evicts for an import.
            let room = usize::try_from(MAX_STORED_FAVICONS - held)
                .unwrap_or(0)
                .min(MAX_IMPORTED_FAVICONS);
            let mut added = 0u32;
            {
                let mut insert = tx.prepare_cached(
                    "INSERT INTO favicons(origin, content_type, icon, fetched_at)
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT(origin) DO NOTHING",
                )?;
                for (origin, bytes) in icons {
                    if added as usize >= room {
                        break;
                    }
                    let Some(content_type) = validated_favicon(origin, bytes) else {
                        continue;
                    };
                    added += insert.execute(params![origin, content_type, bytes, now])? as u32;
                }
            }
            tx.commit()?;
            Ok(added)
        });
        match result {
            Ok(added) => Some(added),
            Err(e) => {
                eprintln!("store: import_favicons failed: {e}");
                None
            }
        }
    }

    pub(crate) fn favicon_bytes(
        &mut self,
        profile: ProfileId,
        origin: &str,
    ) -> Option<(Option<String>, Vec<u8>)> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
        {
            return None;
        }
        let (_stored_content_type, bytes) = self
            .profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT CASE
                            WHEN length(CAST(content_type AS BLOB)) <= ?2 THEN content_type
                        END,
                        CASE WHEN length(icon) <= ?3 THEN icon END
                 FROM favicons WHERE origin = ?1",
                params![origin, MAX_CONTENT_TYPE_BYTES as i64, RGBA32_BYTES as i64],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<Vec<u8>>>(1)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let bytes = bytes?;
        validated_rgba32(&bytes)?;
        Some((Some(RGBA32_MIME.to_owned()), bytes))
    }

    #[cfg(test)]
    pub(crate) fn favicon_rows(&mut self, profile: ProfileId) -> i64 {
        self.profile_conn(profile)
            .and_then(|conn| conn.query_row("SELECT count(*) FROM favicons", [], |r| r.get(0)))
            .unwrap_or(-1)
    }

    #[cfg(test)]
    pub(crate) fn seed_malformed_favicon(&mut self, profile: ProfileId, origin: &str, len: usize) {
        let _ = self.profile_conn(profile).map(|conn| {
            conn.execute(
                "INSERT INTO favicons(origin, content_type, icon, fetched_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![origin, RGBA32_MIME, vec![7u8; len], now_secs()],
            )
        });
    }

    pub(crate) fn favicon_raster_with_age(
        &mut self,
        profile: ProfileId,
        origin: &str,
    ) -> Option<(Vec<u8>, i64)> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
        {
            return None;
        }
        let now = now_secs();
        let (bytes, fetched_at) = self
            .profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT CASE WHEN length(icon) <= ?2 THEN icon END, fetched_at
                 FROM favicons
                 WHERE origin = ?1",
                params![origin, RGBA32_BYTES as i64],
                |row| Ok((row.get::<_, Option<Vec<u8>>>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .ok()
            .flatten()?;
        let bytes = bytes?;
        validated_rgba32(&bytes)?;
        Some((bytes, now.saturating_sub(fetched_at)))
    }
}

pub(crate) fn valid_favicon_origin(origin: &str) -> bool {
    if origin.len() > MAX_URL_BYTES || !navigation::is_allowed_str(origin) {
        return false;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    matches!(url.origin(), url::Origin::Tuple(..)) && url.origin().ascii_serialization() == origin
}

pub(crate) fn validated_favicon(origin: &str, bytes: &[u8]) -> Option<&'static str> {
    valid_favicon_origin(origin).then_some(())?;
    validated_rgba32(bytes)?;
    Some(RGBA32_MIME)
}
