//! Product admission for one objective and bounded observed-link exploration.

use std::{sync::Arc, time::Instant};
use zephium_agent_controller::{
    AgentBrowserModel, AgentWorkAccountSource, AgentWorkContextSpec, AgentWorkDiscoveryTask,
    AgentWorkFailure, AgentWorkLocalActionPolicy, AgentWorkRunInput, AgentWorkRunSettings,
    AgentWorkTask, TerraControllerIds,
};
use zephium_agent_provider_transport::AgentProviderCredential;
use zephium_agentic::*;
use zephium_app::{AgentWorkApplicationConfig, AgentWorkProfileBinding};

use crate::{
    native_work_clock::{authority_window, NativeWorkClock},
    TrustedWorkRequest,
};

/// Product-approved intent, exploration scope and result shape. The model
/// chooses its route from observed links; no destination list or answer oracle
/// participates in admission. This API permits Public semantic disclosure only,
/// including when the selected service account is authenticated. Authentication
/// never declassifies personal, confidential or secret page content.
pub struct PublicReadWorkObjective {
    pub objective: String,
    /// Contains the explicit starting target, same-origin path scope and hop cap.
    pub navigation: AgentNavigationDiscovery,
    pub output_fields: Vec<SemanticExtractionFieldSchema>,
}

/// Independently approved account scope. A browser profile selects storage and
/// cookies; it does not identify a service account.
pub enum PublicReadWorkAccount {
    /// The caller has established anonymous use of the selected service scope.
    Anonymous,
    /// Explicit actor enrollment paired with an independent host collector.
    /// Admission checks the enrolled profile and service origin as well as the
    /// account; enrollment alone supplies no current-document evidence.
    Enrolled(Box<zephium_app::AgentWorkEnrolledAccount>),
    /// A host source independently identifies the account for each current
    /// document. Missing, changed or stale samples prevent execution. The ID
    /// must name that identified account, never a generated stand-in inferred
    /// from profile selection. Sampling must preserve original collection times.
    Identified {
        account: AgentAccountId,
        source: Box<dyn AgentWorkAccountSource>,
    },
}

/// Trusted limits for one worker. These values grant no write capability.
pub struct PublicReadWorkSettings {
    pub account: PublicReadWorkAccount,
    pub model: AgentBrowserModel,
    /// Exactly one context; model tokens, cost and operations remain separately
    /// enforced by the shared run ledger.
    pub budget: AgentRunBudget,
    /// Includes final result mapping and remains independent of context size.
    pub max_model_calls: u8,
    /// Original absolute deadline, including any time spent loading credentials.
    pub deadline: Instant,
}

/// Move-only product invocation, consumed once. Credentials and account
/// sources remain on the trusted host and never enter observation handles.
#[must_use]
pub struct PublicReadWorkInvocation {
    objective: PublicReadWorkObjective,
    settings: PublicReadWorkSettings,
    config: AgentWorkApplicationConfig,
    credential: AgentProviderCredential,
    persist_result: bool,
    #[cfg(feature = "public-qualification")]
    inspectable_public: bool,
}

impl PublicReadWorkInvocation {
    #[cfg(feature = "durable-runtime")]
    pub(crate) fn with_read_interactions(self) -> PublicLocalActionWorkInvocation {
        PublicLocalActionWorkInvocation {
            read: self,
            actions: LocalActions {
                policy: Box::new(read_interactions::ReadingInteractionPolicy),
                max_actions: 8,
                read_only: true,
                commits: Vec::new(),
            },
        }
    }

    /// A page in the person's session is read: scrolled, tabs and details
    /// revealed, never changed.
    #[cfg(feature = "durable-runtime")]
    pub(crate) fn with_session_reading(self) -> PublicLocalActionWorkInvocation {
        PublicLocalActionWorkInvocation {
            read: self,
            actions: LocalActions {
                policy: Box::new(read_interactions::SessionReadingPolicy),
                max_actions: 8,
                read_only: true,
                commits: Vec::new(),
            },
        }
    }

