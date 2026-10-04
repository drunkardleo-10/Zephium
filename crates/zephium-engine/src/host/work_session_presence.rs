//! Whether a profile's own website data holds cookies for each of some
//! hosts' sites. Each answer is one closed fact: no cookie name, value,
//! account or page crosses it, and it never reads a cookie. Work uses it to offer a grant the person
//! still approves; it never grants or identifies anything.
#[cfg(target_os = "macos")]
use std::ptr::NonNull;
use std::sync::mpsc::Sender;

#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSArray, NSSet, NSUUID};
#[cfg(target_os = "macos")]
use objc2_web_kit::{WKWebsiteDataRecord, WKWebsiteDataStore, WKWebsiteDataTypeCookies};
use zephium_core::ids::ProfileId;

#[cfg(any(target_os = "macos", feature = "agentic-browser"))]
use super::profiles::ProfilePersistenceClass;
use super::EngineHost;

/// Records WebKit returns for one store are bounded before they are scanned.
#[cfg(target_os = "macos")]
const MAX_RECORDS: usize = 8192;

#[cfg(target_os = "macos")]
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

/// Without the Work runtime there is no profile session to look up.
#[cfg(all(target_os = "windows", not(feature = "agentic-browser")))]
impl EngineHost {
    pub(crate) fn work_sessions_present(
        &mut self,
        _profile: ProfileId,
        hosts: Vec<String>,
        reply: Sender<Vec<bool>>,
    ) {
        let _ = reply.send(vec![false; hosts.len()]);
    }
}

#[cfg(all(target_os = "windows", feature = "agentic-browser"))]
impl EngineHost {
    pub(crate) fn work_sessions_present(
        &mut self,
        profile: ProfileId,
        hosts: Vec<String>,
        reply: Sender<Vec<bool>>,
    ) {
        use std::time::{Duration, Instant};
        if hosts.is_empty() {
            let _ = reply.send(vec![]);
            return;
        }
        if hosts.len() > 256
            || self.erasure_tombstones.contains(&profile)
            || self.profile_persistence_classes.get(&profile)
                != Some(&ProfilePersistenceClass::Durable)
        {
            return;
        }
        let Some(deadline) = Instant::now().checked_add(Duration::from_secs(5)) else {
            return;
        };
        let mut remaining_constructions = 8;
        let metadata = crate::erasure::prepare_profile_directory(&self.profiles_root, profile)
            .ok()
            .and_then(|path| {
                crate::platform::windows::work_seed_metadata::WorkSeedMetadata::open(&path).ok()
            });
        let Some(metadata) = metadata else {
            return;
        };
        let mut results = Vec::with_capacity(hosts.len());
        for host in hosts {
            if Instant::now() >= deadline {
                return;
            }
            let Some(url) = url::Url::parse(&format!("https://{host}/"))
                .ok()
                .filter(|url| {
                    url.host_str() == Some(host.as_str())
                        && url.username().is_empty()
                        && url.password().is_none()
                })
            else {
                return;
            };
            let Ok(mut present) = self.selected_work_cookie_presence(
                profile,
                &url,
                deadline,
                &mut remaining_constructions,
            ) else {
                return;
            };
            if !present {
                let Ok(base) = zephium_agentic::ContextNavigationTarget::parse(url.as_str()) else {
                    return;
                };
                let Some(site) = zephium_agentic::registrable_site(&base) else {
                    return;
                };
                let Ok(descriptors) = metadata.known_stores(&site) else {
                    return;
                };
                // Only remembered or attested resident Work stores may hold Work authentication.
                // Never invent alternative ports or treat a failed query as cookie absence.
                let mut targets = work_presence_targets(&url, descriptors);
                for resource in self
                    .work_resources
                    .values()
                    .filter(|resource| resource.profile() == profile && resource.view.is_some())
                {
                    if let Some(target) = resource.guard().document().filter(|target| {
                        zephium_agentic::registrable_site(target).as_deref() == Some(site.as_str())
                    }) {
                        let mut resident = url.clone();
                        if resident.set_scheme(target.as_url().scheme()).is_ok()
                            && resident.set_port(target.as_url().port()).is_ok()
                        {
                            if let Ok(target) =
                                zephium_agentic::ContextNavigationTarget::parse(resident.as_str())
                            {
                                targets.push(target);
                            }
                        }
                    }
                }
                let mut names = std::collections::HashSet::new();
                for target in targets {
                    let Ok(crate::platform::windows::AgentOwnedProfile::Automation { name }) =
                        crate::platform::windows::AgentOwnedProfile::work_site(&target)
                    else {
                        return;
                    };
                    if !names.insert(name) {
                        continue;
                    }
                    let Ok(found) = self.work_store_cookie_presence(
                        profile,
                        &target,
                        deadline,
                        &mut remaining_constructions,
                    ) else {
                        return;
                    };
                    present |= found;
                    if present {
                        break;
                    }
                }
            }
            if Instant::now() >= deadline {
                return;
            }
            results.push(present);
        }
        // Dropping the reply on any unknown query is deliberate: the app keeps
        // unavailable/partial facts unknown and asks before using possible authentication.
        let _ = reply.send(results);
    }
}

#[cfg(all(target_os = "windows", feature = "agentic-browser"))]
fn work_presence_targets(
    base: &url::Url,
    descriptors: Vec<crate::platform::windows::work_seed_metadata::WorkStoreDescriptor>,
) -> Vec<zephium_agentic::ContextNavigationTarget> {
    let mut targets = Vec::new();
    for descriptor in descriptors {
        let mut url = base.clone();
        if descriptor.port != 0
            && url
                .set_scheme(if descriptor.https { "https" } else { "http" })
                .is_ok()
            && url.set_port(Some(descriptor.port)).is_ok()
        {
            if let Ok(target) = zephium_agentic::ContextNavigationTarget::parse(url.as_str()) {
                targets.push(target);
            }
        }
    }
    targets
}

#[cfg(all(test, target_os = "windows", feature = "agentic-browser"))]
mod windows_presence_tests {
    #[test]
    fn remembered_stores_preserve_scheme_port_and_requested_host() {
        use crate::platform::windows::work_seed_metadata::WorkStoreDescriptor;
        let base = url::Url::parse("https://app.slack.com/").expect("fixture URL");
        let targets = super::work_presence_targets(
            &base,
            vec![
                WorkStoreDescriptor {
                    https: false,
                    port: 8080,
                },
                WorkStoreDescriptor {
                    https: true,
                    port: 8443,
                },
            ],
        );
        let urls: Vec<_> = targets
            .iter()
            .map(|target| target.as_url().as_str())
            .collect();
        assert_eq!(
            urls,
            vec!["http://app.slack.com:8080/", "https://app.slack.com:8443/"]
        );
        assert!(targets
            .iter()
            .all(|target| target.as_url().host_str() == Some("app.slack.com")));
    }
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
