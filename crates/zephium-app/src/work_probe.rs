//! Retention-only public qualification constructor; never linked into release.

#[cfg(not(debug_assertions))]
compile_error!("Work application qualification is forbidden in optimized builds");

use super::*;
use zephium_agent_controller::AgentBrowserRetention;
use zephium_agent_provider_transport::AgentProviderTransport;

impl PreparedAgentWork {
    /// Explicit public-test-data qualifier. Only provider retention differs;
    /// admission, runtime, native, policy, audit and durable closure are shared.
    #[doc(hidden)]
    pub fn try_new_for_public_probe(
        input: AgentWorkRunInput,
        config: AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        task: Box<dyn AgentWorkTask>,
        ports: AgentWorkApplicationPorts,
    ) -> Result<Self, AgentWorkFailure> {
        let transport = AgentProviderTransport::try_new(config.provider)
            .map_err(|_| AgentWorkFailure::Contract)?;
        let (controller, handle) = AgentWorkController::try_new_for_probe(
            input,
            transport,
            credential,
            ports.audit.clone(),
            task,
            AgentBrowserRetention::InspectablePublicData,
        )?;
        Self::from_controller(controller, handle, config.runtime, ports)
    }
}
