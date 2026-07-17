use super::*;

fn search_sink() -> (Arc<Mutex<Vec<SearchResults>>>, EmitFn) {
    let seen: Arc<Mutex<Vec<SearchResults>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let emit: EmitFn = Box::new(move |p| {
        if let Projection::Search(r) = p {
            sink.lock().unwrap().push(r);
        }
    });
    (seen, emit)
}

#[test]
fn search_ranks_tabs_primary_action_commands_and_history() {
    let (seen, emit) = search_sink();
    let store = Arc::new(FakeStore {
        history: vec![zephium_core::ports::store::HistoryHit {
            url: "https://blog.example.com/".into(),
            title: "Example Blog".into(),
            last_visit: 1,
        }],
        ..Default::default()
    });
    let mut shell = Shell::new(
        Arc::new(FakeEngine::default()),
        store,
        Arc::new(FakeChrome),
        emit,
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    shell.handle(Command::Bootstrap);
    let id = shell.windows.focused().and_then(|w| w.active).unwrap();
    shell.handle(Command::Navigate {
        id,
        input: "example.com".into(),
    });
    shell.handle(Command::Engine(EngineEvent::TitleChanged {
        id,
        title: "Example Site".into(),
    }));

    shell.handle(Command::Search("example".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(last.query, "example");
    let kinds: Vec<&str> = last.results.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, ["tab", "search", "history"]);
    assert!(matches!(
        &last.results[0].action,
        SearchAction::ActivateTab { id: tab } if *tab == id.to_string()
    ));
    // Page-derived image delivery stays absent until a sandboxed broker
    // exists; search projections must not recreate the old protocol URL.
    assert!(last.results[0].favicon.is_none());

    shell.handle(Command::Search("reload".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert!(last.results.iter().any(|r| r.kind == "command"
        && matches!(&r.action, SearchAction::RunCommand { id } if id == "nav.reload")));

    shell.handle(Command::Search("example.com".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert!(last.results.iter().any(|r| r.kind == "url"));

    shell.handle(Command::Search("".into()));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert!(last.results.iter().all(|r| r.kind == "tab"));
}

#[test]
fn stale_history_reply_cannot_replace_a_newer_launcher_query() {
    let (seen, emit) = search_sink();
    let mut shell = Shell::new(
        Arc::new(FakeEngine::default()),
        Arc::new(FakeStore::default()),
        Arc::new(FakeChrome),
        emit,
    );
    shell.handle(Command::SetWindowSize(Size::new(1200.0, 800.0)));
    shell.handle(Command::Bootstrap);
    shell.store_reads = Some(StoreReadQueue::new());
    let profile = shell.windows.focused().unwrap().profile;

    shell.handle(Command::Search("old".into()));
    let old_generation = shell.search.pending.as_ref().unwrap().generation;
    shell.handle(Command::Search("new".into()));
    let new_generation = shell.search.pending.as_ref().unwrap().generation;
    shell.handle(Command::StoreRead(StoreReadResult::History {
        generation: old_generation,
        profile,
        query: "old".into(),
        hits: vec![zephium_core::ports::store::HistoryHit {
            url: "https://old.example/".into(),
            title: "Old".into(),
            last_visit: 1,
        }],
    }));
    assert_eq!(
        shell
            .search
            .pending
            .as_ref()
            .map(|pending| pending.generation),
        Some(new_generation)
    );
    assert_eq!(seen.lock().unwrap().last().unwrap().query, "new");

    shell.handle(Command::StoreRead(StoreReadResult::History {
        generation: new_generation,
        profile,
        query: "new".into(),
        hits: vec![zephium_core::ports::store::HistoryHit {
            url: "https://new.example/".into(),
            title: "New".into(),
            last_visit: 2,
        }],
    }));
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(last.query, "new");
    assert!(last.results.iter().any(|result| {
        matches!(&result.action, SearchAction::OpenUrl { url } if url == "https://new.example/")
    }));
    assert!(last.results.iter().all(|result| {
        !matches!(&result.action, SearchAction::OpenUrl { url } if url == "https://old.example/")
    }));
}
