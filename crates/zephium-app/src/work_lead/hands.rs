//! A part's or the lead's hands on searches, pages and files: the proven
//! page machinery (entry questions, sign-in waits, Confirm steps, frames)
//! runs each request as a durable step carrying the part, and what came back
//! returns to the model as a compact, cited digest, never a page.
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Mutex;

use serde_json::Value;
use zephium_core::work::{artifact::*, collection::WorkBrowseCollection, runtime::*, search::*, *};

use super::call::clip;
use super::run::LeadRun;
use crate::work_agent::{WorkAgentBrowseRequest, WorkBrowserOutcome, WorkPartDriver};
use crate::work_runtime::{WorkAttemptProbe, WorkNodeAttempt};

/// At most this many agent pages are live at once across a run's parts.
pub(crate) const LIVE_PAGES: usize = 2;
/// Searches in flight at once across a run's parts. Each holds one of the
/// run's provider slots (four) while it searches or ranks; past that a
/// search waits instead of failing to be sent.
const LIVE_SEARCHES: usize = 3;
const SEARCH_ANSWER_CHARS: usize = 2_400;
const SEARCH_SOURCES: usize = 8;
const RECORD_LINES: usize = 32;
const FILE_TEXT_BYTES: usize = 6 * 1024;

/// The browser closure the host gives the run, shared by every part, with
/// its gates: at most two pages live, and at most one page task per site,
/// since one session means one actor per site.
pub(crate) struct SharedBrowser<B> {
    browser: Mutex<B>,
    pages: tokio::sync::Semaphore,
    searches: tokio::sync::Semaphore,
    sites: Mutex<Vec<(String, std::sync::Arc<tokio::sync::Mutex<()>>)>>,
}
impl<B> SharedBrowser<B> {
    pub(crate) fn new(browser: B) -> Self {
        Self {
            browser: Mutex::new(browser),
            pages: tokio::sync::Semaphore::new(LIVE_PAGES),
            searches: tokio::sync::Semaphore::new(LIVE_SEARCHES),
            sites: Mutex::new(Vec::new()),
        }
    }
    fn site_lane(&self, site: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let mut sites = self
            .sites
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, lane)) = sites.iter().find(|(known, _)| known == site) {
            return lane.clone();
        }
        let lane = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        sites.push((site.to_owned(), lane.clone()));
        lane
    }
}

/// One request the model made, as the step it becomes.
#[derive(Clone)]
pub(crate) struct Request {
    pub call: String,
    pub kind: WorkStepKindV1,
}

pub(crate) struct Hands<'a, B> {
    run: &'a LeadRun,
    attempt: &'a WorkNodeAttempt,
    search: &'a dyn WorkPublicSearchProvider,
    browser: &'a SharedBrowser<B>,
    driver: tokio::sync::Mutex<WorkPartDriver>,
    part: Option<WorkPartId>,
    charged: Mutex<WorkUsage>,
}

