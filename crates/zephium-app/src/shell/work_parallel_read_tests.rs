use super::*;
use crate::work_agent::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use zephium_core::work::{agent::*, search::*, synthesis::*};

struct Turns(AtomicUsize);
impl WorkAgentTurnProvider for Turns {
    fn turn<'a>(
        &'a self,
        _: &'a WorkAgentTurnDisclosure,
        _: WorkSynthesisTrace,
    ) -> WorkAgentTurnFuture<'a> {
        Box::pin(async move {
            let turn = self.0.fetch_add(1, Ordering::SeqCst);
            assert!(turn < 2);
            Ok(WorkAgentTurnResult {
                output: WorkAgentTurnOutput {
                    say: None,
                    artifacts: vec![],
                    fetch: if turn == 0 {
                        ['a', 'b', 'c', 'd']
                            .into_iter()
                            .map(|key| WorkAgentFetch::Read {
                                url: format!("https://example.test/{key}"),
                                collection: None,
                            })
                            .collect()
                    } else {
                        vec![]
                    },
                    ask: None,
                    finish: turn > 0,
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
        panic!("exact source URLs need no search");
    }
}

#[tokio::test]
async fn work_parallel_reads_partition_retries_and_drain_unknown_outcomes() {
    for unknown in [false, true] {
        let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
        let (mut shell, queue, handle, profile) = fixture(store);
        let create = handle.work_document(WorkIntent::Create {
            objective: "Read https://example.test/a https://example.test/b https://example.test/c https://example.test/d and compare".into(),
        }).unwrap();
        let work = create.work_id().unwrap();
        drive(&mut shell, &queue, create).await.unwrap();
        let provider = Turns(AtomicUsize::new(0));
        let service = WorkAgentService::new(handle);
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::<WorkAgentBrowseRequest>::new()));
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            drive(
                &mut shell,
                &queue,
                service.run(
                    profile,
                    WorkCommandV1 {
                        version: 1,
                        work,
                        expected_revision: WorkRevision::INITIAL,
                        command: WorkCommandId::generate(),
                        intent: WorkRuntimeIntent::BeginAgent {
                            grant: WorkAgentGrantV1 {
                                provider: WorkSearchProvider::OpenAi,
                                model: PUBLIC_SEARCH_MODEL.into(),
                                max_turns: 8,
                                max_steps: 24,
                                browse_hops: 1,
                                folders: vec![],
                            },
                            limits: WorkExecutionLimits {
                                model_tokens: 100_000,
                                cost_micro_usd: 100_000,
                                operations: 32,
                                timeout_seconds: 30,
                                max_workers: 4,
                            },
                        },
                    },
                    None,
                    WorkAgentProviders {
                        turn: &provider,
                        search: &NoSearch,
                    },
                    |probe, request| {
                        let ordinal = {
                            let mut requests = requests.lock().unwrap();
                            let ordinal = requests.len();
                            requests.push(request.clone());
                            ordinal
                        };
                        if ordinal == 3 {
                            assert_eq!(completed.load(Ordering::SeqCst), 3);
                        }
                        let barrier = barrier.clone();
                        let active = active.clone();
                        let peak = peak.clone();
                        let completed = completed.clone();
                        async move {
                            assert!(probe.admit_read_page(WorkStepId::generate()).await.is_err());
                            let _admission = probe.admit_read_page(request.id).await.unwrap();
                            peak.fetch_max(
                                active.fetch_add(1, Ordering::SeqCst) + 1,
                                Ordering::SeqCst,
                            );
                            if ordinal < 3 {
                                barrier.wait().await;
                            }
                            if ordinal > 0 {
                                tokio::time::sleep(Duration::from_millis(30)).await;
                            }
                            active.fetch_sub(1, Ordering::SeqCst);
                            completed.fetch_add(1, Ordering::SeqCst);
                            if ordinal == 0 && unknown {
                                return Err(WorkError::OutcomeUnknown);
                            }
                            let first = ordinal == 0;
                            Ok(WorkBrowserOutcome {
                                status: if first {
                                    WorkStepStatus::Failed
                                } else {
                                    WorkStepStatus::Succeeded
                                },
                                usage: Some(if first {
                                    WorkUsage {
                                        model_tokens: 11,
                                        cost_micro_usd: 7,
                                        operations: 2,
                                        accounting: WorkUsageAccounting::ConservativeReservation,
                                    }
                                } else {
                                    WorkUsage {
                                        model_tokens: 3,
                                        cost_micro_usd: 2,
                                        operations: 1,
                                        accounting: WorkUsageAccounting::Exact,
                                    }
                                }),
                                intervention: None,
                                note: first.then(|| read_note::HUMAN_CHECK.into()),
                                artifacts: if first {
                                    vec![]
                                } else {
                                    vec![WorkArtifactDraft {
                                        output: request.output,
                                        title: "Page result".into(),
                                        evidence: vec![WorkEvidenceLink {
                                            extraction_id: WorkArtifactId::from(100),
                                            source_id: 1,
                                        }],
                                        data: WorkArtifactDataV1::Document {
                                            paragraphs: vec!["Page facts".into()],
                                            formatted: None,
                                        },
                                    }]
                                },
                            })
                        }
                    },
                    |_| {},
                ),
            ),
        )
        .await
        .expect("three reads must overlap")
        .unwrap();
        assert_eq!(peak.load(Ordering::SeqCst), 3);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(
            completed.load(Ordering::SeqCst),
            if unknown { 3 } else { 5 }
        );
        let requests = requests.lock().unwrap();
        let initial = &requests[..3];
        assert_eq!(
            initial
                .iter()
                .map(|request| request.limits.model_tokens)
                .sum::<u32>(),
            100_000
        );
        assert_eq!(
            initial
                .iter()
                .map(|request| request.limits.cost_micro_usd)
                .sum::<u32>(),
            100_000
        );
        assert_eq!(
            initial
                .iter()
                .map(|request| request.limits.operations)
                .sum::<u32>(),
            31
        );
        assert!(requests
            .iter()
            .all(|request| request.limits.max_workers == 1));
        let execution = &result.executions[0];
        assert!(execution
            .steps
            .iter()
            .all(|step| step.status != WorkStepStatus::Running));
        let reads: Vec<_> = execution
            .steps
            .iter()
            .filter(|step| matches!(step.kind, WorkStepKindV1::Read { .. }))
            .collect();
        assert_eq!(reads.len(), if unknown { 3 } else { 4 });
        if unknown {
            assert_eq!(requests.len(), 3);
            assert_eq!(execution.status, WorkExecutionStatus::Interrupted);
            assert_eq!(
                execution.attempts[0].status,
                WorkAttemptStatus::OutcomeUnknown
            );
            assert_eq!(execution.attempts[0].usage, None);
            assert_eq!(
                reads
                    .iter()
                    .filter(|step| step.status == WorkStepStatus::Succeeded)
                    .count(),
                2
            );
        } else {
            assert_eq!(execution.status, WorkExecutionStatus::NeedsReview);
            let retry = &requests[3];
            assert_eq!(retry.id, initial[0].id);
            assert_eq!(
                retry.limits.model_tokens + 11,
                initial[0].limits.model_tokens
            );
            assert_eq!(
                retry.limits.cost_micro_usd + 7,
                initial[0].limits.cost_micro_usd
            );
            assert_eq!(retry.limits.operations + 2, initial[0].limits.operations);
            let retried = reads.iter().find(|step| step.id == retry.id).unwrap();
            assert_eq!(
                retried.usage,
                Some(WorkUsage {
                    model_tokens: 14,
                    cost_micro_usd: 9,
                    operations: 3,
                    accounting: WorkUsageAccounting::ConservativeReservation
                })
            );
        }
    }
}
