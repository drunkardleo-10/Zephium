use super::*;

pub(super) struct ReadingInteractionPolicy;

#[derive(Clone, Copy)]
enum Interaction {
    Scroll,
    Tab,
    Disclosure,
    Dismiss,
}

fn interaction(
    node: &SemanticNode,
    observation: &SemanticObservation,
    operation: SemanticOperationClass,
) -> Option<Interaction> {
    if node.sensitivity() != SemanticSensitivity::Public
        || node.fields_complete() != Some(true)
        || node.geometry().is_none()
        || node.states().contains(SemanticState::Disabled)
    {
        return None;
    }
    let snapshot = observation.frames().first()?;
    let mut ancestors = Vec::new();
    let mut parent = node.parent();
    while let Some(index) = parent {
        let ancestor = snapshot.nodes().get(usize::from(index))?;
        ancestors.push(index);
        parent = ancestor.parent();
    }
    if snapshot
        .nodes()
        .iter()
        .enumerate()
        .any(|(index, candidate)| {
            candidate.role() == SemanticRole::Dialog
                && candidate.geometry().is_some()
                && candidate.reference() != node.reference()
                && !ancestors.contains(&(index as u16))
        })
    {
        return None;
    }
    if operation == SemanticOperationClass::Scroll {
        return node
            .operations()
            .contains(operation)
            .then_some(Interaction::Scroll);
    }
    let complete_context = matches!(
        snapshot.completeness(),
        SemanticCompleteness::Complete
            | SemanticCompleteness::Truncated(SemanticTruncation::FieldLimit)
    );
    let local_disclosure = node.role() == SemanticRole::Button
        && node.activation() == Some(SemanticActivation::Disclosure);
    if (!complete_context && !local_disclosure)
        || !node.operations().contains(SemanticOperationClass::Click)
        || !matches!(
            node.activation(),
            Some(SemanticActivation::Ordinary | SemanticActivation::Disclosure)
        )
    {
        return None;
    }
    let name = node.name()?.as_str().to_lowercase();
    if consequential(&name) {
        return None;
    }
    if node.role() == SemanticRole::Tab {
        return Some(Interaction::Tab);
    }
    if node.role() != SemanticRole::Button {
        return None;
    }
    let dismiss = matches!(
        name.trim(),
        "close"
            | "dismiss"
            | "not now"
            | "reject all"
            | "decline"
            | "necessary only"
            | "continue without accepting"
    );
    if dismiss || matches!(name.trim(), "continue" | "enter") {
        let snapshot = observation.frames().first()?;
        let mut parent = node.parent();
        while let Some(index) = parent {
            let ancestor = snapshot.nodes().get(usize::from(index))?;
            if ancestor.role() == SemanticRole::Dialog {
                // Dialog text is risk evidence, never authority. Missing context refuses.
                if snapshot.completeness() != SemanticCompleteness::Complete {
                    return None;
                }
                let end = snapshot
                    .nodes()
                    .iter()
                    .enumerate()
                    .skip(usize::from(index) + 1)
                    .find(|(_, next)| next.depth() <= ancestor.depth())
                    .map_or(snapshot.nodes().len(), |(end, _)| end);
                if !dismiss
                    && snapshot.nodes()[usize::from(index)..end]
                        .iter()
                        .any(|part| {
                            part.sensitivity() != SemanticSensitivity::Public
                                || part.name().into_iter().chain(part.text()).any(|text| {
                                    consequential_context(&text.as_str().to_lowercase())
                                })
                        })
                {
                    return None;
                }
                return Some(Interaction::Dismiss);
            }
            parent = ancestor.parent();
        }
        return None;
    }
    (node.activation() == Some(SemanticActivation::Disclosure)).then_some(Interaction::Disclosure)
}

fn consequential_context(text: &str) -> bool {
    let words = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    [
        "confirm",
        "payment",
        "purchase",
        "place order",
        "checkout",
        "sign in",
        "sign up",
        "log in",
        "log out",
        "authorize",
        "permission",
        "accept",
        "agree",
        "subscribe",
        "delete",
        "remove",
        "send",
        "publish",
        "submit",
        "save",
    ]
    .iter()
    .any(|term| {
        words
            .windows(term.split_whitespace().count())
            .any(|words| words.iter().copied().eq(term.split_whitespace()))
    })
}

// A supplementary risk screen; native form/navigation boundaries and task scope
// are enforced separately. Arbitrary page handlers cannot be proven side-effect free.
fn consequential(text: &str) -> bool {
    text.split(|ch: char| !ch.is_alphanumeric()).any(|word| {
        matches!(
            word,
            "buy"
                | "purchase"
                | "checkout"
                | "pay"
                | "payment"
                | "order"
                | "book"
                | "reserve"
                | "rent"
                | "subscribe"
                | "unsubscribe"
                | "send"
                | "post"
                | "publish"
                | "submit"
                | "save"
                | "delete"
                | "remove"
                | "erase"
                | "upload"
                | "download"
                | "install"
                | "password"
                | "login"
                | "logout"
                | "signin"
                | "signout"
                | "authorize"
                | "allow"
                | "agree"
                | "accept"
                | "consent"
                | "confirm"
                | "wishlist"
                | "bag"
                | "basket"
                | "cart"
                | "account"
                | "newsletter"
        )
    }) || text.contains("sign in")
        || text.contains("sign up")
        || text.contains("log in")
}

