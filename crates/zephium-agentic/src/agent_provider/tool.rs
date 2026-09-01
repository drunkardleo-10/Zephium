//! Closed model-facing browser-tool proposal decoder.
//!
//! Provider-authored JSON is size/depth checked, parsed with unknown-field
//! denial, and converted immediately into existing semantic domain types. Raw
//! arguments, selectors, JavaScript, DOM, HTML, CDP, native handles, and shell
//! identities cannot cross the public boundary. Every result remains a
//! pre-policy proposal and grants no browser or supervisor authority.

use std::fmt;

use serde::Deserialize;
use thiserror::Error;

use crate::{
    ContextNavigationTarget, SemanticActionContractError, SemanticActionIntent,
    SemanticActionProposal, SemanticActionText, SemanticDialogState, SemanticEffectClass,
    SemanticExtractionSchemaId, SemanticMutationQuietPeriod, SemanticPressKey, SemanticReferenceId,
    SemanticScrollAmount, SemanticScrollDirection, SemanticSettleBudget, SemanticState,
    SemanticTextWindow, SemanticVerification, SemanticWaitCondition,
    MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES, MAX_SEMANTIC_ACTIONS_PER_BATCH,
    MAX_SEMANTIC_ACTION_BATCH_SETTLE_MILLIS, MAX_SEMANTIC_ACTION_BATCH_TEXT_BYTES,
};

use super::request::MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES;
use super::AgentProviderCallIdentity;

/// Maximum bytes in one opaque provider tool-call identity.
pub const MAX_AGENT_PROVIDER_TOOL_CALL_ID_BYTES: usize = 128;
/// Maximum UTF-8 bytes in one semantic locate query.
pub const MAX_AGENT_BROWSER_SEMANTIC_QUERY_BYTES: usize = 1_024;
const MAX_AGENT_BROWSER_TOOL_JSON_DEPTH: u8 = 16;

/// Closed model-facing browser tool names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserToolKind {
    /// Navigate to one validated HTTP(S) target.
    Navigate,
    /// Traverse one native history step backward.
    Back,
    /// Traverse one native history step forward.
    Forward,
    /// Reload the exact current document.
    Reload,
    /// Request one bounded semantic observation.
    Snapshot,
    /// Locate semantics without accepting a selector.
    Locate,
    /// Propose one bounded homogeneous semantic action batch.
    Act,
    /// Wait for one typed condition under a relative ceiling.
    Wait,
    /// Request bounded readable semantic content.
    Read,
    /// Request one shell-registered extraction schema.
    Extract,
    /// Request the fixed bounded viewport screenshot scope.
    Screenshot,
    /// Pause for explicit human control or review.
    ShowForHuman,
    /// Ask the supervisor to check whether human control has ended.
    ResumeAfterHuman,
}

impl AgentBrowserToolKind {
    pub(crate) const ALL: [Self; 13] = [
        Self::Navigate,
        Self::Back,
        Self::Forward,
        Self::Reload,
        Self::Snapshot,
        Self::Locate,
        Self::Act,
        Self::Wait,
        Self::Read,
        Self::Extract,
        Self::Screenshot,
        Self::ShowForHuman,
        Self::ResumeAfterHuman,
    ];

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "navigate" => Some(Self::Navigate),
            "back" => Some(Self::Back),
            "forward" => Some(Self::Forward),
            "reload" => Some(Self::Reload),
            "snapshot" => Some(Self::Snapshot),
            "locate" => Some(Self::Locate),
            "act" => Some(Self::Act),
            "wait" => Some(Self::Wait),
            "read" => Some(Self::Read),
            "extract" => Some(Self::Extract),
            "screenshot" => Some(Self::Screenshot),
            "show_for_human" => Some(Self::ShowForHuman),
            "resume_after_human" => Some(Self::ResumeAfterHuman),
            _ => None,
        }
    }

    /// Sole provider-facing function name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Reload => "reload",
            Self::Snapshot => "snapshot",
            Self::Locate => "locate",
            Self::Act => "act",
            Self::Wait => "wait",
            Self::Read => "read",
            Self::Extract => "extract",
            Self::Screenshot => "screenshot",
            Self::ShowForHuman => "show_for_human",
            Self::ResumeAfterHuman => "resume_after_human",
        }
    }
}

/// Opaque bounded provider identity used only to correlate a later tool result.
#[derive(Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgentBrowserToolCallId(String);

impl AgentBrowserToolCallId {
    pub(crate) fn try_new(value: String) -> Result<Self, AgentBrowserToolContractError> {
        if value.is_empty()
            || value.len() > MAX_AGENT_PROVIDER_TOOL_CALL_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(AgentBrowserToolContractError::CallId);
        }
        Ok(Self(value))
    }

    /// Exact provider correlation value for one fixed tool-result request.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AgentBrowserToolCallId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentBrowserToolCallId([redacted])")
    }
}

/// Bounded natural-language semantic query, never a DOM selector.
#[derive(Eq, PartialEq)]
pub struct AgentBrowserSemanticQuery(String);

impl AgentBrowserSemanticQuery {
    fn try_new(value: String) -> Result<Self, AgentBrowserToolContractError> {
        if value.is_empty() || value.len() > MAX_AGENT_BROWSER_SEMANTIC_QUERY_BYTES {
            return Err(AgentBrowserToolContractError::Query);
        }
        if value.chars().any(invalid_model_text_character) {
            return Err(AgentBrowserToolContractError::Query);
        }
        Ok(Self(value))
    }

    /// Exact untrusted semantic query for a fixed semantic matcher.
    ///
    /// A consumer must never pass this value to a CSS/XPath/JavaScript API.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AgentBrowserSemanticQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentBrowserSemanticQuery")
            .field("bytes", &self.0.len())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Closed progressive semantic scope proposed by the model.
///
/// Reference-bearing variants are not `SemanticScope` authority. The shell
/// must resolve them against one exact acknowledged observation before use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentBrowserScopeProposal {
    /// Filtered current viewport/document scope.
    Initial,
    /// Expand one referenced meaningful region.
    Region(SemanticReferenceId),
    /// Expand one referenced subtree.
    Subtree(SemanticReferenceId),
    /// Expand one referenced table.
    Table(SemanticReferenceId),
    /// Expand one referenced supported frame boundary.
    Frame(SemanticReferenceId),
    /// Expand bounded readable text surrounding one reference.
    SurroundingText {
        /// Opaque prior-observation reference proposal.
        target: SemanticReferenceId,
        /// Existing hard-bounded surrounding window.
        window: SemanticTextWindow,
    },
}

