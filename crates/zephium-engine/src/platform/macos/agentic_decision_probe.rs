use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use zephium_agentic::*;
use zephium_core::ids::{ProfileId, WorkId};

/// Fixed anonymous sources for locally retained decision eval observations.
#[derive(Clone, Copy, Debug)]
pub enum DecisionObservationSite {
    Airbnb,
    Yc,
    Government,
    /// Public demo storefront product page.
    DemoStore,
    /// Public scraping-sandbox book product page.
    BookStore,
    /// Public test-site computer product page.
    TestStore,
    /// Public Airbnb listing page without stay dates.
    AirbnbListing,
    /// LEGO theme catalog as a first visit sees it, with its entry notices.
    LegoTheme,
    /// A public page behind a cookie consent dialog.
    Consent,
    /// A public page behind a binary entry interstitial.
    Interstitial,
}

impl DecisionObservationSite {
    fn target(self) -> &'static str {
        match self {
            Self::Airbnb => "https://www.airbnb.com/s/San-Francisco--CA/homes",
            Self::Yc => "https://www.ycombinator.com/about",
            Self::Government => "https://travel.state.gov/",
            Self::DemoStore => "https://www.scrapingcourse.com/ecommerce/product/adrienne-trek-jacket/",
            Self::BookStore => "https://books.toscrape.com/catalogue/the-black-maria_991/index.html",
            Self::TestStore => "https://www.demoblaze.com/prod.html?idp_=1",
            Self::AirbnbListing => "https://www.airbnb.com/rooms/23813739",
            Self::LegoTheme => "https://www.lego.com/en-us/themes/architecture",
            Self::Consent => "https://www.ikea.com/pl/pl/",
            Self::Interstitial => "https://www.zalando.pl/",
        }
    }

    /// A listing rewrites its query during setup, exactly as a Work read admits.
    fn document_policy(self) -> WorkBrowserDocumentPolicy {
        match self {
            Self::AirbnbListing | Self::LegoTheme | Self::Consent | Self::Interstitial => {
                WorkBrowserDocumentPolicy::PublicQueryFinalization
            }
            _ => WorkBrowserDocumentPolicy::Exact,
        }
    }

    /// A heavy client-rendered listing needs longer before its first capture.
    fn settle(self) -> Duration {
        match self {
            Self::AirbnbListing => Duration::from_secs(8),
            Self::LegoTheme | Self::Consent | Self::Interstitial => Duration::from_secs(5),
            _ => Duration::from_secs(2),
        }
    }
}

pub fn run(
    site: DecisionObservationSite,
    mut capture: impl FnMut(SemanticObservation) -> Result<(), &'static str> + 'static,
) -> Result<(), &'static str> {
    let profile = ProfileId::generate();
    super::agentic_semantic_probe::run_construction_host(profile, move |port| {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), profile);
        let now = crate::work_browser_monotonic_now().ok_or("clock")?;
        let target = ContextNavigationTarget::parse(site.target()).map_err(|_| "target")?;
        let request = rows
            .construct_document_with_policy(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                target,
                site.document_policy(),
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
        let mut lease = None;
        let mut observation_request = None;
        let mut acquired = None;
        let mut captured = Ok(());
        Ok(Box::new(move |failed| {
            if failed {
                return Some(Err("host_failure"));
            }
            let Some(now) = crate::work_browser_monotonic_now() else {
                return Some(Err("clock"));
            };
            if phase == 2 {
                if acquired.is_none_or(|at: Instant| at.elapsed() < site.settle()) {
                    return None;
                }
                let Some(current) = lease.as_ref() else {
                    return Some(Err("lease"));
                };
                let Ok(request) = rows.observe_initial(current, now) else {
                    return Some(Err("read_request"));
                };
                observation_request = Some(request.observation().clone());
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
                phase = 3;
                return None;
            }
            if phase == 3 {
                let completion = match read_rx.try_recv() {
                    Ok(completion) => completion,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(_) => return Some(Err("read_channel")),
                };
                let snapshot = match rows.settle_observation(completion, now) {
                    Ok(WorkBrowserObservationEvent::Snapshot(snapshot)) => snapshot,
                    Ok(WorkBrowserObservationEvent::Refused(reason)) => {
                        eprintln!("decision_observation phase=refused reason={reason:?}");
                        return Some(Err("observation_refused"));
                    }
                    Err(reason) => {
                        eprintln!("decision_observation phase=settlement_failed reason={reason:?}");
                        return Some(Err("observation_settlement"));
                    }
                    _ => return Some(Err("observation")),
                };
                let Some(request) = observation_request.take() else {
                    return Some(Err("observation_request"));
                };
                captured = SemanticObservationAssembler::new(request, *snapshot)
                    .and_then(SemanticObservationAssembler::finish)
                    .map_err(|_| "assembly")
                    .and_then(&mut capture);
                let Some(current) = lease.take() else {
                    return Some(Err("lease"));
                };
                let Ok(request) = rows.revoke(&current) else {
                    return Some(Err("revoke_request"));
                };
                if !dispatch(request, tx.clone()) {
                    return Some(Err("revoke_dispatch"));
                }
                phase = 4;
            }
            let completion = match rx.try_recv() {
                Ok(completion) => completion,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(_) => return Some(Err("lifecycle_channel")),
            };
            let event = match rows.settle_at(completion, now) {
                Ok(event) => event,
                Err(_) => return Some(Err("lifecycle_settlement")),
            };
            let request = match (phase, event) {
                (0, WorkBrowserResourceEvent::Retained(_)) => {
                    phase = 1;
                    rows.acquire(
                        &resource,
                        ContextRunId::generate(),
                        now,
                        AgentPolicyInstant::from_millis(
                            now.millis() + 10_000 + site.settle().as_millis() as u64,
                        ),
                    )
                }
                (1, WorkBrowserResourceEvent::Acquired(current)) => {
                    lease = Some(current);
                    acquired = Some(Instant::now());
                    phase = 2;
                    return None;
                }
                (4, WorkBrowserResourceEvent::LeaseEnded(_)) => {
                    phase = 5;
                    rows.destroy(&resource)
                }
                (5, WorkBrowserResourceEvent::Destroyed(_)) => return Some(captured),
                _ => return Some(Err("unexpected_terminal")),
            };
            let Ok(request) = request else {
                return Some(Err("lifecycle_request"));
            };
            if !dispatch(request, tx.clone()) {
                return Some(Err("lifecycle_dispatch"));
            }
            None
        }))
    })
}
