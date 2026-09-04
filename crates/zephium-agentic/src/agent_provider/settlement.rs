//! Sealed provider-terminal pricing and policy settlement.
//!
//! A decoded browser proposal remains private in the move-only EOF terminal.
//! The proposal and its exact one-shot continuation can leave this module only
//! after trusted catalog pricing and the corresponding policy settlement both
//! succeed.

use std::fmt;

use thiserror::Error;

#[cfg(feature = "provider-transport")]
use super::AgentProviderFailureClass;
use super::{
    AgentProviderCallConfig, AgentProviderCallIdentity, AgentProviderContinuation,
    AgentProviderContinuationSeed, AgentProviderFinishedStream, AgentProviderPricingError,
    AgentProviderPricingSchedule, AgentProviderStopReason, AgentProviderStreamConclusion,
    AgentProviderUsage,
};
use crate::{
    AgentActiveModelCall, AgentBrowserToolProposal, AgentModelCallReceipt,
    AgentModelCallSettlement, AgentModelCallUnaccountedSettlement, AgentPolicyError,
    AgentRunPolicy,
};

/// Move-only exact-usage terminal awaiting trusted pricing and policy settlement.
#[must_use]
pub struct AgentProviderPricingSettlement {
    active: AgentActiveModelCall,
    config: AgentProviderCallConfig,
    finished: AgentProviderFinishedStream,
    usage: AgentProviderUsage,
    identity: super::AgentProviderResponseIdentity,
    continuation: Option<AgentProviderContinuationSeed>,
    settlement: AgentModelCallSettlement,
}

impl AgentProviderPricingSettlement {
    /// Seals a complete EOF terminal to its exact active policy authority.
    ///
    /// A mismatch consumes every untrusted terminal component and returns only
    /// the active authority so transport can charge its reservation ceiling.
    #[cfg(feature = "provider-transport")]
    pub(crate) fn try_from_provider_eof(
        active: AgentActiveModelCall,
        config: AgentProviderCallConfig,
        finished: AgentProviderFinishedStream,
        continuation: Option<AgentProviderContinuationSeed>,
    ) -> Result<Self, AgentProviderPricingSettlementBindingError> {
        let Some(usage) = finished.usage() else {
            return Err(AgentProviderPricingSettlementBindingError {
                active: Box::new(active),
            });
        };
        let Some(identity) = finished.response_identity() else {
            return Err(AgentProviderPricingSettlementBindingError {
                active: Box::new(active),
            });
        };
        if !finished.call().matches_active(&active) {
            return Err(AgentProviderPricingSettlementBindingError {
                active: Box::new(active),
            });
        }
        let settlement = match finished.conclusion() {
            AgentProviderStreamConclusion::Completed(_) => AgentModelCallSettlement::Completed,
            AgentProviderStreamConclusion::Failed(failure)
                if failure.failure().class() == AgentProviderFailureClass::Cancelled =>
            {
                AgentModelCallSettlement::Cancelled
            }
            AgentProviderStreamConclusion::Failed(_) => AgentModelCallSettlement::ProviderFailed,
        };
        Ok(Self {
            active,
            config,
            finished,
            usage,
            identity,
            continuation,
            settlement,
        })
    }

    /// Exact non-authorizing call correlation.
    pub fn call(&self) -> AgentProviderCallIdentity {
        AgentProviderCallIdentity::from_active(&self.active)
    }

    /// Exact trusted provider call configuration awaiting pricing.
    pub const fn config(&self) -> &AgentProviderCallConfig {
        &self.config
    }

    /// Content-free terminal facts; never browser-operation authority.
    pub const fn conclusion(&self) -> AgentProviderStreamConclusion {
        self.finished.conclusion()
    }

    /// Exact normalized provider usage awaiting trusted pricing.
    pub fn usage(&self) -> AgentProviderUsage {
        self.usage
    }

    /// Completed, provider-failed, or cancelled policy terminal class.
    pub const fn settlement(&self) -> AgentModelCallSettlement {
        self.settlement
    }