/// Closed reason offered to the supervisor for a human-control pause.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserHumanReason {
    /// Authentication or account selection needs a person.
    SignIn,
    /// A permission or operating-system boundary needs a person.
    Permission,
    /// The current control has no qualified safe automation backend.
    UnsupportedInteraction,
    /// Effect verification is ambiguous and needs inspection.
    Verification,
    /// The workflow needs an explicit user decision.
    UserDecision,
    /// A sensitive or durable effect requires explicit review.
    SensitiveEffect,
    /// A CAPTCHA or equivalent human challenge is present.
    HumanChallenge,
}

/// Closed standalone wait condition proposed by the model.
///
/// Unlike an action-local `SemanticWaitCondition`, target-scoped variants
/// carry the exact opaque reference they propose to observe. This value is
/// still not observation or timer authority: the shell must bind the target
/// against one exact acknowledged observation and derive one absolute
/// deadline before waiting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBrowserWaitCondition {
    /// Observe immediately without an idle heuristic.
    Immediate,
    /// Wait for one exact native navigation commitment.
    NavigationCommitted,
    /// Wait for the current document to reach the adapter's fixed ready state.
    DocumentReady,
    /// Wait for one referenced target to gain or lose an allowlisted state.
    TargetState {
        /// Opaque prior-observation target proposal.
        target: SemanticReferenceId,
        /// Allowlisted state.
        state: SemanticState,
        /// Required presence.
        present: bool,
    },
    /// Wait for the native-attested committed URL to change.
    UrlChanged,
    /// Wait for bounded semantic title state to change.
    TitleChanged,
    /// Wait for a browser/page dialog boundary.
    Dialog(SemanticDialogState),
    /// Wait for a semantic projection change.
    SemanticChange,
    /// Wait for bounded mutation quiet, never network idle.
    MutationQuiet(SemanticMutationQuietPeriod),
    /// Wait for independently sampled scroll movement of one referenced target.
    ScrollPositionChanged {
        /// Opaque prior-observation target proposal.
        target: SemanticReferenceId,
    },
}

/// Pre-observation bounded semantic action-batch proposal.
#[must_use]
#[derive(Eq, PartialEq)]
pub struct AgentBrowserActProposal {
    actions: Vec<SemanticActionProposal>,
    effect: SemanticEffectClass,
    settle_millis: u32,
    text_bytes: usize,
}

impl AgentBrowserActProposal {
    fn try_new(
        actions: Vec<SemanticActionProposal>,
    ) -> Result<Self, AgentBrowserToolContractError> {
        let Some(first) = actions.first() else {
            return Err(AgentBrowserToolContractError::ActionBatch);
        };
        if actions.len() > MAX_SEMANTIC_ACTIONS_PER_BATCH {
            return Err(AgentBrowserToolContractError::ActionBatch);
        }
        let effect = first.effect();
        if actions.iter().any(|action| action.effect() != effect) {
            return Err(AgentBrowserToolContractError::ActionBatch);
        }
        if matches!(
            effect,
            SemanticEffectClass::ExternalWrite
                | SemanticEffectClass::Communication
                | SemanticEffectClass::Purchase
                | SemanticEffectClass::Destructive
                | SemanticEffectClass::CapabilityBoundary
        ) && actions.len() != 1
        {
            return Err(AgentBrowserToolContractError::ActionBatch);
        }
        let mut settle_millis = 0_u32;
        let mut text_bytes = 0_usize;
        for action in &actions {
            settle_millis = settle_millis
                .checked_add(action.settle_budget().millis())
                .ok_or(AgentBrowserToolContractError::ActionBatch)?;
            if settle_millis > MAX_SEMANTIC_ACTION_BATCH_SETTLE_MILLIS {
                return Err(AgentBrowserToolContractError::ActionBatch);
            }
            if let SemanticActionIntent::Fill { value, .. } = action.intent() {
                text_bytes = text_bytes
                    .checked_add(value.len())
                    .ok_or(AgentBrowserToolContractError::ActionBatch)?;
                if text_bytes > MAX_SEMANTIC_ACTION_BATCH_TEXT_BYTES {
                    return Err(AgentBrowserToolContractError::ActionBatch);
                }
            }
        }
        Ok(Self {
            actions,
            effect,
            settle_millis,
            text_bytes,
        })
    }

    /// Validated unbound semantic proposals in model order.
    pub fn actions(&self) -> &[SemanticActionProposal] {
        &self.actions
    }

    /// Homogeneous untrusted declared effect boundary.
    pub const fn effect(&self) -> SemanticEffectClass {
        self.effect
    }

    /// Aggregate relative settle ceiling.
    pub const fn settle_millis(&self) -> u32 {
        self.settle_millis
    }

    /// Aggregate bounded fill-text bytes.
    pub const fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    /// Consumes proposals for exact observation binding without copying text.
    pub fn into_actions(self) -> Vec<SemanticActionProposal> {
        self.actions
    }
}

impl fmt::Debug for AgentBrowserActProposal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentBrowserActProposal")
            .field("actions", &self.actions.len())
            .field("effect", &self.effect)
            .field("settle_millis", &self.settle_millis)
            .field("text_bytes", &self.text_bytes)
            .finish()
    }
}

/// Closed pre-policy proposal returned from a provider tool call.
#[must_use]
#[derive(Eq, PartialEq)]
pub enum AgentBrowserToolProposal {
    /// Validated navigation target; policy still decides origin scope.
    Navigate(ContextNavigationTarget),
    /// Native history backward proposal.
    Back,
    /// Native history forward proposal.
    Forward,
    /// Current-document reload proposal.
    Reload,
    /// Bounded semantic observation proposal.
    Snapshot(AgentBrowserScopeProposal),
    /// Semantic query and scope proposal.
    Locate {
        /// Bounded untrusted natural-language query.
        query: AgentBrowserSemanticQuery,
        /// Closed semantic scope proposal.
        scope: AgentBrowserScopeProposal,
    },
    /// Bounded semantic action proposal.
    Act(AgentBrowserActProposal),
    /// Typed wait proposal under one relative ceiling.
    Wait {
        /// Closed condition.
        condition: AgentBrowserWaitCondition,
        /// Relative ceiling; the shell derives one absolute deadline.
        timeout: SemanticSettleBudget,
    },
    /// Bounded semantic read proposal.
    Read(AgentBrowserScopeProposal),
    /// Trusted-registry schema selection proposal.
    Extract {
        /// Closed semantic scope proposal.
        scope: AgentBrowserScopeProposal,
        /// Opaque schema identity; the shell must resolve it exactly.
        schema: SemanticExtractionSchemaId,
    },
    /// Fixed current-viewport screenshot proposal.
    Screenshot,
    /// Supervisor-mediated human pause proposal.
    ShowForHuman(AgentBrowserHumanReason),
    /// Supervisor-mediated resume check, never direct resume authority.
    ResumeAfterHuman,
}

