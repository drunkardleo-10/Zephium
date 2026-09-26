//! Signed-in origin grants against two loopback sites: one the person granted,
//! one they did not. Each check reads what the servers saw; no public site,
//! and no page text or title leaves this file's closed-fact lines.
use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use zephium_app::work_agent::{
    WorkAgentBrowseRequest, WorkAgentProviders, WorkAgentService, WorkBrowserOutcome,
};
use zephium_core::{
    ids::ProfileId,
    work::{agent::*, runtime::*, search::*, synthesis::*, *},
};
use zephium_ipc::work::*;
use zephium_work_composition::MacosWorkComposition;

use super::work_durable::{browser_settings, WorkflowResult};

/// The granted site as the Work contract sees it, and the other site.
const ACCOUNT: &str = "https://account.probe.test";
const OTHER: &str = "https://other.probe.test";
const ACCOUNT_COOKIE: &str = "zaccount=signed-in-a";
const OTHER_COOKIE: &str = "zother=signed-in-b";
const ACCOUNT_FACT: &str = "Quarterly report from Dana";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Check {
    /// A read inside the grant carries the session cookie and sees account content.
    Read,
    /// A read outside the grant carries no cookie.
    Outside,
    /// A redirect out of the origin ends the read.
    Redirect,
    /// The thirteenth signed-in page is refused.
    Budget,
    /// A POST-shaped change is refused as AccountWrite.
    Write,
    /// The grant does not survive into a second request.
    SecondRequest,
    /// A request naming a tab's site drafts a grant only once the profile
    /// holds that site's session, and the drafted grant reads signed in.
    Intent,
}
const ALL: [Check; 7] = [
    Check::Read,
    Check::Outside,
    Check::Redirect,
    Check::Budget,
    Check::Write,
    Check::SecondRequest,
    Check::Intent,
];
static CHECKS: OnceLock<Vec<Check>> = OnceLock::new();

pub(super) fn run(which: &std::ffi::OsStr) -> Result<(), super::ProbeFailure> {
    let checks = match which.to_str() {
        Some("all") => ALL.to_vec(),
        Some("read") => vec![Check::Read],
        Some("outside") => vec![Check::Outside],
        Some("redirect") => vec![Check::Redirect],
        Some("budget") => vec![Check::Budget],
        Some("write") => vec![Check::Write],
        Some("second-request") => vec![Check::SecondRequest],
        Some("intent") => vec![Check::Intent],
        _ => return Err(super::ProbeFailure::Authority),
    };
    let _ = CHECKS.set(checks);
    super::work_durable::run_loopback_account()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Site {
    Account,
    Other,
}
struct Hit {
    site: Site,
    post: bool,
    path: String,
    cookie: String,
}

/// A site the person never signed in to; nothing serves it.
const FRESH: &str = "https://fresh.probe.test";

/// The app's session check against the real profile store, with each contract
/// host answered by the loopback host whose cookies stand for it.
pub(super) fn install_presence(engine: std::sync::Arc<zephium_engine::WebviewEngine>) {
    zephium_app::work_context::install_session_presence(std::sync::Arc::new(
        move |profile, hosts: Vec<String>| {
            let hosts = hosts
                .into_iter()
                .map(|host| {
                    match host.as_str() {
                        "account.probe.test" => "127.0.0.1",
                        "other.probe.test" => "localhost",
                        _ => "fresh.probe.invalid",
                    }
                    .to_owned()
                })
                .collect();
            engine.work_sessions_present(profile, hosts)
        },
    ));
}

/// Whether a request naming each site would stop at a drafted grant.
async fn offered(
    profile: ProfileId,
    work: WorkId,
    request: &str,
    origins: &[&str],
) -> Result<Option<WorkAccountGrantV1>, &'static str> {
    zephium_app::work_account_scope::WorkAccountApproval::offer_for_probe(
        profile,
        work,
        request,
        origins.iter().map(|origin| (*origin).to_owned()).collect(),
    )
    .await
    .map_err(|_| "intent_offer")
}

