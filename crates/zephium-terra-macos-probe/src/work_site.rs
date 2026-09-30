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
const THIRD: &str = "https://third.probe.test";
const BOARD: &str = "https://board.probe.test";
const SITE: &str = "account.probe.test";
const OTHER_SITE: &str = "other.probe.test";
const THIRD_SITE: &str = "third.probe.test";
const BOARD_SITE: &str = "board.probe.test";
const ACCOUNT_COOKIE: &str = "zaccount=signed-in-a";
const INBOX_FACT: &str = "Quarterly report from Dana";
const REPORT_FACT: &str = "Reports total is 4,812";
const LANDED_FACT: &str = "Landing desk opens at nine";
const AUTOPOST_FACT: &str = "Autopost page lists three drafts";
const SEARCH_FACT: &str = "Brass lantern stock is 42 units";
const FILTER_FACT: &str = "Open orders number 7";
const VAULT_FACT: &str = "Vault balance is 318 credits";
const HOME_FACT: &str = "Front page lists five offers";
const CHURN_FACT: &str = "Churn list holds 12 items";
const CONSENT_FACT: &str = "Fares list shows 4 flights";
const DESIGN_FACT: &str = "Mira shipped the new onboarding flow";
const AGENDA_FACT: &str = "Design review at 14:30";
const BOARD_FACT: &str = "Ticket ZP-42 is due Thursday";
const DAY_FACT: &str = "Standup at 09:15";
/// The vault's own sign-in, kept by the server: a tab sign-in stands for it.
static VAULT_OPEN: AtomicBool = AtomicBool::new(false);
/// Page loads the person's tabs made on the site, as the engine would count them.
static TAB_LOADS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    /// A search form and a script filter load their results without asking.
    Search,
    /// An approved send runs once, exactly as previewed.
    Approve,
    /// A total that changes after approval spends the approval.
    Changed,
    /// A declined booking commits nothing.
    Decline,
    /// An autosaving editor asks once; the run-wide allowance covers the rest.
    Autosave,
    /// A signed-out start page is worked on without the entry question.
    SignedOut,
    /// A sign-in finished in a tab wakes the held page, which starts over once.
    TabSignIn,
    /// A page that reloads itself after a click neither fails its read nor
    /// holds the pages after it, in its run or the next.
    Churn,
    /// A page whose script stalls the page after a click is lost alone: the
    /// next page of its run and the next run's page both read.
    Freeze,
    /// A cookie banner that saves the refusal with a form POST, or with a
    /// script and a reload, is refused by Rust and the page reads.
    Consent,
    /// A single-page app whose sidebar re-renders its rows on every click,
    /// like Slack: opening a channel reads without a refusal loop.
    Spa,
    /// A heavy calendar whose script holds the page for seconds after load:
    /// its first look waits for it and the day reads.
    Heavy,
    /// Three signed-in apps read by three parts of one lead run: one entry
    /// question names all three and every page opens in the session.
    Apps,
    /// Six daily apps' main views (Slack, Gmail, Calendar, Linear, Notion,
    /// GitHub) read as records with no model call.
    Views,
}
/// Freeze leaves a lost page's debt, which the probe's exit reports as an
/// unclean shutdown; it runs on its own.
const ALL: [Check; 22] = [
    Check::Session,
    Check::Always,
    Check::Never,
    Check::Route,
    Check::Redirect,
    Check::Leave,
    Check::Autopost,
    Check::Held,
    Check::Wall,
    Check::Search,
    Check::Approve,
    Check::Changed,
    Check::Decline,
    Check::Autosave,
    Check::SignedOut,
    Check::TabSignIn,
    Check::Churn,
    Check::Consent,
    Check::Spa,
    Check::Heavy,
    Check::Apps,
    Check::Views,
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
        Some("search") => vec![Check::Search],
        Some("approve") => vec![Check::Approve],
        Some("changed") => vec![Check::Changed],
        Some("decline") => vec![Check::Decline],
        Some("autosave") => vec![Check::Autosave],
        Some("signedout") => vec![Check::SignedOut],
        Some("tabsignin") => vec![Check::TabSignIn],
        Some("churn") => vec![Check::Churn],
        Some("freeze") => vec![Check::Freeze],
        Some("consent") => vec![Check::Consent],
        Some("spa") => vec![Check::Spa],
        Some("heavy") => vec![Check::Heavy],
        Some("apps") => vec![Check::Apps],
        Some("views") => vec![Check::Views],
        _ => return Err(super::ProbeFailure::Authority),
    };
    let _ = CHECKS.set(checks);
    super::work_durable::run_loopback_site()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Site {
    Account,
    Other,
    Third,
    Board,
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
                        SITE | THIRD_SITE | BOARD_SITE => "127.0.0.1",
                        OTHER_SITE => "localhost",
                        _ => "fresh.probe.invalid",
                    }
                    .to_owned()
                })
                .collect();
            engine.work_sessions_present(profile, hosts)
        },
    ));
    zephium_app::work_context::install_site_loads(std::sync::Arc::new(|_, _| {
        let (reply, answer) = std::sync::mpsc::channel();
        let _ = reply.send(TAB_LOADS.load(Ordering::SeqCst));
        answer
    }));
}

