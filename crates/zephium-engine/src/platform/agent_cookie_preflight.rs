#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

//! Secret-safe, bounded preflight for native Windows cookie transfer.
//!
//! The platform adapter gives this owner one native cookie handle together
//! with the exact fields read from that handle. No handle can leave through
//! [`AgentCookiePreflight::finish`] until every requested origin has completed
//! and the whole cohort has passed validation, deduplication, and byte/count
//! ceilings. This makes the zero-destination-write preflight a type boundary,
//! not callback convention.

use std::fmt;

use zephium_agentic::{
    ContextCookieTransferCounts, ContextCookieTransferStats, MAX_COOKIES_PER_TRANSFER,
    MAX_COOKIE_BYTES, MAX_COOKIE_TRANSFER_BYTES, MAX_COOKIE_TRANSFER_ORIGINS,
};
use zeroize::Zeroizing;

/// Closed preflight failure without cookie content or native error detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentCookiePreflightFailure {
    InvalidCookie,
    LimitExceeded,
    ResourceExhausted,
    Incomplete,
}

/// WebView2's complete exposed SameSite vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentCookieSameSite {
    None,
    Lax,
    Strict,
}

/// Exact native-exposed fields for one cookie.
///
/// All strings zeroize on every success and refusal path. This type has no
/// `Debug`, serialization, or value accessor; only the preflight owner can
/// compare and account it.
pub(crate) struct AgentCookieFields {
    name: Zeroizing<String>,
    value: Zeroizing<String>,
    domain: Zeroizing<String>,
    path: Zeroizing<String>,
    expires: f64,
    http_only: bool,
    secure: bool,
    same_site: AgentCookieSameSite,
    session: bool,
    payload_bytes: usize,
}

impl AgentCookieFields {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_new(
        name: String,
        value: String,
        domain: String,
        path: String,
        expires: f64,
        http_only: bool,
        secure: bool,
        same_site: AgentCookieSameSite,
        session: bool,
    ) -> Result<Self, AgentCookiePreflightFailure> {
        // Take zeroizing ownership before the first validation branch so
        // malformed native values receive the same cleanup as valid ones.
        let name = Zeroizing::new(name);
        let value = Zeroizing::new(value);
        let domain = Zeroizing::new(domain);
        let path = Zeroizing::new(path);
        if !valid_cookie_name(&name)
            || domain.is_empty()
            || [&value, &domain, &path]
                .into_iter()
                .any(|field| field.contains('\0'))
            || (same_site == AgentCookieSameSite::None && !secure)
            || !valid_expiry(expires, session)
        {
            return Err(AgentCookiePreflightFailure::InvalidCookie);
        }
        let payload_bytes = name
            .len()
            .checked_add(value.len())
            .and_then(|bytes| bytes.checked_add(domain.len()))
            .and_then(|bytes| bytes.checked_add(path.len()))
            .ok_or(AgentCookiePreflightFailure::LimitExceeded)?;
        if payload_bytes > MAX_COOKIE_BYTES {
            return Err(AgentCookiePreflightFailure::LimitExceeded);
        }
        Ok(Self {
            name,
            value,
            domain,
            path,
            expires,
            http_only,
            secure,
            same_site,
            session,
            payload_bytes,
        })
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.name.as_str() == other.name.as_str()
            && self.domain.as_str() == other.domain.as_str()
            && self.path.as_str() == other.path.as_str()
    }

    fn same_snapshot(&self, other: &Self) -> bool {
        self.same_identity(other)
            && self.value.as_str() == other.value.as_str()
            && self.expires.to_bits() == other.expires.to_bits()
            && self.http_only == other.http_only
            && self.secure == other.secure
            && self.same_site == other.same_site
            && self.session == other.session
    }
}

struct ValidatedCookie<NativeCookie> {
    fields: AgentCookieFields,
    native: NativeCookie,
}

/// Result of admitting one native cookie observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentCookieAdmission {
    Unique,
    ExactDuplicate,
}