/// Two HTTP/1.1 loopback servers that record method, path and cookie only.
struct Sites {
    account: u16,
    other: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    stop: Arc<AtomicBool>,
}
impl Sites {
    fn start() -> Result<Self, &'static str> {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let account = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "loopback_bind")?;
        let other = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "loopback_bind")?;
        let ports = (
            account.local_addr().map_err(|_| "loopback_addr")?.port(),
            other.local_addr().map_err(|_| "loopback_addr")?.port(),
        );
        for (listener, site) in [(account, Site::Account), (other, Site::Other)] {
            listener
                .set_nonblocking(true)
                .map_err(|_| "loopback_nonblocking")?;
            let hits = hits.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("loopback-site".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => serve(stream, site, ports, &hits),
                            Err(_) => std::thread::sleep(Duration::from_millis(5)),
                        }
                    }
                })
                .map_err(|_| "loopback_thread")?;
        }
        Ok(Self {
            account: ports.0,
            other: ports.1,
            hits,
            stop,
        })
    }
    /// The loopback address behind a contract URL. The other site is named
    /// `localhost`, so the two sites never share a cookie jar entry.
    fn local(&self, url: &str) -> String {
        if let Some(path) = url.strip_prefix(ACCOUNT) {
            format!("http://127.0.0.1:{}{path}", self.account)
        } else if let Some(path) = url.strip_prefix(OTHER) {
            format!("http://localhost:{}{path}", self.other)
        } else {
            url.to_owned()
        }
    }
    fn hits(&self, site: Site, path: &str) -> Vec<(bool, String)> {
        self.hits
            .lock()
            .map(|hits| {
                hits.iter()
                    .filter(|hit| hit.site == site && hit.path.starts_with(path))
                    .map(|hit| (hit.post, hit.cookie.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn posts(&self) -> usize {
        self.hits
            .lock()
            .map(|hits| hits.iter().filter(|hit| hit.post).count())
            .unwrap_or(usize::MAX)
    }
}
impl Drop for Sites {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(mut stream: TcpStream, site: Site, ports: (u16, u16), hits: &Mutex<Vec<Hit>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => request.extend_from_slice(&chunk[..read]),
        }
    }
    let head = String::from_utf8_lossy(&request);
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split(' ');
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let cookie = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default();
    let signed_in = cookie.contains(ACCOUNT_COOKIE);
    if let Ok(mut hits) = hits.lock() {
        hits.push(Hit {
            site,
            post: method == "POST",
            path: path.clone(),
            cookie: cookie.clone(),
        });
    }
    let page = |title: &str, body: &str| {
        format!("<!doctype html><html><head><title>{title}</title></head><body><main>{body}</main></body></html>")
    };
    let (status, headers, body) = match (site, method.as_str(), path.as_str()) {
        (_, "POST", _) => ("200 OK", String::new(), page("Done", "<p>Saved.</p>")),
        (Site::Account, _, "/seed") => (
            "200 OK",
            format!("Set-Cookie: {ACCOUNT_COOKIE}; Path=/; Max-Age=3600; HttpOnly\r\n"),
            page("Account", "<h1>Signed in</h1><p>Welcome back to the account site.</p>"),
        ),
        (Site::Other, _, "/seed") => (
            "200 OK",
            format!("Set-Cookie: {OTHER_COOKIE}; Path=/; Max-Age=3600; HttpOnly\r\n"),
            page("Other", "<h1>Signed in elsewhere</h1><p>Welcome back to the other site.</p>"),
        ),
        (Site::Account, _, "/leave") => (
            "302 Found",
            format!("Location: http://localhost:{}/landing\r\n", ports.1),
            String::new(),
        ),
        (Site::Account, _, _) if !signed_in => (
            "200 OK",
            String::new(),
            page("Sign in", "<h1>Please sign in</h1><form method=\"post\" action=\"/login\"><label>User <input name=\"user\"></label><button>Sign in</button></form>"),
        ),
        (Site::Account, _, "/inbox") => (
            "200 OK",
            String::new(),
            page("Inbox", &format!("<h1>Inbox</h1><ul><li>{ACCOUNT_FACT} is due Friday.</li><li>Team lunch moved to Thursday.</li></ul><img alt=\"\" width=\"1\" height=\"1\" src=\"http://localhost:{}/pixel.gif\">", ports.1)),
        ),
        (Site::Account, _, "/settings") => (
            "200 OK",
            String::new(),
            page("Settings", "<h1>Account settings</h1><p>Plan: Team, renews on March 3.</p><form method=\"post\" action=\"/settings/delete\"><button>Delete account</button></form>"),
        ),
        (Site::Account, _, path) => (
            "200 OK",
            String::new(),
            page("Account page", &format!("<h1>Account page</h1><p>This signed-in page is {}.</p>", path.trim_start_matches('/'))),
        ),
        (Site::Other, _, "/pixel.gif") => (
            "200 OK",
            "Content-Type: image/gif\r\n".into(),
            String::from_utf8_lossy(b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;").into_owned(),
        ),
        (Site::Other, _, _) => (
            "200 OK",
            String::new(),
            page("Public notes", "<h1>Public notes</h1><p>The other site lists opening hours: nine to five on weekdays.</p>"),
        ),
    };
    let content_type = if headers.contains("Content-Type") {
        ""
    } else {
        "Content-Type: text/html; charset=utf-8\r\n"
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\n{headers}{content_type}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Fetches each turn in order, then places one knowledge note and finishes.
struct Script {
    turns: Vec<Vec<WorkAgentFetch>>,
    at: AtomicUsize,
    notices: Mutex<Vec<String>>,
}
impl Script {
    fn new(turns: Vec<Vec<WorkAgentFetch>>) -> Self {
        Self {
            turns,
            at: AtomicUsize::new(0),
            notices: Mutex::new(Vec::new()),
        }
    }
}
impl WorkAgentTurnProvider for Script {
    fn turn<'a>(
        &'a self,
        input: &'a WorkAgentTurnDisclosure,
        _: WorkSynthesisTrace,
    ) -> WorkAgentTurnFuture<'a> {
        Box::pin(async move {
            if let Ok(mut notices) = self.notices.lock() {
                notices.extend(input.context().notices.iter().cloned());
            }
            let at = self.at.fetch_add(1, Ordering::SeqCst);
            let fetch = self.turns.get(at).cloned().unwrap_or_default();
            let publish = at == self.turns.len();
            Ok(WorkAgentTurnResult {
                output: WorkAgentTurnOutput {
                    say: None,
                    artifacts: if publish {
                        vec![WorkAgentArtifactOutput {
                            title: "Loopback note".into(),
                            data: artifact::WorkArtifactDataV1::Document {
                                paragraphs: vec!["The loopback check ran.".into()],
                                formatted: None,
                            },
                            evidence: vec![],
                            general_knowledge: true,
                        }]
                    } else {
                        vec![]
                    },
                    finish: at > self.turns.len(),
                    fetch,
                    ask: None,
                    followups: vec![],
                    malformed: 0,
                },
                usage: WorkUsage::default(),
            })
        })
    }
}
struct NoSearch;
impl WorkPublicSearchProvider for NoSearch {
    fn search<'a>(
        &'a self,
        _: &'a WorkPublicSearchScope,
        _: &'a [zephium_core::work::context::WorkContextBody],
        _: WorkExecutionLimits,
    ) -> WorkPublicSearchFuture<'a> {
        Box::pin(async { Err(WorkPublicSearchError::NotDispatched(WorkError::Unavailable)) })
    }
}

fn read(url: &str) -> WorkAgentFetch {
    WorkAgentFetch::Read {
        url: url.into(),
        collection: None,
    }
}
fn grant(origin: &str, pages: u8) -> WorkAccountGrantV1 {
    WorkAccountGrantV1 {
        origin: origin.into(),
        account: serde_json::to_value(zephium_agentic::AgentAccountId::generate())
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
        tab: None,
        pages,
    }
}

struct Run {
    state: WorkRuntimeProjection,
    notices: Vec<String>,
    dispatched: Vec<(String, bool)>,
}

struct Context<'a> {
    handle: &'a zephium_app::Handle,
    composition: &'a MacosWorkComposition,
    profile: ProfileId,
    binding: zephium_app::AgentWorkProfileBinding,
    keys: Mutex<Vec<zephium_agentic::AgentProviderCredential>>,
    sites: Sites,
}
impl Context<'_> {
    async fn create(&self, objective: &str) -> Result<(WorkId, WorkRevision), &'static str> {
        let created = self
            .handle
            .work_authoring_command(
                self.profile,
                WorkAuthoringCommandV1 {
                    version: 1,
                    command: WorkCommandId::generate(),
                    intent: WorkAuthoringIntent::Create {
                        objective: objective.into(),
                    },
                },
            )
            .map_err(|_| "create_admission")?
            .response(self.profile)
            .await;
        let WorkReplyV1::AuthoringApplied { receipt } = created.reply else {
            return Err("create_persistence");
        };
        Ok((receipt.work, receipt.applied_revision))
    }

    /// One request under `grants`, each first approved (the probe stands in
    /// for the person) unless `approve` is false.
    async fn request(
        &self,
        work: (WorkId, WorkRevision),
        grants: Vec<WorkAccountGrantV1>,
        approve: bool,
        turns: Vec<Vec<WorkAgentFetch>>,
    ) -> Result<Run, WorkError> {
        if approve {
            for grant in &grants {
                zephium_app::work_account_scope::record_approved_grant_for_probe(
                    self.profile,
                    work.0,
                    grant.clone(),
                )?;
            }
        }
        let script = Script::new(turns);
        let dispatched = Mutex::new(Vec::new());
        let callback = self.handle.callback_handle();
        let state = WorkAgentService::new(self.handle.clone())
            .with_diagnostic(|event| {
                let _ = writeln!(std::io::stdout().lock(), "loopback-account: loop={event:?}");
            })
            .run(
                self.profile,
                WorkCommandV1 {
                    version: 1,
                    work: work.0,
                    expected_revision: work.1,
                    command: WorkCommandId::generate(),
                    intent: WorkRuntimeIntent::BeginAgent {
                        grant: WorkAgentGrantV1 {
                            provider: WorkSearchProvider::OpenAi,
                            model: PUBLIC_SEARCH_MODEL.into(),
                            max_turns: 8,
                            max_steps: 40,
                            browse_hops: 1,
                            folders: vec![],
                            accounts: grants,
                        },
                        limits: WorkExecutionLimits {
                            model_tokens: 1_000_000,
                            cost_micro_usd: 2_000_000,
                            operations: 256,
                            timeout_seconds: 1200,
                            max_workers: 3,
                        },
                    },
                },
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &NoSearch,
                },
                |probe, request| {
                    let key = self.keys.lock().ok().and_then(|mut keys| keys.pop());
                    let callback = &callback;
                    let dispatched = &dispatched;
                    async move {
                        let request = self.local(request, dispatched)?;
                        let key = match key {
                            Some(key) => key,
                            None => tokio::task::spawn_blocking(
                                zephium_agentic::load_macos_probe_openai_credential,
                            )
                            .await
                            .map_err(|_| WorkError::Unavailable)?
                            .map_err(|_| WorkError::Unavailable)?,
                        };
                        let mut settings = browser_settings(self.binding, key);
                        // Signed-in pages are never retained at the provider.
                        settings.retain_public_responses = false;
                        settings.loopback_anonymous = true;
                        settings.model_diagnostic = None;
                        let outcome = self
                            .composition
                            .run_agent_step(callback, &probe, request, settings)
                            .await;
                        report(&outcome);
                        outcome
                    }
                },
                |_| {},
            )
            .await?;
        let notices = script.notices.lock().map(|n| n.clone()).unwrap_or_default();
        let dispatched = dispatched.lock().map(|d| d.clone()).unwrap_or_default();
        Ok(Run {
            state,
            notices,
            dispatched,
        })
    }

    /// Points a contract request at its loopback site; records only whether
    /// it was signed in.
    fn local(
        &self,
        mut request: WorkAgentBrowseRequest,
        dispatched: &Mutex<Vec<(String, bool)>>,
    ) -> Result<WorkAgentBrowseRequest, WorkError> {
        let WorkStepKindV1::Read { url, .. } = &mut request.step else {
            return Err(WorkError::Invalid);
        };
        if let Ok(mut dispatched) = dispatched.lock() {
            dispatched.push((url.clone(), request.account.is_some()));
        }
        *url = self.sites.local(url);
        if let Some(grant) = &mut request.account {
            grant.origin = self.sites.local(&grant.origin);
        }
        Ok(request)
    }
}

