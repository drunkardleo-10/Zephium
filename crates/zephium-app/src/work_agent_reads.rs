use super::*;
use zephium_core::work::collection::*;

const MAX_CONCURRENT_PAGE_READS: usize = 3;

struct PendingRead<F> {
    request: WorkAgentBrowseRequest,
    future: Pin<Box<F>>,
    prior: Option<WorkUsage>,
}

impl Driver {
    pub(super) async fn fetch_browses<B, F>(
        &mut self,
        attempt: &WorkNodeAttempt,
        browser: &mut B,
        browses: Vec<WorkStepKindV1>,
    ) -> Result<Option<WorkAttemptStatus>, WorkError>
    where
        B: FnMut(WorkAttemptProbe, WorkAgentBrowseRequest) -> F,
        F: Future<Output = Result<WorkBrowserOutcome, WorkError>>,
    {
        let mut reads = Vec::new();
        let mut opened = self.account_pages.clone();
        for kind in browses {
            if matches!(kind, WorkStepKindV1::Discover { .. }) {
                self.notice("Native discovery is not available in this run: provider search covers the web. Use search for facts and read for exact URLs listed in sources.");
                continue;
            }
            let kind = job_listing(kind);
            let WorkStepKindV1::Read { url, .. } = &kind else {
                continue;
            };
            let granted = self
                .grant
                .accounts
                .iter()
                .position(|grant| grant.admits(url));
            if let Some(index) = granted {
                let refusal = if path_class(url) == WorkPathClass::Action {
                    Some(WorkAccountRefusal::AccountWrite)
                } else if opened[index] >= self.grant.accounts[index].pages {
                    Some(WorkAccountRefusal::PageBudget)
                } else {
                    None
                };
                if let Some(reason) = refusal {
                    self.refuse_account(index, reason);
                    continue;
                }
                opened[index] += 1;
            }
            reads.push((kind, granted));
        }
        let cap = usize::from(self.limits.max_workers).clamp(1, MAX_CONCURRENT_PAGE_READS);
        let mut offset = 0;
        while offset < reads.len() {
            let steps = usize::from(self.grant.max_steps).saturating_sub(self.steps as usize + 1);
            if steps == 0 {
                self.notice(STEPS_EXHAUSTED);
                break;
            }
            let Some(remaining) = remaining_limits(self.limits, self.used) else {
                self.notice(BUDGET_EXHAUSTED);
                break;
            };
            // A signed-in page runs alone: the engine groups only isolated pages.
            let run = match reads[offset].1 {
                Some(_) => 1,
                None => reads[offset..]
                    .iter()
                    .take_while(|(_, granted)| granted.is_none())
                    .count(),
            };
            let count = cap
                .min(run)
                .min(steps)
                .min(remaining.model_tokens as usize)
                .min(remaining.cost_micro_usd as usize)
                .min(remaining.operations as usize);
            let Some(limits) = allocations(self.limits, self.used, count) else {
                self.notice(BUDGET_EXHAUSTED);
                break;
            };
            let batch = &reads[offset..offset + count];
            let mut pending = Vec::new();
            let mut retries: Vec<(
                WorkAgentBrowseRequest,
                Result<WorkBrowserOutcome, WorkError>,
            )> = Vec::new();
            let mut terminal = None;
            let mut failure = None;
            for ((kind, granted), limits) in batch.iter().zip(limits) {
                if self.cancelled().await {
                    terminal = Some(WorkAttemptStatus::Cancelled);
                    break;
                }
                let account = granted.and_then(|index| self.grant.accounts.get(index).cloned());
                let mut step = self.step(kind.clone(), WorkStepStatus::Running);
                step.account = account.as_ref().map(|grant| {
                    Box::new(WorkPageAccountV1 {
                        host: grant.host().to_owned(),
                        badge: true,
                    })
                });
                let id = match self.begin(step, vec![], None).await {
                    Ok(id) => id,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                if let (Some(index), WorkStepKindV1::Read { url, .. }) = (*granted, kind) {
                    self.account_pages[index] += 1;
                    self.signed_in.push((id, index));
                    self.report(WorkAgentDiagnostic::AccountRead {
                        grant: u8::try_from(index).unwrap_or(u8::MAX),
                        path: path_class(url),
                    });
                }
                self.probe.record_activity(WorkActivityV1::Reading);
                let board = matches!(kind, WorkStepKindV1::Read { ref url, .. } if job_board(url));
                let request = WorkAgentBrowseRequest {
                    // A board's listings render late: its page gets the longer
                    // loading window from the start, inside the read's budget.
                    construction_attempt: if board {
                        zephium_agentic::WorkBrowserConstructionAttempt::SlowPageRetry
                    } else {
                        Default::default()
                    },
                    id,
                    step: kind.clone(),
                    limits,
                    hops: self.grant.browse_hops,
                    objective: if board {
                        format!("{}\n\n{JOB_BOARD_READING}", self.objective)
                    } else {
                        self.objective.clone()
                    },
                    output: self.output.clone(),
                    account,
                };
                let future = Box::pin(browser(self.probe.clone(), request.clone()));
                pending.push(PendingRead {
                    request,
                    future,
                    prior: None,
                });
            }
            while !pending.is_empty() || !retries.is_empty() {
                let (request, outcome, prior) = if pending.is_empty() {
                    let (request, outcome) = retries.pop().expect("pending read or retry");
                    if terminal.is_none() && failure.is_none() && !self.cancelled().await {
                        let first = outcome
                            .as_ref()
                            .ok()
                            .and_then(|outcome: &WorkBrowserOutcome| outcome.usage)
                            .expect("retry requires known usage");
                        if let Some(limits) = retry_limits(request.limits, first) {
                            self.report(WorkAgentDiagnostic::ReadRetried);
                            let request = retry_request(request, &outcome, limits);
                            let future = Box::pin(browser(self.probe.clone(), request.clone()));
                            pending.push(PendingRead {
                                request,
                                future,
                                prior: Some(first),
                            });
                            continue;
                        }
                    }
                    (request, outcome, None)
                } else {
                    next_read(&mut pending).await
                };
                let outcome = checked_outcome(outcome, request.limits);
                let retry = prior.is_none()
                    && terminal.is_none()
                    && failure.is_none()
                    && matches!(&outcome, Ok(WorkBrowserOutcome { status: WorkStepStatus::Failed, usage: Some(_), note: Some(note), helped, .. })
                        if (note == read_note::HUMAN_CHECK && *helped) || note == read_note::UNSETTLED || note == read_note::CONSTRUCTION_TIMEOUT);
                if retry && !pending.is_empty() {
                    retries.push((request, outcome));
                    continue;
                }
                if retry && !self.cancelled().await {
                    let first = outcome
                        .as_ref()
                        .ok()
                        .and_then(|outcome| outcome.usage)
                        .expect("retry requires known usage");
                    if let Some(limits) = retry_limits(request.limits, first) {
                        self.report(WorkAgentDiagnostic::ReadRetried);
                        let request = retry_request(request, &outcome, limits);
                        let future = Box::pin(browser(self.probe.clone(), request.clone()));
                        pending.push(PendingRead {
                            request,
                            future,
                            prior: Some(first),
                        });
                        continue;
                    }
                }
                match self
                    .settle_fetched(attempt, Fetched::Browse(request.id, outcome, prior))
                    .await
                {
                    Ok(Some(WorkAttemptStatus::OutcomeUnknown)) => {
                        terminal = Some(WorkAttemptStatus::OutcomeUnknown)
                    }
                    Ok(Some(status)) if terminal.is_none() => terminal = Some(status),
                    Err(error) if failure.is_none() => failure = Some(error),
                    _ => {}
                }
            }
            if let Some(error) = failure {
                return Err(error);
            }
            if terminal.is_some() {
                return Ok(terminal);
            }
            offset += count;
        }
        Ok(None)
    }
}

/// Job boards whose listings a script renders after the page loads. Closed.
const JOB_BOARDS: &[&str] = &[
    "ashbyhq.com",
    "greenhouse.io",
    "lever.co",
    "myworkdayjobs.com",
    "myworkdaysite.com",
    "dropbox.jobs",
];
const JOB_BOARD_READING: &str = "This page is a job board whose listings appear after it loads: wait until its list of open positions is shown, scrolling once if it is not, before extracting, then return one record per listed position with its title as the name, its location and its link.";
const JOB_LISTINGS: u8 = 32;

fn job_board(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .filter(|url| url.scheme() == "https")
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| {
            JOB_BOARDS.iter().any(|board| {
                host == *board
                    || host
                        .strip_suffix(board)
                        .is_some_and(|rest| rest.ends_with('.'))
            })
        })
}

