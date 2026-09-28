//! Tool sets and helpers other parts of the app add to the lead: one line each.
use std::sync::Arc;

use super::tools::{LeadHelper, LeadToolSet};

/// Tools offered beside the lead's own (memory, history, notes, tabs).
pub(crate) fn tool_sets() -> Vec<Arc<dyn LeadToolSet>> {
    vec![Arc::new(crate::work_personal::PersonalTools)]
}

/// Helpers for `computer` and `connection` parts. A kind without one here
/// runs the built-in fallback.
pub(crate) fn helpers() -> Vec<Arc<dyn LeadHelper>> {
    vec![
        crate::work_computer::helper::shared(),
        crate::work_connections::helper::shared(),
    ]
}