/// Four HTTP/1.1 loopback servers that record method, path and cookie
/// only. The third and board sites share the account site's host, and so
/// its cookie.
struct Sites {
    account: u16,
    other: u16,
    third: u16,
    board: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    stop: Arc<AtomicBool>,
}
impl Sites {
    fn start() -> Result<Self, &'static str> {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let account = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "loopback_bind")?;
        let other = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "loopback_bind")?;
        let third = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "loopback_bind")?;
        let board = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "loopback_bind")?;
        let board_port = board.local_addr().map_err(|_| "loopback_addr")?.port();
        let ports = (
            account.local_addr().map_err(|_| "loopback_addr")?.port(),
            other.local_addr().map_err(|_| "loopback_addr")?.port(),
        );
        let third_port = third.local_addr().map_err(|_| "loopback_addr")?.port();
        for (listener, site) in [
            (account, Site::Account),
            (other, Site::Other),
            (third, Site::Third),
            (board, Site::Board),
        ] {
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
            third: third_port,
            board: board_port,
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
        } else if let Some(path) = url.strip_prefix(THIRD) {
            format!("http://127.0.0.1:{}{path}", self.third)
        } else if let Some(path) = url.strip_prefix(BOARD) {
            format!("http://127.0.0.1:{}{path}", self.board)
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
    let consented = |name: &str| cookie.contains(&format!("{name}=no"));
    let banner = |form: bool| {
        let buttons = if form {
            "<form method=\"post\" action=\"/consent/save\"><button name=\"set\" value=\"reject\">Reject all</button><button name=\"set\" value=\"accept\">Accept all</button></form>"
        } else {
            "<button id=\"r\" type=\"button\">Reject all</button><button id=\"a\" type=\"button\">Accept all</button><script>document.getElementById('r').addEventListener('click',function(){this.disabled=true;fetch('/consent/jsave',{method:'POST'}).then(function(){location.reload();});});</script>"
        };
        format!("<div role=\"dialog\" aria-modal=\"true\" aria-label=\"Before you continue\"><h2>Before you continue</h2><p>We use cookies and data to deliver and maintain our services.</p>{buttons}</div><main><p>Loading fares</p></main>")
    };
    let (status, headers, body) = match (site, method.as_str(), path.as_str()) {
        (Site::Account, "POST", "/consent/save") => (
            "303 See Other",
            "Set-Cookie: zconsent=no; Path=/; Max-Age=3600\r\nLocation: /gconsent\r\n".into(),
            String::new(),
        ),
        (Site::Account, "POST", "/consent/jsave") => (
            "200 OK",
            "Set-Cookie: zjconsent=no; Path=/; Max-Age=3600\r\n".into(),
            String::new(),
        ),
        (_, "POST", _) => ("200 OK", String::new(), page("Done", "<p>Saved.</p>")),
        (Site::Account, _, "/gconsent") if consented("zconsent") => (
            "200 OK",
            String::new(),
            page("Fares", &format!("<h1>Fares</h1><p>{CONSENT_FACT}.</p>")),
        ),
        (Site::Account, _, "/jconsent") if consented("zjconsent") => (
            "200 OK",
            String::new(),
            page("Fares", &format!("<h1>Fares</h1><p>{CONSENT_FACT}.</p>")),
        ),
        (Site::Account, _, "/gconsent") => ("200 OK", String::new(), banner(true)),
        (Site::Account, _, "/jconsent") => ("200 OK", String::new(), banner(false)),
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
                "<h1>Message #design</h1><label>Message <input id=\"m\" name=\"message\"></label><button id=\"s\" type=\"button\">Send</button><div id=\"log\" role=\"status\"></div><script>document.getElementById('s').addEventListener('click',function(){var m=document.getElementById('m');fetch('/send',{method:'POST',body:m.value});document.getElementById('log').textContent='Message sent: '+m.value;m.value='';});</script>",
            ),
        ),
        (Site::Account, _, path) if path.starts_with("/book") => {
            // /book/live raises its total every second, so a later look differs.
            let live = path.starts_with("/book/live");
            (
                "200 OK",
                String::new(),
                page(
                    "Cabin",
                    &format!("<h1>Cabin by the lake</h1><form method=\"post\" action=\"/book/confirm\" aria-label=\"Reserve\"><p>Dates: 3-5 May</p><p>Guests: 2</p><p>Total: <span id=\"t\">$1,240</span></p><button>Request to book</button></form>{}",
                        if live { "<script>var n=1240,s=Date.now();setInterval(function(){document.getElementById('t').textContent='$'+(n+Math.floor((Date.now()-s)/1000)*10).toLocaleString('en-US');},250);</script>" } else { "" }),
                ),
            )
        }
        (Site::Account, _, "/notes") => (
            "200 OK",
            String::new(),
            page(
                "Notes",
                "<h1>Meeting notes</h1><div id=\"n\" contenteditable=\"true\" role=\"textbox\" aria-label=\"Notes\" aria-multiline=\"true\" style=\"min-height:120px;border:1px solid #999\"></div><script>var t;document.getElementById('n').addEventListener('input',function(){clearTimeout(t);t=setTimeout(function(){fetch('/save',{method:'POST',body:document.getElementById('n').textContent});},300);});</script>",
            ),
        ),
        (Site::Account, _, "/home") => (
            "200 OK",
            String::new(),
            page(
                "Home",
                &format!("<header><nav><a href=\"/login\">Sign in</a></nav></header><h1>Welcome</h1><p>{HOME_FACT}.</p>"),
            ),
        ),
        (Site::Account, _, "/churn") => (
            "200 OK",
            String::new(),
            page(
                "Churn",
                &format!("<h1>List</h1><button id=\"c\" type=\"button\">Continue</button><p id=\"p\">Press Continue to load the list.</p><script>var n=+(sessionStorage.getItem('churn')||0);if(n>0&&n<7){{document.getElementById('p').textContent='Loading';sessionStorage.setItem('churn',n+1);setTimeout(function(){{location.reload();}},350);}}else if(n>=7){{document.getElementById('p').textContent='{CHURN_FACT}.';}}document.getElementById('c').addEventListener('click',function(){{sessionStorage.setItem('churn',1);setTimeout(function(){{location.reload();}},120);}});</script>"),
            ),
        ),
        (Site::Account, _, "/freeze") => (
            "200 OK",
            String::new(),
            page(
                "Freeze",
                "<h1>Report</h1><button id=\"l\" type=\"button\">Load</button><p id=\"p\">Press Load to fill the report.</p><script>document.getElementById('l').addEventListener('click',function(){document.getElementById('p').textContent='Loading the report';setTimeout(function(){var t=Date.now();while(Date.now()-t<40000){}},300);});</script>",
            ),
        ),
        (Site::Account, _, "/vault-login") => {
            VAULT_OPEN.store(true, Ordering::SeqCst);
            ("200 OK", String::new(), page("Signed in", "<p>Signed in.</p>"))
        }
        (Site::Account, _, "/vault") if !VAULT_OPEN.load(Ordering::SeqCst) => (
            "200 OK",
            String::new(),
            page("Sign in", "<h1>Sign in to your vault</h1><form method=\"post\" action=\"/login\"><label>Email <input type=\"email\" name=\"email\" autocomplete=\"email\"></label><label>Password <input type=\"password\" name=\"password\"></label><button>Sign in</button></form>"),
        ),
        (Site::Account, _, "/vault") => (
            "200 OK",
            String::new(),
            page("Vault", &format!("<h1>Vault</h1><p>{VAULT_FACT}.</p>")),
        ),
        (Site::Account, _, "/find") => (
            "200 OK",
            String::new(),
            page(
                "Catalog",
                "<h1>Catalog</h1><form role=\"search\" action=\"/results\" method=\"get\"><label>Search the catalog <input type=\"search\" name=\"q\"></label><button>Search</button></form>",
            ),
        ),
        (Site::Account, _, "/filters") => (
            "200 OK",
            String::new(),
            page(
                "Orders",
                "<h1>Orders</h1><button id=\"o\" type=\"button\">Show open orders</button><script>document.getElementById('o').addEventListener('click',function(){location.assign('/results?status=open');});</script>",
            ),
        ),
        (Site::Account, _, path) if path.starts_with("/results") && signed_in => {
            let found = if path.contains("lantern") {
                format!("<p>{SEARCH_FACT}.</p>")
            } else if path.contains("status=open") {
                format!("<p>{FILTER_FACT}.</p>")
            } else {
                "<p>No matches.</p>".to_owned()
            };
            ("200 OK", String::new(), page("Results", &format!("<h1>Results</h1>{found}")))
        }
        (Site::Account, _, _) if !signed_in => (
            "200 OK",
            String::new(),
            page("Sign in", "<h1>Sign in to your account</h1><form method=\"post\" action=\"/login\"><label>Email <input type=\"email\" name=\"email\" autocomplete=\"email\"></label><label>Password <input type=\"password\" name=\"password\"></label><button>Sign in</button></form>"),
        ),
        (Site::Account, _, path) if super::work_app_views::view(path).is_some() => (
            "200 OK",
            String::new(),
            super::work_app_views::view(path)
                .map(|view| view.html.to_owned())
                .unwrap_or_default(),
        ),
        (Site::Account, _, "/inbox") => (
            "200 OK",
            String::new(),
            page("Inbox", &format!("<h1>Inbox</h1><ul><li>{INBOX_FACT} is due Friday.</li><li>Team lunch moved to Thursday.</li></ul>")),
        ),
        // Slack-like: every click re-renders the sidebar's rows and replaces
        // the message pane, and the address follows the channel.
        (Site::Account, _, path) if path == "/spa" || path.starts_with("/spa/") => (
            "200 OK",
            String::new(),
            format!("<!doctype html><html><head><title>Workspace</title></head><body><nav aria-label=\"Channels\"><div role=\"tree\" id=\"side\"></div></nav><main id=\"pane\"><h1>#general</h1><ul><li>Ola: coffee at ten?</li></ul></main><script>var chans=['general','design','random'];var msgs={{general:['Ola: coffee at ten?'],design:['Tom: can someone review the icons?','Ana: {DESIGN_FACT}.'],random:['Leo: new plant on the desk']}};function side(active){{var h='';for(var i=0;i<chans.length;i++){{var c=chans[i];h+='<div role=\"treeitem\" tabindex=\"0\" aria-selected=\"'+(c===active)+'\" data-c=\"'+c+'\">#'+c+'</div>';}}var el=document.getElementById('side');el.innerHTML=h;var rows=el.querySelectorAll('[data-c]');for(var j=0;j<rows.length;j++){{rows[j].addEventListener('click',function(){{var c=this.getAttribute('data-c');setTimeout(function(){{open(c);}},150);}});}}}}function open(c){{history.pushState({{}},'','/spa/'+c);document.title='#'+c+' - Workspace';side(c);var h='<h1>#'+c+'</h1><ul>';for(var k=0;k<msgs[c].length;k++){{h+='<li>'+msgs[c][k]+'</li>';}}document.getElementById('pane').innerHTML=h+'</ul>';}}side('general');</script></body></html>"),
        ),
        // A heavy day view: a large grid, and a script that holds the page
        // for eight seconds right after load before it draws the day.
        (Site::Account, _, "/agenda") => {
            let mut cells = String::new();
            for hour in 0..24 {
                for slot in 0..120 {
                    cells.push_str(&format!("<div role=\"gridcell\" data-h=\"{hour}\" data-s=\"{slot}\"><span></span></div>"));
                }
            }
            (
                "200 OK",
                String::new(),
                format!("<!doctype html><html><head><title>Calendar</title></head><body><main><h1>Today</h1><ul id=\"day\"><li>Loading your day</li></ul><div role=\"grid\" aria-label=\"Week\" style=\"height:4000px;overflow:hidden\">{cells}</div></main><script>window.addEventListener('load',function(){{setTimeout(function(){{var t=Date.now();while(Date.now()-t<8000){{}}document.getElementById('day').innerHTML='<li>{DAY_FACT}</li><li>{AGENDA_FACT}</li><li>Gym at 18:00</li>';}},0);}});</script></body></html>"),
            )
        }
        (Site::Account, _, path) => (
            "200 OK",
            String::new(),
            page("Account page", &format!("<h1>Account page</h1><p>This signed-in page is {}.</p>", path.trim_start_matches('/'))),
        ),
        (Site::Third | Site::Board, _, _) if !signed_in => (
            "200 OK",
            String::new(),
            page("Sign in", "<h1>Sign in</h1><form method=\"post\" action=\"/login\"><label>Email <input type=\"email\" name=\"email\"></label><label>Password <input type=\"password\" name=\"password\"></label><button>Sign in</button></form>"),
        ),
        (Site::Board, _, _) => (
            "200 OK",
            String::new(),
            page("Board", &format!("<h1>My issues</h1><ul><li>{BOARD_FACT}.</li><li>ZP-17 is in review.</li></ul>")),
        ),
        (Site::Third, _, _) => (
            "200 OK",
            String::new(),
            page("Calendar", &format!("<h1>Today</h1><ul><li>{DAY_FACT}.</li><li>Lunch with Ana at 12:30.</li></ul>")),
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
    browse_at(ACCOUNT, path, goal)
}