impl AgentBrowserToolProposal {
    /// Closed browser tool class.
    pub const fn kind(&self) -> AgentBrowserToolKind {
        match self {
            Self::Navigate(_) => AgentBrowserToolKind::Navigate,
            Self::Back => AgentBrowserToolKind::Back,
            Self::Forward => AgentBrowserToolKind::Forward,
            Self::Reload => AgentBrowserToolKind::Reload,
            Self::Snapshot(_) => AgentBrowserToolKind::Snapshot,
            Self::Locate { .. } => AgentBrowserToolKind::Locate,
            Self::Act(_) => AgentBrowserToolKind::Act,
            Self::Wait { .. } => AgentBrowserToolKind::Wait,
            Self::Read(_) => AgentBrowserToolKind::Read,
            Self::Extract { .. } => AgentBrowserToolKind::Extract,
            Self::Screenshot => AgentBrowserToolKind::Screenshot,
            Self::ShowForHuman(_) => AgentBrowserToolKind::ShowForHuman,
            Self::ResumeAfterHuman => AgentBrowserToolKind::ResumeAfterHuman,
        }
    }
}

impl fmt::Debug for AgentBrowserToolProposal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = formatter.debug_struct("AgentBrowserToolProposal");
        value.field("kind", &self.kind());
        match self {
            Self::Snapshot(scope) | Self::Read(scope) => value.field("scope", scope),
            Self::Locate { query, scope } => value.field("query", query).field("scope", scope),
            Self::Act(actions) => value.field("actions", actions),
            Self::Wait { condition, timeout } => value
                .field("condition", condition)
                .field("timeout", timeout),
            Self::Extract { scope, schema } => value.field("scope", scope).field("schema", schema),
            Self::ShowForHuman(reason) => value.field("reason", reason),
            Self::Navigate(_)
            | Self::Back
            | Self::Forward
            | Self::Reload
            | Self::Screenshot
            | Self::ResumeAfterHuman => &mut value,
        };
        value.finish()
    }
}

/// One typed provider tool call; still not browser or policy authority.
#[must_use]
#[derive(Eq, PartialEq)]
pub struct AgentBrowserToolCall {
    source_call: AgentProviderCallIdentity,
    id: AgentBrowserToolCallId,
    proposal: AgentBrowserToolProposal,
    provider_item_id: Option<String>,
    arguments: String,
}

impl AgentBrowserToolCall {
    pub(crate) fn decode(
        source_call: AgentProviderCallIdentity,
        id: String,
        name: &str,
        arguments: String,
    ) -> Result<Self, AgentBrowserToolContractError> {
        Self::decode_inner(source_call, None, id, name, arguments)
    }

    pub(crate) fn decode_openai(
        source_call: AgentProviderCallIdentity,
        provider_item_id: String,
        id: String,
        name: &str,
        arguments: String,
    ) -> Result<Self, AgentBrowserToolContractError> {
        if provider_item_id.is_empty()
            || provider_item_id.len() > MAX_AGENT_PROVIDER_TOOL_CALL_ID_BYTES
            || !provider_item_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(AgentBrowserToolContractError::CallId);
        }
        Self::decode_inner(source_call, Some(provider_item_id), id, name, arguments)
    }

    fn decode_inner(
        source_call: AgentProviderCallIdentity,
        provider_item_id: Option<String>,
        id: String,
        name: &str,
        arguments: String,
    ) -> Result<Self, AgentBrowserToolContractError> {
        let id = AgentBrowserToolCallId::try_new(id)?;
        let proposal = decode_proposal(name, &arguments)?;
        Ok(Self {
            source_call,
            id,
            proposal,
            provider_item_id,
            arguments,
        })
    }

    /// Opaque provider correlation for a later fixed tool result.
    pub const fn id(&self) -> &AgentBrowserToolCallId {
        &self.id
    }

    /// Closed pre-policy browser proposal.
    pub const fn proposal(&self) -> &AgentBrowserToolProposal {
        &self.proposal
    }

    /// Consumes the call without copying model-authored text.
    pub fn into_parts(self) -> (AgentBrowserToolCallId, AgentBrowserToolProposal) {
        (self.id, self.proposal)
    }

    /// Splits the proposal from its exact one-shot provider-result correlation.
    ///
    /// The correlation retains bounded provider-authored arguments only for a
    /// later fixed continuation request. It exposes no raw JSON or browser
    /// authority and must never be logged or persisted.
    pub fn into_continuation_parts(
        self,
    ) -> (AgentProviderToolCallCorrelation, AgentBrowserToolProposal) {
        (
            AgentProviderToolCallCorrelation {
                source_call: self.source_call,
                id: self.id,
                kind: self.proposal.kind(),
                provider_item_id: self.provider_item_id,
                arguments: self.arguments,
            },
            self.proposal,
        )
    }
}

impl fmt::Debug for AgentBrowserToolCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentBrowserToolCall")
            .field("id", &self.id)
            .field("proposal", &self.proposal)
            .finish()
    }
}

/// Move-only exact provider correlation retained for one fixed tool result.
///
/// This value is neither browser-operation nor model-call authority. Its raw
/// arguments remain private so no caller can reinterpret them as selectors,
/// JavaScript, DOM, or a generic provider payload.
#[must_use]
pub struct AgentProviderToolCallCorrelation {
    pub(super) source_call: AgentProviderCallIdentity,
    pub(super) id: AgentBrowserToolCallId,
    pub(super) kind: AgentBrowserToolKind,
    pub(super) provider_item_id: Option<String>,
    pub(super) arguments: String,
}

impl AgentProviderToolCallCorrelation {
    /// Exact bounded provider tool-call identifier.
    pub const fn id(&self) -> &AgentBrowserToolCallId {
        &self.id
    }

    /// Closed browser tool class returned by the provider.
    pub const fn kind(&self) -> AgentBrowserToolKind {
        self.kind
    }

    /// Exact retained provider-argument bytes without exposing their content.
    pub fn argument_bytes(&self) -> usize {
        self.arguments.len()
    }
}

impl fmt::Debug for AgentProviderToolCallCorrelation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderToolCallCorrelation")
            .field("source_call", &self.source_call)
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field(
                "provider_item_id",
                &self.provider_item_id.as_ref().map(|_| "[redacted]"),
            )
            .field("argument_bytes", &self.arguments.len())
            .field("arguments", &"[redacted]")
            .finish()
    }
}

