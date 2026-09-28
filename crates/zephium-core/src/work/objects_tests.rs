use super::*;
use crate::work::{
    artifact::{WorkArtifactDataV1 as Data, WorkArtifactField as F, *},
    model::{WorkModelProvider, WorkModelRef, WorkModelWire},
    parts::*,
    runtime::*,
    search::*,
    *,
};
use serde_json::json;

fn data(value: serde_json::Value) -> Data {
    serde_json::from_value(value).expect("the wire shape parses")
}
fn fault(value: serde_json::Value, evidence: usize) -> Option<WorkObjectFault> {
    data(value).lead_fault(evidence)
}
fn field(value: serde_json::Value) -> Option<F> {
    fault(value, 0).map(|fault| fault.field)
}

#[test]
fn every_kind_round_trips_its_wire_shape() {
    let shapes = [
        json!({"kind":"reply","headline":"128 PLN","text":"At today's rate, **30 EUR** is about 128 PLN.",
            "figures":[{"label":"Rate","value":"4.27","note":"ECB, 28 Sep"}],"points":["Cards add about 2%"]}),
        json!({"kind":"picks","facet":"flight","items":[{"name":"LOT LO 3","subtitle":"Direct",
            "price":{"display":"$1,240 return","amount":"1240","currency":"USD"},
            "facts":[{"label":"Bags","value":"1 checked","kind":"yes"}],
            "rating":{"value":"4.5","max":5,"count":120},"why":"Only direct flight","tags":["direct"],
            "recommended":true,"route":{"from":"WAW","to":"SFO","depart":"10:05","arrive":"13:40",
            "duration":"12h 35m","stops":0,"carrier":"LOT","carrier_host":"lot.com"},"source":0}]}),
        json!({"kind":"plan","steps":[{"when":"Mon 6 Jan","title":"Fly WAW to SFO","kind":"travel",
            "cost":"$1,240","place":"SFO","pick":{"artifact":"01J00000000000000000000000","index":0}}],
            "total":{"label":"Total","value":"$4,380"},"checkable":true}),
        json!({"kind":"list","style":"messages","items":[{"title":"Reply to Ana about the deck",
            "due":"Today","priority":"high","from":{"host":"slack.com","app":"Slack","who":"Ana",
            "when":"09:12","quote":"Can you look at the deck?","url":"https://app.slack.com/client/T1/C2"}}]}),
        json!({"kind":"sheet","columns":[{"label":"Provider","kind":"entity"},
            {"label":"Price","kind":"money","currency":"USD","best":"min"},
            {"label":"Edge","kind":"yes_no"},{"label":"Rating","kind":"rating"}],
            "rows":[{"cells":["Cloudflare","5","yes","4/5"],"entity":{"logo_host":"cloudflare.com"}},
            {"cells":["Hetzner","4.5","no",""]}],"note":"List prices, September 2026"}),
        json!({"kind":"plot","style":"range","x":{"kind":"category"},
            "y":{"format":"money","currency":"USD","label":"Monthly"},
            "series":[{"name":"Cost","points":[{"x":"Small","y":"550","y2":"900"},{"x":"Large","y":"4000","y2":"9500"}]}],
            "headline":{"label":"Typical","value":"$550–9,500"},"basis":"Published list prices"}),
        json!({"kind":"diff","path":"src/lib.rs","language":"rust","summary":"Guard the empty case",
            "hunks":[{"old_start":10,"new_start":10,"lines":[{"op":"ctx","text":"fn f() {"},
            {"op":"del","text":"    x.unwrap()"},{"op":"add","text":"    x.unwrap_or_default()"}]}]}),
        json!({"kind":"draft","destination":"email","to":"ana@example.com","subject":"The deck",
            "body":"Hi Ana,\n\nThe deck looks good. See [the notes](https://example.com/notes).",
            "target_url":"https://mail.google.com/"}),
        json!({"kind":"media","medium":"video","url":"https://www.youtube.com/watch?v=abc","title":"Lecture 1",
            "provider":"youtube","poster":"https://i.ytimg.com/vi/abc/hqdefault.jpg","duration":"1:12:03","start_secs":30}),
    ];
    for shape in &shapes {
        let parsed = data(shape.clone());
        assert_eq!(serde_json::to_value(&parsed).unwrap(), *shape);
        assert_eq!(parsed.lead_fault(1), None, "{}", parsed.kind_name());
    }
}

