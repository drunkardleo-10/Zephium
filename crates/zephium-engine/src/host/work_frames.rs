//! Latest person-facing frame per hosted Work page. Written on the main
//! thread after native captures, read by the application port. Frames are
//! replaced in place and dropped with their resource; nothing here is
//! model-visible or persisted.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use zephium_agentic::{ContextId, WorkBrowserFrame};

fn frames() -> &'static Mutex<HashMap<ContextId, Arc<WorkBrowserFrame>>> {
    static FRAMES: OnceLock<Mutex<HashMap<ContextId, Arc<WorkBrowserFrame>>>> = OnceLock::new();
    FRAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn store(id: ContextId, frame: WorkBrowserFrame) {
    if let Ok(mut map) = frames().lock() {
        if map.len() < zephium_agentic::MAX_LIVE_CONTEXTS || map.contains_key(&id) {
            map.insert(id, Arc::new(frame));
        }
    }
}

pub(crate) fn latest(id: ContextId) -> Option<Arc<WorkBrowserFrame>> {
    frames().lock().ok()?.get(&id).cloned()
}

pub(crate) fn clear(id: ContextId) {
    if let Ok(mut map) = frames().lock() {
        map.remove(&id);
    }
}
