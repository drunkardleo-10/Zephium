//! A lead run end to end over a scripted model: parts in parallel, found
//! things placed per part, a plan pointing at them, a corrected fault, the
//! reply, and a follow-up that revises instead of adding.
use super::work_planning_tests::{drive, fixture};
use crate::work_agent::{WorkAgentBrowseRequest, WorkBrowserOutcome};
use crate::work_lead::{LeadModel, WorkLeadModels, WorkLeadService};
use crate::work_runtime::WorkArtifactDraft;
use crate::WorkIntent;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use zephium_core::work::{artifact::*, model::*, parts::*, runtime::*, search::*, *};
use zephium_ipc::work::WorkCommandV1;

struct Script {
    calls: Mutex<Vec<String>>,
}

fn call(id: &str, name: &str, arguments: Value) -> WorkModelPart {
    WorkModelPart::ToolCall(WorkModelToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    })
}
fn results(request: &WorkModelRequest) -> String {
    request
        .messages
        .iter()
        .filter_map(|m| match m {
            WorkModelMessage::ToolResults(results) => Some(
                results
                    .iter()
                    .map(|r| r.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn first_key(text: &str) -> String {
    let at = text.find("[s").expect("a source key");
    let end = text[at..].find(']').unwrap();
    text[at + 1..at + end].split(',').next().unwrap().to_owned()
}

impl Script {
    fn lead(&self, request: &WorkModelRequest, turn: usize) -> Vec<WorkModelPart> {
        let seen = results(request);
        let follow_up = request.messages.iter().any(|m| match m {
            WorkModelMessage::User(parts) => parts
                .iter()
                .any(|p| matches!(p, WorkModelPart::Text(t) if t.contains("Earlier requests"))),
            _ => false,
        });
        if follow_up {
            return match turn {
                0 => vec![call("r1", "read_canvas", json!({}))],
                1 => {
                    let plan = seen
                        .lines()
                        .find(|l| l.contains(" plan "))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap()
                        .to_owned();
                    vec![
                        call(
                            "r2",
                            "revise",
                            json!({"id": plan, "data": {"steps": [
                                {"title": "Fly home early", "kind": "travel", "when": "Fri 10 Jan"}
                            ]}}),
                        ),
                        call(
                            "r3",
                            "create",
                            json!({"kind": "reply", "title": "Shorter trip", "data": {
                            "headline": "Home a day early", "text": "The plan now ends on Friday."}}),
                        ),
                    ]
                }
                _ => vec![call(
                    "r4",
                    "finish",
                    json!({"say": "The plan ends a day earlier."}),
                )],
            };
        }
        match turn {
            0 => vec![
                WorkModelPart::Text("Starting the stay and entry parts.".into()),
                call("a", "load_skill", json!({"name": "trip-planning"})),
                call(
                    "b",
                    "start_part",
                    json!({"title": "Stay", "helper": "browser",
                    "goal": "Two homes near the YC office for 6–12 January",
                    "brief": "2 guests, under $300 a night", "service": "airbnb.com"}),
                ),
                call(
                    "c",
                    "start_part",
                    json!({"title": "Entry", "helper": "research",
                    "goal": "What a Polish citizen needs to enter the US"}),
                ),
            ],
            1 => {
                assert!(seen.contains("Part Stay"), "{seen}");
                assert!(seen.contains("Part Entry"), "{seen}");
                let picks = seen
                    .lines()
                    .find(|l| l.contains(" picks \""))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .expect("the Stay part's picks")
                    .to_owned();
                vec![
                    call(
                        "d",
                        "create",
                        json!({"kind": "plan", "title": "Your trip", "data": {
                        "steps": [
                            {"title": "Stay in the Mission", "kind": "stay", "when": "6–12 Jan",
                             "pick": {"artifact": picks, "index": 0}},
                            {"title": "Apply for ESTA", "kind": "task", "when": "Before 3 Jan"}
                        ],
                        "total": {"label": "Estimated total", "value": "$1,740"}}}),
                    ),
                    call(
                        "e",
                        "create",
                        json!({"kind": "reply", "title": "Your trip", "data": {
                        "headline": "x".repeat(90), "text": "Six nights in the Mission."}}),
                    ),
                ]
            }
            2 => {
                assert!(
                    seen.contains("headline is one line of 1 to 80 characters"),
                    "{seen}"
                );
                vec![call(
                    "f",
                    "create",
                    json!({"kind": "reply", "title": "Your trip", "data": {
                    "headline": "Six nights in the Mission, $1,740", "text": "Apply for **ESTA** first."}}),
                )]
            }
            _ => vec![call(
                "g",
                "finish",
                json!({"say": "Your trip is on the canvas.",
                "followups": ["Book the Mission loft"]}),
            )],
        }
    }
    fn browser(&self, request: &WorkModelRequest, turn: usize) -> Vec<WorkModelPart> {
        match turn {
            0 => vec![call(
                "h1",
                "browse",
                json!({"start": "airbnb.com",
                "goal": "Homes near the YC office for 6–12 January, 2 guests",
                "records": {"title": "Homes", "max_items": 2, "columns": [
                    {"name": "price", "value": {"kind": "text"}, "required": false},
                    {"name": "photo", "value": {"kind": "image_url"}, "required": false}]}}),
            )],
            1 => {
                let seen = results(request);
                let key = first_key(&seen);
                vec![call(
                    "h2",
                    "create",
                    json!({"kind": "picks", "title": "Homes near YC", "data": {
                    "facet": "stay", "items": [
                        {"name": "Mission loft", "price": {"display": "$290 / night", "amount": 290, "currency": "USD"},
                         "image_candidates": ["https://a0.muscache.com/im/pictures/loft.jpg"],
                         "recommended": true, "source": key},
                        {"name": "SoMa studio", "price": {"display": "$240 / night"}, "source": key}]},
                    "sources": [key]}),
                )]
            }
            _ => vec![call(
                "h3",
                "finish",
                json!({"summary": "2 homes", "digest": "Mission loft $290/night; SoMa studio $240/night."}),
            )],
        }
    }
    fn research(&self, request: &WorkModelRequest, turn: usize) -> Vec<WorkModelPart> {
        match turn {
            0 => vec![call(
                "e1",
                "web_search",
                json!({"query": "US entry requirements Polish citizens ESTA"}),
            )],
            _ => {
                let key = first_key(&results(request));
                vec![
                    call(
                        "e2",
                        "create",
                        json!({"kind": "list", "title": "Entry needs", "data": {
                        "style": "requirements", "items": [{"title": "ESTA approved before you fly", "source": key}]},
                        "sources": [key]}),
                    ),
                    call(
                        "e3",
                        "finish",
                        json!({"summary": "Entry needs", "digest": format!("ESTA required [{key}]")}),
                    ),
                ]
            }
        }
    }
}

impl WorkModelClient for Script {
    fn call<'a>(
        &'a self,
        request: WorkModelRequest,
        _: &'a (dyn Fn(WorkModelEvent) + Send + Sync),
    ) -> WorkModelFuture<'a> {
        Box::pin(async move {
            let turn = request
                .messages
                .iter()
                .filter(|m| matches!(m, WorkModelMessage::Assistant(_)))
                .count();
            let system = request
                .system
                .iter()
                .map(|b| b.text.as_str())
                .collect::<String>();
            let (who, assistant) = if system.contains("You are the Work agent") {
                ("lead", self.lead(&request, turn))
            } else if system.contains("You work on web pages") {
                ("browser", self.browser(&request, turn))
            } else {
                ("research", self.research(&request, turn))
            };
            self.calls.lock().unwrap().push(format!("{who}:{turn}"));
            Ok(WorkModelOutcome {
                stop: WorkModelStop::ToolUse,
                usage: WorkModelUsage {
                    input_tokens: 2_000,
                    cached_input_tokens: 1_000,
                    output_tokens: 200,
                    reasoning_tokens: 0,
                    cost_micros: None,
                },
                assistant,
            })
        })
    }
}

struct Search;
impl WorkPublicSearchProvider for Search {
    fn search<'a>(
        &'a self,
        scope: &'a WorkPublicSearchScope,
        _: &'a [zephium_core::work::context::WorkContextBody],
        _: WorkExecutionLimits,
    ) -> WorkPublicSearchFuture<'a> {
        Box::pin(async move {
            assert!(scope.query.contains("ESTA"));
            Ok(WorkPublicSearchResult {
                evidence: WorkProviderSearchEvidenceV1 {
                    version: 1,
                    provider: WorkSearchProvider::OpenAi,
                    model: PUBLIC_SEARCH_MODEL.into(),
                    response_model: PUBLIC_SEARCH_MODEL.into(),
                    response_id: "resp_entry".into(),
                    search_call_id: "ws_entry".into(),
                    answer: "Polish citizens travel on ESTA [1].".into(),
                    citations: vec![WorkProviderSearchCitation {
                        url: "https://esta.cbp.dhs.gov/".into(),
                        title: "Official ESTA Application".into(),
                        start_index: 0,
                        end_index: 10,
                    }],
                    actual_input_tokens: 200,
                    actual_output_tokens: 100,
                },
                usage: WorkUsage {
                    model_tokens: 300,
                    cost_micro_usd: 1_000,
                    operations: 1,
                    accounting: WorkUsageAccounting::Exact,
                },
            })
        })
    }
}

fn model(client: Arc<Script>) -> LeadModel {
    LeadModel {
        entry: WorkModelEntry {
            id: "openai/gpt-6".into(),
            model: WorkModelRef {
                provider: WorkModelProvider::OpenAi,
                wire: WorkModelWire::OpenAiResponses,
                model: "gpt-6".into(),
            },
            display_name: "GPT-6".into(),
            roles: vec![WorkModelRole::Lead],
            recommended: true,
            context_window: 400_000,
            max_output: 32_000,
            supports: WorkModelSupports {
                tools: true,
                vision: true,
                prompt_cache: true,
                reasoning: true,
                native_search: true,
            },
            price: Some(WorkModelPrice {
                input: 2_000_000,
                cached_input: 200_000,
                output: 8_000_000,
            }),
        },
        client,
    }
}

fn command(work: WorkId, expected: WorkRevision) -> WorkCommandV1 {
    WorkCommandV1 {
        version: 1,
        work,
        expected_revision: expected,
        command: WorkCommandId::generate(),
        intent: WorkRuntimeIntent::BeginAgent {
            grant: WorkAgentGrantV1 {
                provider: WorkSearchProvider::OpenAi,
                model: PUBLIC_SEARCH_MODEL.into(),
                max_turns: 10,
                max_steps: 32,
                browse_hops: 4,
                folders: vec![],
                accounts: vec![],
                private: false,
                lead: None,
            },
            limits: WorkExecutionLimits {
                model_tokens: 1_000_000,
                cost_micro_usd: 3_000_000,
                operations: 256,
                timeout_seconds: 120,
                max_workers: 4,
            },
        },
    }
}

async fn page(request: WorkAgentBrowseRequest) -> Result<WorkBrowserOutcome, WorkError> {
    let WorkStepKindV1::Read { goal, .. } = &request.step else {
        panic!("a page step");
    };
    assert!(goal.as_deref().is_some_and(|g| g.contains("YC office")));
    let subject = |name: &str, image: &str| WorkSubject {
        name: name.into(),
        descriptor: Some("Entire home".into()),
        homepage: Some(format!("https://www.airbnb.com/rooms/{}", name.len())),
        image_candidates: vec![image.into()],
    };
    Ok(WorkBrowserOutcome {
        status: WorkStepStatus::Succeeded,
        usage: Some(WorkUsage {
            model_tokens: 5_000,
            cost_micro_usd: 20_000,
            operations: 4,
            accounting: WorkUsageAccounting::Exact,
        }),
        artifacts: vec![WorkArtifactDraft {
            output: request.output,
            title: "Homes".into(),
            data: WorkArtifactDataV1::Findings {
                subjects: vec![
                    subject(
                        "Mission loft",
                        "https://a0.muscache.com/im/pictures/loft.jpg",
                    ),
                    subject(
                        "SoMa studio",
                        "https://a0.muscache.com/im/pictures/studio.jpg",
                    ),
                ],
                items: vec![WorkFinding {
                    claim: "Mission loft is $290 a night".into(),
                    subject: Some(0),
                    evidence: vec![0],
                    confidence: WorkConfidence::Supported,
                    detail: None,
                    general_knowledge: false,
                }],
            },
            evidence: vec![WorkEvidenceLink {
                extraction_id: WorkArtifactId::from(900),
                source_id: 1,
            }],
        }],
        intervention: None,
        note: None,
        measurements: None,
        helped: false,
        held_back: false,
        signed_in_elsewhere: false,
    })
}

#[tokio::test]
async fn a_lead_run_splits_into_parts_builds_the_result_and_revises_on_follow_up() {
    let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
    let (mut shell, queue, handle, profile) = fixture(store);
    let create = handle
        .work_document(WorkIntent::Create {
            objective: "Plan my YC batch trip from Warsaw".into(),
        })
        .unwrap();
    let work = create.work_id().unwrap();
    drive(&mut shell, &queue, create).await.unwrap();
    let script = Arc::new(Script {
        calls: Mutex::new(Vec::new()),
    });
    let models = WorkLeadModels {
        lead: model(script.clone()),
        page: model(script.clone()),
        light: model(script.clone()),
    };
    let service = WorkLeadService::new(handle.clone());
    let first = drive(
        &mut shell,
        &queue,
        service.run(
            profile,
            command(work, WorkRevision::INITIAL),
            None,
            models.clone(),
            &Search,
            |_, request| page(request),
            |_| {},
        ),
    )
    .await
    .unwrap();
    let run = &first.executions[0];
    assert!(
        matches!(
            run.status,
            WorkExecutionStatus::NeedsReview | WorkExecutionStatus::Completed
        ),
        "{:?} {:?}",
        run.status,
        run.steps
            .iter()
            .map(|s| (s.status, s.note.clone()))
            .collect::<Vec<_>>()
    );
    assert!(run.agent_grant().unwrap().lead.is_some());
    let titles: Vec<(&str, WorkPartStateV1, Option<&str>)> = run
        .parts
        .iter()
        .map(|p| (p.title.as_str(), p.state, p.summary.as_deref()))
        .collect();
    assert!(
        titles.contains(&("Stay", WorkPartStateV1::Done, Some("2 homes"))),
        "{titles:?}"
    );
    assert!(
        titles.contains(&("Entry", WorkPartStateV1::Done, Some("Entry needs"))),
        "{titles:?}"
    );
    assert_eq!(run.inputs[0].kind, WorkInputKindV1::Skill);
    assert_eq!(run.inputs[0].label, "Trip planning");
    let stay = run.parts.iter().find(|p| p.title == "Stay").unwrap().id;
    let page_step = run
        .steps
        .iter()
        .find(|s| matches!(&s.kind, WorkStepKindV1::Read { goal: Some(_), .. }))
        .unwrap();
    assert_eq!(page_step.part, Some(stay));
    let object = |kind: &str| {
        run.artifacts
            .iter()
            .find(|a| a.data.kind_name() == kind)
            .unwrap_or_else(|| panic!("a {kind}"))
    };
    let picks = object("picks");
    assert_eq!(picks.part, Some(stay));
    assert!(!picks.evidence.is_empty());
    let WorkArtifactDataV1::Plan { steps, .. } = &object("plan").data else {
        panic!()
    };
    assert_eq!(steps[0].pick.as_ref().unwrap().artifact, picks.id);
    assert_eq!(object("plan").part, None);
    assert_eq!(
        run.artifacts
            .iter()
            .filter(|a| a.data.kind_name() == "reply")
            .count(),
        1
    );
    let finish = run.steps.last().unwrap();
    assert!(matches!(&finish.kind, WorkStepKindV1::Finish { followups } if followups.len() == 1));
    assert_eq!(finish.note.as_deref(), Some("Your trip is on the canvas."));
    let calls = script.calls.lock().unwrap().clone();
    assert!(
        calls.contains(&"browser:2".into()) && calls.contains(&"research:1".into()),
        "{calls:?}"
    );

    let second = drive(
        &mut shell,
        &queue,
        service.run(
            profile,
            command(work, first.work.revision),
            None,
            models,
            &Search,
            |_, request| page(request),
            |_| {},
        ),
    )
    .await
    .unwrap();
    let follow_up = second.executions.last().unwrap();
    assert!(follow_up.parts.is_empty());
    let revised = follow_up
        .artifacts
        .iter()
        .find(|a| a.data.kind_name() == "plan")
        .unwrap();
    assert_eq!(revised.revises, Some(object("plan").id));
    assert_eq!(revised.title, "Your trip");
}
