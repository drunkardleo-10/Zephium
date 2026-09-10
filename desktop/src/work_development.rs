//! Release-excluded input/output adapter around ordinary Work admission.
//! No browser engine, policy bypass, tool loop or answer verifier lives here.

#[cfg(any(
    feature = "macos-work-navigation-probe",
    feature = "macos-work-rendering-probe"
))]
compile_error!("select the dynamic Work developer entry without a qualification launcher");

use serde::Deserialize;
use serde_json::json;
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;
use zephium_agent_controller::{AgentBrowserModel, AgentWorkEventKind};
use zephium_agent_provider_transport::{
    load_macos_development_openai_credential, AgentProviderTransportConfig,
};
use zephium_agent_runtime::AgentRuntimeConfig;
use zephium_agentic::*;
use zephium_app::{
    AgentWorkApplicationConfig, AgentWorkProfileBinding, AgentWorkProfileReadiness,
    AgentWorkProfileRequest, RetainedWorkHandle, RetainedWorkPhase,
};
use zephium_work_composition::{
    PublicReadWorkAccount, PublicReadWorkInvocation, PublicReadWorkObjective,
    PublicReadWorkSettings,
};

const CONFIG_BYTES: u64 = 32 * 1024;
const OUTPUT_BYTES: usize = 64 * 1024;
const CONFIG_ENV: &str = "ZEPHIUM_WORK_REQUEST";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    objective: String,
    account: Account,
    start_url: String,
    path_prefix: String,
    max_hops: usize,
    output_fields: Vec<Field>,
    model: Model,
    max_model_calls: u8,
    operations: u32,
    model_tokens: u64,
    cost_micro_usd: u64,
    deadline_seconds: u64,
    inspectable_public: bool,
    persist_result: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    name: String,
    max_bytes: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Model {
    Luna,
    Terra,
}

/// Explicit caller assertion for public pages that do not rely on an account.
/// Authenticated tasks need a real host account source and are not admitted by
/// this developer adapter merely because their browser profile has cookies.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Account {
    AnonymousPublic,
}

struct State(Mutex<Run>);
struct Run {
    preparation: Option<mpsc::Receiver<Result<PublicReadWorkInvocation, &'static str>>>,
    invocation: Option<PublicReadWorkInvocation>,
    profile_request: Option<AgentWorkProfileRequest>,
    next_profile_poll: Instant,
    stop_observer: Option<mpsc::SyncSender<()>>,
    pinned: Option<AgentWorkProfileBinding>,
    view: Option<RetainedWorkHandle>,
    deadline: Instant,
    started: Instant,
    closed: bool,
    calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro_usd: u64,
}

