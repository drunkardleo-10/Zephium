//! Page tasks in the person's session against two loopback sites. Each check
//! reads what the servers saw; no public site, and no page text leaves this
//! file's closed-fact lines.
use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use zephium_app::{
    work_agent::{
        WorkAgentBrowseRequest, WorkAgentProviders, WorkAgentService, WorkBrowserOutcome,
    },
    work_sites::SiteSession,
};
use zephium_core::{
    ids::ProfileId,
    work::{agent::*, runtime::*, search::*, sites::WorkSiteAccessV1, synthesis::*, *},
};
use zephium_ipc::work::*;
use zephium_work_composition::MacosWorkComposition;

use super::work_durable::{browser_settings, WorkflowResult};

/// The person's site as the Work contract sees it, and another site.
const ACCOUNT: &str = "https://account.probe.test";
const OTHER: &str = "https://other.probe.test";
const SITE: &str = "account.probe.test";
const ACCOUNT_COOKIE: &str = "zaccount=signed-in-a";
const INBOX_FACT: &str = "Quarterly report from Dana";
const REPORT_FACT: &str = "Reports total is 4,812";
const LANDED_FACT: &str = "Landing desk opens at nine";
const AUTOPOST_FACT: &str = "Autopost page lists three drafts";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Check {
    /// Asked once; Allow opens the person's session and its cookie reaches the site.
    Session,
    /// Always is kept for the profile: the next run asks nothing.
    Always,
    /// Never and private runs keep the session closed.
    Never,
    /// A same-document route inside the page keeps the page.
    Route,
    /// A same-site redirect is followed to its landing page.
    Redirect,
    /// A redirect off the site ends the page without carrying the session.
    Leave,
    /// A page's own POST without any agent step is cancelled; the page stays.
    Autopost,
    /// A committing control is held back; nothing is posted.
    Held,
    /// A sign-in page stops for the person before any model call.
    Wall,
}
const ALL: [Check; 9] = [
    Check::Session,
    Check::Always,
    Check::Never,
    Check::Route,
    Check::Redirect,
    Check::Leave,
    Check::Autopost,
    Check::Held,
    Check::Wall,
];
static CHECKS: OnceLock<Vec<Check>> = OnceLock::new();