fn report(outcome: &Result<WorkBrowserOutcome, WorkError>) {
    match outcome {
        Ok(outcome) => {
            let _ = writeln!(
                std::io::stdout().lock(),
                "loopback-account: read status={:?} artifacts={} account_write={} note_bytes={}",
                outcome.status,
                outcome.artifacts.len(),
                outcome.account_write,
                outcome.note.as_ref().map_or(0, String::len)
            );
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stdout().lock(),
                "loopback-account: read error={error:?}"
            );
        }
    }
}

fn says(run: &Run, text: &str) -> bool {
    run.state.executions.iter().any(|execution| {
        execution
            .artifacts
            .iter()
            .any(|artifact| artifact.data.plain_text().contains(text))
    })
}

pub(super) async fn workflow(
    handle: &zephium_app::Handle,
    composition: &MacosWorkComposition,
    profile: ProfileId,
    binding: zephium_app::AgentWorkProfileBinding,
    keys: Vec<zephium_agentic::AgentProviderCredential>,
) -> Result<WorkflowResult, &'static str> {
    let checks = CHECKS.get().cloned().unwrap_or_else(|| ALL.to_vec());
    let context = Context {
        handle,
        composition,
        profile,
        binding,
        keys: Mutex::new(keys),
        sites: Sites::start()?,
    };
    let line = |text: String| {
        let _ = writeln!(std::io::stdout().lock(), "loopback-account: {text}");
    };
    // Before any session: naming the site offers nothing.
    let unsigned = if checks.contains(&Check::Intent) {
        let work = context
            .create("Summarise my inbox on account.probe.test")
            .await?;
        Some(
            offered(
                profile,
                work.0,
                "Summarise my inbox on account.probe.test",
                &[ACCOUNT],
            )
            .await?,
        )
    } else {
        None
    };
    // The person signs in to both sites in this profile.
    let seed = context
        .request(
            context.create("Open both sites once").await?,
            vec![grant(ACCOUNT, 1), grant(OTHER, 1)],
            true,
            vec![vec![
                read(&format!("{ACCOUNT}/seed")),
                read(&format!("{OTHER}/seed")),
            ]],
        )
        .await
        .map_err(|_| "seed_run")?;
    let seeded = context.sites.hits(Site::Account, "/seed").len() == 1
        && context.sites.hits(Site::Other, "/seed").len() == 1;
    line(format!(
        "seed signed_in_reads={} sites_seen={seeded}",
        seed.dispatched.iter().filter(|(_, signed)| *signed).count()
    ));
    if !seeded {
        return Err("seed");
    }
    let mut failure = None;
    let mut last = seed.state;
    for check in checks {
        let passed = match check {
            Check::Read => {
                let run = context
                    .request(
                        context.create("Summarize my inbox").await?,
                        vec![grant(ACCOUNT, 12)],
                        true,
                        vec![vec![read(&format!("{ACCOUNT}/inbox"))]],
                    )
                    .await
                    .map_err(|_| "read_run")?;
                let inbox = context.sites.hits(Site::Account, "/inbox");
                let cookie =
                    !inbox.is_empty() && inbox.iter().all(|(_, c)| c.contains(ACCOUNT_COOKIE));
                let content = says(&run, ACCOUNT_FACT);
                let pixel = context.sites.hits(Site::Other, "/pixel.gif");
                let third_party = pixel.iter().any(|(_, c)| !c.is_empty());
                let badge = run.state.executions[0]
                    .steps
                    .iter()
                    .any(|step| step.account.as_ref().is_some_and(|a| a.badge));
                line(format!("check=read signed_in={} session_cookie={cookie} account_content={content} badge={badge} subresource_requests={} subresource_cookie={third_party}", run.dispatched.iter().all(|(_, s)| *s), pixel.len()));
                last = run.state;
                cookie && content && badge && !third_party
            }
            Check::Outside => {
                let run = context
                    .request(
                        context
                            .create(&format!("Compare the notes at {OTHER}/public"))
                            .await?,
                        vec![grant(ACCOUNT, 12)],
                        true,
                        vec![vec![read(&format!("{OTHER}/public"))]],
                    )
                    .await
                    .map_err(|_| "outside_run")?;
                let public = context.sites.hits(Site::Other, "/public");
                let anonymous = run.dispatched.iter().all(|(_, signed)| !signed);
                let cookie = public.iter().any(|(_, c)| !c.is_empty());
                line(format!(
                    "check=outside anonymous={anonymous} requests={} cookie={cookie}",
                    public.len()
                ));
                last = run.state;
                anonymous && !public.is_empty() && !cookie
            }
            Check::Redirect => {
                let run = context
                    .request(
                        context.create("Follow my account link").await?,
                        vec![grant(ACCOUNT, 12)],
                        true,
                        vec![vec![read(&format!("{ACCOUNT}/leave"))]],
                    )
                    .await
                    .map_err(|_| "redirect_run")?;
                let step = run.state.executions[0]
                    .steps
                    .iter()
                    .find(|step| matches!(step.kind, WorkStepKindV1::Read { .. }));
                let ended = step.is_some_and(|step| {
                    step.status != WorkStepStatus::Succeeded
                        && step
                            .note
                            .as_deref()
                            .is_some_and(|note| note.starts_with("The page left "))
                });
                let landing = context.sites.hits(Site::Other, "/landing");
                let leaked = landing.iter().any(|(_, c)| !c.is_empty());
                line(format!("check=redirect ended={ended} status={:?} landing_requests={} landing_cookie={leaked}", step.map(|s| s.status), landing.len()));
                last = run.state;
                ended && !leaked
            }
            Check::Budget => {
                let pages: Vec<WorkAgentFetch> = (1..=13)
                    .map(|n| read(&format!("{ACCOUNT}/page-{n}")))
                    .collect();
                let run = context
                    .request(
                        context.create("Read my account pages").await?,
                        vec![grant(ACCOUNT, 12)],
                        true,
                        pages.chunks(4).map(<[_]>::to_vec).collect(),
                    )
                    .await
                    .map_err(|_| "budget_run")?;
                let served = context.sites.hits(Site::Account, "/page-");
                let thirteenth = context.sites.hits(Site::Account, "/page-13").len();
                let refused = run
                    .notices
                    .contains(&WorkAccountRefusal::PageBudget.notice("account.probe.test"));
                let used = run.state.executions[0]
                    .accounts
                    .first()
                    .map(|a| (a.pages_used, a.pages));
                line(format!("check=budget dispatched={} served={} with_cookie={} thirteenth_served={thirteenth} refused={refused} use={used:?}", run.dispatched.len(), served.len(), served.iter().filter(|(_, c)| c.contains(ACCOUNT_COOKIE)).count()));
                last = run.state;
                run.dispatched.len() == 12 && thirteenth == 0 && refused && used == Some((12, 12))
            }
            Check::Write => {
                let run = context
                    .request(
                        context.create("Check my plan and cancel it").await?,
                        vec![grant(ACCOUNT, 12)],
                        true,
                        vec![vec![
                            read(&format!("{ACCOUNT}/settings/delete?confirm=1")),
                            read(&format!("{ACCOUNT}/settings")),
                        ]],
                    )
                    .await
                    .map_err(|_| "write_run")?;
                let action = context.sites.hits(Site::Account, "/settings/delete").len();
                let refused = run
                    .notices
                    .contains(&WorkAccountRefusal::AccountWrite.notice("account.probe.test"));
                let settings = context.sites.hits(Site::Account, "/settings").len();
                line(format!("check=write refused={refused} action_requests={action} settings_reads={settings} posts={}", context.sites.posts()));
                last = run.state;
                refused && action == 0 && context.sites.posts() == 0
            }
            Check::SecondRequest => {
                let work = context.create("Summarize my inbox twice").await?;
                let granted = grant(ACCOUNT, 12);
                let first = context
                    .request(
                        work,
                        vec![granted.clone()],
                        true,
                        vec![vec![read(&format!("{ACCOUNT}/inbox"))]],
                    )
                    .await
                    .map_err(|_| "first_request")?;
                let before = context.sites.hits(Site::Account, "/").len();
                let second = context
                    .request(
                        (work.0, first.state.work.revision),
                        vec![granted],
                        false,
                        vec![vec![read(&format!("{ACCOUNT}/inbox"))]],
                    )
                    .await;
                let after = context.sites.hits(Site::Account, "/").len();
                let refused = matches!(second, Err(WorkError::ReviewRequired));
                line(format!("check=second_request first_signed_in={} second_refused={refused} new_requests={}", first.dispatched.iter().all(|(_, s)| *s), after - before));
                last = first.state;
                refused && after == before
            }
            Check::Intent => {
                let request = "Summarise my inbox on account.probe.test";
                let work = context.create(request).await?;
                let drafted = offered(profile, work.0, request, &[FRESH, ACCOUNT]).await?;
                let fresh_work = context
                    .create("Summarise my notes on fresh.probe.test")
                    .await?;
                let fresh = offered(
                    profile,
                    fresh_work.0,
                    "Summarise my notes on fresh.probe.test",
                    &[FRESH, ACCOUNT],
                )
                .await?;
                let unnamed_work = context.create("Summarise my notes").await?;
                let unnamed =
                    offered(profile, unnamed_work.0, "Summarise my notes", &[ACCOUNT]).await?;
                let origin = drafted.as_ref().map(|grant| grant.origin.as_str());
                // Allowing the drafted grant rides it with the request, unapproved by the probe.
                let run = match drafted.clone() {
                    Some(grant) => Some(
                        context
                            .request(
                                work,
                                vec![grant],
                                false,
                                vec![vec![read(&format!("{ACCOUNT}/inbox"))]],
                            )
                            .await
                            .map_err(|_| "intent_run")?,
                    ),
                    None => None,
                };
                let signed_in = run
                    .as_ref()
                    .is_some_and(|run| run.dispatched.iter().all(|(_, s)| *s));
                let content = run.as_ref().is_some_and(|run| says(run, ACCOUNT_FACT));
                line(format!("check=intent before_session={} after_session_origin={origin:?} fresh_site={} unnamed={} signed_in_read={signed_in} account_content={content}", unsigned.as_ref().map_or("skipped", |o| if o.is_some() { "drafted" } else { "none" }), fresh.is_some(), unnamed.is_some()));
                if let Some(run) = run {
                    last = run.state;
                }
                matches!(unsigned, Some(None))
                    && origin == Some(ACCOUNT)
                    && fresh.is_none()
                    && unnamed.is_none()
                    && signed_in
                    && content
            }
        };
        line(format!("check={check:?} passed={passed}"));
        if !passed {
            failure.get_or_insert("loopback_check");
        }
    }
    line(format!("posts_total={}", context.sites.posts()));
    if context.sites.posts() != 0 {
        failure.get_or_insert("loopback_post");
    }
    Ok(WorkflowResult {
        state: last,
        failure,
    })
}