/// No file means no runner. All request data comes from the caller's bounded
/// JSON file; the Keychain is the only credential source.
pub(super) fn install(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = std::env::var_os(CONFIG_ENV) else {
        return Ok(());
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let request = parse(&bytes).map_err(std::io::Error::other)?;
    let started = Instant::now();
    let deadline = started
        .checked_add(Duration::from_secs(request.deadline_seconds))
        .ok_or_else(|| std::io::Error::other("invalid Work deadline"))?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let (stop_observer, stop_receiver) = mpsc::sync_channel(1);
    let handle = app.clone();
    // Keychain interaction may require human input. The main/native thread
    // stays responsive; this thread can only prepare a dormant invocation.
    std::thread::Builder::new()
        .name("work-dev-credential".into())
        .spawn(move || {
            let prepared = prepare(request, deadline);
            if sender.send(prepared).is_err() {
                return;
            }
            // A bounded developer observer wake, independent of page frames.
            // It drives no tools and exits as soon as its owner drops the sender.
            let pending = Arc::new(AtomicBool::new(false));
            loop {
                // Keep at most one queued wake even if the main thread stalls.
                if !pending.swap(true, Ordering::AcqRel) {
                    let delivered = pending.clone();
                    if handle
                        .run_on_main_thread(move || delivered.store(false, Ordering::Release))
                        .is_err()
                    {
                        break;
                    }
                }
                if Instant::now() > deadline + Duration::from_secs(10) {
                    break;
                }
                if !matches!(
                    stop_receiver.recv_timeout(Duration::from_millis(100)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
        })?;
    if !app.manage(State(Mutex::new(Run {
        preparation: Some(receiver),
        invocation: None,
        profile_request: None,
        next_profile_poll: started,
        stop_observer: Some(stop_observer),
        pinned: None,
        view: None,
        deadline,
        started,
        closed: false,
        calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_micro_usd: 0,
    }))) {
        return Err(std::io::Error::other("Work developer adapter already installed").into());
    }
    emit(json!({"work_development":"preparing", "credential":"keychain", "content":"redacted"}));
    Ok(())
}

fn parse(bytes: &[u8]) -> Result<Request, &'static str> {
    if bytes.len() as u64 > CONFIG_BYTES {
        return Err("Work request exceeds byte limit");
    }
    let request: Request =
        serde_json::from_slice(bytes).map_err(|_| "invalid Work request JSON")?;
    if request.objective.trim().is_empty() || !(1..=600).contains(&request.deadline_seconds) {
        return Err("invalid Work objective or deadline");
    }
    Ok(request)
}

fn prepare(request: Request, deadline: Instant) -> Result<PublicReadWorkInvocation, &'static str> {
    let navigation = AgentNavigationDiscovery::try_new(
        ContextNavigationTarget::parse(&request.start_url).map_err(|_| "invalid starting URL")?,
        request.path_prefix,
        request.max_hops,
    )
    .map_err(|_| "invalid navigation scope")?;
    let fields = request
        .output_fields
        .into_iter()
        .map(|field| {
            SemanticExtractionFieldSchema::try_text(field.name, true, field.max_bytes)
                .map_err(|_| "invalid result field")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let budget = AgentRunBudget::try_new(
        request.operations,
        request.model_tokens,
        request.cost_micro_usd,
        1,
    )
    .map_err(|_| "invalid Work budget")?;
    let credential = load_macos_development_openai_credential()
        .map_err(|_| "development Keychain credential unavailable")?;
    if Instant::now() >= deadline {
        return Err("Work deadline elapsed during preparation");
    }
    let invocation = PublicReadWorkInvocation::new(
        PublicReadWorkObjective {
            objective: request.objective,
            navigation,
            output_fields: fields,
        },
        PublicReadWorkSettings {
            account: match request.account {
                Account::AnonymousPublic => PublicReadWorkAccount::Anonymous,
            },
            model: match request.model {
                Model::Luna => AgentBrowserModel::Luna,
                Model::Terra => AgentBrowserModel::Terra,
            },
            budget,
            max_model_calls: request.max_model_calls,
            deadline,
        },
        AgentWorkApplicationConfig::new(
            AgentRuntimeConfig::STANDARD,
            AgentProviderTransportConfig::STANDARD,
        ),
        credential,
    );
    let invocation = if request.persist_result {
        invocation.with_persistent_result()
    } else {
        invocation
    };
    Ok(if request.inspectable_public {
        invocation.with_inspectable_public_retention()
    } else {
        invocation
    })
}

pub(super) fn on_run_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    let Some(state) = app.try_state::<State>() else {
        return;
    };
    let Ok(mut run) = state.0.lock() else {
        return;
    };
    if matches!(
        event,
        tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
    ) {
        run.close();
        return;
    }
    if !run.closed
        && matches!(
            event,
            tauri::RunEvent::Ready | tauri::RunEvent::MainEventsCleared
        )
    {
        if let Err(reason) = run.poll(app) {
            emit(json!({"work_development":"stopped", "reason":reason}));
            run.close();
        }
    }
}

impl Run {
    fn close(&mut self) {
        self.closed = true;
        self.preparation.take();
        self.invocation.take();
        self.profile_request.take();
        self.stop_observer.take();
        if let Some(view) = &self.view {
            let _ = view.stop();
        }
    }

    fn poll(&mut self, app: &tauri::AppHandle) -> Result<(), &'static str> {
        if Instant::now() > self.deadline + Duration::from_secs(10) {
            return Err("observer deadline handoff to application cleanup");
        }
        if self.view.is_none() {
            if Instant::now() >= self.deadline {
                return Err("admission deadline elapsed");
            }
            if let Some(receiver) = &self.preparation {
                match receiver.try_recv() {
                    Ok(result) => {
                        self.invocation = Some(result?);
                        self.preparation = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => return Ok(()),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("preparation disconnected")
                    }
                }
            }
            if self.profile_request.is_none() {
                if Instant::now() < self.next_profile_poll {
                    return Ok(());
                }
                self.next_profile_poll = Instant::now() + Duration::from_millis(100);
                self.profile_request = Some(
                    super::work::selected_work_profile(app)
                        .map_err(|_| "profile query unavailable")?,
                );
            }
            let Some(readiness) = self
                .profile_request
                .as_ref()
                .and_then(|request| request.try_recv())
            else {
                return Ok(());
            };
            self.profile_request = None;
            let binding = match readiness {
                AgentWorkProfileReadiness::Ready(binding)
                | AgentWorkProfileReadiness::PolicyPending(binding) => binding,
                AgentWorkProfileReadiness::ProfileMissing
                | AgentWorkProfileReadiness::PolicyMissing
                    if self.pinned.is_none() =>
                {
                    return Ok(())
                }
                _ => return Err("selected profile unavailable"),
            };
            if self.pinned.is_some_and(|prior| prior != binding) {
                return Err("selected profile changed");
            }
            self.pinned = Some(binding);
            if !matches!(readiness, AgentWorkProfileReadiness::Ready(_)) {
                return Ok(());
            }
            let invocation = self.invocation.take().ok_or("missing invocation")?;
            self.view = Some(
                super::work::launch_public_read_work(app, binding, invocation)
                    .map_err(|_| "Work admission refused")?,
            );
            emit(json!({"work_development":"queued", "content":"redacted"}));
        }
        let view = self.view.as_ref().ok_or("missing Work handle")?;
        let mut drained = 0;
        for _ in 0..zephium_agent_controller::MAX_AGENT_WORK_EVENTS {
            let Some(event) = view.take_event() else {
                break;
            };
            drained += 1;
            if let AgentWorkEventKind::ModelSettled {
                input_tokens,
                output_tokens,
                cost_micro_usd,
                ..
            } = event.kind()
            {
                self.calls += 1;
                self.input_tokens += input_tokens;
                self.output_tokens += output_tokens;
                self.cost_micro_usd += cost_micro_usd;
            }
            emit(
                json!({"work_event":format!("{:?}", event.kind()), "sequence":event.sequence(), "elapsed_ms":event.elapsed_millis()}),
            );
        }
        let snapshot = view.snapshot();
        if let Some(result) = view.take_extraction() {
            emit_result(&result)?;
        }
        if matches!(
            snapshot.phase,
            RetainedWorkPhase::Terminal | RetainedWorkPhase::Refused | RetainedWorkPhase::Uncertain
        ) && drained < zephium_agent_controller::MAX_AGENT_WORK_EVENTS
        {
            emit(
                json!({"work_development":"terminal", "phase":format!("{:?}", snapshot.phase), "failure":format!("{:?}", snapshot.failure), "record":format!("{:?}", snapshot.record), "artifact":format!("{:?}", snapshot.artifact), "usage_basis":"settled_model_events", "model_calls":self.calls, "input_tokens":self.input_tokens, "output_tokens":self.output_tokens, "cost_micro_usd":self.cost_micro_usd, "elapsed_ms":self.started.elapsed().as_millis()}),
            );
            self.closed = true;
            self.stop_observer.take();
        }
        Ok(())
    }
}

fn emit_result(result: &SemanticOwnedExtractionResult) -> Result<(), &'static str> {
    emit(
        json!({"work_result":{"trust":"ModelMapped", "requires_human_evaluation":true, "fields":result.fields().len()}}),
    );
    for field in result.fields() {
        let SemanticExtractedValue::Text(text) = field.value() else {
            return Err("unexpected result field type");
        };
        let sources = result.sources(text.source_span()).ok_or("missing source evidence")?.take(4).map(|source| json!({
            "origin":source.frame.origin().as_url().as_str(), "navigation_epoch":source.frame.context().navigation_epoch().get(), "frame_generation":source.frame.frame_generation().get(), "quote":match &source.content {
                SemanticOwnedReadContent::Text(value) => preview(value),
                SemanticOwnedReadContent::ValuePreview { text, .. } => preview(text),
                _ => "",
            }, "role":format!("{:?}", source.role), "observation":format!("{:?}", source.observation),
        })).collect::<Vec<_>>();
        emit(
            json!({"work_result_field":{"name":field.name(), "text":text.as_str(), "source_preview_limit":4,"quote_byte_limit":512,"sources":sources}}),
        );
    }
    Ok(())
}

fn preview(value: &str) -> &str {
    let mut end = value.len().min(512);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn emit(value: serde_json::Value) {
    let Ok(bytes) = serde_json::to_vec(&value) else {
        return;
    };
    let mut output = std::io::stdout().lock();
    if bytes.len() > OUTPUT_BYTES {
        let _ = output.write_all(b"{\"work_development\":\"output_exceeds_limit\"}\n");
        return;
    }
    let _ = output.write_all(&bytes);
    let _ = output.write_all(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_input_rejects_routes_or_answer_oracles() {
        let mut input = json!({"objective":"Find relevant guidance and explain it", "account":"anonymous_public", "start_url":"https://example.test/", "path_prefix":"/", "max_hops":2, "output_fields":[{"name":"answer","max_bytes":2048}], "model":"luna", "max_model_calls":24, "operations":64, "model_tokens":200000, "cost_micro_usd":500000, "deadline_seconds":300, "inspectable_public":true,"persist_result":true});
        assert!(parse(&serde_json::to_vec(&input).unwrap()).is_ok());
        input["expected_answer"] = json!("known answer");
        assert!(parse(&serde_json::to_vec(&input).unwrap()).is_err());
        assert!(parse(&vec![b' '; CONFIG_BYTES as usize + 1]).is_err());
    }

    #[test]
    fn evidence_preview_preserves_utf8_and_is_bounded() {
        let text = "界".repeat(200);
        assert_eq!(preview(&text).len(), 510);
        assert_eq!(preview("short evidence"), "short evidence");
    }
}
