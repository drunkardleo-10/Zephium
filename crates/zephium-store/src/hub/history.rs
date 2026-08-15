//! Bounded per-profile history writes, search, and byte-budget enforcement.

use super::*;

use zephium_core::item::sanitize_page_title;
use zephium_core::ports::store::HistoryHit;

pub(crate) const MAX_HISTORY_QUERY_BYTES: usize = 4 * 1024;
pub(crate) const MAX_HISTORY_RESULTS: u32 = 100;
const MAX_HISTORY_TOKEN_CHARS: usize = 256;
const MAX_VISIT_BATCH: usize = 2048;
pub(crate) const MAX_HISTORY_BYTES: i64 = 64 * 1024 * 1024;
const HISTORY_PRUNE_BATCH: i64 = 2048;
const MAX_HISTORY_PRUNE_PASSES: usize = 26;
const MAX_HISTORY_SEARCH_ROWS: i64 = 4096;

impl Hub {
    #[cfg(test)]
    pub(crate) fn record_visit(&mut self, profile: ProfileId, url: &str, title: &str) {
        let _ = self.record_visits([(profile, url.to_owned(), title.to_owned())]);
    }

    pub(crate) fn record_visits(
        &mut self,
        visits: impl IntoIterator<Item = (ProfileId, String, String)>,
    ) -> Result<(), Vec<(ProfileId, String, String)>> {
        let visits: Vec<_> = visits.into_iter().take(MAX_VISIT_BATCH).collect();
        if self.recovery_required.is_some() {
            return Err(visits);
        }
        let mut grouped: HashMap<ProfileId, Vec<(String, String)>> = HashMap::new();
        for (profile, url, title) in visits {
            if self.registry.contains(&profile)
                && !self.degraded_profiles.contains(&profile)
                && navigation::is_allowed_str(&url)
            {
                grouped
                    .entry(profile)
                    .or_default()
                    .push((url, sanitize_page_title(&title)));
            }
        }
        if grouped.is_empty() {
            return Ok(());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut failed = Vec::new();
        for (profile, visits) in grouped {
            let result = self.profile_conn(profile).and_then(|conn| {
                let tx = conn.transaction()?;
                {
                    let mut insert = tx.prepare_cached(
                        "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)",
                    )?;
                    for (url, title) in &visits {
                        insert.execute(params![url, title, now])?;
                    }
                }
                // Bound page-controlled disk growth once per batch. The
                // visited_at index and FTS triggers keep pruning deterministic.
                tx.execute(
                    "DELETE FROM history WHERE id IN (
                         SELECT id FROM history
                         ORDER BY visited_at DESC, id DESC
                         LIMIT -1 OFFSET 50000
                     )",
                    [],
                )?;
                enforce_history_budget(&tx)?;
                tx.commit()
            });
            if let Err(e) = result {
                eprintln!("store: record_visits failed for profile {profile}: {e}");
                failed.extend(visits.into_iter().map(|(url, title)| (profile, url, title)));
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed)
        }
    }

