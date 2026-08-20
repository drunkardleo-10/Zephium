//! Exact profile-wide extension pause and site-denial persistence.

use rusqlite::{params, Connection};

use zephium_core::extensions::{
    ExtensionProfilePolicy, ExtensionProfilePolicyApplyError, ExtensionProfilePolicyMutation,
    ExtensionProfilePolicyRevision, ExtensionSiteAccessScope,
    MAX_EXTENSION_SITE_DENIALS_PER_PROFILE,
};
use zephium_core::ports::store::{
    ExtensionProfilePolicyLoadOutcome, ExtensionProfilePolicyMutationOutcome,
};

use super::*;

// PROFILE v13 carries these ceilings in immutable CHECK/trigger text.
const _: () = assert!(crate::migrations::PROFILE.len() >= 13);
const _: [(); 128] = [(); MAX_EXTENSION_SITE_DENIALS_PER_PROFILE];

impl Hub {
    pub(crate) fn load_extension_profile_policy(
        &mut self,
        profile: ProfileId,
    ) -> rusqlite::Result<ExtensionProfilePolicyLoadOutcome> {
        if !self.registry.contains(&profile) {
            return Ok(ExtensionProfilePolicyLoadOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(ExtensionProfilePolicyLoadOutcome::DegradedProfile);
        }
        let policy = load_policy(self.profile_conn(profile)?)?;
        Ok(ExtensionProfilePolicyLoadOutcome::Loaded(policy))
    }

    pub(crate) fn mutate_extension_profile_policy(
        &mut self,
        profile: ProfileId,
        expected: ExtensionProfilePolicyRevision,
        mutation: ExtensionProfilePolicyMutation,
    ) -> rusqlite::Result<ExtensionProfilePolicyMutationOutcome> {
        if self.recovery_required.is_some() {
            return Err(invalid_data("session recovery mode is read-only"));
        }
        if !self.registry.contains(&profile) {
            return Ok(ExtensionProfilePolicyMutationOutcome::NotRegistered);
        }
        if self.degraded_profiles.contains(&profile) {
            return Ok(ExtensionProfilePolicyMutationOutcome::DegradedProfile);
        }
        self.profile_conn(profile)?;
        let meta = &self.meta;
        let conn = self
            .profiles
            .get_mut(&profile)
            .ok_or_else(|| invalid_data("registered extension profile connection is absent"))?;
        let tx = conn.transaction()?;
        let current = load_policy(&tx)?;
        let application = match current.apply(expected, mutation) {
            Ok(application) => application,
            Err(ExtensionProfilePolicyApplyError::RevisionConflict { current, .. }) => {
                return Ok(ExtensionProfilePolicyMutationOutcome::Conflict { current })
            }
            Err(ExtensionProfilePolicyApplyError::LimitReached) => {
                return Ok(ExtensionProfilePolicyMutationOutcome::LimitReached)
            }
            Err(ExtensionProfilePolicyApplyError::RevisionExhausted) => {
                return Ok(ExtensionProfilePolicyMutationOutcome::RevisionExhausted)
            }
            Err(ExtensionProfilePolicyApplyError::InvalidPolicy(_)) => {
                return Ok(ExtensionProfilePolicyMutationOutcome::Invalid)
            }
        };
        if !application.changed() {
            return Ok(ExtensionProfilePolicyMutationOutcome::Applied {
                policy: application.into_policy(),
                changed: false,
            });
        }
        if super::native_ownership::has_unresolved_native_ownership_for_profile(meta, profile)? {
            return Ok(ExtensionProfilePolicyMutationOutcome::RuntimeOwnershipConflict);
        }
        let policy = application.into_policy();
        persist_policy(&tx, expected, &policy)?;
        let committed = tx.commit();
        match committed {
            Ok(()) => Ok(ExtensionProfilePolicyMutationOutcome::Applied {
                policy,
                changed: true,
            }),
            Err(error) => {
                eprintln!(
                    "store: profile {profile} extension-policy commit outcome is unknown: {error}"
                );
                Ok(ExtensionProfilePolicyMutationOutcome::OutcomeUnknown)
            }
        }
    }
}

pub(super) fn load_policy(conn: &Connection) -> rusqlite::Result<ExtensionProfilePolicy> {
    let (revision, paused) = conn.query_row(
        "SELECT revision, paused FROM extension_profile_policy WHERE id = 1",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let revision = u64::try_from(revision)
        .ok()
        .and_then(ExtensionProfilePolicyRevision::new)
        .ok_or_else(|| invalid_data("extension profile-policy revision is invalid"))?;
    let paused = durable_bool(paused, "extension profile-policy pause bit is invalid")?;
    let count = conn.query_row(
        "SELECT count(*) FROM extension_profile_site_denials",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if !(0..=MAX_EXTENSION_SITE_DENIALS_PER_PROFILE as i64).contains(&count) {
        return Err(invalid_data("extension site-denial cohort exceeds limit"));
    }
    let mut statement = conn.prepare(
        "SELECT CASE
             WHEN length(CAST(pattern AS BLOB)) BETWEEN 1 AND 2048
             THEN pattern END
         FROM extension_profile_site_denials
         WHERE policy_id = 1
         ORDER BY pattern",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, Option<String>>(0))?;
    let mut denied_sites = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    for row in rows {
        let pattern = row?
            .ok_or_else(|| invalid_data("extension site-denial pattern exceeds durable limit"))?;
        denied_sites.push(
            ExtensionSiteAccessScope::parse_exact(&pattern)
                .map_err(|_| invalid_data("extension site-denial pattern is invalid"))?,
        );
    }
    if denied_sites.len() != usize::try_from(count).unwrap_or(usize::MAX) {
        return Err(invalid_data(
            "extension site-denial cohort changed while loading",
        ));
    }
    ExtensionProfilePolicy::from_persisted(revision, paused, denied_sites)
        .map_err(|_| invalid_data("extension profile policy is invalid"))
}

fn persist_policy(
    tx: &rusqlite::Transaction<'_>,
    expected: ExtensionProfilePolicyRevision,
    policy: &ExtensionProfilePolicy,
) -> rusqlite::Result<()> {
    let changed = tx.execute(
        "UPDATE extension_profile_policy
         SET revision = ?1, paused = ?2
         WHERE id = 1 AND revision = ?3",
        params![
            revision_i64(policy.revision())?,
            i64::from(policy.paused()),
            revision_i64(expected)?,
        ],
    )?;
    if changed != 1 {
        return Err(invalid_data(
            "extension profile policy changed during compare-and-swap",
        ));
    }
    tx.execute("DELETE FROM extension_profile_site_denials", [])?;
    let mut statement = tx.prepare(
        "INSERT INTO extension_profile_site_denials(policy_id, pattern)
         VALUES (1, ?1)",
    )?;
    for scope in policy.denied_sites() {
        statement.execute([scope.as_str()])?;
    }
    Ok(())
}

fn revision_i64(revision: ExtensionProfilePolicyRevision) -> rusqlite::Result<i64> {
    i64::try_from(revision.get())
        .map_err(|_| invalid_data("extension profile-policy revision exceeds SQLite"))
}

fn durable_bool(value: i64, message: &'static str) -> rusqlite::Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid_data(message)),
    }
}