    /// Prices the exact EOF terminal, settles policy, then releases any tool turn.
    pub fn settle(
        self,
        policy: &mut AgentRunPolicy,
        schedule: &AgentProviderPricingSchedule,
    ) -> Result<AgentProviderSettledTerminal, AgentProviderPricingSettlementError> {
        if let Err(error) = policy.prevalidate_active_model_call(&self.active) {
            return Err(AgentProviderPricingSettlementError::PolicyPrecondition {
                error,
                unsettled: Box::new(self),
            });
        }
        if !schedule.exact_binding_matches(&self.config) {
            return Err(AgentProviderPricingSettlementError::Schedule {
                unsettled: Box::new(self),
            });
        }
        let priced = match schedule.try_price(&self.config, self.identity, self.usage) {
            Ok(priced) => priced,
            Err(error) => {
                return Err(AgentProviderPricingSettlementError::TerminalPricing {
                    error,
                    unsettled: Box::new(self),
                });
            }
        };

        let Self {
            active,
            config: _,
            finished,
            usage: _,
            identity: _,
            continuation,
            mut settlement,
        } = self;
        let (conclusion, tool) = finished.into_parts();
        let tool_turn = match tool {
            Some(tool) => {
                if let AgentProviderStreamConclusion::Completed(completion) = conclusion {
                    continuation.and_then(|seed| {
                        let (correlation, proposal) = tool.into_continuation_parts();
                        seed.join_terminal_tool(completion, correlation)
                            .ok()
                            .map(|continuation| AgentProviderSettledToolTurn {
                                proposal,
                                continuation,
                            })
                    })
                } else {
                    None
                }
            }
            None => None,
        };
        if matches!(
            conclusion,
            AgentProviderStreamConclusion::Completed(completion)
                if completion.stop() == AgentProviderStopReason::ToolCalls
        ) && tool_turn.is_none()
        {
            settlement = AgentModelCallSettlement::ProviderFailed;
        }
        let receipt = policy
            .settle_model_call_priced(active, settlement, priced)
            .map_err(AgentProviderPricingSettlementError::Policy)?;
        Ok(AgentProviderSettledTerminal {
            receipt,
            conclusion,
            tool_turn: if settlement == AgentModelCallSettlement::Completed {
                tool_turn
            } else {
                None
            },
        })
    }

    /// Conservatively consumes an unpriceable terminal at reservation ceiling.
    ///
    /// This path permanently drops all provider output, including a buffered
    /// tool proposal and continuation. It exists for authenticated usage whose
    /// exact pre-bound schedule cannot represent the returned token range or
    /// checked price; callers cannot loop forever while retaining live policy
    /// authority. Wrong or sealed policies retain this complete owner.
    pub fn settle_at_reservation_ceiling(
        self,
        policy: &mut AgentRunPolicy,
    ) -> Result<AgentModelCallReceipt, AgentProviderPricingSettlementError> {
        if let Err(error) = policy.prevalidate_active_model_call(&self.active) {
            return Err(AgentProviderPricingSettlementError::PolicyPrecondition {
                error,
                unsettled: Box::new(self),
            });
        }
        let Self { active, .. } = self;
        policy
            .settle_model_call_unaccounted(
                active,
                AgentModelCallUnaccountedSettlement::ProviderFailed,
            )
            .map_err(AgentProviderPricingSettlementError::Policy)
    }
}

impl fmt::Debug for AgentProviderPricingSettlement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderPricingSettlement")
            .field("active", &self.active)
            .field("config", &self.config)
            .field("finished", &self.finished)
            .field("usage", &self.usage)
            .field("identity", &self.identity)
            .field("continuation", &self.continuation.is_some())
            .field("settlement", &self.settlement)
            .field("content", &"[redacted]")
            .finish()
    }
}

/// Failed join between one EOF terminal and exact policy authority.
#[must_use]
#[cfg(feature = "provider-transport")]
pub(crate) struct AgentProviderPricingSettlementBindingError {
    active: Box<AgentActiveModelCall>,
}

#[cfg(feature = "provider-transport")]
impl AgentProviderPricingSettlementBindingError {
    /// Recovers only policy authority; all untrusted provider state is dropped.
    pub(crate) fn into_active(self) -> AgentActiveModelCall {
        *self.active
    }
}

#[cfg(feature = "provider-transport")]
impl fmt::Debug for AgentProviderPricingSettlementBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderPricingSettlementBindingError")
            .field("active", &self.active)
            .field("provider_state", &"[dropped]")
            .finish()
    }
}

/// Terminal produced only after successful exact policy settlement.
#[must_use]
pub struct AgentProviderSettledTerminal {
    receipt: AgentModelCallReceipt,
    conclusion: AgentProviderStreamConclusion,
    tool_turn: Option<AgentProviderSettledToolTurn>,
}

impl AgentProviderSettledTerminal {
    #[cfg(test)]
    pub(crate) const fn for_test(
        receipt: AgentModelCallReceipt,
        conclusion: AgentProviderStreamConclusion,
        tool_turn: Option<AgentProviderSettledToolTurn>,
    ) -> Self {
        Self {
            receipt,
            conclusion,
            tool_turn,
        }
    }