/// A board read the model left without records extracts each listing as
/// one: title, location and link. A collection the model chose stands.
fn job_listing(kind: WorkStepKindV1) -> WorkStepKindV1 {
    match kind {
        WorkStepKindV1::Read {
            url,
            collection: None,
        } if job_board(&url) => WorkStepKindV1::Read {
            url,
            collection: Some(WorkBrowseCollection {
                title: "Open positions".into(),
                columns: vec![
                    WorkBrowseColumn {
                        name: "location".into(),
                        value: WorkBrowseValue::Text,
                        required: false,
                        extraction: WorkBrowseExtraction::Verbatim,
                    },
                    WorkBrowseColumn {
                        name: "url".into(),
                        value: WorkBrowseValue::Url,
                        required: false,
                        extraction: WorkBrowseExtraction::Verbatim,
                    },
                ],
                max_items: JOB_LISTINGS,
            }),
        },
        kind => kind,
    }
}

fn retry_request(
    request: WorkAgentBrowseRequest,
    outcome: &Result<WorkBrowserOutcome, WorkError>,
    limits: WorkExecutionLimits,
) -> WorkAgentBrowseRequest {
    let construction_attempt = if request.account.is_none()
        && matches!(outcome, Ok(WorkBrowserOutcome { note: Some(note), .. }) if note == read_note::CONSTRUCTION_TIMEOUT)
    {
        zephium_agentic::WorkBrowserConstructionAttempt::SlowPageRetry
    } else {
        request.construction_attempt
    };
    WorkAgentBrowseRequest {
        limits,
        construction_attempt,
        ..request
    }
}

