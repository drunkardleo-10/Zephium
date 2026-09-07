//! Explicit public-only diagnostic preparation through the original product join.
use super::*;

impl PreparedRetainedWork {
    #[doc(hidden)]
    pub fn try_new_for_public_qualification(
        input: AgentWorkRunInput,
        profile: AgentWorkProfileBinding,
        config: crate::AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        task: Box<dyn AgentWorkTask>,
        ports: RetainedWorkPorts,
    ) -> Result<Self, AgentWorkFailure> {
        let (runtime, provider) = config.into_parts();
        let spec = input.retained_resource_spec()?;
        let actor = ActorRequest {
            run: spec.identity.owner(),
            deadline: spec.expires_at,
            prepare: Box::new(move |browser, audit| {
                StagedActor::for_public_qualification(
                    input, browser, runtime, provider, credential, audit, task,
                )
            }),
        };
        Self::from_actor(
            spec,
            actor,
            profile,
            ports.engine,
            ports.journal,
            ports.audit,
            ports.native,
        )
    }
}
