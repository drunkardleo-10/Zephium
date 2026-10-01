use std::{sync::mpsc, time::Instant};
use zephium_agentic::*;
use zephium_core::ids::{ProfileId, WorkId};

pub fn run_construction_liveness_probe(stalled: bool) -> Result<(), &'static str> {
    let fixture = if stalled {
        super::fixture::Fixture::stalled()?
    } else {
        super::fixture::Fixture::start()?
    };
    let target = ContextNavigationTarget::parse(fixture.url()).map_err(|_| "fixture_target")?;
    let profile = ProfileId::generate();
    let start = Instant::now();
    super::super::agentic_semantic_probe::run_construction_host(profile, move |port| {
        let mut resources = WorkBrowserResources::new(WorkId::generate(), profile);
        let request = resources
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                target,
                crate::work_browser_monotonic_now().ok_or("clock")?,
            )
            .map_err(|_| "construction_request")?;
        let expected = request.resource().clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let initial = tx.clone();
        if !matches!(
            port.work_resource_lifecycle(
                request,
                Box::new(move |completion| {
                    let _ = initial.try_send(completion);
                })
            ),
            WorkBrowserResourceDispatch::Scheduled
        ) {
            return Err("construction_dispatch");
        }
        let mut ended = false;
        Ok(Box::new(move |failed| {
            if failed {
                return Some(Err("host_failure"));
            }
            let completion = match rx.try_recv() {
                Ok(completion) => completion,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(_) => return Some(Err("completion_channel")),
            };
            let Some(now) = crate::work_browser_monotonic_now() else {
                return Some(Err("clock"));
            };
            match resources.settle_at(completion, now) {
                Ok(event)
                    if !ended
                        && matches!(
                            event,
                            WorkBrowserResourceEvent::Retained(_)
                                | WorkBrowserResourceEvent::Quarantined(_)
                        ) =>
                {
                    let resource = match event {
                        WorkBrowserResourceEvent::Retained(resource) if !stalled => resource,
                        WorkBrowserResourceEvent::Quarantined(
                            WorkBrowserResourceFailure::NativeRefused,
                        ) if stalled && start.elapsed() >= std::time::Duration::from_secs(30) => {
                            expected.clone()
                        }
                        _ => return Some(Err("construction_outcome")),
                    };
                    if !fixture.released() {
                        return Some(Err("animation_not_released"));
                    }
                    ended = true;
                    eprintln!(
                        "construction_probe constructed={} animation_released=true elapsed_ms={}",
                        !stalled,
                        start.elapsed().as_millis()
                    );
                    let Ok(request) = resources.destroy(&resource) else {
                        return Some(Err("destroy_request"));
                    };
                    let completed = tx.clone();
                    if !matches!(
                        port.work_resource_lifecycle(
                            request,
                            Box::new(move |completion| {
                                let _ = completed.try_send(completion);
                            })
                        ),
                        WorkBrowserResourceDispatch::Scheduled
                    ) {
                        return Some(Err("destroy_dispatch"));
                    }
                    None
                }
                Ok(WorkBrowserResourceEvent::Destroyed(_)) if ended => {
                    eprintln!(
                        "construction_probe destroyed=true elapsed_ms={}",
                        start.elapsed().as_millis()
                    );
                    Some(Ok(()))
                }
                _ => Some(Err("construction_outcome")),
            }
        }))
    })
}
