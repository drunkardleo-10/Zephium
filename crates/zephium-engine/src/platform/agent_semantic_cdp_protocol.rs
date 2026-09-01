//! Closed WebView2 CDP vocabulary for the production semantic runtime.
//!
//! WebView2 does not expose a supported native isolated-world script API. The
//! Windows adapter therefore needs a very small internal CDP mechanism, but
//! neither callers nor page content may choose a method, script, selector, or
//! execution context. This module owns the complete protocol vocabulary and
//! parses every browser-controlled response under a pre-allocation native
//! ceiling supplied by the platform adapter.
//!
//! Chromium's document-start `worldName` implementation grants universal
//! access when it creates an inspector world. Production deliberately does not
//! use that command. It first creates one root-frame world with
//! `grantUniveralAccess: false`, joins its system-unique context identity, then
//! lazily installs the immutable program through one fixed function call.

use std::fmt;

use serde_json::{json, Value};
#[cfg(test)]
use zephium_agentic::SEMANTIC_RUNTIME_GLOBAL_NAME;
use zephium_agentic::{
    SemanticRuntimeInvocation, SemanticRuntimeResultError, SemanticSnapshot,
    MAX_SEMANTIC_RUNTIME_REQUEST_BYTES, MAX_SEMANTIC_RUNTIME_SOURCE_BYTES, MAX_SEMANTIC_WIRE_BYTES,
    SEMANTIC_RUNTIME_PROGRAM,
};

const WORLD_NAME_PREFIX: &str = "zephium-semantic-runtime-v1-";
const MAX_WORLD_NAME_BYTES: usize = 96;
const MAX_BROWSER_IDENTIFIER_BYTES: usize = 256;
const MAX_CONTROL_PARAMETERS_BYTES: usize = 128 * 1_024;
pub(crate) const MAX_CONTROL_RESPONSE_BYTES: usize = 512 * 1_024;
pub(crate) const MAX_CONTEXT_EVENT_BYTES: usize = 32 * 1_024;
pub(crate) const MAX_CONTEXT_EVENTS_PER_INVOCATION: u16 = 512;
pub(crate) const MAX_CONTEXT_EVENT_BYTES_PER_INVOCATION: usize = 512 * 1_024;
pub(crate) const MAX_INVOCATION_RESPONSE_BYTES: usize = 2 * MAX_SEMANTIC_WIRE_BYTES + 16 * 1_024;
const MAX_INVOCATION_PARAMETERS_BYTES: usize = 8 * 1_024;
const INSTALL_RESPONSE_MARKER: &str = "I1";
const INSTALL_FUNCTION_PREFIX: &str = "function(){";
const INSTALL_FUNCTION_SUFFIX: &str = concat!(
    ";const descriptor=Object.getOwnPropertyDescriptor(globalThis,\"",
    "__zephiumSemanticRuntimeV1",
    "\");const api=descriptor&&descriptor.value;return descriptor&&",
    "descriptor.writable===false&&descriptor.configurable===false&&",
    "descriptor.enumerable===false&&Object.isFrozen(api)&&",
    "typeof api.invoke===\"function\"&&Object.isFrozen(api.invoke)?\"I1\":\"F1\";}"
);

const INVOKE_FUNCTION: &str =
    "function(encoded){return globalThis.__zephiumSemanticRuntimeV1.invoke(encoded);}";

/// The complete production CDP method vocabulary. No stringly-typed method
/// crosses the adapter boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FixedSemanticCdpMethod {
    GetFrameTree,
    RuntimeEnable,
    CreateIsolatedWorld,
    RuntimeDisable,
    CallFunctionOn,
}

impl FixedSemanticCdpMethod {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::GetFrameTree => "Page.getFrameTree",
            Self::RuntimeEnable => "Runtime.enable",
            Self::CreateIsolatedWorld => "Page.createIsolatedWorld",
            Self::RuntimeDisable => "Runtime.disable",
            Self::CallFunctionOn => "Runtime.callFunctionOn",
        }
    }
}