impl<'a, B, Fut> Hands<'a, B>
where
    B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> Fut,
    Fut: Future<Output = Result<WorkBrowserOutcome, WorkError>>,
{
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new(
        run: &'a LeadRun,
        attempt: &'a WorkNodeAttempt,
        search: &'a dyn WorkPublicSearchProvider,
        browser: &'a SharedBrowser<B>,
        part: Option<WorkPartId>,
        limits: WorkExecutionLimits,
        objective: String,
    ) -> Self {
        let driver = WorkPartDriver::new(
            run.handle.clone(),
            run.profile,
            run.probe.clone(),
            run.grant.clone(),
            limits,
            attempt.node().outputs[0].name.clone(),
            objective,
            part,
        )
        .await;
        Self {
            run,
            attempt,
            search,
            browser,
            driver: tokio::sync::Mutex::new(driver),
            part,
            charged: Mutex::new(WorkUsage::default()),
        }
    }

    /// What this driver has spent in all.
    pub(crate) async fn used(&self) -> WorkUsage {
        self.driver.lock().await.used()
    }

    /// Runs the requests as steps (searches in parallel, pages two at a
    /// time) and returns one digest per request, in order. `Err` carries a
    /// terminal status: the run must end.
    pub(crate) async fn run(
        &self,
        requests: Vec<Request>,
    ) -> Result<Vec<(String, String, bool)>, WorkAttemptStatus> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let before: BTreeSet<WorkStepId> = match self.run.execution().await {
            Ok(execution) => execution.steps.iter().map(|s| s.id).collect(),
            Err(_) => BTreeSet::new(),
        };
        let started = std::time::Instant::now();
        let mut driver = self.driver.lock().await;
        let mut gate = |probe: WorkAttemptProbe, request: WorkAgentBrowseRequest| {
            let lane = match &request.step {
                WorkStepKindV1::Read {
                    url, goal: Some(_), ..
                } => crate::work_sites::site_of(url).map(|site| self.browser.site_lane(&site)),
                _ => None,
            };
            let future = {
                let mut browser = self
                    .browser
                    .browser
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (browser)(probe, request)
            };
            let pages = &self.browser.pages;
            async move {
                let _site = match &lane {
                    Some(lane) => Some(lane.lock().await),
                    None => None,
                };
                let _page = pages.acquire().await.ok();
                future.await
            }
        };
        let kinds: Vec<WorkStepKindV1> = requests.iter().map(|r| r.kind.clone()).collect();
        let search = SearchLane {
            inner: self.search,
            permits: &self.browser.searches,
        };
        let outcome = driver
            .run(
                self.attempt,
                &search,
                &mut gate,
                self.run.turn(),
                before.len(),
                kinds,
            )
            .await;
        let used = driver.used();
        let notices = driver.take_notices();
        {
            let mut charged = self
                .charged
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.run.charge(WorkUsage {
                model_tokens: used.model_tokens.saturating_sub(charged.model_tokens),
                cost_micro_usd: used.cost_micro_usd.saturating_sub(charged.cost_micro_usd),
                operations: used.operations.saturating_sub(charged.operations),
                accounting: used.accounting,
            });
            *charged = used;
        }
        // A page whose outcome is unknown is one lost page, not the end of
        // the run: its step says so and the run's usage becomes a ceiling.
        let terminal = match outcome {
            Ok(Some(WorkAttemptStatus::OutcomeUnknown)) | Err(WorkError::OutcomeUnknown) => {
                self.run.charge(WorkUsage {
                    model_tokens: 0,
                    cost_micro_usd: 0,
                    operations: 0,
                    accounting: WorkUsageAccounting::ConservativeReservation,
                });
                None
            }
            Ok(Some(status)) => Some(status),
            Ok(None) => None,
            Err(_) => Some(WorkAttemptStatus::Failed),
        };
        let execution = self.run.execution().await.ok();
        let new: Vec<WorkStepFact> = execution
            .as_ref()
            .map(|execution| {
                execution
                    .steps
                    .iter()
                    .filter(|s| !before.contains(&s.id) && s.part == self.part)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut used_steps = BTreeSet::new();
        let mut results = Vec::new();
        for request in &requests {
            let step = new.iter().find(|step| {
                !used_steps.contains(&step.id) && same_request(&request.kind, &step.kind)
            });
            let (content, error) = match (step, &execution) {
                (Some(step), Some(execution)) => {
                    used_steps.insert(step.id);
                    let asked = new
                        .iter()
                        .filter(|s| {
                            matches!(
                                s.kind,
                                WorkStepKindV1::Ask { .. } | WorkStepKindV1::Confirm { .. }
                            )
                        })
                        .collect::<Vec<_>>();
                    self.digest(execution, step, &asked, &driver)
                }
                _ => (
                    notices
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Not run: the part's budget or steps are spent".into()),
                    true,
                ),
            };
            results.push((request.call.clone(), content, error));
        }
        if let (Some(notice), Some((_, content, _))) = (notices.first(), results.last_mut()) {
            if !content.contains(notice.as_str()) {
                content.push_str("\nNote: ");
                content.push_str(notice);
            }
        }
        let count = |search: bool| {
            requests
                .iter()
                .filter(|r| matches!(r.kind, WorkStepKindV1::Search { .. }) == search)
                .count()
        };
        let pages = requests
            .iter()
            .filter(|r| matches!(r.kind, WorkStepKindV1::Read { .. }))
            .count();
        self.run.report(super::WorkLeadDiagnostic::Fetched {
            part: self.part.is_some(),
            searches: count(true),
            pages,
            others: count(false) - pages,
            failed: results.iter().filter(|(_, _, error)| *error).count(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        match terminal {
            Some(status) if status != WorkAttemptStatus::Succeeded => Err(status),
            _ => Ok(results),
        }
    }

    fn digest(
        &self,
        execution: &WorkExecutionFact,
        step: &WorkStepFact,
        asked: &[&WorkStepFact],
        driver: &WorkPartDriver,
    ) -> (String, bool) {
        let failed = step.status != WorkStepStatus::Succeeded;
        let mut out = String::new();
        if failed {
            out.push_str(match step.status {
                WorkStepStatus::Cancelled => "Stopped",
                WorkStepStatus::OutcomeUnknown => "Lost",
                _ => "Failed",
            });
            if let Some(note) = &step.note {
                out.push_str(": ");
                out.push_str(note);
            }
            out.push('\n');
        }
        for held in asked {
            match &held.kind {
                WorkStepKindV1::Confirm { confirm } => out.push_str(&format!(
                    "Held for the person on {}: {} — {}\n",
                    confirm.site,
                    confirm.headline,
                    match (confirm.decision, held.status) {
                        (Some(WorkConfirmDecisionV1::Declined), _) => "they declined",
                        (_, WorkStepStatus::Succeeded) => "done",
                        (Some(_), _) => "approved, but it did not complete",
                        (None, _) => "not decided",
                    }
                )),
                WorkStepKindV1::Ask {
                    answer: Some(answer),
                    ..
                } => out.push_str(&format!(
                    "The person answered the site question: {answer}\n"
                )),
                _ => {}
            }
        }
        match &step.kind {
            WorkStepKindV1::Search { .. } => {
                if let Some(record) = step
                    .evidence
                    .and_then(|id| execution.provider_evidence.iter().find(|r| r.id == id))
                {
                    let answer: String = record
                        .evidence
                        .answer
                        .chars()
                        .take(SEARCH_ANSWER_CHARS)
                        .collect();
                    out.push_str(answer.trim());
                    out.push_str("\nSources:\n");
                    let mut seen = BTreeSet::new();
                    for (index, citation) in record.evidence.citations.iter().enumerate() {
                        if seen.len() >= SEARCH_SOURCES || !seen.insert(citation.url.clone()) {
                            continue;
                        }
                        let key = self.run.cite(
                            WorkEvidenceLink {
                                extraction_id: record.id,
                                source_id: (index + 1) as u16,
                            },
                            &citation.title,
                            Some(&citation.url),
                        );
                        out.push_str(&format!(
                            "[{key}] {} — {}\n",
                            clip(&citation.title, 90),
                            citation.url
                        ));
                    }
                }
            }
            WorkStepKindV1::Read { url, .. } => {
                let title = step
                    .local
                    .as_ref()
                    .and_then(|local| local.page_title.clone())
                    .unwrap_or_else(|| host(url));
                for id in &step.artifacts {
                    if let Some(artifact) = execution.artifacts.iter().find(|a| a.id == *id) {
                        out.push_str(&self.records(artifact, &title, url, driver));
                    }
                }
                if step.artifacts.is_empty() && !failed {
                    out.push_str("The page gave nothing usable.\n");
                }
            }
            kind if kind.files() || matches!(kind, WorkStepKindV1::RunCommand { .. }) => {
                if let Some(record) = step
                    .evidence
                    .and_then(|id| execution.file_evidence.iter().find(|r| r.id == id))
                {
                    let key = self.run.cite(
                        WorkEvidenceLink {
                            extraction_id: record.id,
                            source_id: 1,
                        },
                        &record.file.name,
                        None,
                    );
                    out.push_str(&format!("[{key}] {}\n", record.file.path));
                    out.push_str(&clip(&record.file.text, FILE_TEXT_BYTES));
                } else if let Some(record) = step
                    .evidence
                    .and_then(|id| execution.command_evidence.iter().find(|r| r.id == id))
                {
                    let key = self.run.cite(
                        WorkEvidenceLink {
                            extraction_id: record.id,
                            source_id: 1,
                        },
                        &record.command.command,
                        None,
                    );
                    out.push_str(&format!(
                        "[{key}] exit {}\n{}",
                        record
                            .command
                            .exit
                            .map_or("none".into(), |code| code.to_string()),
                        clip(&record.command.text, FILE_TEXT_BYTES)
                    ));
                } else if let Some(note) = &step.note {
                    if !failed {
                        out.push_str(note);
                    }
                }
            }
            _ => {}
        }
        if out.trim().is_empty() {
            out.push_str(if failed { "Failed" } else { "Done" });
        }
        (out, failed)
    }

    /// A page's records as lines the model can build objects from: names,
    /// facts, pictures and links, each with the key of its source.
    fn records(
        &self,
        artifact: &WorkArtifactV1,
        title: &str,
        url: &str,
        driver: &WorkPartDriver,
    ) -> String {
        let keys: Vec<String> = artifact
            .evidence
            .iter()
            .map(|link| {
                let page = driver
                    .preview(link)
                    .and_then(|p| p.link_destination.clone())
                    .unwrap_or_else(|| url.to_owned());
                self.run.allow_url(&page);
                self.run.cite(link.clone(), title, Some(&page))
            })
            .collect();
        let cite = |indices: &[u16]| -> String {
            let keys: Vec<&str> = indices
                .iter()
                .filter_map(|i| keys.get(usize::from(*i)).map(String::as_str))
                .collect();
            if keys.is_empty() {
                String::new()
            } else {
                format!(" [{}]", keys.join(", "))
            }
        };
        let all = if keys.is_empty() {
            String::new()
        } else {
            format!(" [{}]", keys[0])
        };
        let subject = |subject: &WorkSubject| {
            let mut line = subject.name.clone();
            if let Some(descriptor) = &subject.descriptor {
                line.push_str(&format!(" — {descriptor}"));
            }
            if let Some(homepage) = &subject.homepage {
                self.run.allow_url(homepage);
                line.push_str(&format!(" · url {homepage}"));
            }
            for image in &subject.image_candidates {
                line.push_str(&format!(" · photo {image}"));
            }
            line
        };
        let mut out = format!("{} ({title}){all}\n", artifact.title);
        match &artifact.data {
            WorkArtifactDataV1::ComparisonMatrix {
                subjects,
                criteria,
                cells,
                notes,
            } => {
                for (row, item) in subjects.iter().zip(cells).take(RECORD_LINES) {
                    let mut line = format!("- {}", subject(row));
                    for (criterion, cell) in criteria.iter().zip(item) {
                        let value = match &cell.value {
                            WorkCellValue::Text { text } => text.clone(),
                            WorkCellValue::Measurement { value } => value.clone(),
                            WorkCellValue::Money {
                                amount, currency, ..
                            } => format!("{amount} {currency}"),
                            WorkCellValue::Rating { value } => value.to_string(),
                            WorkCellValue::Presence { present } => {
                                if *present { "yes" } else { "no" }.into()
                            }
                            WorkCellValue::Unknown => continue,
                        };
                        line.push_str(&format!(" · {}: {value}", criterion.name));
                        line.push_str(&cite(&cell.evidence));
                    }
                    out.push_str(&clip(&line, 900));
                    out.push('\n');
                }
                for note in notes {
                    out.push_str(&format!("Note: {note}\n"));
                }
            }
            WorkArtifactDataV1::Findings { subjects, items } => {
                for item in subjects.iter().take(RECORD_LINES) {
                    out.push_str(&format!("- {}\n", subject(item)));
                }
                for finding in items.iter().take(RECORD_LINES) {
                    let mut line = format!("• {}", finding.claim);
                    if let Some(detail) = &finding.detail {
                        line.push_str(&format!(" ({detail})"));
                    }
                    line.push_str(&cite(&finding.evidence));
                    out.push_str(&clip(&line, 600));
                    out.push('\n');
                }
            }
            data => {
                out.push_str(&clip(&data.plain_text(), 3_000));
                out.push('\n');
            }
        }
        out
    }
}

/// The run's search provider behind its shared search permits.
struct SearchLane<'a> {
    inner: &'a dyn WorkPublicSearchProvider,
    permits: &'a tokio::sync::Semaphore,
}
impl WorkPublicSearchProvider for SearchLane<'_> {
    fn minimum_reservation(
        &self,
        scope: &WorkPublicSearchScope,
        context: &[zephium_core::work::context::WorkContextBody],
    ) -> Option<WorkUsage> {
        self.inner.minimum_reservation(scope, context)
    }
    fn rerank<'a>(
        &'a self,
        scope: &'a WorkPublicSearchScope,
        evidence: &'a WorkProviderSearchEvidenceV1,
        limits: WorkExecutionLimits,
        deadline: std::time::Instant,
    ) -> WorkPublicSearchRankingFuture<'a> {
        Box::pin(async move {
            let _permit = self.permits.acquire().await;
            self.inner.rerank(scope, evidence, limits, deadline).await
        })
    }
    fn search<'a>(
        &'a self,
        scope: &'a WorkPublicSearchScope,
        context: &'a [zephium_core::work::context::WorkContextBody],
        limits: WorkExecutionLimits,
    ) -> WorkPublicSearchFuture<'a> {
        Box::pin(async move {
            let _permit = self.permits.acquire().await;
            self.inner.search(scope, context, limits).await
        })
    }
}

