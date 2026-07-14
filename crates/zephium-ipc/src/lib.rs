//! Typed contract between the Rust core and the frame. Pure data, no tauri;
//! the desktop crate maps `Projection` onto typed events and exports the TS
//! bindings. ULIDs cross the boundary as strings.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TabView {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub favicon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ItemsState {
    pub tabs: Vec<TabView>,
    pub active: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum SearchAction {
    ActivateTab { id: String },
    OpenUrl { url: String },
    RunCommand { id: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResult {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub favicon: Option<String>,
    pub action: SearchAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SearchResults {
    pub query: String,
    pub results: Vec<SearchResult>,
}

/// Split divider hit-strip in window logical coordinates; the chrome renders
/// these as drag targets on platforms without native stage dividers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DividerView {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub vertical: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct LayoutState {
    pub dividers: Vec<DividerView>,
}

/// Immediate result returned by a privileged IPC command. `accepted` with an
/// `operation_id` means the mutation was successfully and non-evictably
/// admitted to the shell's process-local ordered FIFO; it does not claim that
/// later native/store work succeeded or survive a process restart. `accepted`
/// without an id is reserved for a fully applied, privileged-UI-only action
/// such as toggling the launcher.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationAdmission {
    pub operation_id: Option<String>,
    pub accepted: bool,
}

/// The bounded terminal classification the actor can establish while
/// processing an admitted operation. `Deferred` means native work was queued
/// or an exact discard acknowledgement is still required; it never means a
/// page load or renderer callback succeeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Applied,
    NoOp,
    Rejected,
    NativeAdmissionFailed,
    Deferred,
}

/// Stable, non-page-derived detail for an operation outcome. Keeping this an
/// enum prevents native errors, URLs, or attacker-controlled strings from
/// becoming an unbounded privileged IPC/logging surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationReason {
    MutationApplied,
    StateUnchanged,
    InvalidScope,
    NoFocusedWindow,
    ItemLimitReached,
    InvalidInput,
    HistoryUnavailable,
    LayoutUnavailable,
    UnsupportedCommand,
    NativeDispatchRejected,
    NativeWorkPending,
    DiscardCompletionPending,
    StoreWorkPending,
    StoreAdmissionRejected,
    ProfileDeletionPolicyRejected,
    ProfileDeletionInProgress,
    ProfileDeletionCompleted,
}

/// The shell has processed an admitted operation in actor order. Consumers
/// reconcile logical effects from authoritative projections. A deferred
/// native navigation still resolves independently through engine events.
/// Long-running profile deletion retains its id internally and emits this
/// disposition exactly once, only after definitive rejection or both durable
/// deletion phases complete; retry state is never mislabeled as processed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationDisposition {
    pub operation_id: String,
    pub outcome: OperationOutcome,
    pub reason: OperationReason,
}

/// Process-local reconciliation state for an admitted mutation. Pending and
/// processed entries are retained in a bounded fail-closed desktop ledger;
/// processed entries remain queryable until privileged chrome acknowledges
/// them. `Unknown` means the id was never admitted in this process, was already
/// acknowledged, or belongs to a previous process lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OperationStatus {
    Unknown,
    Pending,
    Processed { disposition: OperationDisposition },
}

/// Process-lifetime browser-runtime state. Once `restart_required` becomes
/// true it remains true until the whole application exits; it is not cleared
/// by rebuilding a content WebView or profile environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuntimeStatus {
    pub restart_required: bool,
}

/// Snapshots for structural changes, single-row deltas for per-tab churn.
#[derive(Clone, Debug)]
pub enum Projection {
    Items(ItemsState),
    Tab(TabView),
    UiCommand(String),
    Search(SearchResults),
    Layout(LayoutState),
    RuntimeStatus(RuntimeStatus),
    OperationProcessed(OperationDisposition),
}