pub(super) fn run(which: &std::ffi::OsStr) -> Result<(), super::ProbeFailure> {
    let checks = match which.to_str() {
        Some("all") => ALL.to_vec(),
        Some("session") => vec![Check::Session],
        Some("always") => vec![Check::Always],
        Some("never") => vec![Check::Never],
        Some("route") => vec![Check::Route],
        Some("redirect") => vec![Check::Redirect],
        Some("leave") => vec![Check::Leave],
        Some("autopost") => vec![Check::Autopost],
        Some("held") => vec![Check::Held],
        Some("wall") => vec![Check::Wall],
        _ => return Err(super::ProbeFailure::Authority),
    };
    let _ = CHECKS.set(checks);
    super::work_durable::run_loopback_site()
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

/// The app's session check against the real profile store, with each
/// contract site answered by the loopback host whose cookies stand for it.
pub(super) fn install_presence(engine: std::sync::Arc<zephium_engine::WebviewEngine>) {
    zephium_app::work_context::install_session_presence(std::sync::Arc::new(
        move |profile, sites: Vec<String>| {
            let hosts = sites
                .into_iter()
                .map(|site| {
                    match site.as_str() {
                        SITE => "127.0.0.1",
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
        (Site::Account, _, "/leave") => (
            "302 Found",
            format!("Location: http://localhost:{}/landing\r\n", ports.1),
            String::new(),
        ),
        (Site::Account, _, "/start") => (
            "302 Found",
            "Location: /landed\r\n".into(),
            String::new(),
        ),
        (Site::Account, _, "/landed") => (
            "200 OK",
            String::new(),
            page("Landed", &format!("<h1>Front desk</h1><p>{LANDED_FACT}.</p>")),
        ),
        (Site::Account, _, "/app") => (
            "200 OK",
            String::new(),
            page(
                "App",
                &format!("<h1>Dashboard</h1><button id=\"r\" type=\"button\">Reports</button><section id=\"view\"><p>Pick a view.</p></section><script>document.getElementById('r').addEventListener('click',function(){{history.pushState({{}},'','/app/reports');document.getElementById('view').innerHTML='<h2>Reports</h2><p>{REPORT_FACT}.</p>';}});</script>"),
            ),
        ),
        (Site::Account, _, "/autopost") => (
            "200 OK",
            String::new(),
            page(
                "Autopost",
                &format!("<h1>Drafts</h1><p>{AUTOPOST_FACT}.</p><form id=\"f\" method=\"post\" action=\"/autopost/submit\"><input type=\"hidden\" name=\"x\" value=\"1\"></form><script>setTimeout(function(){{document.getElementById('f').submit();}},600);</script>"),
            ),
        ),
        (Site::Account, _, "/compose") => (
            "200 OK",
            String::new(),
            page(
                "Compose",
                "<h1>Message #design</h1><label>Message <input id=\"m\" name=\"message\"></label><button id=\"s\" type=\"button\">Send</button><script>document.getElementById('s').addEventListener('click',function(){fetch('/send',{method:'POST',body:document.getElementById('m').value});});</script>",
            ),
        ),
        (Site::Account, _, _) if !signed_in => (
            "200 OK",
            String::new(),
            page("Sign in", "<h1>Sign in to your account</h1><form method=\"post\" action=\"/login\"><label>Email <input type=\"email\" name=\"email\" autocomplete=\"email\"></label><label>Password <input type=\"password\" name=\"password\"></label><button>Sign in</button></form>"),
        ),
        (Site::Account, _, "/inbox") => (
            "200 OK",
            String::new(),
            page("Inbox", &format!("<h1>Inbox</h1><ul><li>{INBOX_FACT} is due Friday.</li><li>Team lunch moved to Thursday.</li></ul>")),
        ),
        (Site::Account, _, path) => (
            "200 OK",
            String::new(),
            page("Account page", &format!("<h1>Account page</h1><p>This signed-in page is {}.</p>", path.trim_start_matches('/'))),
        ),
        (Site::Other, _, _) => (
            "200 OK",
            String::new(),
            page("Other", "<h1>Other site</h1><p>Opening hours: nine to five.</p>"),
        ),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\n{headers}Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
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

fn browse(path: &str, goal: &str) -> WorkAgentFetch {
    WorkAgentFetch::Browse {
        start: format!("{ACCOUNT}{path}"),
        goal: goal.into(),
        collection: None,
    }
}

/// One page as the probe opened it.
#[derive(Clone)]
struct Opened {
    yours: bool,
    held_back: bool,
    status: Option<WorkStepStatus>,
    intervention: Option<WorkInterventionKindV1>,
    model_calls: Option<u16>,
}

struct Run {
    state: WorkRuntimeProjection,
    notices: Vec<String>,
    opened: Vec<Opened>,
    asked: Vec<String>,
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

    async fn projection(&self, work: WorkId) -> Option<WorkRuntimeProjection> {
        let response = self
            .handle
            .work_projection(self.profile, work)
            .ok()?
            .await
            .ok()?;
        match response.reply {
            port::WorkReply::Runtime(state) => Some(*state),
            _ => None,
        }
    }

    /// Answers every question the run puts to the person with `answer`,
    /// and releases any page waiting on them.
    async fn person(
        &self,
        work: WorkId,
        answer: &str,
        done: &AtomicBool,
        asked: &Mutex<Vec<String>>,
    ) {
        while !done.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(pages) = self.composition.human_pages(self.profile, work) {
                for page in pages {
                    if page.phase == WorkHumanPhaseV1::WaitingForHuman {
                        let _ = self
                            .composition
                            .release_human_page(self.profile, work, page.id);
                    }
                }
            }
            let Some(state) = self.projection(work).await else {
                continue;
            };
            let Some((execution, step, prompt)) = state.executions.last().and_then(|execution| {
                execution.steps.iter().find_map(|step| match &step.kind {
                    WorkStepKindV1::Ask { prompt, .. }
                        if step.status == WorkStepStatus::Running =>
                    {
                        Some((execution.id, step.id, prompt.clone()))
                    }
                    _ => None,
                })
            }) else {
                continue;
            };
            let applied = self
                .handle
                .work_command(
                    self.profile,
                    WorkCommandV1 {
                        version: 1,
                        work,
                        expected_revision: state.work.revision,
                        command: WorkCommandId::generate(),
                        intent: WorkRuntimeIntent::AnswerStep {
                            execution,
                            step,
                            answer: answer.into(),
                        },
                    },
                )
                .ok();
            if let Some(applied) = applied {
                if applied.await.is_ok() {
                    if let Ok(mut asked) = asked.lock() {
                        asked.push(prompt);
                    }
                }
            }
        }
    }

    /// One run of `turns`, the probe answering questions with `answer`.
    async fn run(
        &self,
        objective: &str,
        private: bool,
        answer: &str,
        turns: Vec<Vec<WorkAgentFetch>>,
    ) -> Result<Run, &'static str> {
        let work = self.create(objective).await?;
        let script = Script::new(turns);
        let opened = Mutex::new(Vec::<Opened>::new());
        let asked = Mutex::new(Vec::new());
        let done = AtomicBool::new(false);
        let callback = self.handle.callback_handle();
        let service = WorkAgentService::new(self.handle.clone()).with_diagnostic(|event| {
            let _ = writeln!(std::io::stdout().lock(), "loopback-site: loop={event:?}");
        });
        let run = async {
            let state = service
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
                                accounts: vec![],
                                private,
                            },
                            limits: WorkExecutionLimits {
                                model_tokens: 1_000_000,
                                cost_micro_usd: 3_000_000,
                                operations: 512,
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
                        let opened = &opened;
                        async move {
                            let (request, yours) = self.local(request)?;
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
                            // The person's pages are never retained at the provider.
                            settings.retain_public_responses = false;
                            settings.loopback_anonymous = true;
                            let outcome = self
                                .composition
                                .run_agent_step(callback, &probe, request, settings)
                                .await;
                            if let Ok(mut opened) = opened.lock() {
                                opened.push(Opened {
                                    yours,
                                    held_back: outcome.as_ref().is_ok_and(|o| o.held_back),
                                    status: outcome.as_ref().ok().map(|o| o.status),
                                    intervention: outcome
                                        .as_ref()
                                        .ok()
                                        .and_then(|o| o.intervention.as_ref())
                                        .map(|i| i.kind),
                                    model_calls: outcome
                                        .as_ref()
                                        .ok()
                                        .and_then(|o| o.measurements)
                                        .map(|m| m.planner_calls),
                                });
                            }
                            report(&outcome);
                            outcome
                        }
                    },
                    |_| {},
                )
                .await;
            done.store(true, Ordering::Relaxed);
            state
        };
        let (state, ()) = tokio::join!(run, self.person(work.0, answer, &done, &asked));
        let state = state.map_err(|_| "run")?;
        Ok(Run {
            state,
            notices: script.notices.lock().map(|n| n.clone()).unwrap_or_default(),
            opened: opened.into_inner().unwrap_or_default(),
            asked: asked.into_inner().unwrap_or_default(),
        })
    }

    /// Points a contract request at its loopback site.
    fn local(
        &self,
        mut request: WorkAgentBrowseRequest,
    ) -> Result<(WorkAgentBrowseRequest, bool), WorkError> {
        let WorkStepKindV1::Read { url, .. } = &mut request.step else {
            return Err(WorkError::Invalid);
        };
        *url = self.sites.local(url);
        let yours = request.session != SiteSession::Private;
        Ok((request, yours))
    }
}

fn report(outcome: &Result<WorkBrowserOutcome, WorkError>) {
    match outcome {
        Ok(outcome) => {
            let _ = writeln!(
                std::io::stdout().lock(),
                "loopback-site: page status={:?} artifacts={} held_back={} note_bytes={} calls={:?}",
                outcome.status,
                outcome.artifacts.len(),
                outcome.held_back,
                outcome.note.as_ref().map_or(0, String::len),
                outcome.measurements.map(|m| m.planner_calls),
            );
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stdout().lock(),
                "loopback-site: page error={error:?}"
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
        let _ = writeln!(std::io::stdout().lock(), "loopback-site: {text}");
    };
    // Before any session the site opens as an empty session without a
    // question; the seed page signs the person in, in the profile's store.
    let seed = context
        .run(
            "Open the account site",
            false,
            "Not now",
            vec![vec![browse("/seed", "Open the welcome page and report it")]],
        )
        .await?;
    let seeded = context.sites.hits(Site::Account, "/seed").len() == 1;
    line(format!(
        "seed asked={} yours={} sites_seen={seeded}",
        seed.asked.len(),
        seed.opened.iter().all(|page| page.yours)
    ));
    if !seeded || !seed.asked.is_empty() {
        return Err("seed");
    }
    let mut failure = None;
    let mut last = seed.state;
    let goal = "Report what the page shows";
    for check in checks {
        let passed = match check {
            Check::Session => {
                let run = context
                    .run(
                        "Summarize my inbox",
                        false,
                        "Allow",
                        vec![vec![browse("/inbox", "Summarize my inbox")]],
                    )
                    .await?;
                let inbox = context.sites.hits(Site::Account, "/inbox");
                let cookie =
                    !inbox.is_empty() && inbox.iter().all(|(_, c)| c.contains(ACCOUNT_COOKIE));
                let content = says(&run, INBOX_FACT);
                let badge = run.state.executions[0]
                    .steps
                    .iter()
                    .any(|step| step.account.is_some());
                line(format!("check=session asked={} yours={} session_cookie={cookie} content={content} badge={badge}", run.asked.len(), run.opened.iter().all(|p| p.yours)));
                let passed = run.asked.len() == 1 && cookie && content && badge;
                last = run.state;
                passed
            }
            Check::Always => {
                let first = context
                    .run(
                        "Check my inbox",
                        false,
                        "Always for account.probe.test",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                let second = context
                    .run(
                        "Check my inbox again",
                        false,
                        "Not now",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                let standing = zephium_app::work_sites::standing(handle, profile)
                    .await
                    .unwrap_or_default();
                let kept = standing
                    .iter()
                    .any(|entry| entry.site == SITE && entry.access == WorkSiteAccessV1::Always);
                line(format!(
                    "check=always first_asked={} second_asked={} kept={kept} second_yours={}",
                    first.asked.len(),
                    second.asked.len(),
                    second.opened.iter().all(|p| p.yours)
                ));
                let passed = first.asked.len() == 1
                    && second.asked.is_empty()
                    && kept
                    && second.opened.iter().all(|p| p.yours);
                last = second.state;
                passed
            }
            Check::Never => {
                zephium_app::work_sites::set_standing(
                    handle,
                    profile,
                    SITE.into(),
                    Some(WorkSiteAccessV1::Never),
                )
                .await
                .map_err(|_| "never_standing")?;
                let before = context.sites.hits(Site::Account, "/inbox").len();
                let never = context
                    .run(
                        "Inbox, never",
                        false,
                        "Allow",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                zephium_app::work_sites::set_standing(handle, profile, SITE.into(), None)
                    .await
                    .map_err(|_| "never_standing")?;
                let private = context
                    .run(
                        "Inbox, privately",
                        true,
                        "Allow",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                let inbox = context.sites.hits(Site::Account, "/inbox");
                let cookies = inbox[before..]
                    .iter()
                    .filter(|(_, c)| !c.is_empty())
                    .count();
                line(format!(
                    "check=never never_asked={} private_asked={} requests={} with_cookie={cookies}",
                    never.asked.len(),
                    private.asked.len(),
                    inbox.len() - before
                ));
                let passed = never.asked.is_empty()
                    && private.asked.is_empty()
                    && never.opened.iter().chain(&private.opened).all(|p| !p.yours)
                    && cookies == 0;
                last = private.state;
                passed
            }
            Check::Route => {
                let run = context
                    .run(
                        "Reports",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/app",
                            "Open the Reports view and report its total",
                        )]],
                    )
                    .await?;
                let loads = context.sites.hits(Site::Account, "/app/reports").len();
                let content = says(&run, REPORT_FACT);
                line(format!(
                    "check=route content={content} route_loads={loads} status={:?}",
                    run.opened.first().and_then(|p| p.status)
                ));
                last = run.state;
                content && loads == 0
            }
            Check::Redirect => {
                let run = context
                    .run(
                        "Landing",
                        false,
                        "Allow",
                        vec![vec![browse("/start", goal)]],
                    )
                    .await?;
                let landed = context.sites.hits(Site::Account, "/landed");
                let content = says(&run, LANDED_FACT);
                line(format!(
                    "check=redirect content={content} landed_requests={} landed_cookie={}",
                    landed.len(),
                    landed.iter().all(|(_, c)| c.contains(ACCOUNT_COOKIE))
                ));
                last = run.state;
                content && !landed.is_empty()
            }
            Check::Leave => {
                let run = context
                    .run("Leave", false, "Allow", vec![vec![browse("/leave", goal)]])
                    .await?;
                let landing = context.sites.hits(Site::Other, "/landing");
                let status = run.opened.first().and_then(|p| p.status);
                line(format!(
                    "check=leave status={status:?} landing_requests={} landing_cookie={}",
                    landing.len(),
                    landing.iter().any(|(_, c)| !c.is_empty())
                ));
                last = run.state;
                status != Some(WorkStepStatus::Succeeded)
                    && landing.iter().all(|(_, c)| c.is_empty())
            }
            Check::Autopost => {
                let before = context.sites.posts();
                let run = context
                    .run(
                        "Drafts",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/autopost",
                            "Wait a moment, then report the drafts count",
                        )]],
                    )
                    .await?;
                let posts = context.sites.posts() - before;
                let content = says(&run, AUTOPOST_FACT);
                line(format!("check=autopost content={content} posts={posts}"));
                last = run.state;
                content && posts == 0
            }
            Check::Held => {
                let before = context.sites.posts();
                let run = context
                    .run(
                        "Send",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/compose",
                            "Write 'On my way' in the message box, then press Send",
                        )]],
                    )
                    .await?;
                let posts = context.sites.posts() - before;
                let held = run.opened.iter().any(|p| p.held_back);
                let noticed = run
                    .notices
                    .iter()
                    .any(|n| n.starts_with("A browse stopped before a step"));
                line(format!(
                    "check=held held={held} noticed={noticed} posts={posts}"
                ));
                last = run.state;
                // The page agent may stop before Send on its own; if it tried,
                // the gate held it and the lead heard. Nothing is ever sent.
                held == noticed && posts == 0
            }
            Check::Wall => {
                let run = context
                    .run(
                        "Private inbox",
                        true,
                        "Allow",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                let page = run.opened.first().cloned();
                line(format!(
                    "check=wall intervention={:?} model_calls={:?} status={:?}",
                    page.as_ref().and_then(|p| p.intervention),
                    page.as_ref().and_then(|p| p.model_calls),
                    page.as_ref().and_then(|p| p.status)
                ));
                last = run.state;
                // The first view is bounded by landmarks: the page agent (or
                // Rust, on its first fuller view) stops within two calls.
                page.is_some_and(|p| {
                    p.intervention == Some(WorkInterventionKindV1::SignIn)
                        && p.model_calls.is_some_and(|calls| calls <= 2)
                })
            }
        };
        line(format!("check={check:?} passed={passed}"));
        if !passed {
            failure.get_or_insert("loopback_check");
        }
    }
    Ok(WorkflowResult {
        state: last,
        failure,
    })
}
