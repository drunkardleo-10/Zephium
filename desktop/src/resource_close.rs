//! User-initiated shutdown waits for bounded renderer draft flushes. Security and
//! forced teardown keep the existing native shutdown path and never wait on UI.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct Gate {
    touched: BTreeSet<String>,
    pending: BTreeMap<String, (String, tokio::sync::oneshot::Sender<bool>)>,
    requesting: bool,
}
static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
fn gate() -> &'static Mutex<Gate> {
    GATE.get_or_init(|| Mutex::new(Gate::default()))
}
pub(super) fn touch(label: &str) {
    gate()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .touched
        .insert(label.into());
}
impl Gate {
    fn complete(&mut self, label: &str, token: &str, success: bool) -> bool {
        if self
            .pending
            .get(label)
            .is_none_or(|(expected, _)| expected != token)
        {
            return false;
        }
        let Some((_, send)) = self.pending.remove(label) else {
            return false;
        };
        send.send(success).is_ok()
    }
}
pub(super) fn complete(label: &str, token: &str, success: bool) -> bool {
    gate()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .complete(label, token, success)
}
pub(super) fn request(app: tauri::AppHandle, done: impl FnOnce() + Send + 'static) {
    if super::shutdown_started(&app) {
        done();
        return;
    }
    let mut gate = gate().lock().unwrap_or_else(|e| e.into_inner());
    if gate.requesting {
        return;
    }
    if gate.touched.is_empty() {
        drop(gate);
        done();
        return;
    }
    gate.requesting = true;
    let mut receivers = Vec::new();
    let mut requests = Vec::new();
    for label in [super::MAIN_LABEL, super::overlay::PANEL_LABEL] {
        if app.get_webview_window(label).is_none() {
            continue;
        }
        let token = zephium_core::ids::ResourceId::generate().to_string();
        let (send, receive) = tokio::sync::oneshot::channel();
        gate.pending.insert(label.into(), (token.clone(), send));
        receivers.push(receive);
        requests.push((label, token));
    }
    drop(gate);
    for (label, token) in &requests {
        super::emit_to_privileged(&app, label, "zephium:resource-close", token);
    }
    tauri::async_runtime::spawn(async move {
        let completed = tokio::time::timeout(std::time::Duration::from_secs(15), async move {
            let mut okay = true;
            for receive in receivers {
                okay = receive.await.unwrap_or(false) && okay;
            }
            okay
        })
        .await
        .unwrap_or(false);
        {
            let mut gate = crate::resource_close::gate()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            gate.pending.clear();
            gate.requesting = false;
        }
        if completed {
            done();
        } else if !super::shutdown_started(&app) {
            for (label, token) in requests {
                super::emit_to_privileged(&app, label, "zephium:resource-close-cancelled", &token);
            }
            app.dialog().message("Some changes in Notes, Tasks or Work could not be saved. Return to them to retry or resolve a conflict. Drafts in another profile must be saved from that profile.").title("Unsaved changes").kind(tauri_plugin_dialog::MessageDialogKind::Warning).show(|_|{});
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replies_are_bound_to_the_requesting_window_and_current_token() {
        let mut gate = Gate::default();
        let (send, mut receive) = tokio::sync::oneshot::channel();
        gate.pending.insert("main".into(), ("current".into(), send));
        assert!(!gate.complete("panel", "current", true));
        assert!(!gate.complete("main", "old", true));
        assert!(receive.try_recv().is_err());
        assert!(gate.complete("main", "current", true));
        assert_eq!(receive.try_recv(), Ok(true));
        assert!(!gate.complete("main", "current", true));
    }
}