    /// A page task on one site: navigating, searching and drafting proceed;
    /// a committing step waits in `gate` for the person. The scope carries
    /// only the commitments already allowed: the one approved step's class,
    /// and edits once the person allowed them for the run.
    #[cfg(feature = "durable-runtime")]
    pub(crate) fn with_site_work(
        self,
        gate: Arc<site_work::SiteGate>,
        asks: bool,
        max_actions: u64,
    ) -> PublicLocalActionWorkInvocation {
        let mut commits: Vec<SemanticEffectClass> = gate.approved_class().into_iter().collect();
        if gate.allow_edits() && !commits.contains(&SemanticEffectClass::ExternalWrite) {
            commits.push(SemanticEffectClass::ExternalWrite);
        }
        PublicLocalActionWorkInvocation {
            read: self,
            actions: LocalActions {
                policy: Box::new(site_work::SiteWorkPolicy { gate, asks }),
                max_actions,
                read_only: false,
                commits,
            },
        }
    }

    pub fn new(
        objective: PublicReadWorkObjective,
        settings: PublicReadWorkSettings,
        config: AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
    ) -> Self {
        Self {
            objective,
            settings,
            config,
            credential,
            persist_result: false,
            #[cfg(feature = "public-qualification")]
            inspectable_public: false,
        }
    }

    /// Explicitly publishes the accepted result in the selected durable
    /// profile. Ephemeral profiles refuse this request. This local publication
    /// choice is independent of provider-side request retention.
    pub fn with_persistent_result(mut self) -> Self {
        self.persist_result = true;
        self
    }

    /// Explicit development consent to provider retention of this Public-only
    /// run. The feature is forbidden in optimized builds. Only retention changes;
    /// objective, scope, account, tools and execution remain identical.
    #[cfg(feature = "public-qualification")]
    pub fn with_inspectable_public_retention(mut self) -> Self {
        self.inspectable_public = true;
        self
    }

    /// Binds the original actor-selected session and original absolute deadline.
    pub fn into_request(
        self,
        profile: AgentWorkProfileBinding,
    ) -> Result<TrustedWorkRequest, AgentWorkFailure> {
        let mut request = TrustedWorkRequest::public_read_objective(
            profile,
            self.objective,
            self.settings,
            self.config,
            self.credential,
        )?;
        if self.persist_result {
            request.input = request.input.persist_extraction_result()?;
        }
        #[cfg(feature = "public-qualification")]
        if self.inspectable_public {
            return Ok(request.with_public_qualification_retention());
        }
        Ok(request)
    }
}

impl std::fmt::Debug for PublicReadWorkInvocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublicReadWorkInvocation([owned, redacted])")
    }
}

/// One ordinary public objective with explicitly approved reversible local
/// effects. The host policy is independent of objective/page/model text and
/// classifies each proposal against task intent and native control evidence.
/// It grants no remote write, submit, communication or capability authority.
#[must_use]
pub struct PublicLocalActionWorkInvocation {
    read: PublicReadWorkInvocation,
    actions: LocalActions,
}

struct LocalActions {
    policy: Box<dyn AgentWorkLocalActionPolicy>,
    max_actions: u64,
    read_only: bool,
    /// Commitments in scope beyond local drafting.
    commits: Vec<SemanticEffectClass>,
}

impl PublicLocalActionWorkInvocation {
    /// Consumes the same objective, account, profile settings and credential as
    /// public reading, plus trusted local effect approval and an independent
    /// action ceiling. Admission is dormant; no page or provider work runs here.
    pub fn try_new(
        objective: PublicReadWorkObjective,
        settings: PublicReadWorkSettings,
        config: AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
        policy: Box<dyn AgentWorkLocalActionPolicy>,
        max_actions: u64,
    ) -> Result<Self, AgentWorkFailure> {
        if max_actions == 0 || max_actions > 64 {
            return Err(AgentWorkFailure::Contract);
        }
        Ok(Self {
            read: PublicReadWorkInvocation::new(objective, settings, config, credential),
            actions: LocalActions {
                policy,
                max_actions,
                read_only: false,
                commits: Vec::new(),
            },
        })
    }

    /// Publishes the accepted source-bound result in the selected durable profile.
    pub fn with_persistent_result(mut self) -> Self {
        self.read = self.read.with_persistent_result();
        self
    }

    /// Retains provider input for an explicitly inspectable public development
    /// or qualification run. This never changes effect, account or page scope.
    #[cfg(feature = "public-qualification")]
    pub fn with_inspectable_public_retention(mut self) -> Self {
        self.read = self.read.with_inspectable_public_retention();
        self
    }