/// Whole-cohort preflight owner.
pub(crate) struct AgentCookiePreflight<NativeCookie> {
    requested_origins: u8,
    completed_origins: u8,
    current_origin_observations: u16,
    observations: u16,
    payload_bytes: usize,
    http_only: u16,
    cookies: Vec<Option<ValidatedCookie<NativeCookie>>>,
    failure: Option<AgentCookiePreflightFailure>,
}

impl<NativeCookie> AgentCookiePreflight<NativeCookie> {
    pub(crate) fn try_new(requested_origins: usize) -> Result<Self, AgentCookiePreflightFailure> {
        if requested_origins == 0 || requested_origins > MAX_COOKIE_TRANSFER_ORIGINS {
            return Err(AgentCookiePreflightFailure::LimitExceeded);
        }
        let mut cookies = Vec::new();
        cookies
            .try_reserve_exact(MAX_COOKIES_PER_TRANSFER)
            .map_err(|_| AgentCookiePreflightFailure::ResourceExhausted)?;
        Ok(Self {
            requested_origins: u8::try_from(requested_origins)
                .map_err(|_| AgentCookiePreflightFailure::LimitExceeded)?,
            completed_origins: 0,
            current_origin_observations: 0,
            observations: 0,
            payload_bytes: 0,
            http_only: 0,
            cookies,
            failure: None,
        })
    }

    pub(crate) fn admit(
        &mut self,
        fields: AgentCookieFields,
        native: NativeCookie,
    ) -> Result<AgentCookieAdmission, AgentCookiePreflightFailure> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self.completed_origins >= self.requested_origins {
            return self.poison(AgentCookiePreflightFailure::Incomplete);
        }
        let current_origin_observations = self.current_origin_observations.checked_add(1);
        let Some(current_origin_observations) = current_origin_observations else {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        };
        if usize::from(current_origin_observations) > MAX_COOKIES_PER_TRANSFER {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        }
        let Some(observations) = self.observations.checked_add(1) else {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        };
        let Some(maximum_observations) =
            MAX_COOKIES_PER_TRANSFER.checked_mul(usize::from(self.requested_origins))
        else {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        };
        if usize::from(observations) > maximum_observations {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        }
        self.current_origin_observations = current_origin_observations;
        self.observations = observations;
        if let Some(existing) = self
            .cookies
            .iter()
            .flatten()
            .find(|existing| existing.fields.same_identity(&fields))
        {
            return if existing.fields.same_snapshot(&fields) {
                Ok(AgentCookieAdmission::ExactDuplicate)
            } else {
                self.poison(AgentCookiePreflightFailure::InvalidCookie)
            };
        }
        if self.cookies.len() >= MAX_COOKIES_PER_TRANSFER {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        }
        let Some(payload_bytes) = self.payload_bytes.checked_add(fields.payload_bytes) else {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        };
        if payload_bytes > MAX_COOKIE_TRANSFER_BYTES {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        }
        let Some(http_only) = self.http_only.checked_add(u16::from(fields.http_only)) else {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        };
        self.cookies.push(Some(ValidatedCookie { fields, native }));
        self.payload_bytes = payload_bytes;
        self.http_only = http_only;
        Ok(AgentCookieAdmission::Unique)
    }

    pub(crate) fn complete_origin(&mut self) -> Result<(), AgentCookiePreflightFailure> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self.completed_origins >= self.requested_origins {
            return self.poison(AgentCookiePreflightFailure::Incomplete);
        }
        let Some(completed_origins) = self.completed_origins.checked_add(1) else {
            return self.poison(AgentCookiePreflightFailure::LimitExceeded);
        };
        self.completed_origins = completed_origins;
        self.current_origin_observations = 0;
        Ok(())
    }

    pub(crate) fn finish(
        self,
    ) -> Result<AgentCookieApplication<NativeCookie>, AgentCookiePreflightFailure> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        if self.completed_origins != self.requested_origins {
            return Err(AgentCookiePreflightFailure::Incomplete);
        }
        let observed = u16::try_from(self.cookies.len())
            .map_err(|_| AgentCookiePreflightFailure::LimitExceeded)?;
        let payload_bytes = u32::try_from(self.payload_bytes)
            .map_err(|_| AgentCookiePreflightFailure::LimitExceeded)?;
        let stats = ContextCookieTransferStats::try_new(ContextCookieTransferCounts {
            origins_completed: self.completed_origins,
            cookies_observed: observed,
            cookies_applied: 0,
            http_only_observed: self.http_only,
            http_only_applied: 0,
            payload_bytes,
        })
        .map_err(|_| AgentCookiePreflightFailure::LimitExceeded)?;
        Ok(AgentCookieApplication {
            cookies: self.cookies,
            next: 0,
            counts: stats.counts(),
        })
    }

    fn poison<T>(
        &mut self,
        failure: AgentCookiePreflightFailure,
    ) -> Result<T, AgentCookiePreflightFailure> {
        self.failure = Some(failure);
        Err(failure)
    }
}

