use super::*;

use zephium_core::time::{HourTally, Place};

fn spent(store: &FakeStore) -> Vec<(Place, i64, u32)> {
    let mut totals: Vec<(Place, i64, u32)> = Vec::new();
    for (_, tallies) in store.recorded_time.lock().unwrap().iter() {
        for HourTally { place, tally, .. } in tallies {
            match totals.iter_mut().find(|(kept, ..)| kept == place) {
                Some(entry) => {
                    entry.1 += tally.spent_ms;
                    entry.2 += tally.opens;
                }
                None => totals.push((place.clone(), tally.spent_ms, tally.opens)),
            }
        }
    }
    totals
}

fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(40));
}

#[test]
fn time_counts_while_zephium_is_in_front_and_reaches_the_store_on_the_tick() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "https://m.youtube.com/watch");

    shell.handle(Command::SetAppActive(true));
    pause();
    shell.handle(Command::SetAppActive(false));
    pause();
    shell.handle(Command::Tick);

    let totals = spent(&store);
    assert_eq!(totals.len(), 1);
    let (place, ms, opens) = &totals[0];
    assert_eq!(place, &Place::Site("youtube.com".into()));
    assert_eq!(*opens, 1);
    assert!((40..80).contains(ms), "counted {ms} ms");
}

#[test]
fn sleep_stops_the_clock_and_work_counts_apart_from_sites() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "https://github.com/");
    shell.handle(Command::SetAppActive(true));

    shell.handle(Command::SetSystemAwake(false));
    pause();
    shell.handle(Command::SetSystemAwake(true));
    shell.handle(Command::ShowBrowserPage(Some(crate::BrowserPage::Work)));
    pause();
    shell.handle(Command::ShowBrowserPage(None));
    shell.handle(Command::Tick);

    let totals = spent(&store);
    let work = totals
        .iter()
        .find(|(place, ..)| *place == Place::Work)
        .map(|(_, ms, _)| *ms)
        .unwrap_or_default();
    let github = totals
        .iter()
        .find(|(place, ..)| *place == Place::Site("github.com".into()))
        .map(|(_, ms, _)| *ms)
        .unwrap_or_default();
    assert!(work >= 40, "work counted {work} ms");
    assert!(github < 40, "sleep was counted: {github} ms");
}

#[test]
fn turning_tracking_off_counts_nothing() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, _engine, screen) = setup_with(store.clone());
    shell.handle(Command::Bootstrap);
    shell.handle(Command::SetAppSetting {
        key: "time.track".into(),
        value: "false".into(),
    });
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "https://github.com/");
    shell.handle(Command::SetAppActive(true));
    pause();
    shell.handle(Command::Tick);
    assert!(spent(&store).is_empty());
}

fn focus(shell: &mut Shell, control: zephium_ipc::FocusControl) {
    shell.handle(Command::Operation {
        operation_id: format!("focus-{control:?}"),
        command: Box::new(Command::Focus(control)),
    });
}

fn shut(shell: &mut Shell, sites: &str) {
    shell.handle(Command::SetAppSetting {
        key: "focus.blocked".into(),
        value: sites.into(),
    });
}

#[test]
fn a_round_shuts_sites_covers_the_tab_and_holds_its_media() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    navigate_and_commit(&mut shell, id, "https://m.youtube.com/watch");
    shut(&mut shell, "youtube.com");
    assert!(!shell.focus_covers());

    focus(
        &mut shell,
        zephium_ipc::FocusControl::Start {
            minutes: 25,
            breaks: false,
        },
    );
    let calls = engine.calls();
    assert!(calls.contains(&"focus-gate youtube.com".to_owned()));
    assert!(calls.contains(&format!("media {id} still")));
    assert!(shell.focus_covers());

    focus(
        &mut shell,
        zephium_ipc::FocusControl::Allow {
            site: "youtube.com".into(),
        },
    );
    assert!(!shell.focus_covers());

    focus(&mut shell, zephium_ipc::FocusControl::Stop);
    let calls = engine.calls();
    assert_eq!(calls.last().map(String::as_str), Some("focus-gate open"));
    assert!(calls.contains(&format!("media {id} free")));
    assert!(!shell.focus_covers());
}

#[test]
fn a_fresh_tab_shut_by_focus_is_covered_and_opens_when_let_through() {
    let store = Arc::new(FakeStore::default());
    let (mut shell, engine, screen) = setup_with(store);
    shell.handle(Command::Bootstrap);
    let id = active_id(&screen);
    shut(&mut shell, "x.com");
    focus(
        &mut shell,
        zephium_ipc::FocusControl::Start {
            minutes: 50,
            breaks: true,
        },
    );
    shell.handle(Command::Engine(EngineEvent::FocusBlocked {
        id,
        url: "https://x.com/home".into(),
    }));
    assert!(shell.focus_covers());

    let before = engine.calls().len();
    focus(
        &mut shell,
        zephium_ipc::FocusControl::Allow {
            site: "x.com".into(),
        },
    );
    assert!(engine.calls()[before..]
        .iter()
        .any(|call| call.contains("x.com/home")));
}