/// Closed refusal from malicious or malformed model tool output.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AgentBrowserToolContractError {
    /// Provider tool-call identity was empty, oversized, or unsafe ASCII.
    #[error("agent browser tool-call identity is invalid")]
    CallId,
    /// Tool name is outside the fixed browser vocabulary.
    #[error("agent browser tool name is invalid")]
    ToolName,
    /// Raw arguments exceeded the global byte ceiling.
    #[error("agent browser tool argument ceiling exceeded")]
    ArgumentLimit,
    /// JSON was malformed, too deeply nested, or contained unknown fields.
    #[error("agent browser tool arguments are invalid")]
    Arguments,
    /// Opaque semantic reference was not in canonical `@aN` form.
    #[error("agent browser tool reference is invalid")]
    Reference,
    /// Navigation target violated the browser navigation policy.
    #[error("agent browser navigation target is invalid")]
    Navigation,
    /// Semantic query was empty, oversized, or contained forbidden controls.
    #[error("agent browser semantic query is invalid")]
    Query,
    /// Scope kind, reference, or surrounding window was invalid.
    #[error("agent browser semantic scope is invalid")]
    Scope,
    /// One semantic action violated its fixed operation/outcome contract.
    #[error("agent browser semantic action is invalid")]
    Action,
    /// Action count, text, effect, or aggregate settle ceiling was invalid.
    #[error("agent browser semantic action batch is invalid")]
    ActionBatch,
    /// Standalone wait condition or relative timeout was invalid.
    #[error("agent browser wait is invalid")]
    Wait,
    /// Extraction schema identity was zero or otherwise invalid.
    #[error("agent browser extraction schema is invalid")]
    ExtractionSchema,
}

fn decode_proposal(
    name: &str,
    arguments: &str,
) -> Result<AgentBrowserToolProposal, AgentBrowserToolContractError> {
    if arguments.len() > MAX_AGENT_PROVIDER_TOOL_ARGUMENT_BYTES as usize {
        return Err(AgentBrowserToolContractError::ArgumentLimit);
    }
    validate_json_depth(arguments)?;
    let kind = AgentBrowserToolKind::parse(name).ok_or(AgentBrowserToolContractError::ToolName)?;
    match kind {
        AgentBrowserToolKind::Navigate => {
            let value: NavigateWire = parse(arguments)?;
            if value.url.len() > MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES {
                return Err(AgentBrowserToolContractError::Navigation);
            }
            let target = ContextNavigationTarget::parse(&value.url)
                .map_err(|_| AgentBrowserToolContractError::Navigation)?;
            Ok(AgentBrowserToolProposal::Navigate(target))
        }
        AgentBrowserToolKind::Back => {
            parse_empty(arguments)?;
            Ok(AgentBrowserToolProposal::Back)
        }
        AgentBrowserToolKind::Forward => {
            parse_empty(arguments)?;
            Ok(AgentBrowserToolProposal::Forward)
        }
        AgentBrowserToolKind::Reload => {
            parse_empty(arguments)?;
            Ok(AgentBrowserToolProposal::Reload)
        }
        AgentBrowserToolKind::Snapshot => {
            let value: ScopeArgumentsWire = parse(arguments)?;
            Ok(AgentBrowserToolProposal::Snapshot(decode_scope(
                value.scope,
            )?))
        }
        AgentBrowserToolKind::Locate => {
            let value: LocateWire = parse(arguments)?;
            Ok(AgentBrowserToolProposal::Locate {
                query: AgentBrowserSemanticQuery::try_new(value.semantic_query)?,
                scope: decode_scope(value.scope)?,
            })
        }
        AgentBrowserToolKind::Act => {
            let value: ActWire = parse(arguments)?;
            let actions = value
                .actions
                .into_iter()
                .map(decode_action)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(AgentBrowserToolProposal::Act(
                AgentBrowserActProposal::try_new(actions)?,
            ))
        }
        AgentBrowserToolKind::Wait => {
            let value: WaitArgumentsWire = parse(arguments)?;
            let condition = decode_standalone_wait(value.condition)?;
            let timeout = SemanticSettleBudget::try_new(value.timeout_millis)
                .map_err(|_| AgentBrowserToolContractError::Wait)?;
            Ok(AgentBrowserToolProposal::Wait { condition, timeout })
        }
        AgentBrowserToolKind::Read => {
            let value: ScopeArgumentsWire = parse(arguments)?;
            Ok(AgentBrowserToolProposal::Read(decode_scope(value.scope)?))
        }
        AgentBrowserToolKind::Extract => {
            let value: ExtractWire = parse(arguments)?;
            let schema = SemanticExtractionSchemaId::new(value.schema_id)
                .ok_or(AgentBrowserToolContractError::ExtractionSchema)?;
            Ok(AgentBrowserToolProposal::Extract {
                scope: decode_scope(value.scope)?,
                schema,
            })
        }
        AgentBrowserToolKind::Screenshot => {
            parse_empty(arguments)?;
            Ok(AgentBrowserToolProposal::Screenshot)
        }
        AgentBrowserToolKind::ShowForHuman => {
            let value: HumanWire = parse(arguments)?;
            Ok(AgentBrowserToolProposal::ShowForHuman(value.reason.into()))
        }
        AgentBrowserToolKind::ResumeAfterHuman => {
            parse_empty(arguments)?;
            Ok(AgentBrowserToolProposal::ResumeAfterHuman)
        }
    }
}

fn parse_empty(arguments: &str) -> Result<(), AgentBrowserToolContractError> {
    let _: EmptyWire = parse(arguments)?;
    Ok(())
}

fn parse<'a, T>(arguments: &'a str) -> Result<T, AgentBrowserToolContractError>
where
    T: Deserialize<'a>,
{
    serde_json::from_str(arguments).map_err(|_| AgentBrowserToolContractError::Arguments)
}

fn decode_scope(
    scope: Option<ScopeWire>,
) -> Result<AgentBrowserScopeProposal, AgentBrowserToolContractError> {
    match scope.unwrap_or(ScopeWire::Initial) {
        ScopeWire::Initial => Ok(AgentBrowserScopeProposal::Initial),
        ScopeWire::Region { target } => Ok(AgentBrowserScopeProposal::Region(reference(target)?)),
        ScopeWire::Subtree { target } => Ok(AgentBrowserScopeProposal::Subtree(reference(target)?)),
        ScopeWire::Table { target } => Ok(AgentBrowserScopeProposal::Table(reference(target)?)),
        ScopeWire::Frame { target } => Ok(AgentBrowserScopeProposal::Frame(reference(target)?)),
        ScopeWire::SurroundingText {
            target,
            before_bytes,
            after_bytes,
        } => Ok(AgentBrowserScopeProposal::SurroundingText {
            target: reference(target)?,
            window: SemanticTextWindow::try_new(before_bytes, after_bytes)
                .map_err(|_| AgentBrowserToolContractError::Scope)?,
        }),
    }
}

