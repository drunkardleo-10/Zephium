//! Page tasks work in the person's session per site: asked once, answers
//! kept, private runs and declined sites private, and a spent budget asks.
use super::*;
use crate::work_sites::SiteSession;

fn browse(start: &str, goal: &str) -> WorkAgentFetch {
    WorkAgentFetch::Browse {
        start: start.into(),
        goal: goal.into(),
        collection: None,
    }
}

fn run_command(
    work: WorkId,
    expected: WorkRevision,
    private: bool,
    cost_micro_usd: u32,
) -> WorkCommandV1 {
    let mut command = begin(work, expected, 60);
    if let WorkRuntimeIntent::BeginAgent { grant, limits } = &mut command.intent {
        grant.private = private;
        limits.cost_micro_usd = cost_micro_usd;
    }
    command
}

/// The profile holds sessions for these sites only; others read as empty.
fn install_sessions() {
    crate::work_context::install_session_presence(Arc::new(|_, sites: Vec<String>| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = sender.send(
            sites
                .iter()
                .map(|site| ["slack.com", "notion.so", "chase.com"].contains(&site.as_str()))
                .collect(),
        );
        receiver
    }));
}

/// What each page opened with: url, a task or a read, and its session.
type Opened = Mutex<Vec<(String, bool, SiteSession)>>;

async fn open(
    opened: &Opened,
    probe: WorkAttemptProbe,
    request: WorkAgentBrowseRequest,
) -> Result<WorkBrowserOutcome, WorkError> {
    let WorkStepKindV1::Read { url, goal, .. } = &request.step else {
        panic!("pages only");
    };
    let draft = goal.as_deref().is_some_and(|goal| goal.contains("reply"));
    opened
        .lock()
        .unwrap()
        .push((url.clone(), goal.is_some(), request.session.clone()));
    let mut outcome = page(probe, request, false).await?;
    outcome.held_back = draft;
    Ok(outcome)
}

/// Answers the next open question with `answer`.
async fn answer_next(
    handle: &crate::Handle,
    profile: ProfileId,
    work: WorkId,
    answer: &str,
) -> String {
    let (execution, step) = running_step(handle, profile, work, |kind| {
        matches!(kind, WorkStepKindV1::Ask { .. })
    })
    .await;
    let prompt = projection(handle, profile, work)
        .await
        .executions
        .iter()
        .flat_map(|execution| &execution.steps)
        .find_map(|fact| match &fact.kind {
            WorkStepKindV1::Ask { prompt, .. } if fact.id == step => Some(prompt.clone()),
            _ => None,
        })
        .unwrap();
    command(
        handle,
        profile,
        work,
        WorkRuntimeIntent::AnswerStep {
            execution,
            step,
            answer: answer.into(),
        },
    )
    .await;
    prompt
}

