//! Approval drafts for one signed-in page, or for signed-in reads on one
//! origin during the next request. Rust resolves the chosen tab, mints the
//! account identity, and returns the exact draft the user approves. Approval
//! is the user's attestation of the account; Zephium has no independent
//! account collector and never claims one.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use zephium_core::{
    ids::{ItemId, ProfileId},
    work::{environment::*, port::*, proposal::*, runtime::*, *},
};
use zephium_ipc::work::{
    WorkAccountApprovalRequestV1, WorkAccountEffectV1, WorkAccountModeV1, WorkCommandV1,
    WorkReplyV1, WorkResponseV1, WorkSignedInV1,
};

const READ_TIMEOUT: Duration = Duration::from_secs(8);
/// How long a drafted origin grant waits for the request that uses it.
const DRAFT_PATIENCE: Duration = Duration::from_secs(30 * 60);
const MAX_DRAFTED: usize = 16;

/// Origin grants Rust drafted for the person's approval. A request claims
/// each at most once; a later request needs a new approval.
static DRAFTED: Mutex<Vec<Drafted>> = Mutex::new(Vec::new());
struct Drafted {
    profile: ProfileId,
    work: WorkId,
    grant: WorkAccountGrantV1,
    at: Instant,
}
pub(crate) fn draft(
    profile: ProfileId,
    work: WorkId,
    grant: WorkAccountGrantV1,
) -> Result<(), WorkError> {
    let mut drafted = DRAFTED.lock().map_err(|_| WorkError::Unavailable)?;
    drafted.retain(|entry| {
        entry.at.elapsed() < DRAFT_PATIENCE
            && !(entry.profile == profile
                && entry.work == work
                && entry.grant.origin == grant.origin)
    });
    if drafted.len() >= MAX_DRAFTED {
        drafted.remove(0);
    }
    drafted.push(Drafted {
        profile,
        work,
        grant,
        at: Instant::now(),
    });
    Ok(())
}
/// Consumes the drafts a request's grant names, all or none. An account Rust
/// did not draft for this work, or one already used, is refused.
pub fn claim_account_grants(
    profile: ProfileId,
    work: WorkId,
    grants: &[WorkAccountGrantV1],
) -> Result<(), WorkError> {
    if grants.is_empty() {
        return Ok(());
    }
    let mut drafted = DRAFTED.lock().map_err(|_| WorkError::Unavailable)?;
    drafted.retain(|entry| entry.at.elapsed() < DRAFT_PATIENCE);
    let found: Option<Vec<usize>> = grants
        .iter()
        .map(|grant| {
            drafted.iter().position(|entry| {
                entry.profile == profile && entry.work == work && entry.grant == *grant
            })
        })
        .collect();
    let mut found = found.ok_or(WorkError::ReviewRequired)?;
    found.sort_unstable();
    found.dedup();
    if found.len() != grants.len() {
        return Err(WorkError::ReviewRequired);
    }
    for index in found.into_iter().rev() {
        drafted.remove(index);
    }
    Ok(())
}
/// The probe's stand-in for the person approving a drafted origin grant.
#[cfg(feature = "work-execution-probe")]
#[doc(hidden)]
pub fn record_approved_grant_for_probe(
    profile: ProfileId,
    work: WorkId,
    grant: WorkAccountGrantV1,
) -> Result<(), WorkError> {
    grant.validate()?;
    draft(profile, work, grant)
}
pub const ACCOUNT_LIMITS: WorkExecutionLimits = WorkExecutionLimits {
    model_tokens: 128_000,
    cost_micro_usd: 500_000,
    operations: 64,
    timeout_seconds: 600,
    max_workers: 1,
};

pub struct WorkAccountApproval {
    handle: crate::Handle,
}
impl WorkAccountApproval {
    pub fn new(handle: crate::Handle) -> Self {
        Self { handle }
    }