fn decode_action(
    action: ActionWire,
) -> Result<SemanticActionProposal, AgentBrowserToolContractError> {
    let (intent, contract) = match action {
        ActionWire::Click { target, contract } => (
            SemanticActionIntent::Click {
                target: reference(target)?,
            },
            contract,
        ),
        ActionWire::Fill {
            target,
            value,
            contract,
        } => (
            SemanticActionIntent::Fill {
                target: reference(target)?,
                value: SemanticActionText::try_new(value)
                    .map_err(|_| AgentBrowserToolContractError::Action)?,
            },
            contract,
        ),
        ActionWire::Select {
            target,
            option,
            contract,
        } => (
            SemanticActionIntent::Select {
                target: reference(target)?,
                option: reference(option)?,
            },
            contract,
        ),
        ActionWire::Press {
            target,
            key,
            contract,
        } => (
            SemanticActionIntent::Press {
                target: reference(target)?,
                key: key.into(),
            },
            contract,
        ),
        ActionWire::Scroll {
            target,
            direction,
            amount,
            contract,
        } => (
            SemanticActionIntent::Scroll {
                target: reference(target)?,
                direction: direction.into(),
                amount: amount.into(),
            },
            contract,
        ),
    };
    let wait = decode_wait(contract.wait)?;
    let verification = decode_verification(contract.verification)?;
    let settle_budget = SemanticSettleBudget::try_new(contract.settle_millis)
        .map_err(|_| AgentBrowserToolContractError::Action)?;
    SemanticActionProposal::try_new(
        intent,
        contract.effect.into(),
        wait,
        verification,
        settle_budget,
    )
    .map_err(|_: SemanticActionContractError| AgentBrowserToolContractError::Action)
}

fn decode_wait(value: WaitWire) -> Result<SemanticWaitCondition, AgentBrowserToolContractError> {
    match value {
        WaitWire::Immediate => Ok(SemanticWaitCondition::Immediate),
        WaitWire::NavigationCommitted => Ok(SemanticWaitCondition::NavigationCommitted),
        WaitWire::DocumentReady => Ok(SemanticWaitCondition::DocumentReady),
        WaitWire::TargetState { state, present } => Ok(SemanticWaitCondition::TargetState {
            state: state.into(),
            present,
        }),
        WaitWire::UrlChanged => Ok(SemanticWaitCondition::UrlChanged),
        WaitWire::TitleChanged => Ok(SemanticWaitCondition::TitleChanged),
        WaitWire::Dialog { state } => Ok(SemanticWaitCondition::Dialog(state.into())),
        WaitWire::SemanticChange => Ok(SemanticWaitCondition::SemanticChange),
        WaitWire::MutationQuiet { millis } => Ok(SemanticWaitCondition::MutationQuiet(
            SemanticMutationQuietPeriod::try_new(millis)
                .map_err(|_| AgentBrowserToolContractError::Wait)?,
        )),
        WaitWire::ScrollPositionChanged => Ok(SemanticWaitCondition::ScrollPositionChanged),
    }
}

fn decode_standalone_wait(
    value: StandaloneWaitWire,
) -> Result<AgentBrowserWaitCondition, AgentBrowserToolContractError> {
    match value {
        StandaloneWaitWire::Immediate => Ok(AgentBrowserWaitCondition::Immediate),
        StandaloneWaitWire::NavigationCommitted => {
            Ok(AgentBrowserWaitCondition::NavigationCommitted)
        }
        StandaloneWaitWire::DocumentReady => Ok(AgentBrowserWaitCondition::DocumentReady),
        StandaloneWaitWire::TargetState {
            target,
            state,
            present,
        } => Ok(AgentBrowserWaitCondition::TargetState {
            target: reference(target)?,
            state: state.into(),
            present,
        }),
        StandaloneWaitWire::UrlChanged => Ok(AgentBrowserWaitCondition::UrlChanged),
        StandaloneWaitWire::TitleChanged => Ok(AgentBrowserWaitCondition::TitleChanged),
        StandaloneWaitWire::Dialog { state } => Ok(AgentBrowserWaitCondition::Dialog(state.into())),
        StandaloneWaitWire::SemanticChange => Ok(AgentBrowserWaitCondition::SemanticChange),
        StandaloneWaitWire::MutationQuiet { millis } => {
            Ok(AgentBrowserWaitCondition::MutationQuiet(
                SemanticMutationQuietPeriod::try_new(millis)
                    .map_err(|_| AgentBrowserToolContractError::Wait)?,
            ))
        }
        StandaloneWaitWire::ScrollPositionChanged { target } => {
            Ok(AgentBrowserWaitCondition::ScrollPositionChanged {
                target: reference(target)?,
            })
        }
    }
}

fn decode_verification(
    value: VerificationWire,
) -> Result<SemanticVerification, AgentBrowserToolContractError> {
    Ok(match value {
        VerificationWire::TargetState { state, present } => SemanticVerification::TargetState {
            state: state.into(),
            present,
        },
        VerificationWire::TargetValueMatchesInput => SemanticVerification::TargetValueMatchesInput,
        VerificationWire::TargetValueChanged => SemanticVerification::TargetValueChanged,
        VerificationWire::TargetSelectionMatchesOption => {
            SemanticVerification::TargetSelectionMatchesOption
        }
        VerificationWire::TargetSelectionChanged => SemanticVerification::TargetSelectionChanged,
        VerificationWire::NavigationCommitted => SemanticVerification::NavigationCommitted,
        VerificationWire::Dialog { state } => SemanticVerification::Dialog(state.into()),
        VerificationWire::ScrollPositionChanged => SemanticVerification::ScrollPositionChanged,
    })
}

fn reference(value: String) -> Result<SemanticReferenceId, AgentBrowserToolContractError> {
    SemanticReferenceId::parse(&value).ok_or(AgentBrowserToolContractError::Reference)
}

fn validate_json_depth(arguments: &str) -> Result<(), AgentBrowserToolContractError> {
    let mut depth = 0_u8;
    let mut in_string = false;
    let mut escaped = false;
    for byte in arguments.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(AgentBrowserToolContractError::Arguments)?;
                if depth > MAX_AGENT_BROWSER_TOOL_JSON_DEPTH {
                    return Err(AgentBrowserToolContractError::Arguments);
                }
            }
            b'}' | b']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(AgentBrowserToolContractError::Arguments)?;
            }
            _ => {}
        }
    }
    if depth != 0 || in_string || escaped {
        return Err(AgentBrowserToolContractError::Arguments);
    }
    Ok(())
}