#[test]
fn limits_come_back_as_the_field_and_the_item() {
    let long = "x".repeat(61);
    let picks = fault(
        json!({"kind":"picks","facet":"stay","items":[{"name":"Loft"},{"name":long}]}),
        0,
    )
    .unwrap();
    assert_eq!(picks.field, F::PickName);
    assert_eq!(picks.index, Some(1));
    assert!(picks.describe().contains("60 characters"));
    assert!(picks.describe().contains("position 2"));
    assert!(
        picks.describe().contains("it has 61 characters"),
        "{}",
        picks.describe()
    );
    assert_eq!(
        field(
            json!({"kind":"picks","facet":"stay","items":[{"name":"A","recommended":true},{"name":"B","recommended":true}]})
        ),
        Some(F::PickRecommended)
    );
    assert_eq!(
        field(json!({"kind":"reply","headline":"H","text":"See [this](https://x.test)"})),
        Some(F::ReplyMarkup)
    );
    assert_eq!(
        field(json!({"kind":"reply","headline":"H","text":"- one\n- two"})),
        Some(F::ReplyMarkup)
    );
    assert_eq!(
        field(json!({"kind":"reply","headline":"H","text":"x".repeat(481)})),
        Some(F::ReplyText)
    );
    assert_eq!(
        field(
            json!({"kind":"sheet","columns":[{"label":"Note","kind":"text"}],
            "rows":[{"cells":["A sentence that goes on well past the sixty character limit of a cell"]}]})
        ),
        Some(F::SheetCell)
    );
    assert_eq!(
        field(
            json!({"kind":"sheet","columns":[{"label":"Price","kind":"money"}],"rows":[{"cells":["5"]}]})
        ),
        Some(F::SheetColumn)
    );
    assert_eq!(
        field(
            json!({"kind":"sheet","columns":[{"label":"Ok","kind":"yes_no"}],"rows":[{"cells":["maybe"]}]})
        ),
        Some(F::SheetCell)
    );
    assert_eq!(
        field(
            json!({"kind":"plot","style":"bar","x":{"kind":"category"},"y":{"format":"number"},
            "series":[{"name":"S","points":[{"x":"a","y":"3"},{"x":"b","y":"3"}]}],"basis":"Counted"})
        ),
        Some(F::PlotValues)
    );
    assert_eq!(
        field(
            json!({"kind":"plot","style":"bar","x":{"kind":"category"},"y":{"format":"number"},
            "series":[{"name":"S","points":[{"x":"a"}]}],"basis":"Counted"})
        ),
        Some(F::PlotValues)
    );
    assert_eq!(
        field(
            json!({"kind":"media","medium":"video","url":"https://evil.test/v","provider":"youtube"})
        ),
        Some(F::MediaProvider)
    );
    assert_eq!(
        field(json!({"kind":"media","medium":"image","url":"http://example.com/a.jpg"})),
        Some(F::MediaUrl)
    );
    assert_eq!(
        field(json!({"kind":"draft","destination":"slack","subject":"Hi","body":"Hello"})),
        Some(F::DraftSubject)
    );
    assert_eq!(
        field(json!({"kind":"plan","steps":[{"title":"Go","kind":"travel","source":2}]})),
        Some(F::ItemSource)
    );
    assert_eq!(
        fault(
            json!({"kind":"plan","steps":[{"title":"Go","kind":"travel","source":0}]}),
            1
        ),
        None
    );
}

