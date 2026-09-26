//! Sites a run works on as the person: which session each page opens in and
//! when the person is asked first. Independent of the lead's turn format.
use std::collections::BTreeMap;
use zephium_core::{
    ids::ProfileId,
    work::{
        port::{WorkReply, WorkRequest},
        sites::*,
        WorkError,
    },
};

/// Sites whose session the agent never uses unless the person lifts them to
/// ask: banks and payments, password managers, health and tax portals.
pub const SENSITIVE_SITES: &[&str] = &[
    "1password.com",
    "bitwarden.com",
    "lastpass.com",
    "dashlane.com",
    "keepersecurity.com",
    "proton.me",
    "paypal.com",
    "stripe.com",
    "wise.com",
    "revolut.com",
    "venmo.com",
    "cash.app",
    "coinbase.com",
    "binance.com",
    "kraken.com",
    "robinhood.com",
    "schwab.com",
    "fidelity.com",
    "vanguard.com",
    "etrade.com",
    "chase.com",
    "bankofamerica.com",
    "wellsfargo.com",
    "citi.com",
    "capitalone.com",
    "usbank.com",
    "americanexpress.com",
    "discover.com",
    "hsbc.com",
    "hsbc.co.uk",
    "barclays.co.uk",
    "lloydsbank.com",
    "natwest.com",
    "santander.com",
    "santander.pl",
    "ing.com",
    "ing.pl",
    "pkobp.pl",
    "ipko.pl",
    "mbank.pl",
    "pekao.com.pl",
    "bnpparibas.pl",
    "aliorbank.pl",
    "millenniumbank.pl",
    "credit-agricole.pl",
    "n26.com",
    "monzo.com",
    "starlingbank.com",
    "deutsche-bank.de",
    "commerzbank.de",
    "sparkasse.de",
    "bnpparibas.fr",
    "credit-agricole.fr",
    "mychart.org",
    "mychart.com",
    "patient.info",
    "healthcare.gov",
    "nhs.uk",
    "irs.gov",
    "ssa.gov",
    "login.gov",
    "id.me",
    "gov.uk",
    "gov.pl",
    "podatki.gov.pl",
    "epuap.gov.pl",
    "zus.pl",
];

/// People's names for common sites and where their signed-in app starts.
const VENDORS: &[(&str, &str, Option<&str>)] = &[
    ("slack.com", "Slack", Some("https://app.slack.com/client")),
    ("notion.so", "Notion", Some("https://www.notion.so/")),
    ("google.com", "Google", None),
    ("github.com", "GitHub", None),
    (
        "linkedin.com",
        "LinkedIn",
        Some("https://www.linkedin.com/feed/"),
    ),
    ("airbnb.com", "Airbnb", None),
    (
        "figma.com",
        "Figma",
        Some("https://www.figma.com/files/recents-and-sharing"),
    ),
    ("x.com", "X", Some("https://x.com/home")),
    ("facebook.com", "Facebook", None),
    ("instagram.com", "Instagram", None),
    ("amazon.com", "Amazon", None),
    ("booking.com", "Booking.com", None),
    ("trello.com", "Trello", None),
    ("asana.com", "Asana", Some("https://app.asana.com/")),
    ("linear.app", "Linear", None),
    (
        "discord.com",
        "Discord",
        Some("https://discord.com/channels/@me"),
    ),
    ("reddit.com", "Reddit", None),
    ("youtube.com", "YouTube", None),
    (
        "dropbox.com",
        "Dropbox",
        Some("https://www.dropbox.com/home"),
    ),
    ("atlassian.com", "Atlassian", None),
    ("hubspot.com", "HubSpot", Some("https://app.hubspot.com/")),
    ("salesforce.com", "Salesforce", None),
    ("shopify.com", "Shopify", None),
    ("microsoft.com", "Microsoft", None),
    (
        "office.com",
        "Microsoft 365",
        Some("https://www.office.com/"),
    ),
    (
        "live.com",
        "Outlook",
        Some("https://outlook.live.com/mail/"),
    ),
];

pub fn is_sensitive(site: &str) -> bool {
    SENSITIVE_SITES.contains(&site)
}

/// What a person calls this site: a known vendor, else the site itself.
pub fn site_name(site: &str) -> String {
    VENDORS
        .iter()
        .find(|(known, ..)| *known == site)
        .map_or_else(|| site.to_owned(), |(_, name, _)| (*name).to_owned())
}

/// A bare site's own start page for a page task, when the vendor has one.
pub fn entry_url(url: &str) -> Option<&'static str> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.path() != "/" || parsed.query().is_some() {
        return None;
    }
    let site = site_of(url)?;
    VENDORS
        .iter()
        .find(|(known, ..)| *known == site)
        .and_then(|(_, _, entry)| *entry)
        .filter(|entry| site_of(entry).as_deref() == Some(site.as_str()))
}

/// The page's host, for its session badge.
pub fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(str::to_owned)
}