impl<NativeCookie> fmt::Debug for AgentCookiePreflight<NativeCookie> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentCookiePreflight")
            .field("requested_origins", &self.requested_origins)
            .field("completed_origins", &self.completed_origins)
            .field("observations", &self.observations)
            .field("cookie_count", &self.cookies.len())
            .field("payload_bytes", &self.payload_bytes)
            .finish()
    }
}

/// Move-only apply owner created only by a complete preflight.
pub(crate) struct AgentCookieApplication<NativeCookie> {
    cookies: Vec<Option<ValidatedCookie<NativeCookie>>>,
    next: usize,
    counts: ContextCookieTransferCounts,
}

impl<NativeCookie> AgentCookieApplication<NativeCookie> {
    pub(crate) fn current(&self) -> Option<AgentPreparedCookie<'_, NativeCookie>> {
        let cookie = self.cookies.get(self.next)?.as_ref()?;
        Some(AgentPreparedCookie {
            native: &cookie.native,
        })
    }

    pub(crate) fn record_current_applied(&mut self) -> Result<(), AgentCookiePreflightFailure> {
        let cookie = self
            .cookies
            .get(self.next)
            .and_then(Option::as_ref)
            .ok_or(AgentCookiePreflightFailure::Incomplete)?;
        let cookies_applied = self
            .counts
            .cookies_applied
            .checked_add(1)
            .ok_or(AgentCookiePreflightFailure::LimitExceeded)?;
        let http_only_applied = self
            .counts
            .http_only_applied
            .checked_add(u16::from(cookie.fields.http_only))
            .ok_or(AgentCookiePreflightFailure::LimitExceeded)?;
        let next = self
            .next
            .checked_add(1)
            .ok_or(AgentCookiePreflightFailure::LimitExceeded)?;
        let counts = ContextCookieTransferCounts {
            cookies_applied,
            http_only_applied,
            ..self.counts
        };
        ContextCookieTransferStats::try_new(counts)
            .map_err(|_| AgentCookiePreflightFailure::Incomplete)?;
        let cookie = self
            .cookies
            .get_mut(self.next)
            .and_then(Option::take)
            .ok_or(AgentCookiePreflightFailure::Incomplete)?;
        drop(cookie);
        self.counts = counts;
        self.next = next;
        Ok(())
    }

    pub(crate) fn stats(&self) -> Result<ContextCookieTransferStats, AgentCookiePreflightFailure> {
        ContextCookieTransferStats::try_new(self.counts)
            .map_err(|_| AgentCookiePreflightFailure::Incomplete)
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.counts.cookies_applied == self.counts.cookies_observed
            && self.counts.http_only_applied == self.counts.http_only_observed
            && self.next == self.cookies.len()
            && self.cookies.iter().all(Option::is_none)
    }
}

impl<NativeCookie> fmt::Debug for AgentCookieApplication<NativeCookie> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentCookieApplication")
            .field("remaining", &self.cookies.len().saturating_sub(self.next))
            .field("counts", &self.counts)
            .finish()
    }
}

/// One validated cookie released to the apply phase.
pub(crate) struct AgentPreparedCookie<'a, NativeCookie> {
    native: &'a NativeCookie,
}