#[test]
fn a_lead_run_refuses_legacy_kinds_and_holds_diagrams_to_its_limits() {
    assert_eq!(
        field(json!({"kind":"table","columns":["a"],"rows":[["b"]]})),
        Some(F::LegacyKind)
    );
    assert_eq!(
        field(json!({"kind":"answer","markdown":"Hello"})),
        Some(F::LegacyKind)
    );
    let node =
        |id: usize, name: &str| json!({"id": format!("n{id}"), "name": name, "kind": "service"});
    let nodes: Vec<_> = (0..25).map(|i| node(i, "Service")).collect();
    assert_eq!(
        field(json!({"kind":"diagram","nodes":nodes,"edges":[]})),
        Some(F::LeadDiagramSize)
    );
    let named = fault(
        json!({"kind":"diagram","nodes":[node(0, "API"), node(1, "A name longer than twenty-eight")],"edges":[]}),
        0,
    )
    .unwrap();
    assert_eq!((named.field, named.index), (F::LeadDiagramNode, Some(1)));
    // Saved diagrams keep their own, wider limits.
    let saved = data(
        json!({"kind":"diagram","nodes":[node(0, "A name longer than twenty-eight")],"edges":[]}),
    );
    assert_eq!(saved.validate(0), Ok(()));
}

#[test]
fn observed_pictures_and_links_need_a_source() {
    let artifact = |data: Data, evidence: Vec<WorkEvidenceLink>| WorkArtifactV1 {
        version: 1,
        id: 1.into(),
        execution: 2.into(),
        node: 3.into(),
        attempt: 4.into(),
        output: "result".into(),
        title: "Homes".into(),
        data,
        evidence,
        review: WorkOutputReview::SourceMappedNeedsReview,
        presentation: WorkArtifactPresentationV1::Automatic,
        general_knowledge: true,
        revises: None,
        part: None,
    };
    let pictured = data(
        json!({"kind":"picks","facet":"stay","items":[{"name":"Loft",
        "image_candidates":["https://a0.muscache.com/im/pictures/1.jpg"]}]}),
    );
    assert_eq!(
        artifact(pictured.clone(), vec![]).validate(),
        Err(WorkError::Invalid)
    );
    let mut cited = artifact(
        pictured,
        vec![WorkEvidenceLink {
            extraction_id: 9.into(),
            source_id: 1,
        }],
    );
    cited.general_knowledge = false;
    assert_eq!(cited.validate(), Ok(()));
    let known = data(
        json!({"kind":"reply","headline":"Frank Herbert","text":"He wrote *Dune*, published in 1965."}),
    );
    assert_eq!(artifact(known, vec![]).validate(), Ok(()));
}

fn lead_execution() -> (WorkPlanRevision, WorkExecutionFact, WorkRevision) {
    let plan = WorkPlanRevision {
        context: None,
        author: WorkAuthor::User,
        revision: WorkRevision::new(2).unwrap(),
        basis_revision: WorkRevision::INITIAL,
        draft: WorkPlanDraft {
            id: 2.into(),
            nodes: vec![WorkPlanNode {
                id: 3.into(),
                objective: "Plan the trip".into(),
                dependencies: vec![],
                outputs: vec![WorkExpectedOutput {
                    name: "result".into(),
                    description: "The result".into(),
                    review: WorkOutputReview::SourceMappedNeedsReview,
                }],
            }],
        },
    };
    let limits = WorkExecutionLimits {
        model_tokens: 1_000_000,
        cost_micro_usd: 3_000_000,
        operations: 256,
        timeout_seconds: 1800,
        max_workers: 4,
    };
    let grant = WorkAgentGrantV1 {
        provider: WorkSearchProvider::OpenAi,
        model: PUBLIC_SEARCH_MODEL.into(),
        max_turns: 200,
        max_steps: 255,
        browse_hops: 4,
        folders: vec![],
        accounts: vec![],
        private: false,
        lead: Some(WorkModelRef {
            provider: WorkModelProvider::Anthropic,
            wire: WorkModelWire::AnthropicMessages,
            model: "claude-sonnet-5".into(),
        }),
    };
    grant.validate().unwrap();
    assert!(WorkAgentGrantV1 {
        lead: None,
        ..grant.clone()
    }
    .validate()
    .is_err());
    let spec = WorkExecutionSpec::agent(&plan, limits, grant).unwrap();
    let fact = WorkExecutionFact {
        authorization: WorkExecutionAuthorization::UserDirectedAgent,
        id: 7.into(),
        approved_revision: WorkRevision::new(3).unwrap(),
        spec,
        status: WorkExecutionStatus::Running,
        attempts: vec![WorkAttemptFact {
            id: 500.into(),
            node: 3.into(),
            status: WorkAttemptStatus::Running,
            usage: None,
        }],
        artifacts: vec![],
        provider_evidence: vec![],
        file_evidence: vec![],
        command_evidence: vec![],
        folder_approvals: vec![],
        user_artifacts: vec![],
        intervention: None,
        steps: vec![],
        accounts: vec![],
        parts: vec![],
        inputs: vec![],
    };
    (plan, fact, WorkRevision::new(9).unwrap())
}