    /// Binds the original actor-selected profile and absolute run deadline.
    pub fn into_request(
        self,
        profile: AgentWorkProfileBinding,
    ) -> Result<TrustedWorkRequest, AgentWorkFailure> {
        self.into_retained_request(profile, ContextId::generate(), None)
    }

    pub(crate) fn into_retained_request(
        mut self,
        profile: AgentWorkProfileBinding,
        context: ContextId,
        remaining_actions: Option<u64>,
    ) -> Result<TrustedWorkRequest, AgentWorkFailure> {
        if let Some(remaining) = remaining_actions {
            self.actions.max_actions = self.actions.max_actions.min(remaining);
        }
        let identity = ContextIdentity::new(
            context,
            ContextRunId::generate(),
            profile.profile(),
            ContextKind::Owned,
        );
        let (mut input, task) = assemble_with_actions(
            identity,
            profile.storage_class(),
            self.read.objective,
            self.read.settings,
            (self.actions.max_actions > 0).then_some(self.actions),
        )?;
        if self.read.persist_result {
            input = input.persist_extraction_result()?;
        }
        let request = TrustedWorkRequest::new(input, self.read.config, self.read.credential, task)
            .with_browser_profile(profile);
        #[cfg(feature = "public-qualification")]
        if self.read.inspectable_public {
            return Ok(request.with_public_qualification_retention());
        }
        Ok(request)
    }
}

impl std::fmt::Debug for PublicLocalActionWorkInvocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublicLocalActionWorkInvocation([owned, redacted])")
    }
}

impl TrustedWorkRequest {
    /// Consumes approved product operands into the ordinary retained Work path.
    /// The request carries no diagnostic retention or qualification authority.
    /// Admission is dormant: native/browser/provider work starts only when the
    /// Shell admits `NativeWorkComposition::launch_retained`.
    pub fn public_read_objective(
        profile: AgentWorkProfileBinding,
        objective: PublicReadWorkObjective,
        settings: PublicReadWorkSettings,
        config: AgentWorkApplicationConfig,
        credential: AgentProviderCredential,
    ) -> Result<Self, AgentWorkFailure> {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            profile.profile(),
            ContextKind::Owned,
        );
        let (input, task) = assemble(identity, profile.storage_class(), objective, settings)?;
        Ok(Self::new(input, config, credential, task).with_browser_profile(profile))
    }
}

fn assemble(
    identity: ContextIdentity,
    storage: ContextProfileStorageClass,
    objective: PublicReadWorkObjective,
    settings: PublicReadWorkSettings,
) -> Result<(AgentWorkRunInput, Box<dyn AgentWorkTask>), AgentWorkFailure> {
    assemble_with_actions(identity, storage, objective, settings, None)
}

