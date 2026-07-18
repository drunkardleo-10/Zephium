//! Bounded per-profile favicon persistence and raster validation.

use super::*;

use zephium_core::icon::{validated_rgba32, RGBA32_BYTES, RGBA32_MIME};

const MAX_CONTENT_TYPE_BYTES: usize = 128;

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
            tx.execute(
                "DELETE FROM favicons WHERE origin IN (
                     SELECT origin FROM favicons
                     ORDER BY fetched_at DESC, origin
                     LIMIT -1 OFFSET 512
                 )",
                [],
            )?;
            tx.commit()
        });
        if let Err(e) = result {
            eprintln!("store: save_favicon failed: {e}");
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

    pub(crate) fn fresh_favicon_raster(
        &mut self,
        profile: ProfileId,
        origin: &str,
        max_age_seconds: i64,
    ) -> Option<Vec<u8>> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || !valid_favicon_origin(origin)
            || max_age_seconds < 0
        {
            return None;
        }
        let oldest = now_secs().saturating_sub(max_age_seconds);
        let bytes = self
            .profile_conn(profile)
            .ok()?
            .query_row(
                "SELECT CASE WHEN length(icon) <= ?3 THEN icon END
                 FROM favicons
                 WHERE origin = ?1 AND fetched_at >= ?2",
                params![origin, oldest, RGBA32_BYTES as i64],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()
            .ok()
            .flatten()??;
        validated_rgba32(&bytes)?;
        Some(bytes)
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