    /// Mints a single-step plan from the objective when needed, then drafts a
    /// specification naming profile, page, origin, account, and effect.
    pub async fn prepare(
        &self,
        profile: ProfileId,
        request: WorkAccountApprovalRequestV1,
    ) -> Result<WorkResponseV1, WorkError> {
        if request.version != 1 {
            return Err(WorkError::Invalid);
        }
        let state = self.projection(profile, request.work).await?;
        if state.work.revision != request.expected_revision
            || state.work.lifecycle != WorkLifecycle::Active
            || state.work.status == WorkAuthoringStatus::NeedsInput
            || state.executions.iter().any(|e| !e.status.terminal())
        {
            return Err(WorkError::Conflict);
        }
        let tab = self
            .attached_tab(profile, request.environment, request.element)
            .await?;
        let url = tab.url.ok_or(WorkError::Invalid)?;
        let origin = url::Url::parse(&url)
            .map_err(|_| WorkError::Invalid)?
            .origin()
            .ascii_serialization();
        let account = mint_account()?;
        if request.mode == WorkAccountModeV1::Origin {
            if request.effect != WorkAccountEffectV1::Read {
                return Err(WorkError::Invalid);
            }
            let grant = WorkAccountGrantV1 {
                origin,
                account,
                tab: Some(tab.id),
                pages: request.pages.unwrap_or(MAX_WORK_ACCOUNT_PAGES),
            };
            grant.validate()?;
            draft(profile, request.work, grant.clone())?;
            return Ok(WorkResponseV1 {
                version: 1,
                profile: profile.to_string(),
                reply: WorkReplyV1::AccountGrantDraft {
                    work: request.work,
                    grant,
                },
            });
        }
        if request.pages.is_some() {
            return Err(WorkError::Invalid);
        }
        let scope = WorkAccountScope {
            tab: tab.id,
            url,
            origin,
            account,
        };
        let capability = match request.effect {
            WorkAccountEffectV1::Read => WorkCapability::AccountRead { scope },
            WorkAccountEffectV1::Update { update } => {
                WorkCapability::AccountUpdate { scope, update }
            }
        };
        capability.validate()?;
        let (plan, revision) = match state.work.plan.as_ref() {
            Some(plan)
                if state.work.status == WorkAuthoringStatus::PlanReady
                    && plan.draft.nodes.len() == 1 =>
            {
                (plan.clone(), state.work.revision)
            }
            _ => self.mint_plan(profile, &state.work).await?,
        };
        let mut spec = WorkExecutionSpec::account_scoped(&plan, ACCOUNT_LIMITS, capability)?;
        spec.context = plan.context.clone();
        Ok(WorkResponseV1 {
            version: 1,
            profile: profile.to_string(),
            reply: WorkReplyV1::ApprovalDraft {
                work: request.work,
                expected_revision: revision,
                spec,
            },
        })
    }

    /// A request sent without accounts may name a site the person is signed
    /// in to in an attached tab: Rust then drafts an origin grant for that
    /// tab, exactly as `prepare` mode origin does, and the request waits for
    /// the person's answer instead of starting. Only a first-party session's
    /// existence is checked, never an account; the approval still attests it.
    pub async fn signed_in_draft(
        &self,
        profile: ProfileId,
        command: &WorkCommandV1,
        context: Option<&context::WorkContextSelectionV1>,
        choice: &WorkSignedInV1,
    ) -> Result<Option<WorkResponseV1>, WorkError> {
        let WorkRuntimeIntent::BeginAgent { grant, .. } = &command.intent else {
            return Ok(None);
        };
        if !grant.accounts.is_empty() {
            return Ok(None);
        }
        let (origin, tab) = match choice {
            WorkSignedInV1::Declined => return Ok(None),
            WorkSignedInV1::Origin { origin } => {
                let state = self.projection(profile, command.work).await?;
                if state.executions.iter().any(|e| !e.status.terminal()) {
                    return Err(WorkError::Conflict);
                }
                (origin.clone(), None)
            }
            WorkSignedInV1::Offer => {
                let Some(context) = context else {
                    return Ok(None);
                };
                let state = self.projection(profile, command.work).await?;
                if state.work.revision != command.expected_revision {
                    return Ok(None);
                }
                let tabs = self.context_tabs(profile, context).await?;
                let Some(found) = signed_in_tab(profile, &state.work.objective, tabs).await else {
                    return Ok(None);
                };
                found
            }
        };
        let grant = WorkAccountGrantV1 {
            origin,
            account: mint_account()?,
            tab,
            pages: MAX_WORK_ACCOUNT_PAGES,
        };
        grant.validate()?;
        draft(profile, command.work, grant.clone())?;
        Ok(Some(WorkResponseV1 {
            version: 1,
            profile: profile.to_string(),
            reply: WorkReplyV1::AccountGrantDraft {
                work: command.work,
                grant,
            },
        }))
    }

