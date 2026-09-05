//! Public-data retention only. Native and application composition stay identical.
use crate::{MacosWorkComposition, TrustedWorkRequest};
use zephium_agent_controller::AgentWorkFailure;
use zephium_app::PreparedAgentWork;

impl MacosWorkComposition {
    /// Explicit excluded public qualifier; never accepts private/BYOK task data.
    #[doc(hidden)]
    pub fn prepare_public_qualification(
        &self,
        request: TrustedWorkRequest,
    ) -> Result<PreparedAgentWork, AgentWorkFailure> {
        PreparedAgentWork::try_new_for_public_probe(
            request.input,
            request.config,
            request.credential,
            request.task,
            self.ports(),
        )
    }
}
