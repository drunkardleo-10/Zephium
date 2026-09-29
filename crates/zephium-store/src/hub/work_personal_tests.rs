use super::super::tests::{create, session};
use crate::hub::Hub;
use zephium_core::work::{personal::*, port::*, WorkError};

fn call(hub: &mut Hub, request: WorkPersonalRequest) -> Result<WorkPersonalReply, WorkError> {
    match hub.work_document(1.into(), WorkRequest::Personal(request))? {
        WorkReply::Personal(reply) => Ok(reply),
        _ => panic!(),
    }
}

fn remember(
    hub: &mut Hub,
    text: &str,
    work: Option<zephium_core::work::WorkId>,
    now: u64,
) -> WorkMemoryV1 {
    let WorkPersonalReply::Memory(Some(memory)) = call(
        hub,
        WorkPersonalRequest::Remember {
            id: zephium_core::work::WorkId::generate().to_string(),
            text: text.into(),
            kind: WorkMemoryKindV1::Preference,
            work,
            execution: None,
            now_ms: now,
        },
    )
    .unwrap() else {
        panic!()
    };
    memory
}

fn list(
    hub: &mut Hub,
    query: Option<&str>,
    work: Option<zephium_core::work::WorkId>,
) -> Vec<String> {
    let WorkPersonalReply::Memories(memories) = call(
        hub,
        WorkPersonalRequest::Memories {
            query: query.map(str::to_owned),
            work,
            limit: 50,
        },
    )
    .unwrap() else {
        panic!()
    };
    memories.into_iter().map(|memory| memory.text).collect()
}

#[test]
fn memories_are_kept_once_found_by_their_words_and_edited_or_forgotten() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let work = create(&mut hub).id;
    let aisle = remember(
        &mut hub,
        "Prefers aisle seats on long flights",
        Some(work),
        10,
    );
    assert_eq!(aisle.work, Some(work));
    assert_eq!(
        aisle.source.as_deref(),
        Some("Compare implementation choices")
    );
    remember(&mut hub, "Anna leads design", None, 20);
    let again = remember(&mut hub, "prefers aisle  seats on long flights.", None, 30);
    assert_eq!(again.id, aisle.id);
    assert_eq!(again.used_ms.as_deref(), Some("30"));
    assert_eq!(list(&mut hub, None, None).len(), 2);
    assert_eq!(
        list(&mut hub, Some("fli"), None),
        ["Prefers aisle seats on long flights"]
    );
    assert_eq!(
        list(&mut hub, Some("\"design OR"), None),
        ["Anna leads design"]
    );
    assert_eq!(
        list(&mut hub, None, Some(work)),
        ["Prefers aisle seats on long flights"]
    );
    let WorkPersonalReply::Memory(Some(edited)) = call(
        &mut hub,
        WorkPersonalRequest::Edit {
            id: aisle.id.clone(),
            text: "Prefers window seats".into(),
            kind: WorkMemoryKindV1::Preference,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(edited.text, "Prefers window seats");
    assert_eq!(
        list(&mut hub, Some("window"), None),
        ["Prefers window seats"]
    );
    assert!(list(&mut hub, Some("aisle"), None).is_empty());
    call(&mut hub, WorkPersonalRequest::Forget { id: aisle.id }).unwrap();
    assert_eq!(list(&mut hub, None, None), ["Anna leads design"]);
    call(&mut hub, WorkPersonalRequest::ForgetAll).unwrap();
    assert!(list(&mut hub, None, None).is_empty());
    assert!(call(
        &mut hub,
        WorkPersonalRequest::Remember {
            id: zephium_core::work::WorkId::generate().to_string(),
            text: "My password is hunter2".into(),
            kind: WorkMemoryKindV1::Fact,
            work: None,
            execution: None,
            now_ms: 1,
        },
    )
    .is_err());
}

#[test]
fn consent_is_one_standing_answer_per_work_and_source() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let work = create(&mut hub).id;
    let mut consent =
        |source, set| match call(&mut hub, WorkPersonalRequest::Consent { work, source, set })
            .unwrap()
        {
            WorkPersonalReply::Consent(answer) => answer,
            _ => panic!(),
        };
    assert_eq!(consent(WorkContextSourceV1::History, None), None);
    assert_eq!(
        consent(WorkContextSourceV1::History, Some(true)),
        Some(true)
    );
    assert_eq!(
        consent(WorkContextSourceV1::Notes, Some(false)),
        Some(false)
    );
    assert_eq!(consent(WorkContextSourceV1::History, None), Some(true));
    assert_eq!(consent(WorkContextSourceV1::Tabs, None), None);
    assert!(matches!(
        call(
            &mut hub,
            WorkPersonalRequest::Consent {
                work: 99.into(),
                source: WorkContextSourceV1::Tabs,
                set: Some(true)
            }
        ),
        Err(WorkError::NotFound)
    ));
}

#[test]
fn history_search_returns_titles_and_addresses_only() {
    let mut hub = Hub::in_memory().unwrap();
    hub.save(&session()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    hub.profile_conn(1.into())
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO history(url, title, visited_at) VALUES
             ('https://www.google.com/travel/flights?q=WAW-SFO', 'Flights from Warsaw to San Francisco', {now}),
             ('https://news.ycombinator.com/', 'Hacker News', {now})"
        ))
        .unwrap();
    let WorkPersonalReply::History(hits) = call(
        &mut hub,
        WorkPersonalRequest::SearchHistory {
            query: "flights warsaw".into(),
            limit: 5,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Flights from Warsaw to San Francisco");
}
