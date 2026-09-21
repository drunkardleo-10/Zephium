use std::{sync::mpsc, time::Instant};
use zephium_agentic::*;
use zephium_core::ids::{ProfileId, WorkId};

pub fn run_human_takeover_probe() -> Result<(), &'static str> {
    let fixture = super::fixture::Fixture::start()?;
    let target = ContextNavigationTarget::parse(fixture.url()).map_err(|_| "fixture_target")?;
    let profile = ProfileId::generate();
    let start = Instant::now();
    super::super::agentic_semantic_probe::run_construction_host(profile, move |port| {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), profile);
        let now = crate::work_browser_monotonic_now().ok_or("clock")?;
        let request = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                target,
                now,
            )
            .map_err(|_| "construction_request")?;
        let resource = request.resource().clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let read_port = port.clone();
        let (read_tx, read_rx) = mpsc::sync_channel(1);
        let dispatch = move |request, tx: mpsc::SyncSender<WorkBrowserResourceCompletion>| {
            matches!(
                port.work_resource_lifecycle(
                    request,
                    Box::new(move |completion| {
                        let _ = tx.try_send(completion);
                    })
                ),
                WorkBrowserResourceDispatch::Scheduled
            )
        };
        if !dispatch(request, tx.clone()) {
            return Err("construction_dispatch");
        }
        let mut phase = 0u8;
        let mut progress: Option<WorkBrowserHumanProgress> = None;
        let mut read_lease = None;
        Ok(Box::new(move |failed| {
            if failed {
                return Some(Err("host_failure"));
            }
            let Some(now) = crate::work_browser_monotonic_now() else {
                return Some(Err("clock"));
            };
            if phase == 7 {
                let completion = match read_rx.try_recv() {
                    Ok(completion) => completion,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(_) => return Some(Err("read_channel")),
                };
                let snapshot = match rows.settle_observation(completion, now) {
                    Ok(WorkBrowserObservationEvent::Snapshot(snapshot)) => snapshot,
                    _ => return Some(Err("fresh_observation")),
                };
                if snapshot.nodes().is_empty() {
                    return Some(Err("empty_observation"));
                }
                eprintln!(
                    "human_probe fresh_observation=true nodes={} elapsed_ms={}",
                    snapshot.nodes().len(),
                    start.elapsed().as_millis()
                );
                let Some(lease) = read_lease.take() else {
                    return Some(Err("read_lease"));
                };
                let Ok(request) = rows.revoke(&lease) else {
                    return Some(Err("revoke_request"));
                };
                if !dispatch(request, tx.clone()) {
                    return Some(Err("revoke_dispatch"));
                }
                phase = 5;
            }
            if phase == 2
                && progress
                    .as_ref()
                    .is_some_and(|progress| progress.revision() > 0 && progress.ready())
            {
                if !fixture.verified() {
                    return Some(Err("session_cookie_missing"));
                }
                let Ok(request) = rows.continue_after_human(&resource, now) else {
                    return Some(Err("continue_request"));
                };
                if !dispatch(request, tx.clone()) {
                    return Some(Err("continue_dispatch"));
                }
                phase = 3;
            }
            let completion = match rx.try_recv() {
                Ok(value) => value,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(_) => return Some(Err("completion_channel")),
            };
            let event = match rows.settle_at(completion, now) {
                Ok(event) => event,
                Err(_) => return Some(Err("resource_settlement")),
            };
            let next = match (phase, event) {
                (0, WorkBrowserResourceEvent::Retained(_)) => {
                    let region = WorkBrowserHumanRegion::try_new(0, 0, 600, 400).unwrap();
                    let request = rows.present_human(
                        &resource,
                        region,
                        now,
                        AgentPolicyInstant::from_millis(now.millis() + 20_000),
                    );
                    progress = rows.human_progress(&resource);
                    phase = 1;
                    request
                }
                (1, WorkBrowserResourceEvent::HumanPresented(_)) => {
                    let join = resource.clone();
                    if !crate::host::try_with_agent_context_terminal(move |host| {
                        assert!(
                            host.navigate_human_fixture(&join),
                            "fixture navigation refused"
                        );
                    }) {
                        return Some(Err("fixture_navigation_dispatch"));
                    }
                    phase = 2;
                    return None;
                }
                (3, WorkBrowserResourceEvent::HumanContinued(_)) => {
                    phase = 4;
                    rows.acquire(
                        &resource,
                        ContextRunId::generate(),
                        now,
                        AgentPolicyInstant::from_millis(now.millis() + 10_000),
                    )
                }
                (4, WorkBrowserResourceEvent::Acquired(current)) => {
                    let binding = match rows.read_binding(&current, now) {
                        Ok(binding) => binding,
                        Err(_) => return Some(Err("fresh_binding")),
                    };
                    if binding.frame().context().navigation_epoch().get() != 2
                        || binding.document().as_url().path() != "/verified"
                    {
                        return Some(Err("document_binding"));
                    }
                    eprintln!("human_probe presented=true document_changed=true session_cookie=true continued=true generation=2 elapsed_ms={}", start.elapsed().as_millis());
                    let Ok(request) = rows.observe_initial(&current, now) else {
                        return Some(Err("read_request"));
                    };
                    let completed = read_tx.clone();
                    if !matches!(
                        read_port.work_resource_observe(
                            request,
                            Box::new(move |completion| {
                                let _ = completed.try_send(completion);
                            })
                        ),
                        WorkBrowserObservationDispatch::Scheduled
                    ) {
                        return Some(Err("read_dispatch"));
                    }
                    read_lease = Some(current);
                    phase = 7;
                    return None;
                }
                (5, WorkBrowserResourceEvent::LeaseEnded(_)) => {
                    phase = 6;
                    rows.destroy(&resource)
                }
                (6, WorkBrowserResourceEvent::Destroyed(_)) => {
                    eprintln!(
                        "human_probe destroyed=true elapsed_ms={}",
                        start.elapsed().as_millis()
                    );
                    return Some(Ok(()));
                }
                _ => return Some(Err("unexpected_terminal")),
            };
            let Ok(request) = next else {
                return Some(Err("lifecycle_request"));
            };
            if !dispatch(request, tx.clone()) {
                return Some(Err("lifecycle_dispatch"));
            }
            None
        }))
    })
}