    /// Exact model-call accounting receipt.
    pub const fn receipt(&self) -> AgentModelCallReceipt {
        self.receipt
    }

    /// Content-free provider terminal joined to the receipt.
    pub const fn conclusion(&self) -> AgentProviderStreamConclusion {
        self.conclusion
    }

    /// Whether successful settlement released one browser proposal turn.
    pub const fn has_tool_turn(&self) -> bool {
        self.tool_turn.is_some()
    }

    /// Consumes the settled terminal and releases its sole browser proposal turn.
    pub fn into_tool_turn(self) -> Option<AgentProviderSettledToolTurn> {
        self.tool_turn
    }
}

impl fmt::Debug for AgentProviderSettledTerminal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderSettledTerminal")
            .field("receipt", &self.receipt)
            .field("conclusion", &self.conclusion)
            .field("has_tool_turn", &self.tool_turn.is_some())
            .field("content", &"[redacted]")
            .finish()
    }
}

/// One browser proposal and exact stateless continuation released after settlement.
#[must_use]
pub struct AgentProviderSettledToolTurn {
    proposal: AgentBrowserToolProposal,
    continuation: AgentProviderContinuation,
}

impl AgentProviderSettledToolTurn {
    /// Closed browser proposal. It still grants no effect authority.
    pub const fn proposal(&self) -> &AgentBrowserToolProposal {
        &self.proposal
    }

    /// Exact one-shot continuation for the eventual tool result.
    pub const fn continuation(&self) -> &AgentProviderContinuation {
        &self.continuation
    }

    /// Consumes the turn without copying provider-authored correlation.
    pub fn into_parts(self) -> (AgentBrowserToolProposal, AgentProviderContinuation) {
        (self.proposal, self.continuation)
    }
}

impl fmt::Debug for AgentProviderSettledToolTurn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentProviderSettledToolTurn")
            .field("proposal", &self.proposal)
            .field("continuation", &self.continuation)
            .finish()
    }
}

/// Failure while joining trusted pricing to exact terminal policy authority.
#[derive(Debug, Error)]
pub enum AgentProviderPricingSettlementError {
    /// The supplied schedule was not the exact schedule bound before dispatch.
    #[error("agent provider settlement received the wrong pricing schedule")]
    Schedule {
        /// Complete move-only authority retained for the correct schedule.
        unsettled: Box<AgentProviderPricingSettlement>,
    },
    /// Exact bound schedule could not price authenticated terminal usage.
    #[error("agent provider EOF terminal usage is unpriceable")]
    TerminalPricing {
        /// Closed input-range, identity, or checked-arithmetic refusal class.
        error: AgentProviderPricingError,
        /// Complete owner retained for conservative reservation settlement.
        unsettled: Box<AgentProviderPricingSettlement>,
    },
    /// The supplied policy was sealed or did not own this exact active call.
    #[error("agent provider settlement policy precondition failed")]
    PolicyPrecondition {
        /// Content-free policy refusal.
        error: AgentPolicyError,
        /// Complete owner retained for the correct live policy.
        unsettled: Box<AgentProviderPricingSettlement>,
    },
    /// Policy consumed the terminal transition and failed stopped.
    #[error("agent provider priced policy settlement failed")]
    Policy(#[source] AgentPolicyError),
}

impl AgentProviderPricingSettlementError {
    /// Content-free pricing refusal when policy was not invoked.
    pub const fn pricing_error(&self) -> Option<AgentProviderPricingError> {
        match self {
            Self::Schedule { .. } => Some(AgentProviderPricingError::Identity),
            Self::TerminalPricing { error, .. } => Some(*error),
            Self::PolicyPrecondition { .. } | Self::Policy(_) => None,
        }
    }

    /// Content-free policy refusal when the settlement owner was retained.
    pub const fn policy_precondition_error(&self) -> Option<AgentPolicyError> {
        match self {
            Self::PolicyPrecondition { error, .. } => Some(*error),
            Self::Schedule { .. } | Self::TerminalPricing { .. } | Self::Policy(_) => None,
        }
    }

    /// Recovers exact authority only when pricing refused before policy use.
    pub fn into_unsettled(self) -> Option<AgentProviderPricingSettlement> {
        match self {
            Self::Schedule { unsettled }
            | Self::TerminalPricing { unsettled, .. }
            | Self::PolicyPrecondition { unsettled, .. } => Some(*unsettled),
            Self::Policy(_) => None,
        }
    }
}
