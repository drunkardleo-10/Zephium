use super::*;

use zephium_ipc::{HistoryCall, HistoryError, HistoryRange, HistoryResponse};

type Answer = Arc<Mutex<Option<HistoryResponse>>>;

fn dispatch(shell: &mut Shell, profile: ProfileId, call: HistoryCall) -> Answer {
    let slot: Answer = Arc::new(Mutex::new(None));
    let sink = slot.clone();
    shell.handle(Command::HistoryCall {
        expected_profile: profile,
        call: Box::new(call),
        done: crate::api::HistoryCompletion::new(move |response| {
            *sink.lock().unwrap() = Some(response);
        }),
    });
    slot
}

fn taken(answer: &Answer) -> HistoryResponse {
    answer
        .lock()
        .unwrap()
        .take()
        .expect("every history call is answered")
}

fn page(query: &str, limit: u16) -> HistoryCall {
    HistoryCall::Page {
        query: query.into(),
        before: None,
        limit,
    }
}

/// Drives the reader thread the way the actor does and hands its reply back.
fn drain_reads(
    shell: &mut Shell,
    store: Arc<FakeStore>,
    queue: crate::store_reads::StoreReadQueue,
) {
    let reader_store: SharedStore = store;
    let reader_queue = queue.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        crate::store_reads::run_for_test(reader_store, reader_queue, tx);
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must answer a requested history call");
    queue.stop();
    worker.join().unwrap();
    shell.handle(Command::StoreRead(result));
}

/// Visits are deduplicated per tab within a second, so each address gets its
/// own tab rather than racing that rule.
fn visit(shell: &mut Shell, screen: &Screen, host: &str) -> ItemId {
    shell.handle(Command::Open);
    let id = active_id(screen);
    navigate_and_commit(shell, id, host);
    id
}

fn browsed(store: Arc<FakeStore>) -> (Shell, Screen, crate::store_reads::StoreReadQueue) {
    let (mut shell, _engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    let queue = crate::store_reads::StoreReadQueue::new();
    shell.store_reads = Some(queue.clone());
    (shell, screen, queue)
}

#[test]
fn a_history_page_carries_icon_references_for_addresses_chrome_already_holds() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, queue) = browsed(store.clone());
    let id = visit(&mut shell, &screen, "visited.example");
    shell.handle(Command::Engine(EngineEvent::FaviconPixels {
        id,
        page_url: "https://visited.example/".into(),
        rgba: vec![13; zephium_core::icon::RGBA32_BYTES],
    }));
    let profile = shell.windows.focused().unwrap().profile;

    let answer = dispatch(&mut shell, profile, page("", 50));
    drain_reads(&mut shell, store, queue);

    let HistoryResponse::Page { visits, next } = taken(&answer) else {
        panic!("the reader answers with a page")
    };
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0].url, "https://visited.example/");
    assert_eq!(
        visits[0].icon.as_ref().map(|icon| icon.origin.as_str()),
        Some("https://visited.example")
    );
    assert!(next.is_none(), "a short page is the end of the list");
}

#[test]
fn a_full_page_offers_a_cursor_and_a_short_one_ends_the_list() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, queue) = browsed(store.clone());
    for index in 0..3 {
        visit(&mut shell, &screen, &format!("page-{index}.example"));
    }
    let profile = shell.windows.focused().unwrap().profile;

    let answer = dispatch(&mut shell, profile, page("", 2));
    drain_reads(&mut shell, store, queue);

    let HistoryResponse::Page { visits, next } = taken(&answer) else {
        panic!("the reader answers with a page")
    };
    assert_eq!(visits.len(), 2);
    assert!(next.is_some(), "a full page may have more behind it");
}

#[test]
fn history_never_answers_a_profile_other_than_the_focused_one() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, _queue) = browsed(store);
    visit(&mut shell, &screen, "private.example");

    let answer = dispatch(&mut shell, ProfileId::from(9_999), page("", 50));

    assert!(matches!(
        taken(&answer),
        HistoryResponse::Error {
            error: HistoryError::Invalid
        }
    ));
}

#[test]
fn forgetting_an_address_reports_what_it_removed() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, queue) = browsed(store.clone());
    visit(&mut shell, &screen, "forget.example");
    visit(&mut shell, &screen, "keep.example");
    let profile = shell.windows.focused().unwrap().profile;

    let answer = dispatch(
        &mut shell,
        profile,
        HistoryCall::Forget {
            urls: vec!["https://forget.example/".into()],
        },
    );
    drain_reads(&mut shell, store.clone(), queue);

    assert!(matches!(
        taken(&answer),
        HistoryResponse::Removed { count: 1 }
    ));
    assert_eq!(store.history_page(profile, "", None, 50).len(), 1);
}

#[test]
fn clearing_everything_empties_the_list() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, queue) = browsed(store.clone());
    visit(&mut shell, &screen, "one.example");
    visit(&mut shell, &screen, "two.example");
    let profile = shell.windows.focused().unwrap().profile;

    let answer = dispatch(
        &mut shell,
        profile,
        HistoryCall::Clear {
            range: HistoryRange::Everything,
        },
    );
    drain_reads(&mut shell, store.clone(), queue);

    assert!(matches!(
        taken(&answer),
        HistoryResponse::Removed { count: 2 }
    ));
    assert!(store.history_page(profile, "", None, 50).is_empty());
}

#[test]
fn a_title_published_after_the_url_commits_reaches_recorded_history() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, _queue) = browsed(store.clone());
    let id = visit(&mut shell, &screen, "article.example");
    let profile = shell.windows.focused().unwrap().profile;
    // The visit was recorded when the URL committed, before the document had a
    // title of its own.
    assert_eq!(
        store.history_page(profile, "", None, 1)[0].title,
        "article.example"
    );

    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "The Real Headline".into(),
    }));

    assert_eq!(
        store.history_page(profile, "", None, 1)[0].title,
        "The Real Headline"
    );
}

#[test]
fn a_shutting_down_shell_answers_history_calls_it_can_no_longer_serve() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, screen, _queue) = browsed(store);
    visit(&mut shell, &screen, "pending.example");
    let profile = shell.windows.focused().unwrap().profile;

    let answer = dispatch(&mut shell, profile, page("", 50));
    assert!(
        answer.lock().unwrap().is_none(),
        "the reader has not replied"
    );
    shell.fail_pending_history_calls();

    assert!(matches!(
        taken(&answer),
        HistoryResponse::Error {
            error: HistoryError::Unavailable
        }
    ));
}
