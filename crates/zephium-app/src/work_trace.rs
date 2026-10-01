//! Closed facts from the page lane and searches for the Work development
//! log: causes, counts and phases, never page, model or person text. The host
//! installs the sink; without one nothing is written.
use std::sync::{Arc, OnceLock};

pub type WorkTraceSink = dyn Fn(std::fmt::Arguments<'_>) + Send + Sync;

static SINK: OnceLock<Arc<WorkTraceSink>> = OnceLock::new();

pub fn install(sink: Arc<WorkTraceSink>) {
    let _ = SINK.set(sink);
}

pub fn record(arguments: std::fmt::Arguments<'_>) {
    if let Some(sink) = SINK.get() {
        sink(arguments);
    }
}