    pub(crate) fn search_history(
        &mut self,
        profile: ProfileId,
        query: &str,
        limit: u32,
    ) -> Vec<HistoryHit> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || query.len() > MAX_HISTORY_QUERY_BYTES
            || limit == 0
        {
            return Vec::new();
        }
        let Some(fts) = fts_query(query) else {
            return Vec::new();
        };
        let limit = limit.min(MAX_HISTORY_RESULTS);
        if self.recovery_required.is_some() {
            return Vec::new();
        }
        let Ok(conn) = self.profile_conn(profile) else {
            return Vec::new();
        };
        let recent_floor = conn
            .query_row(
                "SELECT COALESCE((
                     SELECT id FROM history
                     ORDER BY id DESC LIMIT 1 OFFSET ?1
                 ), 0)",
                [MAX_HISTORY_SEARCH_ROWS - 1],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(i64::MAX);
        let Ok(mut stmt) = conn.prepare_cached(
            "SELECT h.url, h.title, MAX(h.visited_at) AS last
             FROM history_fts f JOIN history h ON h.id = f.rowid
             WHERE history_fts MATCH ?1
               AND f.rowid >= ?5
               AND length(CAST(h.url AS BLOB)) <= ?3
               AND length(CAST(h.title AS BLOB)) <= ?4
             GROUP BY h.url
             ORDER BY last DESC
             LIMIT ?2",
        ) else {
            return Vec::new();
        };
        stmt.query_map(
            params![
                fts,
                limit,
                MAX_URL_BYTES as i64,
                MAX_TITLE_BYTES as i64,
                recent_floor
            ],
            |r| {
                Ok(HistoryHit {
                    url: r.get(0)?,
                    title: r.get(1)?,
                    last_visit: r.get(2)?,
                })
            },
        )
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter(|hit| navigation::is_allowed_str(&hit.url))
                .map(|mut hit| {
                    hit.title = sanitize_page_title(&hit.title);
                    hit
                })
                .collect()
        })
        .unwrap_or_default()
    }

    pub(crate) fn recent_history(&mut self, profile: ProfileId, limit: u32) -> Vec<HistoryHit> {
        if !self.registry.contains(&profile)
            || self.degraded_profiles.contains(&profile)
            || self.recovery_required.is_some()
            || limit == 0
        {
            return Vec::new();
        }
        let limit = limit.min(MAX_HISTORY_RESULTS);
        let Ok(conn) = self.profile_conn(profile) else {
            return Vec::new();
        };
        let recent_floor = conn
            .query_row(
                "SELECT COALESCE((
                     SELECT id FROM history
                     ORDER BY id DESC LIMIT 1 OFFSET ?1
                 ), 0)",
                [MAX_HISTORY_SEARCH_ROWS - 1],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(i64::MAX);
        let Ok(mut stmt) = conn.prepare_cached(
            "WITH ranked AS (
                 SELECT id, url, title, visited_at,
                        ROW_NUMBER() OVER (
                            PARTITION BY url ORDER BY visited_at DESC, id DESC
                        ) AS rank
                 FROM history
                 WHERE id >= ?4
             )
             SELECT url, title, visited_at
             FROM ranked
             WHERE rank = 1
               AND length(CAST(url AS BLOB)) <= ?2
               AND length(CAST(title AS BLOB)) <= ?3
             ORDER BY visited_at DESC, id DESC
             LIMIT ?1",
        ) else {
            return Vec::new();
        };
        stmt.query_map(
            params![
                limit,
                MAX_URL_BYTES as i64,
                MAX_TITLE_BYTES as i64,
                recent_floor
            ],
            |row| {
                Ok(HistoryHit {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    last_visit: row.get(2)?,
                })
            },
        )
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter(|hit| navigation::is_allowed_str(&hit.url))
                .map(|mut hit| {
                    hit.title = sanitize_page_title(&hit.title);
                    hit
                })
                .collect()
        })
        .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn history_matches(&mut self, profile: ProfileId, query: &str) -> i64 {
        self.profile_conn(profile)
            .and_then(|conn| {
                conn.query_row(
                    "SELECT count(*) FROM history_fts WHERE history_fts MATCH ?1",
                    [query],
                    |r| r.get(0),
                )
            })
            .unwrap_or(-1)
    }

    #[cfg(test)]
    pub(crate) fn history_count(&mut self, profile: ProfileId) -> i64 {
        self.profile_conn(profile)
            .and_then(|conn| conn.query_row("SELECT count(*) FROM history", [], |r| r.get(0)))
            .unwrap_or(-1)
    }

    #[cfg(test)]
    pub(crate) fn fail_history_writes(&mut self, profile: ProfileId) {
        self.profile_conn(profile)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER test_fail_history_write
                 BEFORE INSERT ON history BEGIN
                     SELECT RAISE(FAIL, 'injected history failure');
                 END;",
            )
            .unwrap();
    }
}

pub(super) fn enforce_history_budget(conn: &Connection) -> rusqlite::Result<()> {
    enforce_history_budget_to(conn, MAX_HISTORY_BYTES)
}

fn enforce_history_budget_to(conn: &Connection, maximum_bytes: i64) -> rusqlite::Result<()> {
    for _ in 0..MAX_HISTORY_PRUNE_PASSES {
        let bytes = conn.query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
            row.get::<_, i64>(0)
        })?;
        if bytes <= maximum_bytes {
            return Ok(());
        }
        let deleted = conn.execute(
            "DELETE FROM history WHERE id IN (
                 SELECT id FROM history
                 ORDER BY visited_at, id
                 LIMIT ?1
             )",
            [HISTORY_PRUNE_BATCH],
        )?;
        if deleted == 0 {
            return Err(invalid_data("history byte accounting cannot be reconciled"));
        }
    }
    let bytes = conn.query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if bytes > maximum_bytes {
        return Err(invalid_data("history exceeds bounded pruning work"));
    }
    Ok(())
}

/// Tokenized prefix query; every token is quoted so user input can never be
/// FTS5 syntax.
fn fts_query(query: &str) -> Option<String> {
    if query.len() > MAX_HISTORY_QUERY_BYTES {
        return None;
    }
    let tokens: Vec<String> = query
        .split_whitespace()
        .take(8)
        .map(|token| {
            token
                .chars()
                .filter(|c| !c.is_control() && *c != '"')
                .take(MAX_HISTORY_TOKEN_CHARS)
                .collect::<String>()
        })
        .map(|token| format!("\"{token}\"*"))
        .filter(|t| t.len() > 3)
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

#[cfg(test)]
mod connection_hardening_tests {
    use super::*;

    #[test]
    fn history_usage_triggers_and_byte_budget_prune_oldest_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure(&connection).unwrap();
        migrations::apply(&mut connection, migrations::PROFILE).unwrap();
        for visited_at in 0..3000 {
            connection
                .execute(
                    "INSERT INTO history(url, title, visited_at) VALUES (?1, ?2, ?3)",
                    params![
                        format!("https://example.com/{visited_at}"),
                        "x".repeat(100),
                        visited_at
                    ],
                )
                .unwrap();
        }
        let before: i64 = connection
            .query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(before > 150_000);

        enforce_history_budget_to(&connection, 150_000).unwrap();
        let after: i64 = connection
            .query_row("SELECT bytes FROM history_usage WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let oldest: i64 = connection
            .query_row("SELECT min(visited_at) FROM history", [], |row| row.get(0))
            .unwrap();
        assert!(after <= 150_000);
        assert!(oldest > 0, "oldest rows must be pruned first");
    }
}