    /// The probe's stand-in for a request's tabs: which one, if any, would be
    /// offered, drafted exactly as a request's offer is.
    #[cfg(feature = "work-execution-probe")]
    #[doc(hidden)]
    pub async fn offer_for_probe(
        profile: ProfileId,
        work: WorkId,
        request: &str,
        origins: Vec<String>,
    ) -> Result<Option<WorkAccountGrantV1>, WorkError> {
        let tabs = origins.into_iter().map(|origin| (None, origin)).collect();
        let Some((origin, tab)) = signed_in_tab(profile, request, tabs).await else {
            return Ok(None);
        };
        let grant = WorkAccountGrantV1 {
            origin,
            account: mint_account()?,
            tab,
            pages: MAX_WORK_ACCOUNT_PAGES,
        };
        grant.validate()?;
        draft(profile, work, grant.clone())?;
        Ok(Some(grant))
    }

    /// The request's attached browser tabs, then its consented open tabs, as
    /// distinct HTTPS origins in that order.
    async fn context_tabs(
        &self,
        profile: ProfileId,
        context: &context::WorkContextSelectionV1,
    ) -> Result<Vec<(Option<ItemId>, String)>, WorkError> {
        let snapshot = self.environment(profile, context.environment).await?;
        let attached: Vec<ItemId> = context
            .items
            .iter()
            .filter_map(|item| {
                snapshot
                    .elements
                    .iter()
                    .find(|element| element.id == item.element)
                    .and_then(|element| match element.reference {
                        WorkEnvironmentReference::Browser { tab } => Some(tab),
                        _ => None,
                    })
            })
            .collect();
        let mut tabs = if attached.is_empty() {
            Vec::new()
        } else {
            let receiver = self.handle.tab_metadata(profile, attached.clone());
            let found = tokio::task::spawn_blocking(move || receiver.recv_timeout(READ_TIMEOUT))
                .await
                .map_err(|_| WorkError::Unavailable)?
                .map_err(|_| WorkError::Unavailable)?;
            // Metadata comes back in any order; the attachment order stands.
            attached
                .iter()
                .filter_map(|id| found.iter().find(|tab| tab.id == *id).cloned())
                .collect()
        };
        if context.tabs {
            let receiver = self.handle.window_tabs(profile);
            tabs.extend(
                tokio::task::spawn_blocking(move || receiver.recv_timeout(READ_TIMEOUT))
                    .await
                    .map_err(|_| WorkError::Unavailable)?
                    .map_err(|_| WorkError::Unavailable)?
                    .into_iter()
                    .take(context::MAX_CONTEXT_TABS),
            );
        }
        let mut origins: Vec<(Option<ItemId>, String)> = Vec::new();
        for tab in tabs {
            let Some(origin) = tab
                .url
                .as_deref()
                .and_then(|url| url::Url::parse(url).ok())
                .filter(|url| url.scheme() == "https" && url.host_str().is_some())
                .map(|url| url.origin().ascii_serialization())
            else {
                continue;
            };
            if !origins.iter().any(|(_, known)| *known == origin) {
                origins.push((Some(tab.id), origin));
            }
        }
        Ok(origins)
    }

    async fn environment(
        &self,
        profile: ProfileId,
        environment: WorkEnvironmentId,
    ) -> Result<WorkEnvironmentSnapshot, WorkError> {
        let request = self.handle.submit_work_document(
            WorkRequest::Environment {
                call: WorkEnvironmentCall::Read { id: environment },
                space_available: false,
                browser_available: false,
                note_available: false,
            },
            Some(profile),
        )?;
        let projection = tokio::time::timeout(READ_TIMEOUT, request)
            .await
            .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Environment(WorkEnvironmentReply::Snapshot { snapshot }) = projection.reply
        else {
            return Err(WorkError::NotFound);
        };
        if projection.profile != profile || snapshot.profile != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        Ok(*snapshot)
    }

    async fn mint_plan(
        &self,
        profile: ProfileId,
        work: &WorkSnapshot,
    ) -> Result<(WorkPlanRevision, WorkRevision), WorkError> {
        let draft = WorkPlanProposal {
            nodes: vec![WorkNodeProposal {
                key: 0,
                objective: work.objective.clone(),
                dependencies: vec![],
                outputs: vec![WorkExpectedOutput {
                    name: "Findings".into(),
                    description: "What the signed-in page shows, with sources".into(),
                    review: WorkOutputReview::SourceMappedNeedsReview,
                }],
            }],
        }
        .mint()?;
        let request = self.handle.submit_work_document(
            WorkRequest::Edit {
                id: work.id,
                expected: work.revision,
                edit: WorkEdit::ReplaceDraft { draft },
                author: WorkAuthor::User,
            },
            Some(profile),
        )?;
        let projection = tokio::time::timeout(READ_TIMEOUT, request)
            .await
            .map_err(|_| WorkError::OutcomeUnknown)??;
        let WorkReply::Snapshot(snapshot) = projection.reply else {
            return Err(WorkError::Invalid);
        };
        if projection.profile != profile || snapshot.id != work.id {
            return Err(WorkError::ProfileUnavailable);
        }
        let plan = snapshot.plan.clone().ok_or(WorkError::Invalid)?;
        Ok((plan, snapshot.revision))
    }