async fn next_read<F>(
    pending: &mut Vec<PendingRead<F>>,
) -> (
    WorkAgentBrowseRequest,
    Result<WorkBrowserOutcome, WorkError>,
    Option<WorkUsage>,
)
where
    F: Future<Output = Result<WorkBrowserOutcome, WorkError>>,
{
    std::future::poll_fn(|cx| {
        for index in 0..pending.len() {
            if let Poll::Ready(outcome) = pending[index].future.as_mut().poll(cx) {
                let read = pending.swap_remove(index);
                return Poll::Ready((read.request, outcome, read.prior));
            }
        }
        Poll::Pending
    })
    .await
}

fn allocations(
    limits: WorkExecutionLimits,
    used: WorkUsage,
    count: usize,
) -> Option<Vec<WorkExecutionLimits>> {
    if count > MAX_CONCURRENT_PAGE_READS {
        return None;
    }
    budget_shares(limits, used, count)
}

pub(super) fn budget_shares(
    limits: WorkExecutionLimits,
    used: WorkUsage,
    count: usize,
) -> Option<Vec<WorkExecutionLimits>> {
    if count == 0 || count > usize::from(limits.max_workers) {
        return None;
    }
    let remaining = remaining_limits(limits, used)?;
    let count = count as u32;
    if remaining.model_tokens < count
        || remaining.cost_micro_usd < count
        || remaining.operations < count
    {
        return None;
    }
    let share = |value: u32, index| value / count + u32::from(index < value % count);
    Some(
        (0..count)
            .map(|index| WorkExecutionLimits {
                model_tokens: share(remaining.model_tokens, index),
                cost_micro_usd: share(remaining.cost_micro_usd, index),
                operations: share(remaining.operations, index),
                max_workers: 1,
                ..remaining
            })
            .collect(),
    )
}

fn retry_limits(limits: WorkExecutionLimits, used: WorkUsage) -> Option<WorkExecutionLimits> {
    remaining_limits(
        limits,
        WorkUsage {
            operations: used.operations.max(1),
            ..used
        },
    )
}

pub(super) fn remaining_limits(
    limits: WorkExecutionLimits,
    used: WorkUsage,
) -> Option<WorkExecutionLimits> {
    let limits = WorkExecutionLimits {
        model_tokens: limits.model_tokens.checked_sub(used.model_tokens)?,
        cost_micro_usd: limits.cost_micro_usd.checked_sub(used.cost_micro_usd)?,
        operations: limits.operations.checked_sub(used.operations)?,
        max_workers: 1,
        ..limits
    };
    limits.validate().ok().map(|()| limits)
}

fn checked_outcome(
    outcome: Result<WorkBrowserOutcome, WorkError>,
    limits: WorkExecutionLimits,
) -> Result<WorkBrowserOutcome, WorkError> {
    match outcome {
        Ok(outcome) if outcome.usage.is_none_or(|usage| !usage.within(limits)) => {
            Err(WorkError::OutcomeUnknown)
        }
        outcome => outcome,
    }
}

