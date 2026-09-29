//! GitHub through the person's own `gh`: issues, pull requests, checks and
//! comments. Reading runs `gh … --json` and returns a compact digest; every
//! write (comment, pull request, merge) is shown in a Confirm first.
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use zephium_core::work::model::WorkModelTool;
use zephium_core::work::runtime::{WorkConfirmCategoryV1, WorkConfirmFactV1, WorkConfirmV1};

use super::{bounded, CallFact, ConnectionHost, Decision};
use crate::work_computer::ToolReply;

pub const SERVICE: &str = "github";
/// The question the first use in a work asks, with GitHub's mark; the ask
/// card reads the service and tool from it.
pub const ASK: &str = "Use GitHub (gh)?";
pub const USE: &str = "Use GitHub";
pub const DECLINE: &str = "Use the website instead";

/// Why the first call wants gh, in a sentence the ask card shows.
fn reason(name: &str, args: &Value) -> String {
    let number = args["number"].as_u64().map(|n| format!("#{n}"));
    match (name, number) {
        ("github_issue", Some(n)) => {
            format!("To read issue {n} and its comments with your account.")
        }
        ("github_pr", Some(n)) => format!("To read pull request {n} with your account."),
        ("github_checks", Some(n)) => format!("To read the checks on {n}."),
        ("github_pr_diff", Some(n)) => format!("To read the changes in {n}."),
        ("github_comment", Some(n)) => format!("To comment on {n}; you'll see the text first."),
        ("github_issues", _) => "To list the repository's issues with your account.".into(),
        ("github_prs", _) => "To list the repository's pull requests with your account.".into(),
        ("github_mine", _) => "To see what waits on you across GitHub with your account.".into(),
        _ => "To work on GitHub with your account instead of opening github.com.".into(),
    }
}

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_BODY_CHARS: usize = 3000;
const MAX_COMMENT_CHARS: usize = 600;
const MAX_COMMENTS: usize = 6;
const MAX_LIST: u64 = 30;
const MAX_RAW_BYTES: usize = 12 * 1024;

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> WorkModelTool {
    let mut schema =
        json!({"type": "object", "properties": properties, "additionalProperties": false});
    schema["required"] = json!(required);
    WorkModelTool {
        name: name.into(),
        description: description.into(),
        schema,
    }
}

/// The GitHub tools, as the model sees them.
pub fn definitions() -> Vec<WorkModelTool> {
    let repo = json!({"type": "string", "description": "owner/name; the working folder's repository when omitted."});
    let number = json!({"type": "integer", "minimum": 1});
    let state = json!({"type": "string", "enum": ["open", "closed", "merged", "all"]});
    let search = json!({"type": "string", "description": "GitHub search qualifiers, such as `label:bug author:octo`."});
    let limit = json!({"type": "integer", "minimum": 1, "maximum": MAX_LIST});
    vec![
        tool("github_issue", "Read one issue: title, state, labels, body and recent comments.", json!({"repo": repo, "number": number}), &["number"]),
        tool("github_issues", "List issues, newest first, as number, title, labels and comment count.", json!({"repo": repo, "state": state, "search": search, "limit": limit}), &[]),
        tool("github_pr", "Read one pull request: title, state, branches, changed files, review and check status, body and recent comments.", json!({"repo": repo, "number": number}), &["number"]),
        tool("github_prs", "List pull requests, newest first.", json!({"repo": repo, "state": state, "search": search, "limit": limit}), &[]),
        tool("github_checks", "The CI checks of a pull request, failing first, with links to their logs.", json!({"repo": repo, "number": number}), &["number"]),
        tool("github_pr_diff", "The diff of a pull request, cut at 12 KB; read files locally for more.", json!({"repo": repo, "number": number}), &["number"]),
        tool("github_mine", "What waits on the person across all of GitHub, with no repository: pull requests where their review is requested, open issues assigned to them, their own open pull requests, or their unread notifications (mentions, replies, CI).", json!({"kind": {"type": "string", "enum": ["review_requests", "assigned", "authored", "notifications"]}, "limit": limit}), &["kind"]),
        tool("github_api", "Read any GitHub REST resource with GET, such as `repos/{owner}/{repo}/contents/README.md` or `search/issues?q=…`. Read-only.", json!({"path": {"type": "string"}}), &["path"]),
        tool("github_comment", "Comment on an issue or pull request as the person. The person sees the exact text and confirms before it is posted.", json!({"repo": repo, "number": number, "body": {"type": "string", "description": "Markdown."}}), &["number", "body"]),
        tool("github_create_pr", "Open a pull request from a pushed branch as the person, after they confirm.", json!({"repo": repo, "title": {"type": "string"}, "body": {"type": "string"}, "base": {"type": "string"}, "head": {"type": "string"}, "draft": {"type": "boolean"}}), &["title", "body"]),
        tool("github_merge_pr", "Merge a pull request as the person, after they confirm.", json!({"repo": repo, "number": number, "method": {"type": "string", "enum": ["squash", "merge", "rebase"]}}), &["number"]),
    ]
}

fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |part: Option<&str>| {
        part.is_some_and(|p| {
            !p.is_empty()
                && p.len() <= 100
                && !p.starts_with(['.', '-'])
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// A GET path for `gh api`: no flags, no host, no traversal.
fn valid_api_path(path: &str) -> bool {
    let path = path.trim_start_matches('/');
    !path.is_empty()
        && path.len() <= 400
        && !path.starts_with('-')
        && !path.contains("..")
        && !path.contains("://")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_.-~?=&%,:+@".contains(c))
}

fn login(value: &Value) -> &str {
    value["login"].as_str().unwrap_or("someone")
}
fn day(value: &Value) -> &str {
    value.as_str().and_then(|s| s.get(..10)).unwrap_or("")
}
fn clip(text: &str, max: usize) -> String {
    let text = text.replace("\r\n", "\n");
    let text = text.trim();
    let mut clipped: String = text.chars().take(max).collect();
    if clipped.len() < text.len() {
        clipped.push('…');
    }
    clipped
}
fn labels(value: &Value) -> String {
    value
        .as_array()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|l| l["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}
fn state_word(value: &Value) -> String {
    value.as_str().unwrap_or("").to_ascii_lowercase()
}

fn comments(value: &Value) -> String {
    let Some(comments) = value.as_array().filter(|c| !c.is_empty()) else {
        return String::new();
    };
    let skipped = comments.len().saturating_sub(MAX_COMMENTS);
    let mut out = format!("\n\n{} comments", comments.len());
    if skipped > 0 {
        out.push_str(&format!(" (the last {MAX_COMMENTS})"));
    }
    out.push(':');
    for comment in &comments[skipped..] {
        out.push_str(&format!(
            "\n— {} · {}: {}",
            login(&comment["author"]),
            day(&comment["createdAt"]),
            clip(comment["body"].as_str().unwrap_or(""), MAX_COMMENT_CHARS)
        ));
    }
    out
}

/// One issue for the model, and the row the canvas shows.
pub fn render_issue(value: &Value) -> (String, CallFact) {
    let number = value["number"].as_u64().unwrap_or(0);
    let title = value["title"].as_str().unwrap_or("").to_owned();
    let mut out = format!(
        "#{number} · {} · {title}\nby {} · {}",
        state_word(&value["state"]),
        login(&value["author"]),
        day(&value["createdAt"])
    );
    let labels = labels(&value["labels"]);
    if !labels.is_empty() {
        out.push_str(&format!(" · labels: {labels}"));
    }
    if let Some(url) = value["url"].as_str() {
        out.push_str(&format!("\n{url}"));
    }
    let body = clip(value["body"].as_str().unwrap_or(""), MAX_BODY_CHARS);
    if !body.is_empty() {
        out.push_str(&format!("\n\n{body}"));
    }
    out.push_str(&comments(&value["comments"]));
    (
        out,
        CallFact {
            verb: "issue".into(),
            target: Some(format!("#{number}")),
            title: Some(title),
            ..Default::default()
        },
    )
}

fn checks_line(rollup: &Value) -> Option<String> {
    let checks = rollup.as_array().filter(|c| !c.is_empty())?;
    let (mut pass, mut fail, mut pending) = (0, 0, 0);
    for check in checks {
        let outcome = check["conclusion"]
            .as_str()
            .or_else(|| check["state"].as_str())
            .unwrap_or("");
        match outcome {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => pass += 1,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => fail += 1,
            _ => pending += 1,
        }
    }
    let mut parts = vec![format!("{pass} passing")];
    if fail > 0 {
        parts.push(format!("{fail} failing"));
    }
    if pending > 0 {
        parts.push(format!("{pending} pending"));
    }
    Some(format!("checks: {}", parts.join(", ")))
}

pub fn render_pr(value: &Value) -> (String, CallFact) {
    let number = value["number"].as_u64().unwrap_or(0);
    let title = value["title"].as_str().unwrap_or("").to_owned();
    let draft = if value["isDraft"] == Value::Bool(true) {
        " · draft"
    } else {
        ""
    };
    let mut out = format!(
        "#{number} · {}{draft} · {title}\nby {} · {} → {}",
        state_word(&value["state"]),
        login(&value["author"]),
        value["headRefName"].as_str().unwrap_or("?"),
        value["baseRefName"].as_str().unwrap_or("?")
    );
    if let Some(url) = value["url"].as_str() {
        out.push_str(&format!("\n{url}"));
    }
    let mut facts = Vec::new();
    if let (Some(add), Some(del)) = (value["additions"].as_u64(), value["deletions"].as_u64()) {
        facts.push(format!(
            "{} files, +{add} −{del}",
            value["changedFiles"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(review) = value["reviewDecision"].as_str().filter(|r| !r.is_empty()) {
        facts.push(format!(
            "review: {}",
            review.to_ascii_lowercase().replace('_', " ")
        ));
    }
    if let Some(mergeable) = value["mergeable"].as_str() {
        facts.push(format!("mergeable: {}", mergeable.to_ascii_lowercase()));
    }
    if let Some(checks) = checks_line(&value["statusCheckRollup"]) {
        facts.push(checks);
    }
    if !facts.is_empty() {
        out.push_str(&format!("\n{}", facts.join(" · ")));
    }
    if let Some(files) = value["files"].as_array().filter(|f| !f.is_empty()) {
        out.push_str("\nfiles:");
        for file in files.iter().take(30) {
            out.push_str(&format!(
                "\n  {} +{} −{}",
                file["path"].as_str().unwrap_or("?"),
                file["additions"].as_u64().unwrap_or(0),
                file["deletions"].as_u64().unwrap_or(0)
            ));
        }
        if files.len() > 30 {
            out.push_str(&format!("\n  … {} more", files.len() - 30));
        }
    }
    let body = clip(value["body"].as_str().unwrap_or(""), MAX_BODY_CHARS);
    if !body.is_empty() {
        out.push_str(&format!("\n\n{body}"));
    }
    out.push_str(&comments(&value["comments"]));
    (
        out,
        CallFact {
            verb: "pr".into(),
            target: Some(format!("#{number}")),
            title: Some(title),
            ..Default::default()
        },
    )
}

/// Issues or pull requests as one line each.
pub fn render_list(value: &Value, prs: bool) -> (String, CallFact) {
    let items = value.as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    for item in &items {
        let mut line = format!(
            "#{} {} · {}",
            item["number"].as_u64().unwrap_or(0),
            item["title"].as_str().unwrap_or(""),
            login(&item["author"])
        );
        if item["isDraft"] == Value::Bool(true) {
            line.push_str(" · draft");
        }
        let labels = labels(&item["labels"]);
        if !labels.is_empty() {
            line.push_str(&format!(" · {labels}"));
        }
        if let Some(count) = item["comments"].as_u64().filter(|c| *c > 0) {
            line.push_str(&format!(" · {count} comments"));
        }
        if let Some(state) = item["state"].as_str().filter(|s| *s != "OPEN") {
            line.push_str(&format!(" · {}", state.to_ascii_lowercase()));
        }
        out.push_str(&line);
        out.push('\n');
    }
    if items.is_empty() {
        out = if prs {
            "No pull requests.".into()
        } else {
            "No issues.".into()
        };
    }
    (
        out.trim_end().to_owned(),
        CallFact {
            verb: if prs { "prs" } else { "issues" }.into(),
            count: Some(items.len() as u32),
            ..Default::default()
        },
    )
}

/// Search results across repositories, one line each with its link.
pub fn render_found(value: &Value, prs: bool) -> (String, CallFact) {
    let items = value.as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    for item in &items {
        out.push_str(&format!(
            "{}#{} {} · {} · updated {} · {}\n",
            item["repository"]["nameWithOwner"].as_str().unwrap_or(""),
            item["number"].as_u64().unwrap_or(0),
            clip(item["title"].as_str().unwrap_or(""), 160),
            login(&item["author"]),
            day(&item["updatedAt"]),
            item["url"].as_str().unwrap_or("")
        ));
    }
    if items.is_empty() {
        out = if prs {
            "No open pull requests.".into()
        } else {
            "No open issues.".into()
        };
    }
    (
        out.trim_end().to_owned(),
        CallFact {
            verb: if prs { "prs" } else { "issues" }.into(),
            count: Some(items.len() as u32),
            ..Default::default()
        },
    )
}

/// The person's unread notifications: repository, title, why, when, link.
pub fn render_notifications(value: &Value) -> (String, CallFact) {
    let items = value.as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    for item in &items {
        let repo = item["repository"]["full_name"].as_str().unwrap_or("");
        // The API's subject link as the page a person opens.
        let link = item["subject"]["url"]
            .as_str()
            .and_then(|api| api.strip_prefix("https://api.github.com/repos/"))
            .map(|rest| {
                format!(
                    "https://github.com/{}",
                    rest.replacen("/pulls/", "/pull/", 1)
                )
            })
            .unwrap_or_else(|| format!("https://github.com/{repo}"));
        out.push_str(&format!(
            "{repo} · {} · {} · {} · {} · {link}\n",
            clip(item["subject"]["title"].as_str().unwrap_or(""), 160),
            item["subject"]["type"].as_str().unwrap_or(""),
            item["reason"].as_str().unwrap_or("").replace('_', " "),
            day(&item["updated_at"]),
        ));
    }
    if items.is_empty() {
        out = "No unread notifications.".into();
    }
    (
        out.trim_end().to_owned(),
        CallFact {
            verb: "notifications".into(),
            count: Some(items.len() as u32),
            ..Default::default()
        },
    )
}

pub fn render_checks(value: &Value, number: u64) -> (String, CallFact) {
    let mut checks = value.as_array().cloned().unwrap_or_default();
    let rank = |c: &Value| match c["bucket"].as_str() {
        Some("fail") => 0,
        Some("pending") => 1,
        Some("cancel") => 2,
        Some("pass") => 3,
        _ => 4,
    };
    checks.sort_by_key(rank);
    let failing = checks.iter().filter(|c| c["bucket"] == "fail").count();
    let mut out = format!("{} checks on #{number}: {failing} failing\n", checks.len());
    for check in &checks {
        let workflow = check["workflow"].as_str().filter(|w| !w.is_empty());
        out.push_str(&format!(
            "{} · {}{}",
            check["bucket"].as_str().unwrap_or("?"),
            check["name"].as_str().unwrap_or("?"),
            workflow.map(|w| format!(" ({w})")).unwrap_or_default()
        ));
        if check["bucket"] == "fail" {
            if let Some(link) = check["link"].as_str() {
                out.push_str(&format!(" · {link}"));
            }
        }
        out.push('\n');
    }
    (
        out.trim_end().to_owned(),
        CallFact {
            verb: "checks".into(),
            target: Some(format!("#{number}")),
            count: Some(failing as u32),
            ..Default::default()
        },
    )
}

/// `gh` for one run: consent is asked once, then remembered.
pub struct GitHub {
    program: PathBuf,
    /// The working folder, whose remote names the repository when none is given.
    folder: Option<PathBuf>,
    /// The person's login, for the Confirm's "as octo".
    login: Option<String>,
}

impl GitHub {
    pub fn new(program: PathBuf, folder: Option<PathBuf>, login: Option<String>) -> Self {
        Self {
            program,
            folder,
            login,
        }
    }

    async fn allowed(
        &self,
        host: &dyn ConnectionHost,
        name: &str,
        args: &Value,
    ) -> Result<(), ToolReply> {
        let prompt = format!("{ASK} {}", reason(name, args));
        let answer = host
            .ask(&prompt, &[USE, DECLINE])
            .await
            .map_err(|_| stopped())?;
        if answer.as_deref() == Some(USE) {
            Ok(())
        } else {
            Err(declined())
        }
    }

    async fn gh(&self, args: &[&str], stdin: Option<&str>) -> Result<String, ToolReply> {
        let Some((ok, out, err)) = super::cli::run(
            &self.program,
            args,
            self.folder.as_deref(),
            stdin,
            CALL_TIMEOUT,
        )
        .await
        else {
            return Err(fault("gh did not answer in time."));
        };
        if ok {
            return Ok(out);
        }
        let err = err.to_ascii_lowercase();
        Err(fault(
            if err.contains("could not resolve to")
                || err.contains("not found")
                || err.contains("404")
            {
                "Not found on GitHub, or not visible to the person's account."
            } else if err.contains("auth") || err.contains("401") || err.contains("403") {
                "gh is not signed in or lacks access. The person can run `gh auth login`."
            } else if err.contains("no git remote")
                || err.contains("not a git repository")
                || err.contains("none of the git remotes")
            {
                "No repository here. Give repo as owner/name."
            } else if err.contains("rate limit") {
                "GitHub's rate limit was reached. Try again later."
            } else {
                "gh could not do this."
            },
        ))
    }

    fn repo_args(&self, args: &Value) -> Result<Vec<String>, ToolReply> {
        match args["repo"].as_str().filter(|r| !r.trim().is_empty()) {
            Some(repo) if valid_repo(repo.trim()) => Ok(vec!["-R".into(), repo.trim().into()]),
            Some(_) => Err(fault("`repo` is owner/name.")),
            None if self.folder.is_some() => Ok(Vec::new()),
            None => Err(fault("Give repo as owner/name.")),
        }
    }

    pub async fn call(&self, host: &dyn ConnectionHost, name: &str, args: &Value) -> ToolReply {
        if let Err(reply) = self.allowed(host, name, args).await {
            return reply;
        }
        match self.dispatch(host, name, args).await {
            Ok(reply) | Err(reply) => reply,
        }
    }

    async fn dispatch(
        &self,
        host: &dyn ConnectionHost,
        name: &str,
        args: &Value,
    ) -> Result<ToolReply, ToolReply> {
        if name == "github_mine" {
            return self.mine(host, args).await;
        }
        let repo = if name == "github_api" {
            Vec::new()
        } else {
            self.repo_args(args)?
        };
        let number = args["number"].as_u64();
        let need_number = || {
            number
                .filter(|n| *n > 0)
                .ok_or_else(|| fault("`number` is required."))
        };
        let with_repo = |mut list: Vec<String>| {
            list.extend(repo.iter().cloned());
            list
        };
        match name {
            "github_issue" => {
                let n = need_number()?.to_string();
                let argv = with_repo(vec![
                    "issue".into(),
                    "view".into(),
                    n,
                    "--json".into(),
                    "number,title,state,author,labels,body,comments,url,createdAt".into(),
                ]);
                self.read(
                    host,
                    argv,
                    CallFact {
                        verb: "issue".into(),
                        target: Some(format!("#{}", need_number()?)),
                        ..Default::default()
                    },
                    render_issue,
                )
                .await
            }
            "github_pr" => {
                let n = need_number()?.to_string();
                let argv = with_repo(vec!["pr".into(), "view".into(), n, "--json".into(), "number,title,state,author,body,headRefName,baseRefName,additions,deletions,changedFiles,files,reviewDecision,statusCheckRollup,url,mergeable,isDraft,comments".into()]);
                self.read(
                    host,
                    argv,
                    CallFact {
                        verb: "pr".into(),
                        target: Some(format!("#{}", need_number()?)),
                        ..Default::default()
                    },
                    render_pr,
                )
                .await
            }
            "github_issues" | "github_prs" => {
                let prs = name == "github_prs";
                let limit = args["limit"]
                    .as_u64()
                    .unwrap_or(15)
                    .clamp(1, MAX_LIST)
                    .to_string();
                let state = match args["state"].as_str() {
                    None | Some("open") => "open",
                    Some("closed") => "closed",
                    Some("merged") if prs => "merged",
                    Some("all") => "all",
                    Some(_) => {
                        return Err(fault(
                            "`state` is open, closed, merged (pull requests) or all.",
                        ))
                    }
                };
                let fields = if prs {
                    "number,title,state,author,headRefName,isDraft,updatedAt,reviewDecision,url"
                } else {
                    "number,title,state,labels,updatedAt,author,comments"
                };
                let mut argv: Vec<String> = vec![
                    if prs { "pr" } else { "issue" }.into(),
                    "list".into(),
                    "--state".into(),
                    state.into(),
                    "-L".into(),
                    limit,
                    "--json".into(),
                    fields.into(),
                ];
                if !prs {
                    argv.extend(["--jq".into(), "map(.comments |= length)".into()]);
                }
                if let Some(search) = args["search"].as_str().filter(|s| !s.trim().is_empty()) {
                    if search.len() > 256 {
                        return Err(fault("`search` is at most 256 bytes."));
                    }
                    argv.extend(["--search".into(), search.trim().into()]);
                }
                let argv = with_repo(argv);
                self.read(
                    host,
                    argv,
                    CallFact {
                        verb: if prs { "prs" } else { "issues" }.into(),
                        ..Default::default()
                    },
                    |v| render_list(v, prs),
                )
                .await
            }
            "github_checks" => {
                let n = need_number()?;
                let argv = with_repo(vec![
                    "pr".into(),
                    "checks".into(),
                    n.to_string(),
                    "--json".into(),
                    "name,state,bucket,workflow,link".into(),
                ]);
                self.read(
                    host,
                    argv,
                    CallFact {
                        verb: "checks".into(),
                        target: Some(format!("#{n}")),
                        ..Default::default()
                    },
                    |v| render_checks(v, n),
                )
                .await
            }
            "github_pr_diff" => {
                let n = need_number()?;
                let argv = with_repo(vec![
                    "pr".into(),
                    "diff".into(),
                    n.to_string(),
                    "--color".into(),
                    "never".into(),
                ]);
                self.raw(
                    host,
                    argv,
                    CallFact {
                        verb: "diff".into(),
                        target: Some(format!("#{n}")),
                        ..Default::default()
                    },
                )
                .await
            }
            "github_api" => {
                let path = args["path"].as_str().unwrap_or("").trim();
                if !valid_api_path(path) {
                    return Err(fault(
                        "`path` is a REST path such as repos/owner/name/issues.",
                    ));
                }
                let argv = vec![
                    "api".into(),
                    "-X".into(),
                    "GET".into(),
                    path.trim_start_matches('/').into(),
                ];
                self.raw(
                    host,
                    argv,
                    CallFact {
                        verb: "api".into(),
                        target: Some(path.chars().take(60).collect()),
                        ..Default::default()
                    },
                )
                .await
            }
            "github_comment" => self.comment(host, args, repo).await,
            "github_create_pr" => self.create_pr(host, args, repo).await,
            "github_merge_pr" => self.merge_pr(host, args, repo).await,
            _ => Err(fault(&format!("There is no `{name}` tool."))),
        }
    }

    async fn read(
        &self,
        host: &dyn ConnectionHost,
        argv: Vec<String>,
        fact: CallFact,
        render: impl FnOnce(&Value) -> (String, CallFact),
    ) -> Result<ToolReply, ToolReply> {
        let fact = self.fact(fact);
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        let result = self.gh(&args, None).await.and_then(|out| {
            serde_json::from_str::<Value>(&out)
                .map_err(|_| fault("gh returned something unexpected."))
        });
        match result {
            Ok(value) => {
                let (text, rendered) = render(&value);
                let url = value["url"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| self.list_url(&argv, &rendered.verb));
                let fact = CallFact {
                    tool: fact.tool.clone(),
                    service: fact.service.clone(),
                    url,
                    ..rendered
                };
                let (text, _) = bounded(&text, MAX_RAW_BYTES);
                host.record(fact, true, Some(text.clone()))
                    .await
                    .map_err(|_| stopped())?;
                Ok(ToolReply {
                    content: text,
                    is_error: false,
                })
            }
            Err(reply) => {
                let _ = host.record(fact, false, None).await;
                Err(reply)
            }
        }
    }

    /// What waits on the person across GitHub: `gh search` for review
    /// requests, assignments and their own pull requests, `gh api
    /// notifications` for the rest. Read-only; no repository needed.
    async fn mine(&self, host: &dyn ConnectionHost, args: &Value) -> Result<ToolReply, ToolReply> {
        let limit = args["limit"]
            .as_u64()
            .unwrap_or(15)
            .clamp(1, MAX_LIST)
            .to_string();
        let kind = args["kind"].as_str().unwrap_or("");
        if kind == "notifications" {
            let argv = vec![
                "api".into(),
                "-X".into(),
                "GET".into(),
                format!("notifications?per_page={limit}"),
            ];
            return self
                .read(
                    host,
                    argv,
                    CallFact {
                        verb: "notifications".into(),
                        ..Default::default()
                    },
                    render_notifications,
                )
                .await;
        }
        let (what, filter, prs) = match kind {
            "review_requests" => ("prs", "--review-requested=@me", true),
            "assigned" => ("issues", "--assignee=@me", false),
            "authored" => ("prs", "--author=@me", true),
            _ => {
                return Err(fault(
                    "`kind` is review_requests, assigned, authored or notifications.",
                ))
            }
        };
        let argv: Vec<String> = vec![
            "search".into(),
            what.into(),
            filter.into(),
            "--state=open".into(),
            "--limit".into(),
            limit,
            "--json".into(),
            "number,title,repository,author,updatedAt,url".into(),
        ];
        self.read(
            host,
            argv,
            CallFact {
                verb: what.into(),
                ..Default::default()
            },
            |v| render_found(v, prs),
        )
        .await
    }

    /// The page of a listing, when the repository is named.
    fn list_url(&self, argv: &[String], verb: &str) -> Option<String> {
        let repo = argv
            .iter()
            .position(|a| a == "-R")
            .and_then(|at| argv.get(at + 1))?;
        match verb {
            "issues" => Some(format!("https://github.com/{repo}/issues")),
            "prs" => Some(format!("https://github.com/{repo}/pulls")),
            "checks" | "diff" => argv
                .get(2)
                .map(|n| format!("https://github.com/{repo}/pull/{n}")),
            _ => None,
        }
    }

    async fn raw(
        &self,
        host: &dyn ConnectionHost,
        argv: Vec<String>,
        fact: CallFact,
    ) -> Result<ToolReply, ToolReply> {
        let fact = self.fact(fact);
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        match self.gh(&args, None).await {
            Ok(out) => {
                let (mut text, cut) = bounded(&out, MAX_RAW_BYTES);
                if cut {
                    text.push_str(&format!("\n(cut at 12 KB of {} KB)", out.len() / 1024));
                }
                let url = self.list_url(&argv, &fact.verb);
                host.record(CallFact { url, ..fact }, true, Some(text.clone()))
                    .await
                    .map_err(|_| stopped())?;
                Ok(ToolReply {
                    content: text,
                    is_error: false,
                })
            }
            Err(reply) => {
                let _ = host.record(fact, false, None).await;
                Err(reply)
            }
        }
    }

    fn fact(&self, fact: CallFact) -> CallFact {
        CallFact {
            service: SERVICE.into(),
            tool: format!("github_{}", fact.verb),
            ..fact
        }
    }

    fn as_whom(&self) -> String {
        self.login
            .as_deref()
            .map(|login| format!(" as {login}"))
            .unwrap_or_default()
    }

    /// Runs a write only after the person confirmed its exact preview.
    async fn confirmed(
        &self,
        host: &dyn ConnectionHost,
        confirm: WorkConfirmV1,
        fact: CallFact,
        argv: Vec<String>,
        stdin: Option<&str>,
    ) -> Result<ToolReply, ToolReply> {
        match host.confirm(confirm).await.map_err(|_| stopped())? {
            Decision::Approved => {}
            Decision::Declined => {
                return Ok(ToolReply {
                    content: "The person declined. Don't try this again unchanged.".into(),
                    is_error: true,
                })
            }
            Decision::Stopped => return Err(stopped()),
        }
        let fact = self.fact(fact);
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        match self.gh(&args, stdin).await {
            Ok(out) => {
                let url = out
                    .lines()
                    .rev()
                    .find(|l| l.starts_with("https://"))
                    .map(str::to_owned);
                host.record(
                    CallFact {
                        url: url.clone(),
                        ..fact
                    },
                    true,
                    url.clone(),
                )
                .await
                .map_err(|_| stopped())?;
                Ok(ToolReply {
                    content: match url {
                        Some(url) => format!("Done: {url}"),
                        None => "Done.".into(),
                    },
                    is_error: false,
                })
            }
            Err(reply) => {
                let _ = host.record(fact, false, None).await;
                Err(reply)
            }
        }
    }

    async fn title_of(&self, repo: &[String], number: u64) -> Option<String> {
        let mut argv: Vec<String> = vec![
            "issue".into(),
            "view".into(),
            number.to_string(),
            "--json".into(),
            "title".into(),
            "--jq".into(),
            ".title".into(),
        ];
        argv.extend(repo.iter().cloned());
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        self.gh(&args, None)
            .await
            .ok()
            .map(|t| t.trim().chars().take(120).collect())
            .filter(|t: &String| !t.is_empty())
    }

    fn repo_name(&self, repo: &[String]) -> String {
        repo.get(1).cloned().unwrap_or_else(|| {
            self.folder
                .as_deref()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "this repository".into())
        })
    }

    async fn comment(
        &self,
        host: &dyn ConnectionHost,
        args: &Value,
        repo: Vec<String>,
    ) -> Result<ToolReply, ToolReply> {
        let number = args["number"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| fault("`number` is required."))?;
        let body = args["body"].as_str().unwrap_or("").trim();
        if body.is_empty() || body.len() > 4096 {
            return Err(fault("`body` is 1 to 4096 bytes."));
        }
        let title = self.title_of(&repo, number).await;
        let mut facts = vec![fact_line("Repository", &self.repo_name(&repo))];
        facts.push(fact_line(
            "On",
            &match &title {
                Some(title) => format!("#{number} · {title}"),
                None => format!("#{number}"),
            },
        ));
        let confirm = WorkConfirmV1 {
            site: "github.com".into(),
            category: WorkConfirmCategoryV1::Communication,
            headline: format!("Comment on #{number}{}?", self.as_whom()),
            action: "post the comment".into(),
            text: Some(body.to_owned()),
            facts,
            page: None,
            provenance: vec![],
            run_option: false,
            decision: None,
        };
        let mut argv: Vec<String> = vec![
            "issue".into(),
            "comment".into(),
            number.to_string(),
            "--body-file".into(),
            "-".into(),
        ];
        argv.extend(repo);
        let fact = CallFact {
            verb: "comment".into(),
            target: Some(format!("#{number}")),
            title,
            ..Default::default()
        };
        self.confirmed(host, confirm, fact, argv, Some(body)).await
    }

    async fn create_pr(
        &self,
        host: &dyn ConnectionHost,
        args: &Value,
        repo: Vec<String>,
    ) -> Result<ToolReply, ToolReply> {
        let title = args["title"].as_str().unwrap_or("").trim();
        let body = args["body"].as_str().unwrap_or("").trim();
        if title.is_empty() || title.len() > 256 || body.len() > 4096 {
            return Err(fault("`title` is 1 to 256 bytes and `body` at most 4096."));
        }
        let branch = |key: &str| -> Result<Option<String>, ToolReply> {
            match args[key].as_str().map(str::trim).filter(|b| !b.is_empty()) {
                Some(b)
                    if b.len() <= 200
                        && !b.starts_with('-')
                        && !b.contains(char::is_whitespace) =>
                {
                    Ok(Some(b.to_owned()))
                }
                Some(_) => Err(fault(&format!("`{key}` is a branch name."))),
                None => Ok(None),
            }
        };
        let (base, head) = (branch("base")?, branch("head")?);
        let draft = args["draft"].as_bool().unwrap_or(false);
        let mut facts = vec![fact_line("Repository", &self.repo_name(&repo))];
        facts.push(fact_line("Title", title));
        if let Some(head) = &head {
            facts.push(fact_line("From", head));
        }
        if let Some(base) = &base {
            facts.push(fact_line("Into", base));
        }
        if draft {
            facts.push(fact_line("Draft", "yes"));
        }
        let confirm = WorkConfirmV1 {
            site: "github.com".into(),
            category: WorkConfirmCategoryV1::Save,
            headline: format!("Open a pull request{}?", self.as_whom()),
            action: "open the pull request".into(),
            text: Some(body.to_owned()),
            facts,
            page: None,
            provenance: vec![],
            run_option: false,
            decision: None,
        };
        let mut argv: Vec<String> = vec![
            "pr".into(),
            "create".into(),
            "--title".into(),
            title.into(),
            "--body-file".into(),
            "-".into(),
        ];
        if let Some(base) = base {
            argv.extend(["--base".into(), base]);
        }
        if let Some(head) = head {
            argv.extend(["--head".into(), head]);
        }
        if draft {
            argv.push("--draft".into());
        }
        argv.extend(repo);
        let fact = CallFact {
            verb: "create_pr".into(),
            title: Some(title.chars().take(120).collect()),
            ..Default::default()
        };
        self.confirmed(host, confirm, fact, argv, Some(body)).await
    }

    async fn merge_pr(
        &self,
        host: &dyn ConnectionHost,
        args: &Value,
        repo: Vec<String>,
    ) -> Result<ToolReply, ToolReply> {
        let number = args["number"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| fault("`number` is required."))?;
        let method = match args["method"].as_str() {
            None | Some("squash") => "squash",
            Some("merge") => "merge",
            Some("rebase") => "rebase",
            Some(_) => return Err(fault("`method` is squash, merge or rebase.")),
        };
        let title = self.title_of(&repo, number).await;
        let confirm = WorkConfirmV1 {
            site: "github.com".into(),
            category: WorkConfirmCategoryV1::Save,
            headline: format!("Merge #{number}{}?", self.as_whom()),
            action: format!("{method} and merge"),
            text: None,
            facts: vec![
                fact_line("Repository", &self.repo_name(&repo)),
                fact_line(
                    "Pull request",
                    &match &title {
                        Some(title) => format!("#{number} · {title}"),
                        None => format!("#{number}"),
                    },
                ),
                fact_line("Method", method),
            ],
            page: None,
            provenance: vec![],
            run_option: false,
            decision: None,
        };
        let mut argv: Vec<String> = vec![
            "pr".into(),
            "merge".into(),
            number.to_string(),
            format!("--{method}"),
        ];
        argv.extend(repo);
        let fact = CallFact {
            verb: "merge".into(),
            target: Some(format!("#{number}")),
            title,
            ..Default::default()
        };
        self.confirmed(host, confirm, fact, argv, None).await
    }
}

fn fact_line(label: &str, value: &str) -> WorkConfirmFactV1 {
    WorkConfirmFactV1 {
        label: label.into(),
        value: value.chars().take(200).collect(),
    }
}
fn fault(text: &str) -> ToolReply {
    ToolReply {
        content: text.into(),
        is_error: true,
    }
}
fn stopped() -> ToolReply {
    fault("The run stopped.")
}
fn declined() -> ToolReply {
    fault("The person chose not to use gh in this work. Use the Browser helper on github.com instead.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/work_connections/fixtures")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn issue_digest_is_compact_and_complete() {
        let (text, fact) = render_issue(&fixture("gh-issue.json"));
        assert!(text.starts_with("#10000 · closed · "), "{text}");
        assert!(text.contains("by celloza · 2024-"));
        assert!(text.contains("https://github.com/cli/cli/issues/10000"));
        assert!(text.contains("3 comments:"));
        assert!(!text.contains('\r'));
        assert!(text.len() < 6000, "{}", text.len());
        assert_eq!(fact.verb, "issue");
        assert_eq!(fact.target.as_deref(), Some("#10000"));
        assert!(fact.title.is_some_and(|t| !t.is_empty()));
    }

    #[test]
    fn pr_lists_and_checks() {
        let (text, fact) = render_pr(&fixture("gh-pr.json"));
        assert!(text.contains("#14543 · open"));
        assert!(text.contains("2 files, +4 −4"));
        assert!(text.contains("review: review required"));
        assert!(text.contains("checks: "));
        assert!(text.contains("files:\n  .github/workflows/codeql.yml +3 −3"));
        assert_eq!(fact.verb, "pr");
        let (text, fact) = render_list(&fixture("gh-prs.json"), true);
        assert_eq!(text.lines().count(), 4);
        assert_eq!(fact.count, Some(4));
        assert_eq!(fact.verb, "prs");
        let (text, fact) = render_list(&fixture("gh-issues.json"), false);
        assert_eq!(fact.count, Some(5));
        assert!(text.lines().all(|l| l.starts_with('#')));
        let (text, fact) = render_checks(&fixture("gh-checks.json"), 14543);
        assert!(text.starts_with("8 checks on #14543: 0 failing"));
        assert_eq!(fact.count, Some(0));
        let (_, empty) = render_list(&json!([]), false);
        assert_eq!(empty.count, Some(0));
        let (text, fact) = render_found(
            &json!([{"number": 7, "title": "Fix login", "repository": {"nameWithOwner": "octo/app"},
                "author": {"login": "ana"}, "updatedAt": "2026-09-29T10:00:00Z",
                "url": "https://github.com/octo/app/pull/7"}]),
            true,
        );
        assert_eq!(
            text,
            "octo/app#7 Fix login · ana · updated 2026-09-29 · https://github.com/octo/app/pull/7"
        );
        assert_eq!((fact.verb.as_str(), fact.count), ("prs", Some(1)));
        let (text, fact) = render_notifications(&json!([{"reason": "review_requested",
            "updated_at": "2026-09-29T09:00:00Z", "repository": {"full_name": "octo/app"},
            "subject": {"title": "Fix login", "type": "PullRequest",
                "url": "https://api.github.com/repos/octo/app/pulls/7"}}]));
        assert_eq!(
            text,
            "octo/app · Fix login · PullRequest · review requested · 2026-09-29 · https://github.com/octo/app/pull/7"
        );
        assert_eq!(fact.count, Some(1));
    }

    #[test]
    fn arguments_are_checked() {
        assert!(valid_repo("cli/cli") && valid_repo("a-b/c.d_e"));
        for bad in ["cli", "cli/cli/x", "-x/y", "a/b c", "../x"] {
            assert!(!valid_repo(bad), "{bad}");
        }
        assert!(valid_api_path("repos/cli/cli/issues?state=open&per_page=5"));
        for bad in [
            "--method=POST",
            "repos/../x",
            "https://evil/x",
            "repos/a b",
            "",
        ] {
            assert!(!valid_api_path(bad), "{bad}");
        }
        let names: Vec<String> = definitions().into_iter().map(|t| t.name).collect();
        assert!(names
            .iter()
            .all(|n| n.starts_with("github_") && n.len() <= 64));
    }
}
