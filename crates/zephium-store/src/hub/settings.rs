//! Bounded application settings stored in the authoritative meta database.

use super::*;

pub(crate) const MAX_SETTING_KEY_BYTES: usize = 256;
pub(crate) const MAX_SETTING_VALUE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_APP_SETTINGS: i64 = 128;

impl Hub {
    pub(crate) fn app_setting(&mut self, key: &str) -> Option<String> {
        if key.is_empty() || key.len() > MAX_SETTING_KEY_BYTES {
            return None;
        }
        self.meta
            .query_row(
                "SELECT CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN value END
                 FROM settings WHERE key = ?1",
                params![key, MAX_SETTING_VALUE_BYTES as i64],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
    }

    /// Loads the exact durable key cohort before the actor starts. The shared
    /// admission registry is initialized from this set, so a newly accepted key
    /// can never discover only later that the durable table was already full.
    pub(crate) fn app_setting_keys(&self) -> rusqlite::Result<HashSet<String>> {
        let count = self
            .meta
            .query_row("SELECT count(*) FROM settings", [], |row| {
                row.get::<_, i64>(0)
            })?;
        if !(0..=MAX_APP_SETTINGS).contains(&count) {
            return Err(invalid_data(
                "application-setting registry exceeds persistence limit",
            ));
        }
        let mut statement = self.meta.prepare(
            "SELECT CASE
                        WHEN length(CAST(key AS BLOB)) BETWEEN 1 AND ?1 THEN key
                    END
             FROM settings ORDER BY key",
        )?;
        let rows = statement.query_map([MAX_SETTING_KEY_BYTES as i64], |row| {
            row.get::<_, Option<String>>(0)
        })?;
        let mut keys = HashSet::with_capacity(count as usize);
        for row in rows {
            let key = row?.ok_or_else(|| invalid_data("application-setting key exceeds limit"))?;
            if !keys.insert(key) {
                return Err(invalid_data(
                    "application-setting registry contains duplicate keys",
                ));
            }
        }
        if keys.len() != count as usize {
            return Err(invalid_data(
                "application-setting registry changed while loading",
            ));
        }
        Ok(keys)
    }

    pub(crate) fn set_app_setting(&mut self, key: &str, value: &str) -> rusqlite::Result<bool> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if key.is_empty()
            || key.len() > MAX_SETTING_KEY_BYTES
            || value.len() > MAX_SETTING_VALUE_BYTES
        {
            return Ok(false);
        }
        let exists = self.meta.query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1)",
            [key],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            let full = self.meta.query_row(
                "SELECT count(*) >= ?1 FROM settings",
                [MAX_APP_SETTINGS],
                |row| row.get::<_, bool>(0),
            )?;
            if full {
                return Ok(false);
            }
        }
        let changed = self.meta.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(changed == 1)
    }

    #[cfg(test)]
    pub(crate) fn fail_setting_writes(&mut self) {
        self.meta
            .execute_batch(
                "CREATE TRIGGER test_fail_setting_write
                 BEFORE INSERT ON settings BEGIN
                     SELECT RAISE(FAIL, 'injected setting failure');
                 END;",
            )
            .unwrap();
    }
}