/// The registrable domain a page is on.
pub fn site_of(url: &str) -> Option<String> {
    let target = zephium_agentic::ContextNavigationTarget::parse(url).ok()?;
    zephium_agentic::registrable_site(&target)
}

/// Which storage a page opens in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SiteSession {
    /// The person's own session on the site, under this run's account key.
    Yours { site: String, account: String },
    /// The run's own empty storage.
    Private,
}

/// Why a page task works without the person's session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateBecause {
    PrivateRun,
    Never,
    Sensitive,
    Declined,
}

/// What the run does before a page task on a site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Entry {
    Session(SiteSession),
    Private(PrivateBecause),
    /// Ask the person once: the profile holds a session for the site.
    Ask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryAnswer {
    Allow,
    Always,
    NotNow,
}

pub const ALLOW: &str = "Allow";
pub const NOT_NOW: &str = "Not now";
pub fn always_label(site: &str) -> String {
    format!("Always for {}", site_name(site))
}

/// The question a run asks once per site: the site is Rust's, the plan the agent's.
pub fn entry_question(site: &str, goal: &str) -> (String, Vec<String>) {
    let plan = goal.trim().trim_end_matches('.');
    let prompt = format!("Work in your {}? {plan}.", site_name(site));
    (
        zephium_core::work::agent::clip_text(&prompt, 480),
        vec![ALLOW.into(), always_label(site), NOT_NOW.into()],
    )
}

/// Anything but an exact yes is a no.
pub fn entry_answer(site: &str, answer: &str) -> EntryAnswer {
    let answer = answer.trim();
    if answer.eq_ignore_ascii_case(ALLOW) {
        EntryAnswer::Allow
    } else if answer.eq_ignore_ascii_case(&always_label(site)) {
        EntryAnswer::Always
    } else {
        EntryAnswer::NotNow
    }
}

/// One run's decisions per site, and the profile's standing answers.
#[derive(Default)]
pub struct RunSites {
    private: bool,
    standing: Vec<WorkSiteEntryV1>,
    decided: BTreeMap<String, Option<String>>,
}
impl RunSites {
    pub fn new(private: bool, standing: Vec<WorkSiteEntryV1>) -> Self {
        Self {
            private,
            standing,
            decided: BTreeMap::new(),
        }
    }
    fn standing(&self, site: &str) -> Option<WorkSiteAccessV1> {
        self.standing
            .iter()
            .find(|entry| entry.site == site)
            .map(|entry| entry.access)
    }
    fn yours(&mut self, site: &str) -> SiteSession {
        let account = self
            .decided
            .entry(site.to_owned())
            .or_insert(None)
            .get_or_insert_with(|| {
                serde_json::to_value(zephium_agentic::AgentAccountId::generate())
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default()
            })
            .clone();
        SiteSession::Yours {
            site: site.to_owned(),
            account,
        }
    }
    /// A page task's entry: `present` says whether the profile holds a
    /// session for the site (a cookie record, never its contents).
    pub fn entry(&mut self, site: &str, present: bool) -> Entry {
        if self.private {
            return Entry::Private(PrivateBecause::PrivateRun);
        }
        match self.decided.get(site) {
            Some(Some(_)) => return Entry::Session(self.yours(site)),
            Some(None) => return Entry::Private(PrivateBecause::Declined),
            None => {}
        }
        match self.standing(site) {
            Some(WorkSiteAccessV1::Never) => return Entry::Private(PrivateBecause::Never),
            Some(WorkSiteAccessV1::Always) if !is_sensitive(site) => {
                return Entry::Session(self.yours(site))
            }
            None if is_sensitive(site) => return Entry::Private(PrivateBecause::Sensitive),
            _ => {}
        }
        if present {
            Entry::Ask
        } else {
            Entry::Session(self.yours(site))
        }
    }
    /// Records the person's answer; Always also needs the standing write.
    pub fn answer(&mut self, site: &str, answer: EntryAnswer) -> Entry {
        match answer {
            EntryAnswer::NotNow => {
                self.decided.insert(site.to_owned(), None);
                Entry::Private(PrivateBecause::Declined)
            }
            EntryAnswer::Allow | EntryAnswer::Always => {
                if answer == EntryAnswer::Always {
                    self.standing.retain(|entry| entry.site != site);
                    self.standing.push(WorkSiteEntryV1 {
                        site: site.to_owned(),
                        access: WorkSiteAccessV1::Always,
                    });
                }
                Entry::Session(self.yours(site))
            }
        }
    }
    /// A plain page read uses the person's session only on a site this run
    /// already works on as them.
    pub fn read_session(&mut self, site: &str) -> SiteSession {
        if !self.private
            && !is_sensitive(site)
            && (matches!(self.decided.get(site), Some(Some(_)))
                || (!self.decided.contains_key(site)
                    && self.standing(site) == Some(WorkSiteAccessV1::Always)))
        {
            self.yours(site)
        } else {
            SiteSession::Private
        }
    }
    /// The decided sites, for the lead's view.
    pub fn view(&self) -> Vec<(String, &'static str)> {
        self.decided
            .iter()
            .map(|(site, account)| {
                (
                    site.clone(),
                    if account.is_some() {
                        "yours"
                    } else {
                        "private"
                    },
                )
            })
            .collect()
    }
}