fn host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "the page".into())
}

/// The step the driver made for a request: same query, url or path. A page
/// task may start from the site's entry page, so its goal decides.
fn same_request(request: &WorkStepKindV1, step: &WorkStepKindV1) -> bool {
    match (request, step) {
        (WorkStepKindV1::Search { query: a }, WorkStepKindV1::Search { query: b }) => a == b,
        (
            WorkStepKindV1::Read {
                url: a, goal: None, ..
            },
            WorkStepKindV1::Read {
                url: b, goal: None, ..
            },
        ) => a == b,
        (
            WorkStepKindV1::Read { goal: Some(a), .. },
            WorkStepKindV1::Read { goal: Some(b), .. },
        ) => a == b,
        (WorkStepKindV1::List { path: a, .. }, WorkStepKindV1::List { path: b, .. })
        | (WorkStepKindV1::ReadFile { path: a, .. }, WorkStepKindV1::ReadFile { path: b, .. })
        | (WorkStepKindV1::WriteFile { path: a, .. }, WorkStepKindV1::WriteFile { path: b, .. })
        | (WorkStepKindV1::EditFile { path: a, .. }, WorkStepKindV1::EditFile { path: b, .. })
        | (
            WorkStepKindV1::DeleteFile { path: a, .. },
            WorkStepKindV1::DeleteFile { path: b, .. },
        )
        | (WorkStepKindV1::MoveFile { from: a, .. }, WorkStepKindV1::MoveFile { from: b, .. })
        | (
            WorkStepKindV1::RunCommand { command: a, .. },
            WorkStepKindV1::RunCommand { command: b, .. },
        ) => a == b,
        (
            WorkStepKindV1::SearchFiles { query: a, .. },
            WorkStepKindV1::SearchFiles { query: b, .. },
        ) => a == b,
        _ => false,
    }
}

