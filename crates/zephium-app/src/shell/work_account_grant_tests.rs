use super::*;
use crate::work_agent::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use zephium_core::work::{agent::*, search::*, synthesis::*};

const INBOX_FACT: &str = "Quarterly report from Dana is due Friday";

/// Each turn's granted origins with pages left, and its notices.
type Seen = (Vec<(String, u8)>, Vec<String>);
/// Reads inside and outside the grant, an action path, a search that would
/// repeat a signed-in fact, and a read past the page budget.
struct Turns {
    turn: AtomicUsize,
    seen: Mutex<Vec<Seen>>,
}
impl WorkAgentTurnProvider for Turns {
    fn turn<'a>(
        &'a self,
        input: &'a WorkAgentTurnDisclosure,
        _: WorkSynthesisTrace,
    ) -> WorkAgentTurnFuture<'a> {
        Box::pin(async move {
            let turn = self.turn.fetch_add(1, Ordering::SeqCst);
            let context = input.context();
            self.seen.lock().unwrap().push((
                context
                    .accounts
                    .iter()
                    .map(|account| (account.origin.clone(), account.pages_left))
                    .collect(),
                context.notices.clone(),
            ));
            let read = |url: &str| WorkAgentFetch::Read {
                url: url.into(),
                collection: None,
            };
            let fetch = match turn {
                0 => vec![
                    read("https://mail.example.com/inbox"),
                    read("https://mail.example.com/drafts"),
                    read("https://mail.example.com/account/delete"),
                    read("https://public.example.org/guide"),
                ],
                1 => vec![WorkAgentFetch::Search {
                    query: format!("{INBOX_FACT} deadline"),
                }],
                2 => vec![read("https://mail.example.com/sent")],
                _ => vec![],
            };
            Ok(WorkAgentTurnResult {
                output: WorkAgentTurnOutput {
                    say: None,
                    artifacts: vec![],
                    finish: fetch.is_empty(),
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
        panic!("a query repeating a signed-in fact is never sent");
    }
}

fn mail_grant() -> WorkAccountGrantV1 {
    WorkAccountGrantV1 {
        origin: "https://mail.example.com".into(),
        account: "01J8GRANTMAIL".into(),
        tab: None,
        pages: 2,
    }
}
fn begin(work: WorkId, expected: WorkRevision) -> WorkCommandV1 {
    WorkCommandV1 {
        version: 1,
        work,
        expected_revision: expected,
        command: WorkCommandId::generate(),
        intent: WorkRuntimeIntent::BeginAgent {
            grant: WorkAgentGrantV1 {
                provider: WorkSearchProvider::OpenAi,
                model: PUBLIC_SEARCH_MODEL.into(),
                max_turns: 8,
                max_steps: 24,
                browse_hops: 1,
                folders: vec![],
                accounts: vec![mail_grant()],
            },
            limits: WorkExecutionLimits {
                model_tokens: 100_000,
                cost_micro_usd: 100_000,
                operations: 32,
                timeout_seconds: 30,
                max_workers: 4,
            },
        },
    }
}

#[tokio::test]
async fn signed_in_reads_stay_inside_the_grant_budget_and_never_write() {
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Summarize my inbox and compare with https://public.example.org/guide"
                .into(),
        })
        .unwrap();
    let work = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let service = WorkAgentService::new(handle.clone());

    // No approval drafted: the grant is refused before anything is admitted.
    let refused = drive(
        &mut shell,
        &queue,
        service.run(
            profile,
            begin(work, WorkRevision::INITIAL),
            None,
            WorkAgentProviders {
                turn: &Turns {
                    turn: AtomicUsize::new(0),
                    seen: Mutex::new(vec![]),
                },
                search: &NoSearch,
            },
            |_, _| async { panic!("an unapproved grant opens nothing") },
            |_| {},
        ),
    )
    .await;
    assert_eq!(refused.unwrap_err(), WorkError::ReviewRequired);

    crate::work_account_scope::draft(profile, work, mail_grant()).unwrap();
    let turns = Turns {
        turn: AtomicUsize::new(0),
        seen: Mutex::new(vec![]),
    };
    let requests = Mutex::new(Vec::<WorkAgentBrowseRequest>::new());
    let active = AtomicUsize::new(0);
    let projection = drive(
        &mut shell,
        &queue,
        service.run(
            profile,
            begin(work, WorkRevision::INITIAL),
            None,
            WorkAgentProviders {
                turn: &turns,
                search: &NoSearch,
            },
            |probe, request| {
                requests.lock().unwrap().push(request.clone());
                let active = &active;
                async move {
                    // A signed-in page never runs beside another page.
                    let others = active.fetch_add(1, Ordering::SeqCst);
                    assert!(request.account.is_none() || others == 0);
                    let _admission = probe.admit_read_page(request.id).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    let WorkStepKindV1::Read { url, .. } = &request.step else {
                        panic!("reads only");
                    };
                    Ok(WorkBrowserOutcome {
                        status: WorkStepStatus::Succeeded,
                        usage: Some(WorkUsage {
                            model_tokens: 3,
                            cost_micro_usd: 2,
                            operations: 1,
                            accounting: WorkUsageAccounting::Exact,
                        }),
                        intervention: None,
                        measurements: None,
                        helped: false,
                        note: None,
                        // The drafts page agent tried to change something.
                        account_write: url.ends_with("/drafts"),
                        artifacts: vec![WorkArtifactDraft {
                            output: request.output,
                            title: "Page result".into(),
                            evidence: vec![WorkEvidenceLink {
                                extraction_id: WorkArtifactId::from(100),
                                source_id: 1,
                            }],
                            data: WorkArtifactDataV1::Document {
                                paragraphs: vec![INBOX_FACT.into()],
                                formatted: None,
                            },
                        }],
                    })
                }
            },
            |_| {},
        ),
    )
    .await
    .unwrap();

    let requests = requests.lock().unwrap().clone();
    let dispatched: Vec<(&str, bool)> = requests
        .iter()
        .map(|request| {
            let WorkStepKindV1::Read { url, .. } = &request.step else {
                unreachable!()
            };
            (url.as_str(), request.account.is_some())
        })
        .collect();
    assert_eq!(
        dispatched,
        [
            ("https://mail.example.com/inbox", true),
            ("https://mail.example.com/drafts", true),
            ("https://public.example.org/guide", false),
        ]
    );
    assert!(requests
        .iter()
        .filter_map(|request| request.account.as_ref())
        .all(|grant| *grant == mail_grant()));

    let execution = &projection.executions[0];
    assert_eq!(
        execution.accounts,
        [WorkAccountUseV1 {
            host: "mail.example.com".into(),
            pages_used: 2,
            pages: 2,
        }]
    );
    let badges: Vec<bool> = execution
        .steps
        .iter()
        .filter(|step| matches!(step.kind, WorkStepKindV1::Read { .. }))
        .map(|step| step.account.as_ref().is_some_and(|account| account.badge))
        .collect();
    assert_eq!(badges, [true, true, false]);
    assert!(!execution
        .steps
        .iter()
        .any(|step| matches!(step.kind, WorkStepKindV1::Search { .. })));

    let seen = turns.seen.lock().unwrap().clone();
    let origin = "https://mail.example.com".to_owned();
    assert_eq!(seen[0].0, [(origin.clone(), 2)]);
    assert_eq!(seen[1].0, [(origin, 0)]);
    let write = WorkAccountRefusal::AccountWrite.notice("mail.example.com");
    let budget = WorkAccountRefusal::PageBudget.notice("mail.example.com");
    assert!(seen[1].1.contains(&write));
    assert!(seen[2]
        .1
        .iter()
        .any(|notice| notice.starts_with("A search query repeated private context")));
    assert!(seen[3].1.contains(&budget));

    // The grant ended with its request: the same accounts need a new approval.
    let again = drive(
        &mut shell,
        &queue,
        service.run(
            profile,
            begin(work, projection.work.revision),
            None,
            WorkAgentProviders {
                turn: &Turns {
                    turn: AtomicUsize::new(0),
                    seen: Mutex::new(vec![]),
                },
                search: &NoSearch,
            },
            |_, _| async { panic!("a used grant opens nothing") },
            |_| {},
        ),
    )
    .await;
    assert_eq!(again.unwrap_err(), WorkError::ReviewRequired);
}

#[tokio::test]
async fn an_origin_grant_is_drafted_from_an_attached_tab_and_claimed_once() {
    use crate::Command;
    use zephium_core::{ports::engine::EngineEvent, work::environment::*};
    use zephium_ipc::work::*;
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let space = shell.windows.focused().unwrap().space;
    let tab = shell.windows.focused().unwrap().active.unwrap();
    shell.handle(Command::Navigate {
        id: tab,
        input: "https://mail.example.com/u/0/".into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id: tab,
        url: "https://mail.example.com/u/0/".into(),
    }));
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Summarize my inbox".into(),
        })
        .unwrap();
    let work = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let environment = |call| {
        handle
            .submit_work_document(
                WorkRequest::Environment {
                    call,
                    space_available: true,
                    browser_available: true,
                    note_available: false,
                },
                Some(profile),
            )
            .unwrap()
    };
    let created = drive(
        &mut shell,
        &queue,
        environment(WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Create {
                space,
                title: "Mail".into(),
            },
        }),
    )
    .await
    .unwrap();
    let WorkReply::Environment(WorkEnvironmentReply::Applied { snapshot, .. }) = created.reply
    else {
        panic!("environment")
    };
    let attached = drive(
        &mut shell,
        &queue,
        environment(WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Edit {
                id: snapshot.id,
                expected: snapshot.revision,
                edit: WorkEnvironmentEdit::Add {
                    reference: WorkEnvironmentReference::Browser { tab },
                    area: None,
                },
            },
        }),
    )
    .await
    .unwrap();
    let WorkReply::Environment(WorkEnvironmentReply::Applied { snapshot, .. }) = attached.reply
    else {
        panic!("attach")
    };
    let request = |effect, pages| WorkAccountApprovalRequestV1 {
        version: 1,
        work,
        expected_revision: WorkRevision::INITIAL,
        environment: snapshot.id,
        element: snapshot.elements[0].id,
        effect,
        mode: WorkAccountModeV1::Origin,
        pages,
    };
    let approval = crate::work_account_scope::WorkAccountApproval::new(handle.clone());
    for (effect, pages) in [
        (
            WorkAccountEffectV1::Update {
                update: WorkFieldUpdateV1 {
                    field: None,
                    from: "a".into(),
                    to: "b".into(),
                },
            },
            None,
        ),
        (WorkAccountEffectV1::Read, Some(13)),
        (WorkAccountEffectV1::Read, Some(0)),
    ] {
        let refused = drive(
            &mut shell,
            &queue,
            approval.prepare(profile, request(effect, pages)),
        )
        .await;
        assert_eq!(refused.unwrap_err(), WorkError::Invalid);
    }
    let drafted = drive(
        &mut shell,
        &queue,
        approval.prepare(profile, request(WorkAccountEffectV1::Read, None)),
    )
    .await
    .unwrap();
    let WorkReplyV1::AccountGrantDraft {
        work: drafted_work,
        grant,
    } = drafted.reply
    else {
        panic!("origin grant draft");
    };
    assert_eq!(drafted_work, work);
    assert_eq!(grant.origin, "https://mail.example.com");
    assert_eq!(grant.pages, MAX_WORK_ACCOUNT_PAGES);
    assert_eq!(grant.tab, Some(tab));
    grant.validate().unwrap();
    let other = WorkAccountGrantV1 {
        account: "01J8OTHERACCOUNT".into(),
        ..grant.clone()
    };
    assert_eq!(
        crate::work_account_scope::claim_account_grants(profile, work, &[other]),
        Err(WorkError::ReviewRequired)
    );
    assert_eq!(
        crate::work_account_scope::claim_account_grants(
            profile,
            WorkId::generate(),
            std::slice::from_ref(&grant)
        ),
        Err(WorkError::ReviewRequired)
    );
    crate::work_account_scope::claim_account_grants(profile, work, std::slice::from_ref(&grant))
        .unwrap();
    assert_eq!(
        crate::work_account_scope::claim_account_grants(profile, work, &[grant]),
        Err(WorkError::ReviewRequired)
    );
}