impl<NativeCookie> AgentPreparedCookie<'_, NativeCookie> {
    pub(crate) const fn native(&self) -> &NativeCookie {
        self.native
    }
}

fn valid_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii()
                && !byte.is_ascii_control()
                && !matches!(
                    byte,
                    b' ' | b'\t'
                        | b'('
                        | b')'
                        | b'<'
                        | b'>'
                        | b'@'
                        | b','
                        | b';'
                        | b':'
                        | b'\\'
                        | b'"'
                        | b'/'
                        | b'['
                        | b']'
                        | b'?'
                        | b'='
                        | b'{'
                        | b'}'
                )
        })
}

fn valid_expiry(expires: f64, session: bool) -> bool {
    if session {
        expires == -1.0
    } else {
        expires.is_finite() && expires >= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(name: &str, value: &str, http_only: bool) -> AgentCookieFields {
        AgentCookieFields::try_new(
            name.to_owned(),
            value.to_owned(),
            ".example.test".to_owned(),
            "/".to_owned(),
            1_900_000_000.0,
            http_only,
            true,
            AgentCookieSameSite::Lax,
            false,
        )
        .expect("fields")
    }

    #[test]
    fn exact_duplicates_count_once_and_contradictions_fail_closed() {
        let mut preflight = AgentCookiePreflight::try_new(2).expect("preflight");
        assert_eq!(
            preflight.admit(fields("session", "one", true), 1_u8),
            Ok(AgentCookieAdmission::Unique)
        );
        preflight.complete_origin().expect("first origin");
        assert_eq!(
            preflight.admit(fields("session", "one", true), 2_u8),
            Ok(AgentCookieAdmission::ExactDuplicate)
        );
        preflight.complete_origin().expect("second origin");
        let application = preflight.finish().expect("application");
        let counts = application.stats().expect("stats").counts();
        assert_eq!(counts.origins_completed, 2);
        assert_eq!(counts.cookies_observed, 1);
        assert_eq!(counts.http_only_observed, 1);
        assert_eq!(counts.cookies_applied, 0);

        let mut conflicting = AgentCookiePreflight::try_new(2).expect("preflight");
        conflicting
            .admit(fields("session", "one", true), 1_u8)
            .expect("source cookie");
        conflicting.complete_origin().expect("first origin");
        assert_eq!(
            conflicting.admit(fields("session", "changed", true), 2_u8),
            Err(AgentCookiePreflightFailure::InvalidCookie)
        );
        assert_eq!(
            conflicting.complete_origin(),
            Err(AgentCookiePreflightFailure::InvalidCookie)
        );
        assert_eq!(
            conflicting.finish().err(),
            Some(AgentCookiePreflightFailure::InvalidCookie)
        );
    }

    #[test]
    fn finish_is_a_complete_origin_barrier_and_apply_accounting_is_exact() {
        let mut preflight = AgentCookiePreflight::try_new(1).expect("preflight");
        preflight
            .admit(fields("first", "value", true), 1_u8)
            .expect("first");
        preflight
            .admit(fields("second", "value", false), 2_u8)
            .expect("second");
        assert_eq!(
            preflight.finish().err(),
            Some(AgentCookiePreflightFailure::Incomplete)
        );

        let mut preflight = AgentCookiePreflight::try_new(1).expect("preflight");
        preflight
            .admit(fields("first", "value", true), 1_u8)
            .expect("first");
        preflight
            .admit(fields("second", "value", false), 2_u8)
            .expect("second");
        preflight.complete_origin().expect("complete");
        let mut application = preflight.finish().expect("application");
        let first = application.current().expect("first cookie");
        assert_eq!(*first.native(), 1);
        application.record_current_applied().expect("first applied");
        assert!(!application.is_complete());
        let second = application.current().expect("second cookie");
        assert_eq!(*second.native(), 2);
        application
            .record_current_applied()
            .expect("second applied");
        assert!(application.is_complete());
        let counts = application.stats().expect("stats").counts();
        assert_eq!(counts.cookies_applied, 2);
        assert_eq!(counts.http_only_applied, 1);
    }

    #[test]
    fn every_field_count_and_origin_ceiling_is_closed() {
        assert!(matches!(
            AgentCookiePreflight::<u8>::try_new(0),
            Err(AgentCookiePreflightFailure::LimitExceeded)
        ));
        assert!(matches!(
            AgentCookiePreflight::<u8>::try_new(MAX_COOKIE_TRANSFER_ORIGINS + 1),
            Err(AgentCookiePreflightFailure::LimitExceeded)
        ));
        assert_eq!(
            AgentCookieFields::try_new(
                "name".to_owned(),
                "x".repeat(MAX_COOKIE_BYTES),
                ".example.test".to_owned(),
                "/".to_owned(),
                1.0,
                false,
                true,
                AgentCookieSameSite::Strict,
                false,
            )
            .err(),
            Some(AgentCookiePreflightFailure::LimitExceeded)
        );

        let mut preflight = AgentCookiePreflight::try_new(1).expect("preflight");
        for index in 0..MAX_COOKIES_PER_TRANSFER {
            preflight
                .admit(fields(&format!("c{index}"), "v", false), index)
                .expect("bounded cookie");
        }
        assert_eq!(
            preflight.admit(fields("overflow", "v", false), usize::MAX),
            Err(AgentCookiePreflightFailure::LimitExceeded)
        );

        let mut duplicate_flood = AgentCookiePreflight::try_new(1).expect("preflight");
        for index in 0..MAX_COOKIES_PER_TRANSFER {
            assert_eq!(
                duplicate_flood.admit(fields("same", "v", false), index),
                if index == 0 {
                    Ok(AgentCookieAdmission::Unique)
                } else {
                    Ok(AgentCookieAdmission::ExactDuplicate)
                }
            );
        }
        assert_eq!(
            duplicate_flood.admit(fields("same", "v", false), usize::MAX),
            Err(AgentCookiePreflightFailure::LimitExceeded)
        );
        assert_eq!(
            duplicate_flood.finish().err(),
            Some(AgentCookiePreflightFailure::LimitExceeded)
        );
    }

    #[test]
    fn invalid_native_shapes_are_rejected_and_debug_is_content_free() {
        for candidate in [
            AgentCookieFields::try_new(
                "bad name".to_owned(),
                "secret".to_owned(),
                ".example.test".to_owned(),
                "/".to_owned(),
                1.0,
                false,
                true,
                AgentCookieSameSite::Lax,
                false,
            ),
            AgentCookieFields::try_new(
                "name".to_owned(),
                "secret".to_owned(),
                String::new(),
                "/".to_owned(),
                1.0,
                false,
                true,
                AgentCookieSameSite::Lax,
                false,
            ),
            AgentCookieFields::try_new(
                "name".to_owned(),
                "secret".to_owned(),
                ".example.test".to_owned(),
                "/".to_owned(),
                -1.0,
                false,
                true,
                AgentCookieSameSite::Lax,
                false,
            ),
            AgentCookieFields::try_new(
                "name".to_owned(),
                "secret".to_owned(),
                ".example.test".to_owned(),
                "/".to_owned(),
                f64::NAN,
                false,
                true,
                AgentCookieSameSite::Lax,
                false,
            ),
            AgentCookieFields::try_new(
                "name".to_owned(),
                "secret".to_owned(),
                ".example.test".to_owned(),
                "/".to_owned(),
                -1.0,
                false,
                false,
                AgentCookieSameSite::None,
                true,
            ),
        ] {
            assert_eq!(
                candidate.err(),
                Some(AgentCookiePreflightFailure::InvalidCookie)
            );
        }

        let mut preflight = AgentCookiePreflight::try_new(1).expect("preflight");
        preflight
            .admit(fields("session", "do-not-log-me", true), ())
            .expect("cookie");
        let debug = format!("{preflight:?}");
        assert!(!debug.contains("session"));
        assert!(!debug.contains("do-not-log-me"));
        assert!(!debug.contains("example.test"));
    }
}