/// The profile's standing answers.
pub async fn standing(
    handle: &crate::Handle,
    profile: ProfileId,
) -> Result<Vec<WorkSiteEntryV1>, WorkError> {
    call(handle, profile, None).await
}

/// Sets or clears one standing answer and returns the list.
pub async fn set_standing(
    handle: &crate::Handle,
    profile: ProfileId,
    site: String,
    access: Option<WorkSiteAccessV1>,
) -> Result<Vec<WorkSiteEntryV1>, WorkError> {
    validate_site(&site)?;
    if access == Some(WorkSiteAccessV1::Always) && is_sensitive(&site) {
        return Err(WorkError::Invalid);
    }
    call(handle, profile, Some((site, access))).await
}

async fn call(
    handle: &crate::Handle,
    profile: ProfileId,
    set: Option<(String, Option<WorkSiteAccessV1>)>,
) -> Result<Vec<WorkSiteEntryV1>, WorkError> {
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        handle.submit_work_document(WorkRequest::SiteAccess { set }, Some(profile))?,
    )
    .await
    .map_err(|_| WorkError::Unavailable)??;
    match response.reply {
        WorkReply::SiteAccess(entries) if response.profile == profile => Ok(entries),
        _ => Err(WorkError::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_site_entries_ask_once_and_respect_standing_answers() {
        let mut run = RunSites::new(
            false,
            vec![
                WorkSiteEntryV1 {
                    site: "notion.so".into(),
                    access: WorkSiteAccessV1::Always,
                },
                WorkSiteEntryV1 {
                    site: "reddit.com".into(),
                    access: WorkSiteAccessV1::Never,
                },
            ],
        );
        assert_eq!(run.entry("slack.com", true), Entry::Ask);
        assert!(matches!(
            run.entry("fresh.com", false),
            Entry::Session(SiteSession::Yours { .. })
        ));
        assert!(matches!(
            run.entry("notion.so", true),
            Entry::Session(SiteSession::Yours { .. })
        ));
        assert_eq!(
            run.entry("reddit.com", true),
            Entry::Private(PrivateBecause::Never)
        );
        assert_eq!(
            run.entry("chase.com", true),
            Entry::Private(PrivateBecause::Sensitive)
        );
        let mut lifted = RunSites::new(
            false,
            vec![WorkSiteEntryV1 {
                site: "chase.com".into(),
                access: WorkSiteAccessV1::Ask,
            }],
        );
        assert_eq!(lifted.entry("chase.com", true), Entry::Ask);
        let Entry::Session(first) = run.answer("slack.com", EntryAnswer::Allow) else {
            panic!()
        };
        assert_eq!(run.entry("slack.com", true), Entry::Session(first.clone()));
        assert_eq!(run.read_session("slack.com"), first);
        assert_eq!(run.read_session("example.com"), SiteSession::Private);
        run.answer("figma.com", EntryAnswer::NotNow);
        assert_eq!(
            run.entry("figma.com", true),
            Entry::Private(PrivateBecause::Declined)
        );
        assert_eq!(run.read_session("figma.com"), SiteSession::Private);
        let mut private = RunSites::new(true, vec![]);
        assert_eq!(
            private.entry("slack.com", true),
            Entry::Private(PrivateBecause::PrivateRun)
        );
        assert_eq!(private.read_session("slack.com"), SiteSession::Private);
        assert_eq!(
            run.view(),
            [
                ("figma.com".to_owned(), "private"),
                ("fresh.com".to_owned(), "yours"),
                ("notion.so".to_owned(), "yours"),
                ("slack.com".to_owned(), "yours"),
            ]
        );
    }

    #[test]
    fn work_site_questions_name_the_site_and_read_only_exact_yeses() {
        let (prompt, options) = entry_question(
            "slack.com",
            "I'll read #design since Monday and draft a reply.",
        );
        assert_eq!(
            prompt,
            "Work in your Slack? I'll read #design since Monday and draft a reply."
        );
        assert_eq!(options, ["Allow", "Always for Slack", "Not now"]);
        assert_eq!(entry_answer("slack.com", "allow"), EntryAnswer::Allow);
        assert_eq!(
            entry_answer("slack.com", "Always for Slack"),
            EntryAnswer::Always
        );
        assert_eq!(
            entry_answer("slack.com", "sure, go ahead"),
            EntryAnswer::NotNow
        );
        assert_eq!(
            site_of("https://app.slack.com/client").as_deref(),
            Some("slack.com")
        );
        assert_eq!(
            entry_url("https://slack.com/"),
            Some("https://app.slack.com/client")
        );
        assert_eq!(entry_url("https://slack.com/archives"), None);
        assert!(is_sensitive("chase.com") && !is_sensitive("slack.com"));
    }
}