    async fn projection(
        &self,
        profile: ProfileId,
        work: WorkId,
    ) -> Result<WorkRuntimeProjection, WorkError> {
        let response =
            tokio::time::timeout(READ_TIMEOUT, self.handle.work_projection(profile, work)?)
                .await
                .map_err(|_| WorkError::Unavailable)??;
        let WorkReply::Runtime(state) = response.reply else {
            return Err(WorkError::Invalid);
        };
        if response.profile != profile || state.work.profile != profile {
            return Err(WorkError::ProfileUnavailable);
        }
        Ok(*state)
    }

    /// The element must be a browser attachment of this environment; the tab
    /// must belong to the profile and currently show an HTTPS page.
    async fn attached_tab(
        &self,
        profile: ProfileId,
        environment: WorkEnvironmentId,
        element: WorkElementId,
    ) -> Result<crate::TabMetadata, WorkError> {
        let snapshot = self.environment(profile, environment).await?;
        let tab = snapshot
            .elements
            .iter()
            .find(|entry| entry.id == element)
            .and_then(|entry| match entry.reference {
                WorkEnvironmentReference::Browser { tab } => Some(tab),
                _ => None,
            })
            .ok_or(WorkError::NotFound)?;
        let receiver = self.handle.tab_metadata(profile, vec![tab]);
        let mut tabs = tokio::task::spawn_blocking(move || receiver.recv_timeout(READ_TIMEOUT))
            .await
            .map_err(|_| WorkError::Unavailable)?
            .map_err(|_| WorkError::Unavailable)?;
        tabs.pop()
            .filter(|entry| entry.id == tab)
            .ok_or(WorkError::NotFound)
    }
}

fn mint_account() -> Result<String, WorkError> {
    serde_json::to_value(zephium_agentic::AgentAccountId::generate())
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(WorkError::Unavailable)
}

/// The first tab whose site the request names and the profile holds a
/// session for, as its origin and tab.
async fn signed_in_tab(
    profile: ProfileId,
    request: &str,
    tabs: Vec<(Option<ItemId>, String)>,
) -> Option<(String, Option<ItemId>)> {
    let named: Vec<(Option<ItemId>, String)> = tabs
        .into_iter()
        .filter(|(_, origin)| names_site(request, host_of(origin)))
        .collect();
    let hosts = named
        .iter()
        .map(|(_, origin)| host_of(origin).to_owned())
        .collect();
    let present = crate::work_context::sessions_present(profile, hosts).await;
    named
        .into_iter()
        .zip(present)
        .find_map(|((tab, origin), present)| present.then_some((origin, tab)))
}

fn host_of(origin: &str) -> &str {
    origin.strip_prefix("https://").unwrap_or(origin)
}