pub(super) fn combined_usage(
    prior: Option<WorkUsage>,
    latest: Option<WorkUsage>,
) -> Option<WorkUsage> {
    match (prior, latest) {
        (None, latest) => latest,
        (Some(prior), Some(latest)) => Some(WorkUsage {
            model_tokens: prior.model_tokens.checked_add(latest.model_tokens)?,
            cost_micro_usd: prior.cost_micro_usd.checked_add(latest.cost_micro_usd)?,
            operations: prior
                .operations
                .max(1)
                .checked_add(latest.operations.max(1))?,
            accounting: if prior.accounting == WorkUsageAccounting::ConservativeReservation
                || latest.accounting == WorkUsageAccounting::ConservativeReservation
            {
                WorkUsageAccounting::ConservativeReservation
            } else {
                WorkUsageAccounting::Exact
            },
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_job_boards_read_their_listings_as_records() {
        // The three boards of the person's run, and the other closed hosts.
        for url in [
            "https://jobs.ashbyhq.com/dropbox",
            "https://boards.greenhouse.io/dropbox",
            "https://job-boards.greenhouse.io/figma/jobs/123",
            "https://jobs.lever.co/acme",
            "https://acme.wd5.myworkdayjobs.com/en-US/careers",
            "https://www.dropbox.jobs/en/jobs",
            "https://dropbox.jobs/",
        ] {
            assert!(job_board(url), "{url}");
            let WorkStepKindV1::Read {
                collection: Some(collection),
                ..
            } = job_listing(WorkStepKindV1::Read {
                url: url.into(),
                collection: None,
            })
            else {
                panic!("a board read carries its listings");
            };
            collection.validate().unwrap();
            let columns: Vec<_> = collection.columns.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(columns, ["location", "url"]);
            assert_eq!(collection.max_items, JOB_LISTINGS);
        }
        for url in [
            "https://greenhouse.io.evil.test/jobs",
            "https://notlever.co/jobs",
            "http://jobs.lever.co/acme",
            "https://www.lego.com/en-us/themes/star-wars",
        ] {
            assert!(!job_board(url), "{url}");
            let kind = WorkStepKindV1::Read {
                url: url.into(),
                collection: None,
            };
            assert_eq!(job_listing(kind.clone()), kind);
        }
        // A collection the model chose is its own.
        let chosen = WorkStepKindV1::Read {
            url: "https://jobs.lever.co/acme".into(),
            collection: Some(WorkBrowseCollection {
                title: "Roles".into(),
                columns: vec![WorkBrowseColumn {
                    name: "team".into(),
                    value: WorkBrowseValue::Text,
                    required: false,
                    extraction: WorkBrowseExtraction::Generate,
                }],
                max_items: 5,
            }),
        };
        assert_eq!(job_listing(chosen.clone()), chosen);
    }

    #[test]
    fn work_read_budget_shares_preserve_the_original_reservation() {
        let limits = WorkExecutionLimits {
            model_tokens: 10,
            cost_micro_usd: 14,
            operations: 8,
            timeout_seconds: 30,
            max_workers: 4,
        };
        let shares = allocations(limits, WorkUsage::default(), 3).unwrap();
        assert_eq!(
            shares.iter().map(|share| share.model_tokens).sum::<u32>(),
            10
        );
        assert_eq!(
            shares.iter().map(|share| share.cost_micro_usd).sum::<u32>(),
            14
        );
        assert_eq!(shares.iter().map(|share| share.operations).sum::<u32>(), 8);
        assert!(shares
            .iter()
            .all(|share| share.max_workers == 1 && share.timeout_seconds == 30));
        assert!(allocations(limits, WorkUsage::default(), 4).is_none());
        assert!(allocations(
            limits,
            WorkUsage {
                operations: 8,
                ..WorkUsage::default()
            },
            1
        )
        .is_none());
        assert_eq!(
            retry_limits(shares[0], WorkUsage::default())
                .unwrap()
                .operations
                + 1,
            shares[0].operations
        );
        for usage in [
            None,
            Some(WorkUsage {
                model_tokens: 11,
                ..WorkUsage::default()
            }),
            Some(WorkUsage {
                cost_micro_usd: 15,
                ..WorkUsage::default()
            }),
            Some(WorkUsage {
                operations: 9,
                ..WorkUsage::default()
            }),
        ] {
            let outcome = WorkBrowserOutcome {
                status: WorkStepStatus::Succeeded,
                usage,
                artifacts: vec![],
                intervention: None,
                note: None,
                measurements: None,
                helped: false,
                account_write: false,
            };
            assert!(matches!(
                checked_outcome(Ok(outcome), limits),
                Err(WorkError::OutcomeUnknown)
            ));
        }
        assert!(combined_usage(Some(WorkUsage::default()), None).is_none());
    }
}
