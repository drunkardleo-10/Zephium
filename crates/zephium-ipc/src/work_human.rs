//! Ephemeral native page handoff. Commands acknowledge queue admission only.
use super::*;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkHumanPageIdV1 {
    pub attempt: WorkAttemptId,
    pub step: WorkStepId,
    pub generation: u32,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkHumanPhaseV1 {
    Reading,
    WaitingForHuman,
    Presenting,
    Presented,
    Continuing,
    Released,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkHumanReasonV1 {
    SignIn,
    Challenge,
    Permission,
    Verification,
    UserDecision,
    SensitiveEffect,
    UnsupportedInteraction,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkHumanPageV1 {
    pub id: WorkHumanPageIdV1,
    pub phase: WorkHumanPhaseV1,
    pub reason: WorkHumanReasonV1,
    pub remaining_millis: u32,
    pub document_revision: String,
    pub can_continue: bool,
}
/// Logical points relative to the native content view's top-left corner.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkHumanRegionV1 {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
/// Explicit user attestation; neither choice permits sensitive provider disclosure.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkHumanAccountV1 {
    Anonymous,
    SignedInPublicOnly,
}
/// Invalidation, including document changes; read current pages after delivery.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkHumanChangedV1 {
    pub profile: String,
    pub work: WorkId,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkHumanResponseV1 {
    pub version: u16,
    pub profile: String,
    pub work: WorkId,
    pub accepted: bool,
    pub pages: Vec<WorkHumanPageV1>,
    pub error: Option<WorkFailureV1>,
}