/// A `records` argument as the page machinery's collection, if valid.
pub(crate) fn collection(value: Option<&Value>) -> Result<Option<WorkBrowseCollection>, String> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let mut collection: WorkBrowseCollection = serde_json::from_value(value.clone())
        .map_err(|error| format!("records does not match its shape: {error}"))?;
    // A row missing a price or a picture is still a find the lead can use;
    // only a link is ever required.
    for column in &mut collection.columns {
        if column.value != zephium_core::work::collection::WorkBrowseValue::Url {
            column.required = false;
        }
    }
    collection.validate().map_err(|_| {
        "records needs a title, 1 to 32 max_items, 1 to 16 distinct ASCII column names other than name, at most three image_url columns, money only with generate, and (columns + 1) × max_items ≤ 256".to_owned()
    })?;
    Ok(Some(collection))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Counting {
        live: AtomicUsize,
        most: AtomicUsize,
    }
    impl WorkPublicSearchProvider for Counting {
        fn search<'a>(
            &'a self,
            _: &'a WorkPublicSearchScope,
            _: &'a [zephium_core::work::context::WorkContextBody],
            _: WorkExecutionLimits,
        ) -> WorkPublicSearchFuture<'a> {
            Box::pin(async move {
                let live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
                self.most.fetch_max(live, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                self.live.fetch_sub(1, Ordering::SeqCst);
                Err(WorkPublicSearchError::NotDispatched(WorkError::Unavailable))
            })
        }
    }

    #[test]
    fn a_runs_searches_wait_for_a_provider_slot_instead_of_crowding_it() {
        let provider = Counting::default();
        let permits = tokio::sync::Semaphore::new(LIVE_SEARCHES);
        let lane = SearchLane {
            inner: &provider,
            permits: &permits,
        };
        let scope = WorkPublicSearchScope {
            provider: WorkSearchProvider::OpenAi,
            model: PUBLIC_SEARCH_MODEL.into(),
            query: "public query".into(),
        };
        let limits = WorkExecutionLimits {
            model_tokens: 1,
            cost_micro_usd: 1,
            operations: 1,
            timeout_seconds: 1,
            max_workers: 1,
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut pending: Vec<_> =
                    (0..8).map(|_| lane.search(&scope, &[], limits)).collect();
                std::future::poll_fn(|cx| {
                    pending.retain_mut(|search| search.as_mut().poll(cx).is_pending());
                    if pending.is_empty() {
                        std::task::Poll::Ready(())
                    } else {
                        std::task::Poll::Pending
                    }
                })
                .await;
            });
        assert_eq!(provider.most.load(Ordering::SeqCst), LIVE_SEARCHES);
    }
}