fn browse_at(site: &str, path: &str, goal: &str) -> WorkAgentFetch {
    WorkAgentFetch::Browse {
        start: format!("{site}{path}"),
        goal: goal.into(),
        collection: None,
    }
}

/// One page as the probe opened it.
#[derive(Clone)]
struct Opened {
    yours: bool,
    not_ready: bool,
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
    /// Headlines of the held steps the probe decided, in order.
    confirmed: Vec<String>,
}

struct Context<'a> {
    handle: &'a zephium_app::Handle,
    composition: &'a MacosWorkComposition,
    profile: ProfileId,
    binding: zephium_app::AgentWorkProfileBinding,
    keys: Mutex<Vec<zephium_agentic::AgentProviderCredential>>,
    sites: Sites,
    /// Sign-in walls are passed in a tab rather than released.
    tab_sign_in: AtomicBool,
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
        decisions: &Mutex<Vec<(bool, bool)>>,
        confirmed: &Mutex<Vec<String>>,
    ) {
        while !done.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(pages) = self.composition.human_pages(self.profile, work) {
                for page in pages {
                    // The person signs in in a tab instead of the pane.
                    if self.tab_sign_in.load(Ordering::Relaxed)
                        && page.reason == WorkHumanReasonV1::SignIn
                        && page.phase == WorkHumanPhaseV1::WaitingForHuman
                    {
                        if !VAULT_OPEN.load(Ordering::SeqCst) {
                            let _ = std::net::TcpStream::connect(("127.0.0.1", self.sites.account))
                                .and_then(|mut stream| {
                                    write!(stream, "GET /vault-login HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
                                    let mut sink = Vec::new();
                                    stream.read_to_end(&mut sink).map(|_| ())
                                });
                            TAB_LOADS.fetch_add(1, Ordering::SeqCst);
                        }
                        continue;
                    }
                    // A held step is decided on its Confirm step, not in the pane.
                    if page.phase == WorkHumanPhaseV1::WaitingForHuman
                        && page.reason != WorkHumanReasonV1::UserDecision
                    {
                        let _ = self
                            .composition
                            .release_human_page(self.profile, work, page.id);
                    }
                }
            }
            let Some(state) = self.projection(work).await else {
                continue;
            };
            let held = state.executions.last().and_then(|execution| {
                execution.steps.iter().find_map(|step| match &step.kind {
                    WorkStepKindV1::Confirm { confirm: held }
                        if step.status == WorkStepStatus::Running && held.decision.is_none() =>
                    {
                        Some((execution.id, step.id, held.headline.clone()))
                    }
                    _ => None,
                })
            });
            if let Some((execution, step, headline)) = held {
                let (approve, for_run) = decisions
                    .lock()
                    .ok()
                    .and_then(|mut queue| (!queue.is_empty()).then(|| queue.remove(0)))
                    .unwrap_or((false, false));
                let applied = self
                    .handle
                    .work_command(
                        self.profile,
                        WorkCommandV1 {
                            version: 1,
                            work,
                            expected_revision: state.work.revision,
                            command: WorkCommandId::generate(),
                            intent: WorkRuntimeIntent::ApproveStep {
                                execution,
                                step,
                                approve,
                                for_run,
                            },
                        },
                    )
                    .ok();
                if let Some(applied) = applied {
                    if applied.await.is_ok() {
                        if let Ok(mut confirmed) = confirmed.lock() {
                            confirmed.push(headline);
                        }
                    }
                }
                continue;
            }
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
        self.run_deciding(objective, private, answer, turns, vec![])
            .await
    }

    /// One run whose held steps the probe decides in order, then declines.
    async fn run_deciding(
        &self,
        objective: &str,
        private: bool,
        answer: &str,
        turns: Vec<Vec<WorkAgentFetch>>,
        decisions: Vec<(bool, bool)>,
    ) -> Result<Run, &'static str> {
        let decisions = Mutex::new(decisions);
        let confirmed = Mutex::new(Vec::new());
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
                                lead: None,
                                provider: WorkSearchProvider::OpenAi,
                                model: PUBLIC_SEARCH_MODEL.into(),
                                max_turns: 8,
                                max_steps: 40,
                                browse_hops: 1,
                                folders: vec![],
                                accounts: vec![],
                                private,
                                skill: None,
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
                                    not_ready: outcome.as_ref().is_ok_and(|o| {
                                        o.note.as_deref()
                                            == Some("The browser was not ready for this page")
                                    }),
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
        let (state, ()) = tokio::join!(
            run,
            self.person(work.0, answer, &done, &asked, &decisions, &confirmed)
        );
        let state = state.map_err(|_| "run")?;
        Ok(Run {
            state,
            notices: script.notices.lock().map(|n| n.clone()).unwrap_or_default(),
            opened: opened.into_inner().unwrap_or_default(),
            asked: asked.into_inner().unwrap_or_default(),
            confirmed: confirmed.into_inner().unwrap_or_default(),
        })
    }

    /// One lead run whose scripted lead starts one browser part per start
    /// page; only the page agents call a model. The probe answers with `answer`.
    async fn lead_run(
        &self,
        objective: &str,
        starts: &'static str,
        answer: &str,
    ) -> Result<Run, &'static str> {
        use zephium_app::work_lead::{LeadModel, WorkLeadModels, WorkLeadService};
        let entry = zephium_app::work_models::resolve_entry(
            self.profile,
            zephium_core::work::model::WorkModelRole::Lead,
        )
        .await
        .map_err(|_| "lead_model")?
        .0;
        let scripted = LeadModel {
            entry,
            client: Arc::new(super::acceptance::Scripted { start: starts }),
        };
        let models = WorkLeadModels {
            lead: scripted.clone(),
            page: scripted.clone(),
            light: scripted,
        };
        let work = self.create(objective).await?;
        let started = std::time::Instant::now();
        let opened = Mutex::new(Vec::<Opened>::new());
        let asked = Mutex::new(Vec::new());
        let decisions = Mutex::new(Vec::new());
        let confirmed = Mutex::new(Vec::new());
        let done = AtomicBool::new(false);
        let callback = self.handle.callback_handle();
        let service = WorkLeadService::new(self.handle.clone()).with_diagnostic(|event| {
            let _ = writeln!(std::io::stdout().lock(), "loopback-site: lead={event:?}");
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
                                lead: None,
                                provider: WorkSearchProvider::OpenAi,
                                model: PUBLIC_SEARCH_MODEL.into(),
                                max_turns: 10,
                                max_steps: 32,
                                browse_hops: 4,
                                folders: vec![],
                                accounts: vec![],
                                private: false,
                                skill: None,
                            },
                            limits: WorkExecutionLimits {
                                model_tokens: 1_000_000,
                                cost_micro_usd: 3_000_000,
                                operations: 256,
                                timeout_seconds: 1_200,
                                max_workers: 4,
                            },
                        },
                    },
                    None,
                    models,
                    &NoSearch,
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
                            settings.retain_public_responses = false;
                            settings.loopback_anonymous = true;
                            let outcome = self
                                .composition
                                .run_agent_step(callback, &probe, request, settings)
                                .await;
                            if let Ok(mut opened) = opened.lock() {
                                opened.push(Opened {
                                    yours,
                                    not_ready: false,
                                    held_back: outcome.as_ref().is_ok_and(|o| o.held_back),
                                    status: outcome.as_ref().ok().map(|o| o.status),
                                    intervention: None,
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
        let (state, ()) = tokio::join!(
            run,
            self.person(work.0, answer, &done, &asked, &decisions, &confirmed)
        );
        let state = state.map_err(|_| "lead_run")?;
        if let Some(execution) = state.executions.last() {
            super::acceptance::run_row("apps", 1, execution, started.elapsed().as_millis());
            super::acceptance::part_rows(execution);
        }
        Ok(Run {
            state,
            notices: Vec::new(),
            opened: opened.into_inner().unwrap_or_default(),
            asked: asked.into_inner().unwrap_or_default(),
            confirmed: confirmed.into_inner().unwrap_or_default(),
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
                "loopback-site: page status={:?} artifacts={} held_back={} rerun={} usage={} note={:?} calls={:?} tokens={:?} wall_ms={:?}",
                outcome.status,
                outcome.artifacts.len(),
                outcome.held_back,
                outcome.rerun,
                outcome.usage.is_some(),
                outcome.note,
                outcome.measurements.map(|m| m.planner_calls),
                outcome.measurements.map(|m| m.model_tokens),
                outcome.measurements.map(|m| m.wall_millis),
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

/// Each page's status and model calls, in the order they settled.
fn pages_of(run: &Run) -> String {
    run.opened
        .iter()
        .map(|page| format!("{:?}:{}", page.status, page.model_calls.unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(",")
}

fn says(run: &Run, text: &str) -> bool {
    run.state.executions.iter().any(|execution| {
        execution
            .artifacts
            .iter()
            .any(|artifact| artifact.data.plain_text().contains(text))
    })
}

/// Each Confirm step's decision and status, in order.
fn confirms(run: &Run) -> Vec<(Option<WorkConfirmDecisionV1>, WorkStepStatus)> {
    run.state
        .executions
        .iter()
        .flat_map(|execution| &execution.steps)
        .filter_map(|step| match &step.kind {
            WorkStepKindV1::Confirm { confirm: held } => Some((held.decision, step.status)),
            _ => None,
        })
        .collect()
}

/// Every word appears in one artifact, for facts a page task may rephrase.
fn mentions(run: &Run, words: &[&str]) -> bool {
    run.state.executions.iter().any(|execution| {
        execution.artifacts.iter().any(|artifact| {
            let text = artifact.data.plain_text().to_lowercase();
            words.iter().all(|word| text.contains(word))
        })
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
        tab_sign_in: AtomicBool::new(false),
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
            Check::Search => {
                let run = context
                    .run(
                        "Catalog",
                        false,
                        "Allow",
                        vec![vec![
                            browse(
                                "/find",
                                "Search the catalog for lantern and report the stock",
                            ),
                            browse(
                                "/filters",
                                "Show the open orders and report how many there are",
                            ),
                        ]],
                    )
                    .await?;
                let queried = context.sites.hits(Site::Account, "/results?q=");
                let filtered = context.sites.hits(Site::Account, "/results?status=open");
                let found = says(&run, SEARCH_FACT);
                let filters = says(&run, FILTER_FACT) || mentions(&run, &["open orders", "7"]);
                let held = run.opened.iter().any(|p| p.held_back);
                line(format!(
                    "check=search asked={} queried={} filtered={} found={found} filters={filters} held={held} cookie={}",
                    run.asked.len(),
                    queried.len(),
                    filtered.len(),
                    queried.iter().chain(&filtered).all(|(_, c)| c.contains(ACCOUNT_COOKIE)),
                ));
                last = run.state;
                run.asked.len() <= 1 && found && filters && !held && !queried.is_empty()
            }
            Check::Approve => {
                let before = context.sites.hits(Site::Account, "/send").len();
                let run = context
                    .run_deciding(
                        "Tell #design I'm on my way",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/compose",
                            "Write 'On my way' in the message box and send it",
                        )]],
                        vec![(true, false)],
                    )
                    .await?;
                let sent = context.sites.hits(Site::Account, "/send").len() - before;
                let held = confirms(&run);
                line(format!(
                    "check=approve sent={sent} confirms={held:?} headline={:?}",
                    run.confirmed.first()
                ));
                last = run.state;
                sent == 1
                    && held.len() == 1
                    && held[0]
                        == (
                            Some(WorkConfirmDecisionV1::Approved),
                            WorkStepStatus::Succeeded,
                        )
                    && run.confirmed.first().is_some_and(|h| h.contains("#design"))
            }
            Check::Changed => {
                let before = context.sites.posts();
                let run = context
                    .run_deciding(
                        "Book the cabin",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/book/live",
                            "Request to book the cabin for these dates",
                        )]],
                        vec![(true, false)],
                    )
                    .await?;
                let posts = context.sites.posts() - before;
                let held = confirms(&run);
                line(format!(
                    "check=changed posts={posts} confirms={held:?} headlines={:?}",
                    run.confirmed
                ));
                last = run.state;
                // The approved total moved on before the step ran: nothing is
                // posted; a new total would be put to the person again.
                posts == 0
                    && held.first()
                        == Some(&(
                            Some(WorkConfirmDecisionV1::Approved),
                            WorkStepStatus::Failed,
                        ))
            }
            Check::Decline => {
                let before = context.sites.posts();
                let run = context
                    .run_deciding(
                        "Book the cabin",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/book",
                            "Request to book the cabin for these dates",
                        )]],
                        vec![(false, false)],
                    )
                    .await?;
                let posts = context.sites.posts() - before;
                let held = confirms(&run);
                line(format!(
                    "check=decline posts={posts} confirms={held:?} headlines={:?}",
                    run.confirmed
                ));
                last = run.state;
                posts == 0
                    && held.first()
                        == Some(&(
                            Some(WorkConfirmDecisionV1::Declined),
                            WorkStepStatus::Cancelled,
                        ))
                    && held.len() == 1
            }
            Check::Autosave => {
                let before = context.sites.hits(Site::Account, "/save").len();
                let run = context
                    .run_deciding(
                        "Meeting notes",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/notes",
                            "Type 'Agenda: launch' into the notes, then replace it with 'Agenda: launch. Owner: Dana'",
                        )]],
                        vec![(true, true)],
                    )
                    .await?;
                let saves = context.sites.hits(Site::Account, "/save").len() - before;
                let held = confirms(&run);
                line(format!("check=autosave saves={saves} confirms={held:?}"));
                last = run.state;
                saves >= 1
                    && held.len() == 1
                    && held[0]
                        == (
                            Some(WorkConfirmDecisionV1::AllowedForRun),
                            WorkStepStatus::Succeeded,
                        )
            }
            Check::SignedOut => {
                let run = context
                    .run(
                        "Offers",
                        false,
                        "Not now",
                        vec![vec![browse(
                            "/home",
                            "Report how many offers the front page lists",
                        )]],
                    )
                    .await?;
                let content = says(&run, HOME_FACT) || mentions(&run, &["five offers"]);
                line(format!(
                    "check=signedout asked={} content={content} yours={}",
                    run.asked.len(),
                    run.opened.iter().all(|p| p.yours)
                ));
                last = run.state;
                run.asked.is_empty() && content
            }
            Check::TabSignIn => {
                VAULT_OPEN.store(false, Ordering::SeqCst);
                context.tab_sign_in.store(true, Ordering::Relaxed);
                let run = context
                    .run(
                        "Vault",
                        false,
                        "Allow",
                        vec![vec![browse("/vault", "Report the vault balance")]],
                    )
                    .await;
                context.tab_sign_in.store(false, Ordering::Relaxed);
                let run = run?;
                let loads = context.sites.hits(Site::Account, "/vault").len();
                let content = says(&run, VAULT_FACT) || mentions(&run, &["318"]);
                line(format!(
                    "check=tabsignin asked={} vault_loads={loads} content={content} pages={}",
                    run.asked.len(),
                    run.opened.len()
                ));
                last = run.state;
                content && loads >= 2 && run.asked.is_empty()
            }
            Check::Spa => {
                let run = context
                    .run(
                        "Read the design channel",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/spa",
                            "Open the #design channel and report its newest message",
                        )]],
                    )
                    .await?;
                let read = says(&run, DESIGN_FACT) || mentions(&run, &["onboarding"]);
                line(format!("check=spa read={read} pages=[{}]", pages_of(&run)));
                last = run.state;
                read
            }
            Check::Heavy => {
                let run = context
                    .run(
                        "My day",
                        false,
                        "Allow",
                        vec![vec![browse(
                            "/agenda",
                            "Report today's events with their times",
                        )]],
                    )
                    .await?;
                let read = says(&run, AGENDA_FACT) || mentions(&run, &["14:30"]);
                line(format!(
                    "check=heavy read={read} pages=[{}]",
                    pages_of(&run)
                ));
                last = run.state;
                read
            }
            Check::Apps => {
                for site in [SITE, BOARD_SITE, THIRD_SITE] {
                    zephium_app::work_sites::set_standing(handle, profile, site.into(), None)
                        .await
                        .map_err(|_| "apps_standing")?;
                }
                let before = (
                    context.sites.hits(Site::Account, "/inbox").len(),
                    context.sites.hits(Site::Board, "/board").len(),
                    context.sites.hits(Site::Third, "/day").len(),
                );
                let run = context
                    .lead_run(
                        "What do I need to do today?",
                        "https://account.probe.test/inbox https://board.probe.test/board https://third.probe.test/day",
                        "Allow",
                    )
                    .await?;
                let with = |site: Site, path: &str, skip: usize, cookie: &str| {
                    let hits = context.sites.hits(site, path);
                    hits.len() > skip && hits[skip..].iter().all(|(_, c)| c.contains(cookie))
                };
                let cookies = with(Site::Account, "/inbox", before.0, ACCOUNT_COOKIE)
                    && with(Site::Board, "/board", before.1, ACCOUNT_COOKIE)
                    && with(Site::Third, "/day", before.2, ACCOUNT_COOKIE);
                let named = run.asked.first().is_some_and(|question| {
                    [SITE, BOARD_SITE, THIRD_SITE]
                        .iter()
                        .all(|site| question.contains(site))
                });
                let read = (says(&run, INBOX_FACT) || mentions(&run, &["quarterly"]))
                    && (says(&run, BOARD_FACT) || mentions(&run, &["zp-42"]))
                    && (says(&run, DAY_FACT) || mentions(&run, &["09:15"]));
                line(format!(
                    "check=apps asked={} named={named} yours={} cookies={cookies} read={read} pages=[{}]",
                    run.asked.len(),
                    run.opened.iter().all(|p| p.yours),
                    pages_of(&run)
                ));
                last = run.state;
                run.asked.len() == 1 && named && cookies && read
            }
            Check::Views => {
                zephium_app::work_sites::set_standing(handle, profile, SITE.into(), None)
                    .await
                    .map_err(|_| "views_standing")?;
                let starts: &'static str = Box::leak(
                    super::work_app_views::VIEWS
                        .iter()
                        .map(|view| format!("{ACCOUNT}{}", view.path))
                        .collect::<Vec<_>>()
                        .join(" ")
                        .into_boxed_str(),
                );
                let run = context
                    .lead_run("What's new in my apps today?", starts, "Allow")
                    .await?;
                let read: Vec<bool> = super::work_app_views::VIEWS
                    .iter()
                    .map(|view| says(&run, view.fact))
                    .collect();
                let calls: u32 = run
                    .opened
                    .iter()
                    .filter_map(|p| p.model_calls)
                    .map(u32::from)
                    .sum();
                line(format!(
                    "check=views asked={} read={read:?} model_calls={calls} pages=[{}]",
                    run.asked.len(),
                    pages_of(&run)
                ));
                last = run.state;
                run.asked.len() == 1
                    && read.iter().all(|read| *read)
                    && run.opened.len() == super::work_app_views::VIEWS.len()
                    && calls == 0
            }
            Check::Churn => {
                let first = context
                    .run(
                        "Churn list",
                        false,
                        "Allow",
                        vec![
                            vec![browse(
                                "/churn",
                                "Press Continue, wait for the list, then report how many items it holds",
                            )],
                            vec![browse("/inbox", goal)],
                        ],
                    )
                    .await?;
                let second = context
                    .run(
                        "Inbox after",
                        false,
                        "Allow",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                let pages = |run: &Run| {
                    run.opened
                        .iter()
                        .map(|p| {
                            format!(
                                "{:?}{}",
                                p.status,
                                if p.not_ready { ":not_ready" } else { "" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let churned = says(&first, CHURN_FACT) || mentions(&first, &["12"]);
                line(format!(
                    "check=churn churned={churned} first=[{}] second=[{}] inbox={}",
                    pages(&first),
                    pages(&second),
                    says(&second, INBOX_FACT)
                ));
                let passed = first
                    .opened
                    .iter()
                    .chain(&second.opened)
                    .all(|p| !p.not_ready)
                    && says(&second, INBOX_FACT);
                last = second.state;
                passed
            }
            Check::Freeze => {
                let started = std::time::Instant::now();
                let first = context
                    .run(
                        "Frozen report",
                        false,
                        "Allow",
                        vec![
                            vec![browse(
                                "/freeze",
                                "Press Load, then report what the report says",
                            )],
                            vec![browse("/inbox", goal)],
                        ],
                    )
                    .await?;
                let first_ms = started.elapsed().as_millis();
                let started = std::time::Instant::now();
                let second = context
                    .run(
                        "Inbox after freeze",
                        false,
                        "Allow",
                        vec![vec![browse("/inbox", goal)]],
                    )
                    .await?;
                let pages = |run: &Run| {
                    run.opened
                        .iter()
                        .map(|p| {
                            format!(
                                "{:?}{}",
                                p.status,
                                if p.not_ready { ":not_ready" } else { "" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                };
                line(format!(
                    "check=freeze first=[{}] first_ms={first_ms} second=[{}] second_ms={} inbox={}",
                    pages(&first),
                    pages(&second),
                    started.elapsed().as_millis(),
                    says(&second, INBOX_FACT)
                ));
                let passed = first
                    .opened
                    .iter()
                    .chain(&second.opened)
                    .all(|p| !p.not_ready)
                    && says(&first, INBOX_FACT)
                    && says(&second, INBOX_FACT);
                last = second.state;
                passed
            }
            Check::Consent => {
                let mut passed = true;
                for path in ["/gconsent", "/jconsent"] {
                    let before = context.sites.posts();
                    let run = context
                        .run(
                            "Fares",
                            false,
                            "Allow",
                            vec![vec![browse(
                                path,
                                "Report how many flights the fares list shows",
                            )]],
                        )
                        .await?;
                    let saves = context.sites.hits(Site::Account, "/consent/").len();
                    let loads = context
                        .sites
                        .hits(Site::Account, path)
                        .iter()
                        .map(|(_, cookie)| {
                            (
                                cookie.contains(ACCOUNT_COOKIE),
                                cookie.contains("consent=no"),
                            )
                        })
                        .collect::<Vec<_>>();
                    line(format!("consent loads={loads:?}"));
                    let content = says(&run, CONSENT_FACT) || mentions(&run, &["4"]);
                    let held = run.opened.iter().any(|p| p.held_back);
                    line(format!(
                        "check=consent page={path} content={content} posts={} saves={saves} held={held} status={:?}",
                        context.sites.posts() - before,
                        run.opened.first().and_then(|p| p.status)
                    ));
                    passed &= content && !held;
                    last = run.state;
                }
                passed
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
