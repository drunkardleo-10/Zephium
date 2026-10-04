//! Bounded host-initiated picker calls. Ordinary pages receive no IPC bridge.
use super::permits::{navigation_callback_matches, EventPermit};
use super::{try_with, EngineHost};
use crate::navigation_epoch::{NavigationEpoch, NavigationEpochTracker};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use zephium_core::blocker::{
    BlockerSite, ElementPickerCompletion, ElementPickerRequest, ElementPickerResult,
    ElementSelection,
};
use zephium_core::ids::{ItemId, ProfileId};

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
pub(super) struct PickerSession {
    id: ItemId,
    profile: ProfileId,
    session: u64,
    url: String,
    epoch: NavigationEpoch,
    navigation: NavigationEpochTracker,
    permit: EventPermit,
    token: Mutex<Option<String>>,
    busy: AtomicBool,
    cancelled: AtomicBool,
}
struct Job(Arc<PickerSession>);
impl Drop for Job {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

impl EngineHost {
    pub(crate) fn element_picker(
        &mut self,
        profile: ProfileId,
        id: ItemId,
        site: BlockerSite,
        request: ElementPickerRequest,
        completion: ElementPickerCompletion,
    ) {
        if self
            .partitions
            .get(&id)
            .is_none_or(|p| p.profile() != profile)
        {
            return;
        }
        let Some(view) = self.views.get(&id) else {
            return;
        };
        let Some((epoch, url)) = view.navigation.committed_snapshot() else {
            return;
        };
        if BlockerSite::from_url(&url).as_ref() != Some(&site)
            || view.event_permit.active_token().is_none()
        {
            return;
        }
        let session = if matches!(request, ElementPickerRequest::Start) {
            if self
                .picker
                .as_ref()
                .is_some_and(|s| s.busy.load(Ordering::Acquire))
            {
                return;
            }
            let Some(next) = self.next_picker.checked_add(1) else {
                return;
            };
            self.next_picker = next;
            let session = Arc::new(PickerSession {
                id,
                profile,
                session: next,
                url,
                epoch,
                navigation: view.navigation.clone(),
                permit: view.event_permit.clone(),
                token: Mutex::new(None),
                busy: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
            });
            if let Some(old) = self.picker.replace(session.clone()) {
                old.cancelled.store(true, Ordering::Release);
                if let (Some(view), Some(token)) = (
                    self.views.get(&old.id),
                    old.token.lock().unwrap_or_else(|p| p.into_inner()).as_ref(),
                ) {
                    let args = serde_json::json!([token, old.url]);
                    let _ = view.evaluate_script(&format!(
                        "((p)=>globalThis.__zephium_content_style_v1__?.stopPicker(...p))({args})"
                    ));
                }
            }
            session
        } else {
            let requested = match request {
                ElementPickerRequest::Read { session }
                | ElementPickerRequest::Preview { session, .. }
                | ElementPickerRequest::Stop { session } => session,
                ElementPickerRequest::Start => unreachable!(),
            };
            let Some(session) = self
                .picker
                .as_ref()
                .filter(|s| {
                    s.id == id
                        && s.profile == profile
                        && s.session == requested
                        && s.epoch == epoch
                        && s.url == url
                })
                .cloned()
            else {
                return;
            };
            session
        };
        if session.cancelled.load(Ordering::Acquire) || session.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        if IN_FLIGHT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < 2).then_some(n + 1)
            })
            .is_err()
        {
            session.busy.store(false, Ordering::Release);
            return;
        }
        let job = Job(session.clone());
        let hex = format!("{:016x}", session.session);
        let script = if matches!(request, ElementPickerRequest::Start) {
            format!(
                "globalThis.__zephium_content_style_v1__?.beginEncoded({})",
                serde_json::json!(hex)
            )
        } else {
            let Some(token) = session
                .token
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
            else {
                return;
            };
            let args = serde_json::json!([token, session.url, hex]);
            match request {
                ElementPickerRequest::Read { .. } => format!(
                    "((p)=>globalThis.__zephium_content_style_v1__?.pickerEncoded(...p))({args})"
                ),
                ElementPickerRequest::Preview { enabled, .. } => format!(
                    "((p)=>{{const a=globalThis.__zephium_content_style_v1__;return a?.preview(...p,{enabled})?a.pickerEncoded(...p):null;}})({args})"
                ),
                ElementPickerRequest::Stop { .. } => format!(
                    "((p)=>{{const a=globalThis.__zephium_content_style_v1__;return a?.stopPicker(p[0],p[1])?a.pickerEncoded(...p):null;}})({args})"
                ),
                ElementPickerRequest::Start => unreachable!(),
            }
        };
        let holder = Mutex::new(Some((job, completion)));
        let Some(view) = self.views.get(&id) else {
            return;
        };
        let _ = view.evaluate_script_with_callback(&script, move |text| {
            let Some((job, completion)) = holder.lock().unwrap_or_else(|p| p.into_inner()).take()
            else {
                return;
            };
            let Some(value) = decode(&text) else { return };
            let _ = try_with(move |host| {
                let session = &job.0;
                if session.cancelled.load(Ordering::Acquire)
                    || host
                        .picker
                        .as_ref()
                        .is_none_or(|s| !Arc::ptr_eq(s, session))
                {
                    return;
                }
                let Some(view) = host.views.get(&id) else {
                    return;
                };
                if !navigation_callback_matches(
                    &view.event_permit,
                    &view.navigation,
                    &session.permit,
                    &session.navigation,
                    session.epoch,
                ) || !session
                    .navigation
                    .matches_committed_snapshot(session.epoch, &session.url)
                {
                    return;
                }
                if matches!(request, ElementPickerRequest::Start) {
                    let Some(token) = value
                        .get("token")
                        .and_then(|v| v.as_str())
                        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    else {
                        return;
                    };
                    if value.get("url").and_then(|v| v.as_str()) != Some(&session.url) {
                        return;
                    }
                    *session.token.lock().unwrap_or_else(|p| p.into_inner()) = Some(token.into());
                }
                let Some(active) = value.get("active").and_then(|v| v.as_bool()) else {
                    return;
                };
                let selection = if active {
                    value.get("selection").and_then(selection)
                } else {
                    None
                };
                let result = ElementPickerResult {
                    session: session.session,
                    active,
                    selection,
                };
                if !active {
                    host.picker = None;
                }
                completion.finish(Some(result));
            });
        });
    }
}

fn decode(text: &str) -> Option<serde_json::Value> {
    if text.len() > 80 * 1024 {
        return None;
    }
    let encoded: String = serde_json::from_str(text).ok()?;
    if encoded.len() > 40 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&encoded).ok()?;
    value.is_object().then_some(value)
}
fn selection(value: &serde_json::Value) -> Option<ElementSelection> {
    let selector = value.get("selector")?.as_str()?;
    let label = value.get("label")?.as_str()?;
    let count = value.get("count")?.as_u64()?;
    if selector.is_empty()
        || selector.len() > 2048
        || selector.chars().any(char::is_control)
        || label.is_empty()
        || label.len() > 256
        || label.chars().any(char::is_control)
        || !(1..=100).contains(&count)
    {
        return None;
    }
    Some(ElementSelection {
        selector: selector.into(),
        label: label.into(),
        count: count as u32,
        positional: value.get("positional")?.as_bool()?,
    })
}