/// Products the canvas's vendor table names that people sign in to, by the
/// site they sign in on. Closed: a product missing here is matched by host.
const PRODUCTS: &[(&str, &[&str])] = &[
    ("aws.amazon.com", &["AWS", "Amazon Web Services"]),
    ("cloud.google.com", &["Google Cloud", "GCP", "BigQuery"]),
    ("azure.microsoft.com", &["Azure", "Microsoft Azure"]),
    ("cloudflare.com", &["Cloudflare"]),
    ("vercel.com", &["Vercel"]),
    ("netlify.com", &["Netlify"]),
    ("stripe.com", &["Stripe"]),
    ("twilio.com", &["Twilio"]),
    ("sendgrid.com", &["SendGrid"]),
    ("auth0.com", &["Auth0"]),
    ("okta.com", &["Okta"]),
    ("elastic.co", &["Elastic", "Elasticsearch", "Kibana"]),
    ("docker.com", &["Docker", "Docker Hub"]),
    ("github.com", &["GitHub"]),
    ("gitlab.com", &["GitLab"]),
    ("sentry.io", &["Sentry"]),
    ("datadoghq.com", &["Datadog"]),
    ("grafana.com", &["Grafana"]),
    ("openai.com", &["OpenAI", "ChatGPT"]),
    ("anthropic.com", &["Anthropic", "Claude"]),
    ("huggingface.co", &["Hugging Face", "HuggingFace"]),
    ("supabase.com", &["Supabase"]),
    ("firebase.google.com", &["Firebase", "Firestore"]),
    ("planetscale.com", &["PlanetScale"]),
    ("neon.tech", &["Neon"]),
    ("snowflake.com", &["Snowflake"]),
    ("digitalocean.com", &["DigitalOcean"]),
    ("hetzner.com", &["Hetzner"]),
    ("fly.io", &["Fly.io"]),
    ("render.com", &["Render"]),
    ("railway.app", &["Railway"]),
    ("heroku.com", &["Heroku"]),
    ("linode.com", &["Linode"]),
    ("algolia.com", &["Algolia"]),
    ("mapbox.com", &["Mapbox"]),
    ("segment.com", &["Segment"]),
    ("mixpanel.com", &["Mixpanel"]),
    ("amplitude.com", &["Amplitude"]),
    ("posthog.com", &["PostHog"]),
    ("resend.com", &["Resend"]),
    ("postmarkapp.com", &["Postmark"]),
    ("mailgun.com", &["Mailgun"]),
    ("slack.com", &["Slack"]),
    ("discord.com", &["Discord"]),
    ("notion.so", &["Notion"]),
    ("figma.com", &["Figma"]),
    ("linear.app", &["Linear"]),
    ("atlassian.net", &["Jira", "Confluence"]),
];

/// Whether the request names this host, its site, or a product signed in on
/// that site, as whole words. Closed matching over the person's own words;
/// it decides only whether to ask.
pub(crate) fn names_site(request: &str, host: &str) -> bool {
    let request = request.to_lowercase();
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.split(':').next().unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(host);
    let site = site_of(host);
    word(&request, host)
        || word(&request, site)
        || PRODUCTS.iter().any(|(vendor, names)| {
            on_site(host, vendor)
                && names
                    .iter()
                    .any(|name| word(&request, &name.to_lowercase()))
        })
}
/// The last two labels of a name; an address stands for itself.
fn site_of(host: &str) -> &str {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }
    match host.rmatch_indices('.').nth(1) {
        Some((dot, _)) => &host[dot + 1..],
        None => host,
    }
}
/// A vendor names a site (`slack.com`) or a service on one (`aws.amazon.com`):
/// the host is on it when it shares the vendor's site.
fn on_site(host: &str, vendor: &str) -> bool {
    let site = site_of(vendor);
    host == vendor || host == site || host.ends_with(&format!(".{site}"))
}
fn word(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let inner = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    text.match_indices(needle).any(|(at, _)| {
        let rest = &text[at + needle.len()..];
        let mut after = rest.chars();
        // A dot ends a sentence, or continues a longer name.
        let continues = match after.next() {
            Some('.') => after.next().is_some_and(char::is_alphanumeric),
            Some(c) => inner(c),
            None => false,
        };
        !continues
            && !text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| inner(c) || c == '.')
    })
}

#[cfg(test)]
mod intent_tests {
    use super::names_site;

    #[test]
    fn work_signed_in_intent_matches_a_host_its_site_or_a_product_on_it() {
        assert!(names_site(
            "Summarise the #launch channel in Slack",
            "app.slack.com"
        ));
        assert!(names_site(
            "what changed on app.slack.com today?",
            "app.slack.com"
        ));
        assert!(names_site("check slack.com/help", "app.slack.com"));
        assert!(names_site("My open GitHub pull requests", "github.com"));
        assert!(names_site(
            "Jira tickets assigned to me",
            "acme.atlassian.net"
        ));
        assert!(names_site(
            "costs in the AWS console",
            "us-east-1.console.aws.amazon.com"
        ));
        assert!(names_site("read account.probe.test", "account.probe.test"));
        // Other sites, other words, and a name inside a longer word do not ask.
        assert!(!names_site("Summarise the channel", "app.slack.com"));
        assert!(!names_site("Compare Notion and Linear", "app.slack.com"));
        assert!(!names_site("slackware release notes", "app.slack.com"));
        assert!(!names_site("see notslack.com", "slack.com"));
        assert!(!names_site(
            "Summarise my Slack channel",
            "slack.com.evil.test"
        ));
        assert!(!names_site("GitHub stars for zephium", "gitlab.com"));
    }
}