fn assemble_with_actions(
    identity: ContextIdentity,
    storage: ContextProfileStorageClass,
    objective: PublicReadWorkObjective,
    settings: PublicReadWorkSettings,
    actions: Option<LocalActions>,
) -> Result<(AgentWorkRunInput, Box<dyn AgentWorkTask>), AgentWorkFailure> {
    let fail = |_| AgentWorkFailure::Contract;
    if settings.budget.contexts() != 1 || objective.objective.trim().is_empty() {
        return Err(AgentWorkFailure::Contract);
    }
    if actions
        .as_ref()
        .is_some_and(|actions| actions.max_actions == 0 || actions.max_actions > 64)
    {
        return Err(AgentWorkFailure::Contract);
    }
    let mut classes = vec![SemanticEffectClass::Read];
    if let Some(actions) = actions.as_ref().filter(|actions| !actions.read_only) {
        classes.push(SemanticEffectClass::LocalWrite);
        classes.extend(actions.commits.iter().copied());
    }
    let effects = AgentEffectScope::try_new(&classes).map_err(fail)?;
    let mut origins = objective.navigation.origins().cloned().collect::<Vec<_>>();
    origins.sort();
    origins.dedup();
    if origins.is_empty() || origins.len() > MAX_AGENT_NAVIGATION_DISCOVERY_RULES {
        return Err(AgentWorkFailure::Contract);
    }
    let account = match &settings.account {
        PublicReadWorkAccount::Anonymous => AgentAccountScope::Anonymous,
        PublicReadWorkAccount::Enrolled(source) => {
            let enrollment = source.enrollment();
            if enrollment.profile().profile() != identity.profile()
                || enrollment.profile().storage_class() != storage
                || enrollment.origin() != objective.navigation.origin()
                || origins.len() != 1
            {
                return Err(AgentWorkFailure::Contract);
            }
            AgentAccountScope::Authenticated(enrollment.account())
        }
        PublicReadWorkAccount::Identified { account, .. } => {
            AgentAccountScope::Authenticated(*account)
        }
    };
    // The person's own session discloses their pages to the model: its
    // content is personal, never public.
    let sensitivity =
        if objective.navigation.is_site_session() && account != AgentAccountScope::Anonymous {
            SemanticSensitivity::Sensitive
        } else {
            SemanticSensitivity::Public
        };
    let node = AgentPlanNodeId::generate();
    let authority = AgentPlanNodeAuthority::try_new(
        vec![identity.profile()],
        vec![account],
        origins.clone(),
        sensitivity,
        effects,
    )
    .map_err(fail)?
    .with_navigation_discovery(objective.navigation.clone())
    .map_err(fail)?;
    let (issued, expires) =
        authority_window(settings.deadline).map_err(|_| AgentWorkFailure::Deadline)?;
    let manifest = AgentRunManifest::try_new(
        AgentRunManifestId::generate(),
        identity.owner(),
        AgentRunScope::try_new(
            vec![identity.profile()],
            vec![account],
            origins,
            sensitivity,
            effects,
            Vec::new(),
        )
        .map_err(fail)?,
        settings.budget,
        issued,
        expires,
        vec![AgentPlanNodeScope::new(
            node,
            authority,
            settings.budget,
            expires,
        )],
    )
    .map_err(fail)?;
    // These counters belong to the fresh run; generated run/context/lease identities
    // distinguish concurrent admissions. No caller can supply colliding runs.
    let ids = TerraControllerIds::try_new(
        AgentSupervisorId::new(1).ok_or(AgentWorkFailure::Contract)?,
        AgentSupervisorAttemptId::new(1).ok_or(AgentWorkFailure::Contract)?,
        AgentSupervisorCancellationId::new(1).ok_or(AgentWorkFailure::Contract)?,
        AgentModelCallId::new(1).ok_or(AgentWorkFailure::Contract)?,
        [1, 2, 3, 4].map(|id| AgentAuditEventId::new(id).expect("nonzero run-local ID")),
        AgentAuditDeliveryId::new(1).ok_or(AgentWorkFailure::Contract)?,
    )
    .map_err(|_| AgentWorkFailure::Contract)?;
    let input = AgentWorkRunInput::try_new(
        manifest,
        AgentPlanLeaseBinding::new(AgentPlanLeaseId::generate(), node),
        AgentWorkContextSpec::try_new_with_document_policy(
            identity,
            storage,
            objective.navigation.departure().clone(),
            objective.navigation.document_policy(),
        )?,
        objective.objective,
        AgentWorkRunSettings::new(
            settings.model,
            ids,
            Arc::new(NativeWorkClock),
            settings.deadline,
        )
        .with_max_model_calls(settings.max_model_calls)?
        .with_max_actions(actions.as_ref().map_or(0, |actions| actions.max_actions))?,
    )?;
    let task = match settings.account {
        PublicReadWorkAccount::Anonymous => AgentWorkDiscoveryTask::try_new(
            identity,
            objective.navigation,
            objective.output_fields,
        )?,
        PublicReadWorkAccount::Identified { source, .. } => {
            AgentWorkDiscoveryTask::try_new_with_account_source(
                identity,
                objective.navigation,
                objective.output_fields,
                account,
                source,
            )?
        }
        PublicReadWorkAccount::Enrolled(source) => {
            AgentWorkDiscoveryTask::try_new_with_account_source(
                identity,
                objective.navigation,
                objective.output_fields,
                account,
                source,
            )?
        }
    };
    let task = if let Some(actions) = actions {
        task.with_local_actions(actions.policy)
    } else {
        task
    };
    Ok((input, Box::new(task)))
}

#[cfg(test)]
mod tests;

#[cfg(feature = "durable-runtime")]
mod consent;
#[cfg(feature = "durable-runtime")]
mod read_interactions;
#[cfg(feature = "durable-runtime")]
pub(crate) mod site_work;
