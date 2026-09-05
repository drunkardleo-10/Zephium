//! Actual production controller/provider worker with a localhost-only fixture.
//! No successful runtime, policy, native shutdown or publication owner is faked.
use super::*;
use std::io::{Read, Write};
use std::sync::mpsc;
use zephium_agent_controller::{AgentBrowserRetention, AgentWorkExtractionTask};
use zephium_agent_provider_transport::AgentProviderTransport;

fn fixture_provider() -> (AgentProviderTransport, std::thread::JoinHandle<usize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut turns = 0;
        for _ in 0..4 {
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "bounded fixture listener");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => panic!("fixture listener"),
                }
            };
            // Darwin inherits the listener's nonblocking flag at accept.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut part = [0; 4096];
            loop {
                let read = socket.read(&mut part).unwrap();
                assert!(read > 0);
                request.extend_from_slice(&part[..read]);
                assert!(request.len() < 128 * 1024);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|value| value.parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let count = request.starts_with(b"POST /v1/responses/input_tokens ");
            let (kind, body) = if count {
                (
                    "application/json",
                    "{\"object\":\"response.input_tokens\",\"input_tokens\":17}".to_owned(),
                )
            } else {
                turns += 1;
                ("text/event-stream", response_stream(turns))
            };
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        turns
    });
    (
        AgentProviderTransport::try_new_loopback(
            AgentProviderTransportConfig::STANDARD,
            &endpoint,
            "http://127.0.0.1:9/v1/messages",
        )
        .unwrap(),
        server,
    )
}

