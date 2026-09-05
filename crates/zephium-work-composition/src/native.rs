//! The only production adapter from actual desktop owners to Work admission.

use std::sync::Arc;
use zephium_agent_controller::{AgentWorkFailure, AgentWorkRunInput, AgentWorkTask};
use zephium_agent_provider_transport::AgentProviderCredential;
use zephium_app::{
    AgentWorkApplicationConfig, AgentWorkApplicationHandle, AgentWorkApplicationPorts,
    CallbackHandle, PreparedAgentWork,
};
use zephium_engine::WebviewEngine;
use zephium_store::SqliteStore;

/// Product-owned input. No objective parser or model text may mint the task,
/// effect assessment, account attestation, manifest or profile assignment.
/// This layer intentionally has no default task or implicit effect permission.
pub struct TrustedWorkRequest {
    pub(crate) input: AgentWorkRunInput,
    pub(crate) config: AgentWorkApplicationConfig,
    pub(crate) credential: AgentProviderCredential,
    pub(crate) task: Box<dyn AgentWorkTask>,
}

impl TrustedWorkRequest {
    /// Transfers the approved functional-core input and trusted task authority.
    pub fn new(
        input: AgentWorkRunInput,
        config: AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        task: Box<dyn AgentWorkTask>,
    ) -> Self {
        Self {
            input,
            config,
            credential,
            task,
        }
    }
}

impl std::fmt::Debug for TrustedWorkRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrustedWorkRequest([owned, redacted])")
    }
}

/// Dormant, move-only desktop owners. Construction neither claims persistence
/// nor takes native authority. The shell checks both exact Arc identities.
pub struct MacosWorkComposition {
    engine: Arc<WebviewEngine>,
    store: Arc<SqliteStore>,
}

impl MacosWorkComposition {
    /// Uses the same owners passed to the normal application shell.
    pub fn new(engine: Arc<WebviewEngine>, store: Arc<SqliteStore>) -> Self {
        Self { engine, store }
    }

    /// Claims content-free recovery inventory only. No runtime/native page starts.
    pub fn attach(&self, shell: &CallbackHandle) -> Option<AgentWorkApplicationHandle> {
        shell.attach_work(self.store.clone(), self.engine.clone())
    }

    /// Validates without consuming this composition or attaching the journal.
    /// The returned factory still acquires the sealed port only after both
    /// durable admission acknowledgements. Clones retain exact owner identity.
    pub fn prepare(
        &self,
        request: TrustedWorkRequest,
    ) -> Result<PreparedAgentWork, AgentWorkFailure> {
        PreparedAgentWork::try_new(
            request.input,
            request.config,
            request.credential,
            request.task,
            self.ports(),
        )
    }

    pub(crate) fn ports(&self) -> AgentWorkApplicationPorts {
        let engine = self.engine.clone();
        AgentWorkApplicationPorts::new(
            engine.clone(),
            self.store.clone(),
            Box::new(move |sink| {
                engine.take_agent_browser_port(move |event| {
                    let _ = sink.publish(event);
                })
            }),
        )
    }
}