fn asks(execution: &WorkExecutionFact) -> Vec<(String, Option<String>)> {
    execution
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            WorkStepKindV1::Ask { prompt, answer, .. } => Some((prompt.clone(), answer.clone())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn work_page_tasks_ask_once_per_site_and_keep_the_answer() {
    install_sessions();
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let work = new_work(&mut shell, &queue, &handle, "Catch up on #design in Slack").await;
    let service = WorkAgentService::new(handle.clone());

    // Origin grants are retired: a run carrying one is refused outright.
    let mut granted = run_command(work, WorkRevision::INITIAL, false, 200_000);
    if let WorkRuntimeIntent::BeginAgent { grant, .. } = &mut granted.intent {
        grant.accounts = vec![WorkAccountGrantV1 {
            origin: "https://mail.example.com".into(),
            account: "01J8GRANTMAIL".into(),
            tab: None,
            pages: 2,
        }];
    }
    let refused = drive(
        &mut shell,
        &queue,
        service.run(
            profile,
            granted,
            None,
            WorkAgentProviders {
                turn: &Script::default(),
                search: &Sources::default(),
            },
            |_, _| async { panic!("a retired grant opens nothing") },
            |_| {},
        ),
    )
    .await;
    assert_eq!(refused.unwrap_err(), WorkError::Invalid);

    let script = Script::default();
    script.play([
        output(vec![
            browse("slack.com", "I'll read #design since Monday"),
            browse("https://fresh.example.com/hours", "Find the opening hours"),
        ]),
        output(vec![read("https://app.slack.com/client/T1/C2")]),
        output(vec![browse("slack.com", "Draft a reply to Dana")]),
        output(vec![search("Page detail. Page detail. Page detail. hours")]),
    ]);
    let opened = Opened::default();
    let sources = Sources::default();
    let (result, prompt) = drive(&mut shell, &queue, async {
        tokio::join!(
            service.run(
                profile,
                run_command(work, WorkRevision::INITIAL, false, 200_000),
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &sources,
                },
                |probe, request| open(&opened, probe, request),
                |_| {},
            ),
            answer_next(&handle, profile, work, "Allow")
        )
    })
    .await;
    let execution = &result.unwrap().executions[0];
    settled_with_notes(execution);
    assert_eq!(
        prompt,
        "Work in your Slack? I'll read #design since Monday."
    );
    assert_eq!(asks(execution), [(prompt, Some("Allow".into()))]);

    let opened = opened.into_inner().unwrap();
    let pages: Vec<(&str, bool)> = opened
        .iter()
        .map(|(url, task, _)| (url.as_str(), *task))
        .collect();
    assert_eq!(
        pages,
        [
            ("https://app.slack.com/client", true),
            ("https://fresh.example.com/hours", true),
            ("https://app.slack.com/client/T1/C2", false),
            ("https://app.slack.com/client", true),
        ]
    );
    let SiteSession::Yours { site, account } = &opened[0].2 else {
        panic!("Slack opens in the person's session");
    };
    assert_eq!(site, "slack.com");
    // One key per site for the whole run; a site with no session asks nothing.
    assert_eq!(opened[2].2, opened[0].2);
    assert_eq!(opened[3].2, opened[0].2);
    assert!(
        matches!(&opened[1].2, SiteSession::Yours { site, account: other } if site == "example.com" && other != account)
    );
    let badges: Vec<Option<&str>> = execution
        .steps
        .iter()
        .filter(|step| matches!(step.kind, WorkStepKindV1::Read { .. }))
        .map(|step| step.account.as_ref().map(|account| account.host.as_str()))
        .collect();
    assert_eq!(
        badges,
        [
            Some("app.slack.com"),
            Some("fresh.example.com"),
            Some("app.slack.com"),
            Some("app.slack.com"),
        ]
    );
    let seen = script.seen.lock().unwrap();
    assert!(seen[1].sites.contains(&("slack.com".into(), "yours")));
    // The held-back draft reaches the lead, and a search repeating the
    // person's own page never runs.
    assert!(seen[3]
        .notices
        .iter()
        .any(|n| n.starts_with("A browse stopped before a step")));
    assert!(seen[4]
        .notices
        .iter()
        .any(|n| n.starts_with("A search query repeated private context")));
    assert!(!execution
        .steps
        .iter()
        .any(|step| matches!(step.kind, WorkStepKindV1::Search { .. })));
}

#[tokio::test]
async fn work_declined_never_sensitive_and_private_runs_keep_the_session_closed() {
    install_sessions();
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let service = WorkAgentService::new(handle.clone());
    drive(
        &mut shell,
        &queue,
        crate::work_sites::set_standing(
            &handle,
            profile,
            "notion.so".into(),
            Some(zephium_core::work::sites::WorkSiteAccessV1::Never),
        ),
    )
    .await
    .unwrap();

    // Not now: Slack is worked privately for the rest of the run.
    let work = new_work(&mut shell, &queue, &handle, "Check Slack").await;
    let script = Script::default();
    script.play([
        output(vec![browse("slack.com", "Read #design")]),
        output(vec![browse(
            "https://app.slack.com/client",
            "Read #general",
        )]),
        output(vec![browse("chase.com", "Read my balance")]),
    ]);
    let opened = Opened::default();
    let sources = Sources::default();
    let (result, _) = drive(&mut shell, &queue, async {
        tokio::join!(
            service.run(
                profile,
                run_command(work, WorkRevision::INITIAL, false, 200_000),
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &sources,
                },
                |probe, request| open(&opened, probe, request),
                |_| {},
            ),
            answer_next(&handle, profile, work, "Not now")
        )
    })
    .await;
    let execution = &result.unwrap().executions[0];
    assert_eq!(asks(execution).len(), 1);
    let sessions: Vec<SiteSession> = opened
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|(_, _, session)| session)
        .collect();
    assert!(
        sessions.len() == 3
            && sessions
                .iter()
                .all(|session| *session == SiteSession::Private)
    );
    {
        let seen = script.seen.lock().unwrap();
        assert!(seen[1]
            .notices
            .iter()
            .any(|n| n.starts_with("Slack is worked on without the person's session")));
        assert!(seen[3].notices.iter().any(|n| n.contains("sensitive site")));
    }

    // Always is kept for the profile; Never stays closed; a private run asks nothing.
    let work = new_work(&mut shell, &queue, &handle, "Slack again").await;
    let script = Script::default();
    script.play([output(vec![browse("slack.com", "Read #design")])]);
    let opened = Opened::default();
    let sources = Sources::default();
    let (result, _) = drive(&mut shell, &queue, async {
        tokio::join!(
            service.run(
                profile,
                run_command(work, WorkRevision::INITIAL, false, 200_000),
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &sources,
                },
                |probe, request| open(&opened, probe, request),
                |_| {},
            ),
            answer_next(&handle, profile, work, "Always for Slack")
        )
    })
    .await;
    result.unwrap();
    let standing = drive(
        &mut shell,
        &queue,
        crate::work_sites::standing(&handle, profile),
    )
    .await
    .unwrap();
    assert_eq!(
        standing
            .iter()
            .map(|entry| (entry.site.as_str(), entry.access))
            .collect::<Vec<_>>(),
        [
            (
                "notion.so",
                zephium_core::work::sites::WorkSiteAccessV1::Never
            ),
            (
                "slack.com",
                zephium_core::work::sites::WorkSiteAccessV1::Always
            ),
        ]
    );
    for private in [false, true] {
        let work = new_work(&mut shell, &queue, &handle, "Slack and Notion").await;
        let script = Script::default();
        script.play([output(vec![
            browse("slack.com", "Read #design"),
            browse("notion.so", "Open the plan"),
        ])]);
        let opened = Opened::default();
        let sources = Sources::default();
        let result = drive(
            &mut shell,
            &queue,
            service.run(
                profile,
                run_command(work, WorkRevision::INITIAL, private, 200_000),
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &sources,
                },
                |probe, request| open(&opened, probe, request),
                |_| {},
            ),
        )
        .await;
        assert!(asks(&result.unwrap().executions[0]).is_empty());
        let sessions: Vec<bool> = opened
            .into_inner()
            .unwrap()
            .into_iter()
            .map(|(_, _, session)| session != SiteSession::Private)
            .collect();
        assert_eq!(sessions, [!private, false]);
    }
}

#[tokio::test]
async fn work_a_spent_budget_asks_to_keep_going_and_grows_by_the_same_amount() {
    install_sessions();
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let service = WorkAgentService::new(handle.clone());
    let work = new_work(&mut shell, &queue, &handle, "Find the opening hours").await;
    let script = Script::default();
    script.play([output(vec![browse(
        "https://fresh.example.com/hours",
        "Find the opening hours",
    )])]);
    let opened = Opened::default();
    let sources = Sources::default();
    let (result, prompt) = drive(&mut shell, &queue, async {
        tokio::join!(
            service.run(
                profile,
                run_command(work, WorkRevision::INITIAL, false, 100_050),
                None,
                WorkAgentProviders {
                    turn: &script,
                    search: &sources,
                },
                |probe, request| open(&opened, probe, request),
                |_| {},
            ),
            answer_next(&handle, profile, work, "Keep going")
        )
    })
    .await;
    let execution = &result.unwrap().executions[0];
    assert_eq!(prompt, "Used $0.00. Keep going?");
    assert_eq!(execution.spec.limits.cost_micro_usd, 200_100);
    assert_eq!(opened.into_inner().unwrap().len(), 1);
    settled_with_notes(execution);
}
