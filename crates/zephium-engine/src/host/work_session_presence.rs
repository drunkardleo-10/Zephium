//! Whether a profile's own website data holds cookies for each of some
//! hosts' sites. Each answer is one closed fact: no cookie name, value,
//! account or page crosses it, and it never reads a cookie. Work uses it to offer a grant the person
//! still approves; it never grants or identifies anything.
use std::{ptr::NonNull, sync::mpsc::Sender};

use objc2_foundation::{MainThreadMarker, NSArray, NSSet, NSUUID};
use objc2_web_kit::{WKWebsiteDataRecord, WKWebsiteDataStore, WKWebsiteDataTypeCookies};
use zephium_core::ids::ProfileId;

use super::{profiles::ProfilePersistenceClass, EngineHost};

/// Records WebKit returns for one store are bounded before they are scanned.
const MAX_RECORDS: usize = 8192;

impl EngineHost {
    pub(crate) fn work_sessions_present(
        &mut self,
        profile: ProfileId,
        hosts: Vec<String>,
        reply: Sender<Vec<bool>>,
    ) {
        let none = vec![false; hosts.len()];
        // Only a live durable profile: never create a store for one that is
        // gone, erasing, or private.
        if self.erasure_tombstones.contains(&profile)
            || self.profile_persistence_classes.get(&profile)
                != Some(&ProfilePersistenceClass::Durable)
        {
            let _ = reply.send(none);
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = reply.send(none);
            return;
        };
        let identifier = NSUUID::from_bytes(profile.bytes());
        // SAFETY: main thread (`mtm`); the retained store, set and block stay
        // live for the call and WebKit copies the block.
        unsafe {
            let store = WKWebsiteDataStore::dataStoreForIdentifier(&identifier, mtm);
            if !store.isPersistent()
                || store.identifier().map(|value| value.as_bytes()) != Some(profile.bytes())
            {
                let _ = reply.send(none);
                return;
            }
            let types = NSSet::from_slice(&[WKWebsiteDataTypeCookies]);
            let callback =
                block2::RcBlock::new(move |records: NonNull<NSArray<WKWebsiteDataRecord>>| {
                    // SAFETY: WebKit owns the array for the callback's duration.
                    let records = records.as_ref();
                    let sites: Vec<String> = records
                        .iter()
                        .take(MAX_RECORDS)
                        .map(|record| record.displayName().to_string())
                        .collect();
                    let _ = reply.send(
                        hosts
                            .iter()
                            .map(|host| sites.iter().any(|site| site_matches(host, site)))
                            .collect(),
                    );
                });
            store.fetchDataRecordsOfTypes_completionHandler(&types, &callback);
        }
    }
}

/// Hosts counted before the oldest is dropped.
const MAX_COUNTED_HOSTS: usize = 256;

impl EngineHost {
    /// Counts one finished page load in an ordinary tab for its profile.
    pub(crate) fn count_site_load(&mut self, id: crate::ItemId, url: &str) {
        let Some(profile) = self
            .partitions
            .get(&id)
            .map(|partition| partition.profile())
        else {
            return;
        };
        let Some(host) = url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        else {
            return;
        };
        if self.work_site_loads.len() >= MAX_COUNTED_HOSTS
            && !self.work_site_loads.contains_key(&(profile, host.clone()))
        {
            self.work_site_loads.clear();
        }
        *self.work_site_loads.entry((profile, host)).or_default() += 1;
    }
    /// Finished loads in the profile's tabs on hosts on `site`.
    pub(crate) fn work_site_loads(&self, profile: ProfileId, site: &str) -> u64 {
        self.work_site_loads
            .iter()
            .filter(|((owner, host), _)| *owner == profile && site_matches(host, site))
            .map(|(_, count)| *count)
            .sum()
    }
}

/// A record names a site (usually its registrable domain); the host is on it
/// when it is that site or a subdomain of it.
pub(super) fn site_matches(host: &str, site: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let site = site
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    !site.is_empty()
        && (host == site
            || host
                .strip_suffix(&site)
                .is_some_and(|rest| rest.ends_with('.')))
}

#[cfg(test)]
mod tests {
    use super::site_matches;

    #[test]
    fn a_session_belongs_to_its_site_and_its_subdomains_only() {
        assert!(site_matches("app.slack.com", "slack.com"));
        assert!(site_matches("slack.com", "slack.com"));
        assert!(site_matches("Slack.COM", ".slack.com"));
        assert!(site_matches("127.0.0.1", "127.0.0.1"));
        assert!(!site_matches("notslack.com", "slack.com"));
        assert!(!site_matches("slack.com.evil.test", "slack.com"));
        assert!(!site_matches("slack.com", ""));
        assert!(!site_matches("slack.com", "app.slack.com"));
    }
}