/// One fixed native command. Its parameters can contain the immutable runtime
/// or private invocation payload and are therefore always redacted.
pub(crate) struct FixedSemanticCdpCommand {
    method: FixedSemanticCdpMethod,
    parameters: String,
    response_limit: usize,
}

impl FixedSemanticCdpCommand {
    pub(crate) const fn method(&self) -> FixedSemanticCdpMethod {
        self.method
    }

    pub(crate) fn parameters(&self) -> &str {
        &self.parameters
    }

    pub(crate) const fn response_limit(&self) -> usize {
        self.response_limit
    }
}

impl fmt::Debug for FixedSemanticCdpCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FixedSemanticCdpCommand")
            .field("method", &self.method)
            .field("parameters", &"[redacted]")
            .field("parameter_bytes", &self.parameters.len())
            .field("response_limit", &self.response_limit)
            .finish()
    }
}

/// Process-local, non-reusable namespace for one owned WebView2 controller.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct SemanticWorldName(String);

impl SemanticWorldName {
    pub(crate) fn from_nonce(
        epoch: u64,
        unpredictable: u128,
    ) -> Result<Self, SemanticCdpProtocolError> {
        if epoch == 0 || unpredictable == 0 {
            return Err(SemanticCdpProtocolError::InvalidAuthority);
        }
        let value = format!("{WORLD_NAME_PREFIX}{epoch:016x}-{unpredictable:032x}");
        if value.len() > MAX_WORLD_NAME_BYTES {
            return Err(SemanticCdpProtocolError::Limit);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SemanticWorldName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticWorldName([redacted])")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct SemanticRootFrameId(String);

impl SemanticRootFrameId {
    fn parse(value: &str) -> Result<Self, SemanticCdpProtocolError> {
        validate_browser_identifier(value)?;
        Ok(Self(value.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SemanticRootFrameId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SemanticRootFrameId([redacted])")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct SemanticExecutionContext {
    id: i64,
    unique_id: String,
}

impl SemanticExecutionContext {
    fn new(id: i64, unique_id: &str) -> Result<Self, SemanticCdpProtocolError> {
        if id <= 0 {
            return Err(SemanticCdpProtocolError::InvalidResponse);
        }
        validate_browser_identifier(unique_id)?;
        Ok(Self {
            id,
            unique_id: unique_id.to_owned(),
        })
    }

    fn unique_id(&self) -> &str {
        &self.unique_id
    }
}

impl fmt::Debug for SemanticExecutionContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticExecutionContext")
            .field("id", &"[redacted]")
            .field("unique_id", &"[redacted]")
            .finish()
    }
}

/// Bounded discovery state for one invocation. Runtime reporting is enabled
/// only while this value exists; unrelated contexts are counted then dropped.
pub(crate) struct SemanticContextDiscovery {
    world: SemanticWorldName,
    root_frame: SemanticRootFrameId,
    created_context_id: Option<i64>,
    observed_context: Option<SemanticExecutionContext>,
    observed_events: u16,
    observed_bytes: usize,
}

impl SemanticContextDiscovery {
    pub(crate) const fn new(world: SemanticWorldName, root_frame: SemanticRootFrameId) -> Self {
        Self {
            world,
            root_frame,
            created_context_id: None,
            observed_context: None,
            observed_events: 0,
            observed_bytes: 0,
        }
    }

    pub(crate) fn observe_context_created(
        &mut self,
        event_json: &str,
    ) -> Result<(), SemanticCdpProtocolError> {
        if event_json.len() > MAX_CONTEXT_EVENT_BYTES {
            return Err(SemanticCdpProtocolError::Limit);
        }
        self.observed_events = self
            .observed_events
            .checked_add(1)
            .filter(|count| *count <= MAX_CONTEXT_EVENTS_PER_INVOCATION)
            .ok_or(SemanticCdpProtocolError::Limit)?;
        self.observed_bytes = self
            .observed_bytes
            .checked_add(event_json.len())
            .filter(|bytes| *bytes <= MAX_CONTEXT_EVENT_BYTES_PER_INVOCATION)
            .ok_or(SemanticCdpProtocolError::Limit)?;

        let event = parse_object(event_json, MAX_CONTEXT_EVENT_BYTES)?;
        let context = event
            .get("context")
            .and_then(Value::as_object)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        let name = context
            .get("name")
            .and_then(Value::as_str)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        if name != self.world.as_str() {
            return Ok(());
        }
        let auxiliary = context
            .get("auxData")
            .and_then(Value::as_object)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        let frame = auxiliary
            .get("frameId")
            .and_then(Value::as_str)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        if frame != self.root_frame.as_str() {
            return Ok(());
        }
        if auxiliary.get("isDefault").and_then(Value::as_bool) != Some(false)
            || auxiliary.get("type").and_then(Value::as_str) != Some("isolated")
        {
            return Err(SemanticCdpProtocolError::IsolationUnproven);
        }
        let id = context
            .get("id")
            .and_then(Value::as_i64)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        let unique_id = context
            .get("uniqueId")
            .and_then(Value::as_str)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        let candidate = SemanticExecutionContext::new(id, unique_id)?;
        match self.observed_context.as_ref() {
            None => self.observed_context = Some(candidate),
            Some(current) if current == &candidate => {}
            Some(_) => return Err(SemanticCdpProtocolError::InvalidResponse),
        }
        Ok(())
    }

    pub(crate) fn record_created_context(
        &mut self,
        response: &str,
    ) -> Result<(), SemanticCdpProtocolError> {
        let object = parse_success_object(response, MAX_CONTROL_RESPONSE_BYTES)?;
        let id = object
            .get("executionContextId")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
        match self.created_context_id {
            None => self.created_context_id = Some(id),
            Some(current) if current == id => {}
            Some(_) => return Err(SemanticCdpProtocolError::InvalidResponse),
        }
        Ok(())
    }

    pub(crate) fn resolved(
        &self,
    ) -> Result<Option<SemanticExecutionContext>, SemanticCdpProtocolError> {
        match (self.created_context_id, self.observed_context.as_ref()) {
            (Some(created), Some(observed)) if created == observed.id => Ok(Some(observed.clone())),
            (Some(_), Some(_)) => Err(SemanticCdpProtocolError::InvalidResponse),
            (None, _) | (_, None) => Ok(None),
        }
    }

    pub(crate) fn create_world_command(
        &self,
    ) -> Result<FixedSemanticCdpCommand, SemanticCdpProtocolError> {
        create_isolated_world_command(&self.world, &self.root_frame)
    }
}

impl fmt::Debug for SemanticContextDiscovery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticContextDiscovery")
            .field("world", &"[redacted]")
            .field("root_frame", &"[redacted]")
            .field("created_context", &self.created_context_id.is_some())
            .field("observed_context", &self.observed_context.is_some())
            .field("observed_events", &self.observed_events)
            .field("observed_bytes", &self.observed_bytes)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticCdpProtocolError {
    InvalidAuthority,
    InvalidResponse,
    IsolationUnproven,
    Limit,
}

#[derive(Debug)]
pub(crate) enum SemanticCdpInvocationError {
    Protocol(SemanticCdpProtocolError),
    Result(SemanticRuntimeResultError),
}

pub(crate) fn install_runtime_in_context_command(
    context: &SemanticExecutionContext,
) -> Result<FixedSemanticCdpCommand, SemanticCdpProtocolError> {
    let source = SEMANTIC_RUNTIME_PROGRAM.source();
    if source.len() > MAX_SEMANTIC_RUNTIME_SOURCE_BYTES || !source.is_ascii() {
        return Err(SemanticCdpProtocolError::Limit);
    }
    let capacity = INSTALL_FUNCTION_PREFIX
        .len()
        .checked_add(source.len())
        .and_then(|length| length.checked_add(INSTALL_FUNCTION_SUFFIX.len()))
        .ok_or(SemanticCdpProtocolError::Limit)?;
    let mut declaration = String::new();
    declaration
        .try_reserve_exact(capacity)
        .map_err(|_| SemanticCdpProtocolError::Limit)?;
    declaration.push_str(INSTALL_FUNCTION_PREFIX);
    declaration.push_str(source);
    declaration.push_str(INSTALL_FUNCTION_SUFFIX);
    if declaration.len() != capacity {
        return Err(SemanticCdpProtocolError::Limit);
    }
    command(
        FixedSemanticCdpMethod::CallFunctionOn,
        json!({
            "functionDeclaration": declaration,
            "silent": true,
            "returnByValue": true,
            "generatePreview": false,
            "userGesture": false,
            "awaitPromise": false,
            "uniqueContextId": context.unique_id(),
        }),
        MAX_CONTROL_PARAMETERS_BYTES,
        MAX_CONTROL_RESPONSE_BYTES,
    )
}

pub(crate) fn decode_runtime_install_response(
    response: &str,
) -> Result<(), SemanticCdpProtocolError> {
    let object = parse_success_object(response, MAX_CONTROL_RESPONSE_BYTES)?;
    let result = object
        .get("result")
        .and_then(Value::as_object)
        .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
    if result.get("type").and_then(Value::as_str) == Some("string")
        && result.get("value").and_then(Value::as_str) == Some(INSTALL_RESPONSE_MARKER)
        && result.get("subtype").is_none()
    {
        Ok(())
    } else {
        Err(SemanticCdpProtocolError::InvalidResponse)
    }
}

pub(crate) fn get_frame_tree_command() -> FixedSemanticCdpCommand {
    command(
        FixedSemanticCdpMethod::GetFrameTree,
        json!({}),
        MAX_CONTROL_PARAMETERS_BYTES,
        MAX_CONTROL_RESPONSE_BYTES,
    )
    .expect("fixed empty CDP parameters fit their compile-time ceiling")
}

pub(crate) fn decode_root_frame(
    response: &str,
) -> Result<SemanticRootFrameId, SemanticCdpProtocolError> {
    let object = parse_success_object(response, MAX_CONTROL_RESPONSE_BYTES)?;
    let identifier = object
        .get("frameTree")
        .and_then(Value::as_object)
        .and_then(|tree| tree.get("frame"))
        .and_then(Value::as_object)
        .and_then(|frame| frame.get("id"))
        .and_then(Value::as_str)
        .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
    SemanticRootFrameId::parse(identifier)
}

pub(crate) fn runtime_enable_command() -> FixedSemanticCdpCommand {
    command(
        FixedSemanticCdpMethod::RuntimeEnable,
        json!({}),
        MAX_CONTROL_PARAMETERS_BYTES,
        MAX_CONTROL_RESPONSE_BYTES,
    )
    .expect("fixed empty CDP parameters fit their compile-time ceiling")
}

pub(crate) fn runtime_disable_command() -> FixedSemanticCdpCommand {
    command(
        FixedSemanticCdpMethod::RuntimeDisable,
        json!({}),
        MAX_CONTROL_PARAMETERS_BYTES,
        MAX_CONTROL_RESPONSE_BYTES,
    )
    .expect("fixed empty CDP parameters fit their compile-time ceiling")
}

pub(crate) fn decode_empty_success(response: &str) -> Result<(), SemanticCdpProtocolError> {
    let object = parse_success_object(response, MAX_CONTROL_RESPONSE_BYTES)?;
    object
        .is_empty()
        .then_some(())
        .ok_or(SemanticCdpProtocolError::InvalidResponse)
}

fn create_isolated_world_command(
    world: &SemanticWorldName,
    root_frame: &SemanticRootFrameId,
) -> Result<FixedSemanticCdpCommand, SemanticCdpProtocolError> {
    command(
        FixedSemanticCdpMethod::CreateIsolatedWorld,
        json!({
            "frameId": root_frame.as_str(),
            "worldName": world.as_str(),
            "grantUniveralAccess": false,
        }),
        MAX_CONTROL_PARAMETERS_BYTES,
        MAX_CONTROL_RESPONSE_BYTES,
    )
}

pub(crate) fn invoke_runtime_command(
    context: &SemanticExecutionContext,
    invocation: &SemanticRuntimeInvocation,
) -> Result<FixedSemanticCdpCommand, SemanticCdpProtocolError> {
    if invocation.as_str().is_empty()
        || invocation.as_str().len() > MAX_SEMANTIC_RUNTIME_REQUEST_BYTES
        || !invocation.as_str().is_ascii()
    {
        return Err(SemanticCdpProtocolError::InvalidAuthority);
    }
    command(
        FixedSemanticCdpMethod::CallFunctionOn,
        json!({
            "functionDeclaration": INVOKE_FUNCTION,
            "arguments": [{ "value": invocation.as_str() }],
            "silent": true,
            "returnByValue": true,
            "generatePreview": false,
            "userGesture": false,
            "awaitPromise": false,
            "uniqueContextId": context.unique_id(),
        }),
        MAX_INVOCATION_PARAMETERS_BYTES,
        MAX_INVOCATION_RESPONSE_BYTES,
    )
}

pub(crate) fn decode_invocation_response(
    invocation: &SemanticRuntimeInvocation,
    response: &str,
) -> Result<SemanticSnapshot, SemanticCdpInvocationError> {
    let object = parse_success_object(response, MAX_INVOCATION_RESPONSE_BYTES)
        .map_err(SemanticCdpInvocationError::Protocol)?;
    let result = object.get("result").and_then(Value::as_object).ok_or(
        SemanticCdpInvocationError::Protocol(SemanticCdpProtocolError::InvalidResponse),
    )?;
    if result.get("type").and_then(Value::as_str) != Some("string")
        || result.get("subtype").is_some()
    {
        return Err(SemanticCdpInvocationError::Protocol(
            SemanticCdpProtocolError::InvalidResponse,
        ));
    }
    let value =
        result
            .get("value")
            .and_then(Value::as_str)
            .ok_or(SemanticCdpInvocationError::Protocol(
                SemanticCdpProtocolError::InvalidResponse,
            ))?;
    if value.len() > invocation.budget().max_wire_bytes() as usize {
        return Err(SemanticCdpInvocationError::Result(
            SemanticRuntimeResultError::OutputLimit,
        ));
    }
    invocation
        .decode_result(value.as_bytes())
        .map_err(SemanticCdpInvocationError::Result)
}

fn command(
    method: FixedSemanticCdpMethod,
    parameters: Value,
    parameter_limit: usize,
    response_limit: usize,
) -> Result<FixedSemanticCdpCommand, SemanticCdpProtocolError> {
    let parameters = parameters.to_string();
    if parameters.len() > parameter_limit {
        return Err(SemanticCdpProtocolError::Limit);
    }
    Ok(FixedSemanticCdpCommand {
        method,
        parameters,
        response_limit,
    })
}

fn parse_object(response: &str, limit: usize) -> Result<Value, SemanticCdpProtocolError> {
    if response.is_empty() || response.len() > limit {
        return Err(SemanticCdpProtocolError::Limit);
    }
    let value: Value =
        serde_json::from_str(response).map_err(|_| SemanticCdpProtocolError::InvalidResponse)?;
    value
        .is_object()
        .then_some(value)
        .ok_or(SemanticCdpProtocolError::InvalidResponse)
}

fn parse_success_object(
    response: &str,
    limit: usize,
) -> Result<serde_json::Map<String, Value>, SemanticCdpProtocolError> {
    let value = parse_object(response, limit)?;
    let object = value
        .as_object()
        .ok_or(SemanticCdpProtocolError::InvalidResponse)?;
    if object.contains_key("error") || object.contains_key("exceptionDetails") {
        return Err(SemanticCdpProtocolError::InvalidResponse);
    }
    value
        .as_object()
        .cloned()
        .ok_or(SemanticCdpProtocolError::InvalidResponse)
}

fn validate_browser_identifier(value: &str) -> Result<(), SemanticCdpProtocolError> {
    if value.is_empty()
        || value.len() > MAX_BROWSER_IDENTIFIER_BYTES
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(SemanticCdpProtocolError::InvalidResponse);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_agentic::{
        encode_semantic_runtime_invocation, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameGeneration, FrameId, SemanticFrameJoin, SemanticFrameTrust,
        SemanticInvocationId, SemanticObservationBudget, SemanticObservationId,
        SemanticObservationRequest, SemanticOrigin, SemanticRuntimeBudget,
        SemanticSnapshotGeneration, SEMANTIC_WIRE_VERSION,
    };
    use zephium_core::ids::ProfileId;

    fn invocation() -> SemanticRuntimeInvocation {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(43),
            ContextKind::Owned,
        );
        let capabilities =
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settle");
        let context = registry.join(identity.id()).expect("join");
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            FrameGeneration::INITIAL,
            SemanticOrigin::parse("https://semantic.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::try_new(128, 16 * 1024, 1).expect("budget"),
        );
        encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(7).expect("invocation"),
            SemanticSnapshotGeneration::new(9).expect("generation"),
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("encode")
    }

    #[test]
    fn fixed_vocabulary_installs_only_the_pinned_program_in_the_joined_world() {
        let context = SemanticExecutionContext::new(19, "unique-context-19").expect("context");
        let command = install_runtime_in_context_command(&context).expect("command");
        assert_eq!(command.method(), FixedSemanticCdpMethod::CallFunctionOn);
        let parameters: Value = serde_json::from_str(command.parameters()).expect("parameters");
        let declaration = parameters["functionDeclaration"]
            .as_str()
            .expect("fixed declaration");
        assert!(declaration.starts_with(INSTALL_FUNCTION_PREFIX));
        assert!(declaration.ends_with(INSTALL_FUNCTION_SUFFIX));
        assert_eq!(
            declaration
                .matches(SEMANTIC_RUNTIME_PROGRAM.source())
                .count(),
            1
        );
        assert!(INSTALL_FUNCTION_SUFFIX.contains(SEMANTIC_RUNTIME_GLOBAL_NAME));
        assert_eq!(parameters["uniqueContextId"], "unique-context-19");
        assert_eq!(parameters["returnByValue"], true);
        assert_eq!(parameters["generatePreview"], false);
        assert_eq!(parameters["userGesture"], false);
        assert_eq!(parameters["awaitPromise"], false);
        assert_eq!(parameters["silent"], true);
        assert!(parameters.get("arguments").is_none());
        assert!(parameters.get("executionContextId").is_none());
        assert_eq!(parameters.as_object().map(|object| object.len()), Some(7));
        assert!(format!("{command:?}").contains("[redacted]"));
        assert!(!format!("{command:?}").contains(SEMANTIC_RUNTIME_GLOBAL_NAME));
    }

    #[test]
    fn invocation_is_fixed_unique_context_non_gesture_call_by_value() {
        let context = SemanticExecutionContext::new(19, "unique-context-19").expect("context");
        let invocation = invocation();
        let command = invoke_runtime_command(&context, &invocation).expect("command");
        assert_eq!(command.method(), FixedSemanticCdpMethod::CallFunctionOn);
        assert_eq!(command.response_limit(), MAX_INVOCATION_RESPONSE_BYTES);
        let parameters: Value = serde_json::from_str(command.parameters()).expect("parameters");
        assert_eq!(parameters["functionDeclaration"], INVOKE_FUNCTION);
        assert_eq!(parameters["arguments"][0]["value"], invocation.as_str());
        assert_eq!(parameters["uniqueContextId"], "unique-context-19");
        assert_eq!(parameters["returnByValue"], true);
        assert_eq!(parameters["generatePreview"], false);
        assert_eq!(parameters["userGesture"], false);
        assert_eq!(parameters["awaitPromise"], false);
        assert_eq!(parameters["silent"], true);
        assert!(parameters.get("executionContextId").is_none());
        assert!(parameters.get("objectId").is_none());
        assert_eq!(parameters.as_object().map(|object| object.len()), Some(8));
        assert!(!format!("{command:?}").contains(invocation.as_str()));
    }

    #[test]
    fn context_discovery_requires_matching_isolated_unique_context() {
        let world = SemanticWorldName::from_nonce(12, 0x12).expect("world");
        let root = SemanticRootFrameId::parse("root-frame").expect("frame");
        let mut discovery = SemanticContextDiscovery::new(world.clone(), root);
        discovery
            .observe_context_created(
                &json!({
                    "context": {
                        "id": 31,
                        "uniqueId": "unique-main",
                        "name": "other-world",
                        "auxData": {
                            "frameId": "root-frame",
                            "isDefault": false,
                            "type": "isolated"
                        }
                    }
                })
                .to_string(),
            )
            .expect("ignore unrelated");
        assert!(discovery.resolved().expect("resolve").is_none());
        discovery
            .observe_context_created(
                &json!({
                    "context": {
                        "id": 32,
                        "uniqueId": "unique-main",
                        "name": world.as_str(),
                        "auxData": {
                            "frameId": "root-frame",
                            "isDefault": false,
                            "type": "isolated"
                        }
                    }
                })
                .to_string(),
            )
            .expect("observe");
        assert!(discovery.resolved().expect("resolve").is_none());
        discovery
            .record_created_context(r#"{"executionContextId":32}"#)
            .expect("created");
        assert_eq!(
            discovery.resolved().expect("resolve"),
            Some(SemanticExecutionContext::new(32, "unique-main").expect("context"))
        );

        let create = discovery.create_world_command().expect("create");
        let parameters: Value = serde_json::from_str(create.parameters()).expect("parameters");
        assert_eq!(parameters["grantUniveralAccess"], false);
        assert_eq!(parameters["frameId"], "root-frame");
        assert_eq!(parameters["worldName"], world.as_str());
    }

    #[test]
    fn context_discovery_rejects_default_spoof_and_identity_races() {
        let world = SemanticWorldName::from_nonce(3, 0x34).expect("world");
        let root = SemanticRootFrameId::parse("root").expect("frame");
        let mut discovery = SemanticContextDiscovery::new(world.clone(), root.clone());
        let default_world = json!({
            "context": {
                "id": 1,
                "uniqueId": "page",
                "name": world.as_str(),
                "auxData": {"frameId": "root", "isDefault": true, "type": "default"}
            }
        })
        .to_string();
        assert_eq!(
            discovery.observe_context_created(&default_world),
            Err(SemanticCdpProtocolError::IsolationUnproven)
        );

        let mut discovery = SemanticContextDiscovery::new(world.clone(), root);
        let isolated = |id, unique: &str| {
            json!({
                "context": {
                    "id": id,
                    "uniqueId": unique,
                    "name": world.as_str(),
                    "auxData": {"frameId": "root", "isDefault": false, "type": "isolated"}
                }
            })
            .to_string()
        };
        discovery
            .observe_context_created(&isolated(7, "first"))
            .expect("first");
        assert_eq!(
            discovery.observe_context_created(&isolated(8, "second")),
            Err(SemanticCdpProtocolError::InvalidResponse)
        );

        let mut discovery = SemanticContextDiscovery::new(
            world.clone(),
            SemanticRootFrameId::parse("root").expect("frame"),
        );
        discovery
            .observe_context_created(&isolated(7, "first"))
            .expect("first");
        discovery
            .record_created_context(r#"{"executionContextId":8}"#)
            .expect("created");
        assert_eq!(
            discovery.resolved(),
            Err(SemanticCdpProtocolError::InvalidResponse)
        );
    }

    #[test]
    fn responses_are_bounded_and_closed() {
        assert_eq!(
            get_frame_tree_command().method().as_str(),
            "Page.getFrameTree"
        );
        assert_eq!(runtime_enable_command().method().as_str(), "Runtime.enable");
        assert_eq!(
            runtime_disable_command().method().as_str(),
            "Runtime.disable"
        );
        assert_eq!(
            decode_runtime_install_response(r#"{"result":{"type":"string","value":"I1"}}"#),
            Ok(())
        );
        assert_eq!(
            decode_runtime_install_response(r#"{"result":{"type":"string","value":"F1"}}"#),
            Err(SemanticCdpProtocolError::InvalidResponse)
        );
        assert_eq!(
            decode_root_frame(r#"{"frameTree":{"frame":{"id":"root"},"childFrames":[]}}"#),
            Ok(SemanticRootFrameId("root".to_owned()))
        );
        assert_eq!(decode_empty_success("{}"), Ok(()));
        assert_eq!(
            decode_empty_success(r#"{"error":{"message":"page supplied"}}"#),
            Err(SemanticCdpProtocolError::InvalidResponse)
        );
        assert_eq!(
            decode_empty_success(r#"{"exceptionDetails":{"text":"private"}}"#),
            Err(SemanticCdpProtocolError::InvalidResponse)
        );
        assert_eq!(
            decode_root_frame(&"x".repeat(MAX_CONTROL_RESPONSE_BYTES + 1)),
            Err(SemanticCdpProtocolError::Limit)
        );
        assert_eq!(
            SemanticWorldName::from_nonce(0, 1),
            Err(SemanticCdpProtocolError::InvalidAuthority)
        );
        assert_eq!(
            SemanticWorldName::from_nonce(1, 0),
            Err(SemanticCdpProtocolError::InvalidAuthority)
        );
    }

    #[test]
    fn invocation_response_decodes_only_the_hostile_wire_value() {
        let invocation = invocation();
        let wire = json!({
            "v": SEMANTIC_WIRE_VERSION,
            "i": invocation.invocation().get(),
            "g": invocation.snapshot_generation().get(),
            "c": "complete",
            "n": [{"k": 1, "r": "document"}]
        })
        .to_string();
        let response = json!({"result": {"type": "string", "value": wire}}).to_string();
        let snapshot = decode_invocation_response(&invocation, &response).expect("snapshot");
        assert_eq!(snapshot.invocation(), invocation.invocation());
        assert_eq!(snapshot.generation(), invocation.snapshot_generation());

        let exception = json!({
            "result": {"type": "string", "value": "E1:internal"},
            "exceptionDetails": {"text": "private page detail"}
        })
        .to_string();
        assert!(matches!(
            decode_invocation_response(&invocation, &exception),
            Err(SemanticCdpInvocationError::Protocol(
                SemanticCdpProtocolError::InvalidResponse
            ))
        ));
        assert!(matches!(
            decode_invocation_response(&invocation, r#"{"result":{"type":"object","value":{}}}"#),
            Err(SemanticCdpInvocationError::Protocol(
                SemanticCdpProtocolError::InvalidResponse
            ))
        ));
        let fixed_fault = json!({
            "result": {"type": "string", "value": "E1:internal"}
        })
        .to_string();
        match decode_invocation_response(&invocation, &fixed_fault) {
            Err(SemanticCdpInvocationError::Result(error)) => {
                assert!(matches!(error, SemanticRuntimeResultError::Runtime(_)));
            }
            outcome => panic!("unexpected fixed-fault outcome: {outcome:?}"),
        }
    }

    #[test]
    fn event_volume_and_debug_output_fail_closed_without_content() {
        let world = SemanticWorldName::from_nonce(99, 0x56).expect("world");
        let root = SemanticRootFrameId::parse("private-root-identifier").expect("frame");
        let mut discovery = SemanticContextDiscovery::new(world, root);
        let unrelated = r#"{"context":{"id":1,"uniqueId":"private-page-identifier","name":"","auxData":{"frameId":"private-root-identifier","isDefault":true,"type":"default"}}}"#;
        for _ in 0..MAX_CONTEXT_EVENTS_PER_INVOCATION {
            discovery
                .observe_context_created(unrelated)
                .expect("within event bound");
        }
        assert_eq!(
            discovery.observe_context_created(unrelated),
            Err(SemanticCdpProtocolError::Limit)
        );
        let debug = format!("{discovery:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("private-root-identifier"));
        assert!(!debug.contains("private-page-identifier"));
    }
}