fn part(id: u128, state: WorkPartStateV1) -> WorkPartFactV1 {
    WorkPartFactV1 {
        id: id.into(),
        title: "Stay".into(),
        helper: WorkHelperV1::Browser,
        service: Some(WorkPartServiceV1 {
            host: Some("airbnb.com".into()),
            connection: None,
        }),
        goal: "Three homes near the YC office for the batch dates".into(),
        state,
        started_ms: (state != WorkPartStateV1::Planned).then(|| "1790000000000".into()),
        ended_ms: state.terminal().then(|| "1790000100000".into()),
        summary: state.terminal().then(|| "3 homes".into()),
    }
}

#[test]
fn parts_and_inputs_are_lead_facts_that_steps_and_objects_name() {
    let (plan, mut fact, revision) = lead_execution();
    fact.parts.push(part(60, WorkPartStateV1::Running));
    fact.inputs.push(WorkInputFactV1 {
        kind: WorkInputKindV1::Skill,
        label: "Trip planning".into(),
        count: None,
        reference: Some("trip-planning".into()),
    });
    let step = WorkStepFact {
        id: 1.into(),
        turn: 1,
        kind: WorkStepKindV1::Search {
            query: "homes near YC".into(),
        },
        status: WorkStepStatus::Running,
        usage: None,
        artifacts: vec![],
        evidence: None,
        note: None,
        measurements: None,
        local: None,
        account: None,
        part: Some(60.into()),
    };
    fact.steps.push(step.clone());
    fact.validate(&plan, revision).unwrap();
    let mut orphan = fact.clone();
    orphan.steps[0].part = Some(61.into());
    assert!(orphan.validate(&plan, revision).is_err());
    let mut legacy = fact.clone();
    legacy.spec.nodes[0].capability = WorkCapability::Agent {
        grant: WorkAgentGrantV1 {
            lead: None,
            max_turns: 8,
            max_steps: 24,
            ..fact.agent_grant().unwrap().clone()
        },
    };
    assert!(legacy.validate(&plan, revision).is_err());
    for broken in [
        WorkPartFactV1 {
            title: "A title longer than twenty-four".into(),
            ..part(62, WorkPartStateV1::Running)
        },
        WorkPartFactV1 {
            ended_ms: None,
            ..part(62, WorkPartStateV1::Done)
        },
        WorkPartFactV1 {
            service: Some(WorkPartServiceV1 {
                host: Some("https://airbnb.com".into()),
                connection: None,
            }),
            ..part(62, WorkPartStateV1::Running)
        },
    ] {
        assert!(broken.validate().is_err());
    }
    let wire = serde_json::to_value(&fact).unwrap();
    assert_eq!(wire["parts"][0]["state"], "running");
    assert_eq!(wire["steps"][0]["part"], fact.parts[0].id.to_string());
    let back: WorkExecutionFact = serde_json::from_value(wire).unwrap();
    assert_eq!(back, fact);
}