fn invalid_model_text_character(character: char) -> bool {
    (character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
        || matches!(
            character,
            '\u{00ad}'
                | '\u{061c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{e0001}'
                | '\u{e0020}'..='\u{e007f}'
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyWire {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NavigateWire {
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeArgumentsWire {
    #[serde(default)]
    scope: Option<ScopeWire>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScopeWire {
    Initial,
    Region {
        target: String,
    },
    Subtree {
        target: String,
    },
    Table {
        target: String,
    },
    Frame {
        target: String,
    },
    SurroundingText {
        target: String,
        before_bytes: u16,
        after_bytes: u16,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocateWire {
    semantic_query: String,
    #[serde(default)]
    scope: Option<ScopeWire>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActWire {
    actions: Vec<ActionWire>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ActionWire {
    Click {
        target: String,
        #[serde(flatten)]
        contract: ActionContractWire,
    },
    Fill {
        target: String,
        value: String,
        #[serde(flatten)]
        contract: ActionContractWire,
    },
    Select {
        target: String,
        option: String,
        #[serde(flatten)]
        contract: ActionContractWire,
    },
    Press {
        target: String,
        key: PressKeyWire,
        #[serde(flatten)]
        contract: ActionContractWire,
    },
    Scroll {
        target: String,
        direction: ScrollDirectionWire,
        amount: ScrollAmountWire,
        #[serde(flatten)]
        contract: ActionContractWire,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionContractWire {
    effect: EffectWire,
    wait: WaitWire,
    verification: VerificationWire,
    settle_millis: u32,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EffectWire {
    Read,
    LocalWrite,
    ExternalWrite,
    Communication,
    Purchase,
    Destructive,
    CapabilityBoundary,
}

impl From<EffectWire> for SemanticEffectClass {
    fn from(value: EffectWire) -> Self {
        match value {
            EffectWire::Read => Self::Read,
            EffectWire::LocalWrite => Self::LocalWrite,
            EffectWire::ExternalWrite => Self::ExternalWrite,
            EffectWire::Communication => Self::Communication,
            EffectWire::Purchase => Self::Purchase,
            EffectWire::Destructive => Self::Destructive,
            EffectWire::CapabilityBoundary => Self::CapabilityBoundary,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum WaitWire {
    Immediate,
    NavigationCommitted,
    DocumentReady,
    TargetState { state: StateWire, present: bool },
    UrlChanged,
    TitleChanged,
    Dialog { state: DialogWire },
    SemanticChange,
    MutationQuiet { millis: u32 },
    ScrollPositionChanged,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StandaloneWaitWire {
    Immediate,
    NavigationCommitted,
    DocumentReady,
    TargetState {
        target: String,
        state: StateWire,
        present: bool,
    },
    UrlChanged,
    TitleChanged,
    Dialog {
        state: DialogWire,
    },
    SemanticChange,
    MutationQuiet {
        millis: u32,
    },
    ScrollPositionChanged {
        target: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum VerificationWire {
    TargetState { state: StateWire, present: bool },
    TargetValueMatchesInput,
    TargetValueChanged,
    TargetSelectionMatchesOption,
    TargetSelectionChanged,
    NavigationCommitted,
    Dialog { state: DialogWire },
    ScrollPositionChanged,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StateWire {
    Checked,
    Selected,
    Expanded,
    Disabled,
    Required,
    Invalid,
    Focused,
}

impl From<StateWire> for SemanticState {
    fn from(value: StateWire) -> Self {
        match value {
            StateWire::Checked => Self::Checked,
            StateWire::Selected => Self::Selected,
            StateWire::Expanded => Self::Expanded,
            StateWire::Disabled => Self::Disabled,
            StateWire::Required => Self::Required,
            StateWire::Invalid => Self::Invalid,
            StateWire::Focused => Self::Focused,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DialogWire {
    Present,
    Absent,
}

impl From<DialogWire> for SemanticDialogState {
    fn from(value: DialogWire) -> Self {
        match value {
            DialogWire::Present => Self::Present,
            DialogWire::Absent => Self::Absent,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PressKeyWire {
    Enter,
    Escape,
    Space,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
}

impl From<PressKeyWire> for SemanticPressKey {
    fn from(value: PressKeyWire) -> Self {
        match value {
            PressKeyWire::Enter => Self::Enter,
            PressKeyWire::Escape => Self::Escape,
            PressKeyWire::Space => Self::Space,
            PressKeyWire::Tab => Self::Tab,
            PressKeyWire::ArrowUp => Self::ArrowUp,
            PressKeyWire::ArrowDown => Self::ArrowDown,
            PressKeyWire::ArrowLeft => Self::ArrowLeft,
            PressKeyWire::ArrowRight => Self::ArrowRight,
            PressKeyWire::Home => Self::Home,
            PressKeyWire::End => Self::End,
            PressKeyWire::PageUp => Self::PageUp,
            PressKeyWire::PageDown => Self::PageDown,
            PressKeyWire::Backspace => Self::Backspace,
            PressKeyWire::Delete => Self::Delete,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScrollDirectionWire {
    Up,
    Down,
    Left,
    Right,
}

impl From<ScrollDirectionWire> for SemanticScrollDirection {
    fn from(value: ScrollDirectionWire) -> Self {
        match value {
            ScrollDirectionWire::Up => Self::Up,
            ScrollDirectionWire::Down => Self::Down,
            ScrollDirectionWire::Left => Self::Left,
            ScrollDirectionWire::Right => Self::Right,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScrollAmountWire {
    Line,
    HalfPage,
    Page,
    IntoView,
}

impl From<ScrollAmountWire> for SemanticScrollAmount {
    fn from(value: ScrollAmountWire) -> Self {
        match value {
            ScrollAmountWire::Line => Self::Line,
            ScrollAmountWire::HalfPage => Self::HalfPage,
            ScrollAmountWire::Page => Self::Page,
            ScrollAmountWire::IntoView => Self::IntoView,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitArgumentsWire {
    condition: StandaloneWaitWire,
    timeout_millis: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractWire {
    #[serde(default)]
    scope: Option<ScopeWire>,
    schema_id: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanWire {
    reason: HumanReasonWire,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HumanReasonWire {
    SignIn,
    Permission,
    UnsupportedInteraction,
    Verification,
    UserDecision,
    SensitiveEffect,
    HumanChallenge,
}

impl From<HumanReasonWire> for AgentBrowserHumanReason {
    fn from(value: HumanReasonWire) -> Self {
        match value {
            HumanReasonWire::SignIn => Self::SignIn,
            HumanReasonWire::Permission => Self::Permission,
            HumanReasonWire::UnsupportedInteraction => Self::UnsupportedInteraction,
            HumanReasonWire::Verification => Self::Verification,
            HumanReasonWire::UserDecision => Self::UserDecision,
            HumanReasonWire::SensitiveEffect => Self::SensitiveEffect,
            HumanReasonWire::HumanChallenge => Self::HumanChallenge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_call() -> AgentProviderCallIdentity {
        AgentProviderCallIdentity {
            manifest: crate::AgentRunManifestId::from_raw(1),
            call: crate::AgentModelCallId::new(2).expect("call"),
            lease: crate::AgentPlanLeaseId::from_raw(3),
            node: crate::AgentPlanNodeId::from_raw(4),
        }
    }

    fn decode(
        name: &str,
        arguments: &str,
    ) -> Result<AgentBrowserToolCall, AgentBrowserToolContractError> {
        AgentBrowserToolCall::decode(
            source_call(),
            "call_abc-123".to_owned(),
            name,
            arguments.to_owned(),
        )
    }

    #[test]
    fn navigation_history_observation_and_human_tools_are_closed() {
        let navigate =
            decode("navigate", r#"{"url":"https://example.test/path"}"#).expect("navigate");
        assert_eq!(navigate.proposal().kind(), AgentBrowserToolKind::Navigate);
        for name in [
            "back",
            "forward",
            "reload",
            "screenshot",
            "resume_after_human",
        ] {
            assert_eq!(
                decode(name, "{}")
                    .expect("empty tool")
                    .proposal()
                    .kind()
                    .as_str(),
                name
            );
        }
        let snapshot = decode(
            "snapshot",
            r#"{"scope":{"kind":"surrounding_text","target":"@a7","before_bytes":40,"after_bytes":60}}"#,
        )
        .expect("snapshot");
        let AgentBrowserToolProposal::Snapshot(AgentBrowserScopeProposal::SurroundingText {
            target,
            window,
        }) = snapshot.proposal()
        else {
            panic!("scope proposal");
        };
        assert_eq!(target.get(), 7);
        assert_eq!(window.before_bytes(), 40);
        assert_eq!(window.after_bytes(), 60);

        let human =
            decode("show_for_human", r#"{"reason":"unsupported_interaction"}"#).expect("human");
        assert!(matches!(
            human.proposal(),
            AgentBrowserToolProposal::ShowForHuman(AgentBrowserHumanReason::UnsupportedInteraction)
        ));
    }

    #[test]
    fn locate_read_extract_and_wait_retain_only_typed_bounded_values() {
        let locate = decode(
            "locate",
            r#"{"semantic_query":"Save changes button","scope":{"kind":"region","target":"@a3"}}"#,
        )
        .expect("locate");
        let AgentBrowserToolProposal::Locate { query, scope } = locate.proposal() else {
            panic!("locate");
        };
        assert_eq!(query.as_str(), "Save changes button");
        assert!(
            matches!(scope, AgentBrowserScopeProposal::Region(reference) if reference.get() == 3)
        );
        assert!(!format!("{locate:?}").contains("Save changes button"));

        let read = decode("read", r#"{"scope":{"kind":"table","target":"@a4"}}"#).expect("read");
        assert!(matches!(
            read.proposal(),
            AgentBrowserToolProposal::Read(AgentBrowserScopeProposal::Table(reference))
                if reference.get() == 4
        ));

        let extract = decode("extract", r#"{"schema_id":29}"#).expect("extract");
        assert!(matches!(
            extract.proposal(),
            AgentBrowserToolProposal::Extract { schema, .. } if schema.get() == 29
        ));

        let wait = decode(
            "wait",
            r#"{"condition":{"kind":"mutation_quiet","millis":250},"timeout_millis":5000}"#,
        )
        .expect("wait");
        assert!(matches!(
            wait.proposal(),
            AgentBrowserToolProposal::Wait {
                condition: AgentBrowserWaitCondition::MutationQuiet(quiet),
                timeout,
            } if quiet.millis() == 250 && timeout.millis() == 5_000
        ));

        let target_wait = decode(
            "wait",
            r#"{"condition":{"kind":"target_state","target":"@a5","state":"expanded","present":true},"timeout_millis":1000}"#,
        )
        .expect("target wait");
        assert!(matches!(
            target_wait.proposal(),
            AgentBrowserToolProposal::Wait {
                condition: AgentBrowserWaitCondition::TargetState {
                    target,
                    state: SemanticState::Expanded,
                    present: true,
                },
                timeout,
            } if target.get() == 5 && timeout.millis() == 1_000
        ));

        let scroll_wait = decode(
            "wait",
            r#"{"condition":{"kind":"scroll_position_changed","target":"@a6"},"timeout_millis":1000}"#,
        )
        .expect("scroll wait");
        assert!(matches!(
            scroll_wait.proposal(),
            AgentBrowserToolProposal::Wait {
                condition: AgentBrowserWaitCondition::ScrollPositionChanged { target },
                ..
            } if target.get() == 6
        ));

        for arguments in [
            r#"{"condition":{"kind":"target_state","state":"expanded","present":true},"timeout_millis":1000}"#,
            r#"{"condition":{"kind":"scroll_position_changed"},"timeout_millis":1000}"#,
            r#"{"condition":{"kind":"target_state","target":"@a01","state":"expanded","present":true},"timeout_millis":1000}"#,
        ] {
            assert!(matches!(
                decode("wait", arguments),
                Err(AgentBrowserToolContractError::Arguments)
                    | Err(AgentBrowserToolContractError::Reference)
            ));
        }
    }

    #[test]
    fn all_semantic_action_variants_decode_through_existing_contracts() {
        let arguments = r#"{"actions":[
          {"kind":"click","target":"@a1","effect":"read","wait":{"kind":"target_state","state":"expanded","present":true},"verification":{"kind":"target_state","state":"expanded","present":true},"settle_millis":1000},
          {"kind":"fill","target":"@a2","value":"ordinary value","effect":"read","wait":{"kind":"semantic_change"},"verification":{"kind":"target_value_matches_input"},"settle_millis":1000},
          {"kind":"select","target":"@a3","option":"@a4","effect":"read","wait":{"kind":"immediate"},"verification":{"kind":"target_selection_matches_option"},"settle_millis":1000},
          {"kind":"press","target":"@a5","key":"enter","effect":"read","wait":{"kind":"navigation_committed"},"verification":{"kind":"navigation_committed"},"settle_millis":1000},
          {"kind":"scroll","target":"@a6","direction":"down","amount":"page","effect":"read","wait":{"kind":"scroll_position_changed"},"verification":{"kind":"scroll_position_changed"},"settle_millis":1000}
        ]}"#;
        let call = decode("act", arguments).expect("act");
        let AgentBrowserToolProposal::Act(batch) = call.proposal() else {
            panic!("act");
        };
        assert_eq!(batch.actions().len(), 5);
        assert_eq!(batch.effect(), SemanticEffectClass::Read);
        assert_eq!(batch.settle_millis(), 5_000);
        assert_eq!(batch.text_bytes(), "ordinary value".len());
    }

    #[test]
    fn malicious_unknown_and_generic_browser_fields_fail_closed() {
        for (name, arguments) in [
            ("eval", r#"{"js":"document.cookie"}"#),
            (
                "navigate",
                r##"{"url":"https://example.test","selector":"#x"}"##,
            ),
            ("back", r#"{"javascript":"alert(1)"}"#),
            (
                "snapshot",
                r#"{"scope":{"kind":"region","target":"@a1","xpath":"//*"}}"#,
            ),
            ("screenshot", r#"{"html":"<script>"}"#),
        ] {
            assert!(matches!(
                decode(name, arguments),
                Err(AgentBrowserToolContractError::ToolName)
                    | Err(AgentBrowserToolContractError::Arguments)
            ));
        }
    }

    #[test]
    fn canonical_references_urls_queries_and_schema_ids_are_enforced() {
        for arguments in [
            r#"{"scope":{"kind":"region","target":"@a0"}}"#,
            r#"{"scope":{"kind":"region","target":"@a01"}}"#,
            r##"{"scope":{"kind":"region","target":"#submit"}}"##,
        ] {
            assert_eq!(
                decode("snapshot", arguments).map(|_| ()),
                Err(AgentBrowserToolContractError::Reference)
            );
        }
        for url in [
            "javascript:alert(1)",
            "file:///tmp/private",
            "data:text/html,x",
        ] {
            assert_eq!(
                decode("navigate", &format!(r#"{{"url":"{url}"}}"#)).map(|_| ()),
                Err(AgentBrowserToolContractError::Navigation)
            );
        }
        let oversized_url = format!(
            "https://example.test/{}",
            "x".repeat(MAX_AGENT_BROWSER_NAVIGATION_URL_BYTES)
        );
        assert_eq!(
            decode("navigate", &format!(r#"{{"url":"{oversized_url}"}}"#)).map(|_| ()),
            Err(AgentBrowserToolContractError::Navigation)
        );
        assert_eq!(
            decode("locate", r#"{"semantic_query":""}"#).map(|_| ()),
            Err(AgentBrowserToolContractError::Query)
        );
        assert_eq!(
            decode("extract", r#"{"schema_id":0}"#).map(|_| ()),
            Err(AgentBrowserToolContractError::ExtractionSchema)
        );
    }

    #[test]
    fn action_secret_outcome_mismatch_and_batch_widening_are_refused() {
        let secret = r#"{"actions":[{"kind":"fill","target":"@a2","value":"sk-secret-secret-secret-secret-value","effect":"local_write","wait":{"kind":"immediate"},"verification":{"kind":"target_value_matches_input"},"settle_millis":1000}]}"#;
        assert_eq!(
            decode("act", secret).map(|_| ()),
            Err(AgentBrowserToolContractError::Action)
        );

        let mismatch = r#"{"actions":[{"kind":"click","target":"@a1","effect":"read","wait":{"kind":"immediate"},"verification":{"kind":"scroll_position_changed"},"settle_millis":1000}]}"#;
        assert_eq!(
            decode("act", mismatch).map(|_| ()),
            Err(AgentBrowserToolContractError::Action)
        );

        let mixed = r#"{"actions":[
          {"kind":"click","target":"@a1","effect":"read","wait":{"kind":"navigation_committed"},"verification":{"kind":"navigation_committed"},"settle_millis":1000},
          {"kind":"click","target":"@a2","effect":"external_write","wait":{"kind":"navigation_committed"},"verification":{"kind":"navigation_committed"},"settle_millis":1000}
        ]}"#;
        assert_eq!(
            decode("act", mixed).map(|_| ()),
            Err(AgentBrowserToolContractError::ActionBatch)
        );
    }

    #[test]
    fn argument_size_depth_and_call_identity_are_bounded_before_use() {
        let deep = format!("{}{{}}{}", "[".repeat(17), "]".repeat(17));
        assert_eq!(
            decode("back", &deep).map(|_| ()),
            Err(AgentBrowserToolContractError::Arguments)
        );
        let oversized = format!(r#"{{"semantic_query":"{}"}}"#, "x".repeat(32 * 1024));
        assert_eq!(
            decode("locate", &oversized).map(|_| ()),
            Err(AgentBrowserToolContractError::ArgumentLimit)
        );
        assert_eq!(
            AgentBrowserToolCall::decode(
                source_call(),
                "bad id".to_owned(),
                "back",
                "{}".to_owned(),
            )
            .map(|_| ()),
            Err(AgentBrowserToolContractError::CallId)
        );
        assert_eq!(
            AgentBrowserToolCall::decode(source_call(), "x".repeat(129), "back", "{}".to_owned(),)
                .map(|_| ()),
            Err(AgentBrowserToolContractError::CallId)
        );
        for provider_item_id in ["".to_owned(), "bad item".to_owned(), "x".repeat(129)] {
            assert_eq!(
                AgentBrowserToolCall::decode_openai(
                    source_call(),
                    provider_item_id,
                    "call_abc-123".to_owned(),
                    "back",
                    "{}".to_owned(),
                )
                .map(|_| ()),
                Err(AgentBrowserToolContractError::CallId)
            );
        }
    }

    #[test]
    fn provider_continuation_correlation_debug_is_content_free() {
        let arguments = r#"{"url":"https://private.example.test/path?token=provider-secret"}"#;
        let call = AgentBrowserToolCall::decode_openai(
            source_call(),
            "fc_private_item".to_owned(),
            "call_private_tool".to_owned(),
            "navigate",
            arguments.to_owned(),
        )
        .expect("OpenAI tool call");
        let call_debug = format!("{call:?}");
        assert!(!call_debug.contains("private.example.test"));
        assert!(!call_debug.contains("call_private_tool"));

        let correlation = call.into_continuation_parts().0;
        assert_eq!(correlation.kind(), AgentBrowserToolKind::Navigate);
        assert_eq!(correlation.argument_bytes(), arguments.len());
        let correlation_debug = format!("{correlation:?}");
        for private in [
            "private.example.test",
            "provider-secret",
            "fc_private_item",
            "call_private_tool",
        ] {
            assert!(!correlation_debug.contains(private));
        }
        assert!(correlation_debug.contains("[redacted]"));
    }
}