#[tokio::test]
async fn a_request_naming_a_signed_in_tab_drafts_its_grant_and_waits() {
    use crate::Command;
    use zephium_core::{
        ports::engine::EngineEvent,
        work::{context::*, environment::*},
    };
    use zephium_ipc::work::*;
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let space = shell.windows.focused().unwrap().space;
    let tab = shell.windows.focused().unwrap().active.unwrap();
    shell.handle(Command::Navigate {
        id: tab,
        input: "https://app.slack.com/client/T1".into(),
    });
    shell.handle(Command::Engine(EngineEvent::UrlChanged {
        id: tab,
        url: "https://app.slack.com/client/T1".into(),
    }));
    // The engine's answer stands in: only Slack's site holds a session.
    let signed_in = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let answer = signed_in.clone();
    crate::work_context::install_session_presence(Arc::new(move |_, hosts: Vec<String>| {
        let (reply, receiver) = std::sync::mpsc::channel();
        let _ = reply.send(
            hosts
                .iter()
                .map(|host| host == "app.slack.com" && answer.load(Ordering::SeqCst))
                .collect(),
        );
        receiver
    }));
    let mut works = Vec::new();
    for objective in [
        "Summarise the launch channel in Slack",
        "Summarise the channel",
    ] {
        let create = handle
            .work_document(WorkIntent::Create {
                objective: objective.into(),
            })
            .unwrap();
        works.push(create.work_id().unwrap());
        drive(&mut shell, &queue, create).await.unwrap();
    }
    let environment = |call| {
        handle
            .submit_work_document(
                WorkRequest::Environment {
                    call,
                    space_available: true,
                    browser_available: true,
                    note_available: false,
                },
                Some(profile),
            )
            .unwrap()
    };
    let created = drive(
        &mut shell,
        &queue,
        environment(WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Create {
                space,
                title: "Slack".into(),
            },
        }),
    )
    .await
    .unwrap();
    let WorkReply::Environment(WorkEnvironmentReply::Applied { snapshot, .. }) = created.reply
    else {
        panic!("environment")
    };
    let attached = drive(
        &mut shell,
        &queue,
        environment(WorkEnvironmentCall::Command {
            command: WorkCommandId::generate(),
            intent: WorkEnvironmentIntent::Edit {
                id: snapshot.id,
                expected: snapshot.revision,
                edit: WorkEnvironmentEdit::Add {
                    reference: WorkEnvironmentReference::Browser { tab },
                    area: None,
                },
            },
        }),
    )
    .await
    .unwrap();
    let WorkReply::Environment(WorkEnvironmentReply::Applied { snapshot, .. }) = attached.reply
    else {
        panic!("attach")
    };
    let context = WorkContextSelectionV1 {
        environment: snapshot.id,
        items: vec![WorkContextSelectionItem {
            element: snapshot.elements[0].id,
            revision: "https://app.slack.com/client/T1".into(),
        }],
        tabs: false,
    };
    let command = |work: WorkId, accounts: Vec<WorkAccountGrantV1>| {
        let mut command = begin(work, WorkRevision::INITIAL);
        if let WorkRuntimeIntent::BeginAgent { grant, .. } = &mut command.intent {
            grant.accounts = accounts;
        }
        command
    };
    let approval = crate::work_account_scope::WorkAccountApproval::new(handle.clone());
    let offered = drive(
        &mut shell,
        &queue,
        approval.signed_in_draft(
            profile,
            &command(works[0], vec![]),
            Some(&context),
            &WorkSignedInV1::Offer,
        ),
    )
    .await
    .unwrap()
    .expect("a drafted grant");
    let WorkReplyV1::AccountGrantDraft { work, grant } = offered.reply else {
        panic!("origin grant draft");
    };
    assert_eq!(work, works[0]);
    assert_eq!(grant.origin, "https://app.slack.com");
    assert_eq!(grant.tab, Some(tab));
    assert_eq!(grant.pages, MAX_WORK_ACCOUNT_PAGES);
    // The same grant as `prepare` drafts: claimed once, by this work.
    crate::work_account_scope::claim_account_grants(profile, work, std::slice::from_ref(&grant))
        .unwrap();
    assert_eq!(
        crate::work_account_scope::claim_account_grants(
            profile,
            work,
            std::slice::from_ref(&grant)
        ),
        Err(WorkError::ReviewRequired)
    );
    // The consented open tabs are candidates too.
    let open = WorkContextSelectionV1 {
        items: vec![],
        tabs: true,
        ..context.clone()
    };
    let from_open = drive(
        &mut shell,
        &queue,
        approval.signed_in_draft(
            profile,
            &command(works[0], vec![]),
            Some(&open),
            &WorkSignedInV1::Offer,
        ),
    )
    .await
    .unwrap();
    assert!(from_open.is_some());
    // Nothing is drafted for a request that does not name the site, one that
    // already carries accounts, one the person declined, or one without context.
    for (work, accounts, choice, context) in [
        (works[1], vec![], WorkSignedInV1::Offer, Some(&context)),
        (works[0], vec![grant], WorkSignedInV1::Offer, Some(&context)),
        (works[0], vec![], WorkSignedInV1::Declined, Some(&context)),
        (works[0], vec![], WorkSignedInV1::Offer, None),
    ] {
        let none = drive(
            &mut shell,
            &queue,
            approval.signed_in_draft(profile, &command(work, accounts), context, &choice),
        )
        .await
        .unwrap();
        assert!(none.is_none());
    }
    // Without a session nothing is offered.
    signed_in.store(false, Ordering::SeqCst);
    let absent = drive(
        &mut shell,
        &queue,
        approval.signed_in_draft(
            profile,
            &command(works[0], vec![]),
            Some(&context),
            &WorkSignedInV1::Offer,
        ),
    )
    .await
    .unwrap();
    assert!(absent.is_none());
    // After signing in, the person asks for one origin by name: no tab.
    let origin = drive(
        &mut shell,
        &queue,
        approval.signed_in_draft(
            profile,
            &command(works[1], vec![]),
            None,
            &WorkSignedInV1::Origin {
                origin: "https://jobs.example.com".into(),
            },
        ),
    )
    .await
    .unwrap()
    .expect("a drafted grant");
    let WorkReplyV1::AccountGrantDraft { grant, .. } = origin.reply else {
        panic!("origin grant draft");
    };
    assert_eq!(
        (grant.origin.as_str(), grant.tab),
        ("https://jobs.example.com", None)
    );
    for origin in ["http://jobs.example.com", "https://jobs.example.com/path"] {
        let refused = drive(
            &mut shell,
            &queue,
            approval.signed_in_draft(
                profile,
                &command(works[1], vec![]),
                None,
                &WorkSignedInV1::Origin {
                    origin: origin.into(),
                },
            ),
        )
        .await;
        assert_eq!(refused.unwrap_err(), WorkError::Invalid);
    }
}