#[test]
fn a_revision_names_one_earlier_object_once() {
    let (plan, mut fact, revision) = lead_execution();
    let object = |id: u128, revises: Option<u128>| WorkArtifactV1 {
        version: 1,
        id: id.into(),
        execution: fact.id,
        node: 3.into(),
        attempt: 500.into(),
        output: "result".into(),
        title: "Your trip".into(),
        data: data(json!({"kind":"reply","headline":"Your trip","text":"Six nights in SoMa."})),
        evidence: vec![],
        review: WorkOutputReview::SourceMappedNeedsReview,
        presentation: WorkArtifactPresentationV1::Automatic,
        general_knowledge: true,
        revises: revises.map(Into::into),
        part: None,
    };
    let publish = |id: u128, artifacts: Vec<u128>| WorkStepFact {
        id: id.into(),
        turn: 1,
        kind: WorkStepKindV1::Publish,
        status: WorkStepStatus::Succeeded,
        usage: None,
        artifacts: artifacts.into_iter().map(Into::into).collect(),
        evidence: None,
        note: None,
        measurements: None,
        local: None,
        account: None,
        part: None,
    };
    fact.artifacts = vec![object(10, None), object(11, Some(10))];
    fact.steps = vec![publish(1, vec![10]), publish(2, vec![11])];
    fact.validate(&plan, revision).unwrap();
    let mut twice = fact.clone();
    twice.artifacts.push(object(12, Some(10)));
    twice.steps.push(publish(3, vec![12]));
    assert!(twice.validate(&plan, revision).is_err());
    let mut forward = fact.clone();
    forward.artifacts[0].revises = Some(11.into());
    forward.artifacts[1].revises = None;
    assert!(forward.validate(&plan, revision).is_err());
    let mut own = fact.clone();
    own.artifacts[1].revises = Some(11.into());
    assert!(own.validate(&plan, revision).is_err());
    // An object of an earlier run is revised by name; the store checks it exists.
    let mut earlier = fact.clone();
    earlier.artifacts[1].revises = Some(99.into());
    earlier.validate(&plan, revision).unwrap();
}

#[test]
fn saved_artifacts_without_the_new_fields_still_load() {
    let saved = json!({
        "version":1,"id":"01J00000000000000000000001","execution":"01J00000000000000000000002",
        "node":"01J00000000000000000000003","attempt":"01J00000000000000000000004",
        "output":"comparison","title":"Answer","data":{"kind":"answer","markdown":"Hello"},
        "evidence":[],"review":"source_mapped_needs_review","presentation":"automatic",
        "general_knowledge":true
    });
    let artifact: WorkArtifactV1 = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(artifact.revises, None);
    assert_eq!(artifact.part, None);
    artifact.validate().unwrap();
    assert_eq!(serde_json::to_value(&artifact).unwrap(), saved);
}

#[test]
fn a_connection_call_is_a_settled_step_with_closed_fields() {
    let call = |url: Option<&str>| WorkStepFact {
        id: 1.into(),
        turn: 1,
        kind: WorkStepKindV1::Call {
            call: Box::new(WorkConnectionCallV1 {
                service: "github".into(),
                tool: "github_issue".into(),
                verb: "issue".into(),
                target: Some("#123".into()),
                title: Some("Totals skip a night".into()),
                count: None,
                url: url.map(str::to_owned),
            }),
        },
        status: WorkStepStatus::Succeeded,
        usage: None,
        artifacts: vec![],
        evidence: None,
        note: Some("Read issue #123".into()),
        measurements: None,
        local: None,
        account: None,
        part: None,
    };
    call(Some("https://github.com/octo/app/issues/123"))
        .validate()
        .unwrap();
    assert!(call(Some("http://github.com/x")).validate().is_err());
    let mut running = call(None);
    running.status = WorkStepStatus::Running;
    assert!(running.validate().is_err());
    let wire = serde_json::to_value(call(None).kind).unwrap();
    assert_eq!(wire["kind"], "call");
    assert_eq!(wire["call"]["verb"], "issue");
}