fn response_stream(turn: usize) -> String {
    let created = format!(
        r#"{{"type":"response.created","response":{{"id":"resp_{turn}","status":"in_progress","model":"gpt-5.6-luna","service_tier":"default"}}}}"#
    );
    let mut events = vec![("response.created", created)];
    let terminal_item = if turn == 1 {
        let item = r#"{"type":"function_call","id":"fc_1","call_id":"call_1","name":"extract","arguments":"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}","status":"completed"}"#;
        events.push(("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"extract","arguments":"","status":"in_progress"}}"#.into()));
        events.push(("response.function_call_arguments.delta", r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"}"#.into()));
        events.push(("response.function_call_arguments.done", r#"{"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"scope\":{\"kind\":\"initial\"},\"schema_id\":1}"}"#.into()));
        events.push((
            "response.output_item.done",
            format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        ));
        item.to_owned()
    } else {
        let output = r#"{\"v\":1,\"schema\":1,\"fields\":[{\"name\":\"label\",\"value\":{\"k\":\"text\",\"value\":\"Fixture result\",\"sources\":[\"@r1\"]}}]}"#;
        events.push(("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_2","status":"in_progress","role":"assistant"}}"#.into()));
        events.push(("response.content_part.added", r#"{"type":"response.content_part.added","item_id":"msg_2","output_index":0,"content_index":0,"part":{"type":"output_text"}}"#.into()));
        events.push(("response.output_text.delta", format!(r#"{{"type":"response.output_text.delta","item_id":"msg_2","output_index":0,"content_index":0,"delta":"{output}"}}"#)));
        events.push(("response.output_text.done", format!(r#"{{"type":"response.output_text.done","item_id":"msg_2","output_index":0,"content_index":0,"text":"{output}"}}"#)));
        events.push(("response.content_part.done", r#"{"type":"response.content_part.done","item_id":"msg_2","output_index":0,"content_index":0,"part":{"type":"output_text"}}"#.into()));
        events.push(("response.output_item.done", r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"msg_2","status":"completed","role":"assistant"}}"#.into()));
        r#"{"type":"message","id":"msg_2","status":"completed","role":"assistant","content":[{"type":"output_text"}]}"#.into()
    };
    events.push(("response.completed", format!(r#"{{"type":"response.completed","response":{{"id":"resp_{turn}","status":"completed","model":"gpt-5.6-luna","service_tier":"default","output":[{terminal_item}],"usage":{{"input_tokens":17,"output_tokens":3,"total_tokens":20,"input_tokens_details":{{"cached_tokens":0}},"output_tokens_details":{{"reasoning_tokens":0}}}}}}}}"#)));
    events
        .into_iter()
        .map(|(event, body)| format!("event: {event}\ndata: {body}\n\n"))
        .collect::<String>()
        + "data: [DONE]\n\n"
}

fn prepared_result(
    audit: Arc<dyn AgentAuditPort>,
    calls: Arc<Mutex<Vec<u8>>>,
) -> (PreparedAgentWork, std::thread::JoinHandle<usize>) {
    let (transport, server) = fixture_provider();
    let input = input_with_storage(
        Instant::now() + HEALTHY_RUN_HORIZON,
        ContextProfileStorageClass::Durable,
    )
    .persist_extraction_result()
    .unwrap();
    let task = AgentWorkExtractionTask::try_new(
        vec![SemanticExtractionFieldSchema::try_text("label".into(), true, 64).unwrap()],
        AgentAccountScope::Anonymous,
    )
    .unwrap();
    let (controller, handle) = AgentWorkController::try_new_for_probe(
        input,
        transport,
        AgentProviderCredential::try_new(
            AgentProviderKind::OpenAiResponses,
            "fixture-not-a-secret".into(),
        )
        .unwrap(),
        audit.clone(),
        Box::new(task),
        AgentBrowserRetention::Stateless,
    )
    .unwrap();
    let ports = AgentWorkApplicationPorts::new(
        fixture_engine(),
        audit,
        Box::new(move |sink| {
            Some(Arc::new(NativeFixture {
                sink,
                fault: Fault::None,
                calls,
            }))
        }),
    );
    (
        PreparedAgentWork::from_controller(controller, handle, AgentRuntimeConfig::STANDARD, ports)
            .unwrap(),
        server,
    )
}

fn publication(actor: &ApplicationWork) -> Arc<AgentWorkArtifactPublication> {
    let DurableRequest::Artifact(AgentWorkArtifactRequest::Publish(publication)) =
        &actor.flight.as_ref().unwrap().request
    else {
        panic!("original publication must remain owned")
    };
    publication.clone()
}
fn acknowledge_publication(journal: &Journal) {
    let (AgentWorkArtifactRequest::Publish(publication), completion) =
        lock(&journal.artifacts).pop_front().unwrap()
    else {
        panic!()
    };
    completion(Ok(AgentWorkArtifactReply::Published {
        record: publication.mutation().next(),
        descriptor: publication.descriptor(),
    }));
}

#[test]
fn publication_refusal_loss_conflict_cancel_and_takeover_preserve_exact_result_owner() {
    let _serial = lock(&SERIAL);
    for fault in 0..6 {
        let journal = Arc::new(Journal::default());
        let (mut actor, _owner, view) = coordinator(journal.clone());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (prepared, server) = prepared_result(journal.clone(), calls.clone());
        let run = prepared.run;
        if fault == 3 {
            journal.refuse_artifact.store(true, Ordering::Release);
        }
        start(&mut actor, &journal, prepared);
        pump(&mut actor, |actor| {
            actor
                .flight
                .as_ref()
                .is_some_and(|flight| matches!(flight.purpose, DurablePurpose::ArtifactPublish))
        });
        assert_eq!(server.join().unwrap(), 2);
        assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
        let original = publication(&actor);
        assert!(view.take_extraction().is_none());
        assert!(actor.active.as_ref().unwrap().lifecycle_clean.unwrap());
        if matches!(fault, 1 | 2) {
            let (_, completion) = lock(&journal.artifacts).pop_front().unwrap();
            if fault == 1 {
                drop(completion);
                actor.flight.as_mut().unwrap().deadline = Instant::now();
            } else {
                completion(Ok(AgentWorkArtifactReply::Published {
                    record: original.mutation().expected().unwrap(),
                    descriptor: original.descriptor(),
                }));
            }
        }
        if matches!(fault, 1..=3) {
            actor.poll();
            assert_eq!(
                view.snapshot().phase,
                AgentWorkApplicationPhase::PersistenceUncertain
            );
            assert!(Arc::ptr_eq(&original, &publication(&actor)));
            assert!(view.take_extraction().is_none());
            journal.refuse_artifact.store(false, Ordering::Release);
            actor.control(WorkCommand {
                projection: actor.projection.clone(),
                control: WorkControl::Reconcile,
            });
            assert!(Arc::ptr_eq(&original, &publication(&actor)));
        }
        if matches!(fault, 4 | 5) {
            actor.control(WorkCommand {
                projection: actor.projection.clone(),
                control: WorkControl::Stop {
                    run,
                    reason: if fault == 4 {
                        AgentRuntimeStopReason::Cancelled
                    } else {
                        AgentRuntimeStopReason::HumanTakeover
                    },
                },
            });
        }
        acknowledge_publication(&journal);
        actor.poll();
        assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Succeeded);
        assert!(view.snapshot().failure.is_none());
        assert_eq!(view.snapshot().artifact, Some(original.descriptor()));
        let (mut successor, _successor_owner, _) = coordinator(Arc::new(Journal::default()));
        successor.predecessor = Some(WorkPredecessor {
            projection: actor.projection.clone(),
            record: actor.record.unwrap(),
        });
        assert!(!successor.accepts_predecessor(Some(&actor)));
        while view.take_event().is_some() {}
        assert!(
            !successor.accepts_predecessor(Some(&actor)),
            "published but unconsumed output still belongs to the predecessor"
        );
        let result = view.take_extraction().unwrap();
        assert_eq!(result.stats().values(), 1);
        assert!(view.take_extraction().is_none());
        assert!(successor.accepts_predecessor(Some(&actor)));
        actor.control(WorkCommand {
            projection: actor.projection.clone(),
            control: WorkControl::ReadArtifact {
                record: actor.record.unwrap(),
                profile: original.descriptor().profile(),
            },
        });
        assert!(
            !successor.accepts_predecessor(Some(&actor)),
            "pending archive read"
        );
        let (_, read) = lock(&journal.artifacts).pop_front().unwrap();
        read(Ok(AgentWorkArtifactReply::Read(Some(
            zephium_agentic::AgentWorkArchivedExtraction::decode(
                original.descriptor(),
                original.body(),
            )
            .unwrap(),
        ))));
        actor.poll();
        assert!(
            !successor.accepts_predecessor(Some(&actor)),
            "undelivered historical result"
        );
        assert!(view.take_archived_extraction().is_some());
        assert!(successor.accepts_predecessor(Some(&actor)));
        assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
        assert!(actor.shutdown_until(Instant::now() + Duration::from_secs(1)));
    }
}

#[test]
fn archived_read_failure_and_late_callback_do_not_consume_future_admission_or_overwrite_result() {
    let _serial = lock(&SERIAL);
    let journal = Arc::new(Journal::default());
    let (mut actor, _owner, view) = coordinator(journal.clone());
    actor.initialize();
    journal.settle(|_| {
        Ok(AgentWorkJournalReply::Claimed {
            owner: AgentWorkIncarnation::generate(),
            records: vec![],
        })
    });
    actor.poll();
    for refusal in [false, true] {
        actor.dispatch_request(
            DurableRequest::Artifact(AgentWorkArtifactRequest::Read {
                owner: actor.owner.unwrap(),
                record: historical(
                    actor.owner.unwrap(),
                    ContextRunId::generate(),
                    AgentWorkDisposition::Succeeded,
                ),
                profile: 1_u128.into(),
            }),
            DurablePurpose::ArtifactRead,
            0,
        );
        let (_, completion) = lock(&journal.artifacts).pop_front().unwrap();
        if refusal {
            completion(Err(AgentWorkJournalError::Fenced));
        } else {
            actor.flight.as_mut().unwrap().deadline = Instant::now();
            actor.poll();
            completion(Ok(AgentWorkArtifactReply::Read(None)));
        }
        actor.poll();
        assert_eq!(view.snapshot().phase, AgentWorkApplicationPhase::Ready);
        assert!(view.snapshot().artifact_read.unwrap().is_err());
        assert!(!actor.stopping && !actor.used && actor.flight.is_none());
        assert!(view.take_archived_extraction().is_none());
    }
}

#[test]
fn actual_shell_sqlite_artifact_survives_process_exit_without_restoring_execution() {
    let directory = tempfile::tempdir().unwrap();
    for phase in ["publish", "read"] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "work::tests::artifact_tests::durable_result_process",
                "--ignored",
            ])
            .env("ZEPHIUM_ARTIFACT_TEST_DIRECTORY", directory.path())
            .env("ZEPHIUM_ARTIFACT_TEST_PHASE", phase)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "bounded artifact subprocess {phase}");
    }
}

#[test]
#[ignore = "child process for the actual Shell/Store crash-restart fixture"]
fn durable_result_process() {
    use zephium_core::{
        ports::store::Store,
        profiles::ProfileKind,
        session::{PersistedProfile, PersistedSpace, SessionState},
    };
    let directory = std::env::var_os("ZEPHIUM_ARTIFACT_TEST_DIRECTORY").unwrap();
    let phase = std::env::var("ZEPHIUM_ARTIFACT_TEST_PHASE").unwrap();
    assert!(matches!(phase.as_str(), "publish" | "read"));
    let store =
        Arc::new(zephium_store::SqliteStore::open(std::path::Path::new(&directory)).unwrap());
    if phase == "publish" {
        store.save_session(SessionState {
            profiles: vec![PersistedProfile {
                id: 1_u128.into(),
                name: "Fixture".into(),
                kind: ProfileKind::Default,
            }],
            spaces: vec![PersistedSpace {
                id: 2_u128.into(),
                profile: 1_u128.into(),
                name: "Fixture".into(),
            }],
            items: vec![],
            active_space: Some(2_u128.into()),
            active_item: None,
            splits: None,
            recently_closed: vec![],
        });
        assert!(store.flush());
    }
    let queue = crate::actor::CommandQueue::new();
    let owner = crate::actor::Handle::new(queue.clone());
    let mut shell = crate::Shell::new(
        fixture_engine(),
        store.clone(),
        Arc::new(crate::shell::tests::FakeChrome),
        Box::new(|_| {}),
    );
    shell.attach_queue(queue.clone());
    let view = owner
        .callback_handle()
        .attach_work(store.clone(), fixture_engine())
        .unwrap();
    let pump_shell = |shell: &mut crate::Shell, complete: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !complete() {
            if let Some(command) = queue.try_recv() {
                shell.handle(command);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                Instant::now() < deadline,
                "bounded Shell artifact fixture: {:?}",
                view.snapshot()
            );
        }
    };
    pump_shell(&mut shell, &|| {
        view.snapshot().phase == AgentWorkApplicationPhase::Ready
    });
    if phase == "publish" {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (prepared, server) = prepared_result(store.clone(), calls.clone());
        view.admit(prepared).unwrap();
        pump_shell(&mut shell, &|| {
            view.snapshot().phase == AgentWorkApplicationPhase::Succeeded
        });
        assert_eq!(server.join().unwrap(), 2);
        assert_eq!(*lock(&calls), [1, 2, 3, 4, 5, 6]);
        assert!(view.snapshot().artifact.is_some());
        assert_eq!(view.records().len(), 1);
        // The original native/runtime/provider owners already closed. Exit
        // after durable ACK but before in-memory consumer handoff or Store Drop.
        // The next process has neither the successful owner nor a native factory.
        std::process::exit(0);
    }
    let records = view.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].disposition(), AgentWorkDisposition::Succeeded);
    assert!(view.take_extraction().is_none());
    assert!(view.read_artifact(records[0], 1_u128.into()));
    pump_shell(&mut shell, &|| {
        view.snapshot().artifact_read == Some(Ok(true))
    });
    // One unread result is a bounded backpressure slot, not an overwrite queue.
    assert!(view.read_artifact(records[0], 1_u128.into()));
    pump_shell(&mut shell, &|| {
        view.snapshot().artifact_read == Some(Err(AgentWorkJournalError::Capacity))
    });
    let archive = view.take_archived_extraction().unwrap();
    assert_eq!(archive.trust(), SemanticExtractionTrust::ModelMapped);
    assert_eq!(archive.fields().len(), 1);
    let ArchivedValue::Text { value, sources } = archive.fields()[0].value() else {
        panic!()
    };
    assert_eq!(value, "Fixture result");
    assert!(
        matches!(archive.source(sources[0]).unwrap().content(), ArchivedSourceContent::Text { value } if value == "Fixture result")
    );
    assert!(view.take_archived_extraction().is_none());
    assert_eq!(view.records(), records);
    let (tx, rx) = mpsc::sync_channel(1);
    shell.handle(Command::Shutdown {
        deadline: Instant::now() + Duration::from_secs(2),
        ack: tx,
    });
    assert_eq!(rx.recv().unwrap(), crate::ShutdownOutcome::Clean);
}

#[test]
fn ephemeral_extraction_cannot_opt_into_durable_artifact_storage() {
    assert!(matches!(
        input().persist_extraction_result(),
        Err(AgentWorkFailure::Contract)
    ));
}