impl AgentWorkLocalActionPolicy for ReadingInteractionPolicy {
    fn decision_action_recipe(
        &self,
        operation: &DecisionOperation,
        observation: &SemanticObservation,
    ) -> Result<Option<SemanticActionProposal>, AgentWorkFailure> {
        let (target, class) = match operation {
            DecisionOperation::Click(target) => (*target, SemanticOperationClass::Click),
            DecisionOperation::Scroll(target) => (*target, SemanticOperationClass::Scroll),
            _ => return Ok(None),
        };
        let node = observation
            .frames()
            .first()
            .and_then(|frame| frame.nodes().iter().find(|node| node.reference() == target))
            .ok_or(AgentWorkFailure::Contract)?;
        let (intent, verification) = match interaction(node, observation, class) {
            Some(Interaction::Scroll) => (
                SemanticActionIntent::Scroll {
                    target,
                    direction: SemanticScrollDirection::Down,
                    amount: SemanticScrollAmount::HalfPage,
                },
                SemanticVerification::ScrollPositionChanged,
            ),
            Some(Interaction::Tab) => (
                SemanticActionIntent::Click { target },
                SemanticVerification::TargetState {
                    state: SemanticState::Selected,
                    present: true,
                },
            ),
            Some(Interaction::Disclosure) => (
                SemanticActionIntent::Click { target },
                SemanticVerification::TargetState {
                    state: SemanticState::Expanded,
                    present: !node.states().contains(SemanticState::Expanded),
                },
            ),
            Some(Interaction::Dismiss) => (
                SemanticActionIntent::Click { target },
                SemanticVerification::PageDialogClosed,
            ),
            None => return Ok(None),
        };
        let recipe = SemanticActionProposal::try_new(
            intent,
            SemanticEffectClass::Read,
            if class == SemanticOperationClass::Scroll {
                SemanticWaitCondition::Immediate
            } else {
                SemanticWaitCondition::MutationQuiet(
                    SemanticMutationQuietPeriod::try_new(100)
                        .map_err(|_| AgentWorkFailure::Contract)?,
                )
            },
            verification,
            SemanticSettleBudget::try_new(MIN_AGENT_BROWSER_SNAPSHOT_SETTLE_MILLIS)
                .map_err(|_| AgentWorkFailure::Contract)?,
        )
        .map_err(|_| AgentWorkFailure::Contract)?;
        Ok(Some(recipe))
    }

    fn model_action_effect(&self) -> Option<SemanticEffectClass> {
        Some(SemanticEffectClass::Read)
    }

    fn model_action_operations(
        &self,
        node: &SemanticNode,
        observation: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        let operations = [
            SemanticOperationClass::Click,
            SemanticOperationClass::Scroll,
        ]
        .into_iter()
        .filter(|operation| interaction(node, observation, *operation).is_some())
        .collect::<Vec<_>>();
        SemanticOperations::try_new(&operations).map_err(|_| AgentWorkFailure::Contract)
    }

    fn assess(
        &self,
        action: &SemanticPreparedAction,
        observation: &SemanticObservation,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        let snapshot = observation
            .frames()
            .first()
            .ok_or(AgentWorkFailure::Contract)?;
        let node = snapshot
            .nodes()
            .iter()
            .find(|node| node.reference() == action.target_reference())
            .ok_or(AgentWorkFailure::Contract)?;
        let permitted = match interaction(
            node,
            observation,
            if action.kind() == SemanticActionKind::Scroll {
                SemanticOperationClass::Scroll
            } else {
                SemanticOperationClass::Click
            },
        ) {
            Some(Interaction::Scroll) => {
                action.kind() == SemanticActionKind::Scroll
                    && action.verification() == SemanticVerification::ScrollPositionChanged
            }
            Some(kind) if action.kind() == SemanticActionKind::Click => match kind {
                Interaction::Tab => {
                    action.verification()
                        == SemanticVerification::TargetState {
                            state: SemanticState::Selected,
                            present: true,
                        }
                }
                Interaction::Disclosure => matches!(
                    action.verification(),
                    SemanticVerification::TargetState {
                        state: SemanticState::Expanded,
                        ..
                    }
                ),
                Interaction::Dismiss => {
                    action.verification() == SemanticVerification::PageDialogClosed
                }
                Interaction::Scroll => false,
            },
            _ => false,
        };
        if !permitted || action.effect() != SemanticEffectClass::Read {
            return Err(AgentWorkFailure::ActionDenied);
        }
        Ok(AgentEffectAssessment::new(
            action,
            action.frame().origin().clone(),
            SemanticEffectClass::Read,
        ))
    }
}
