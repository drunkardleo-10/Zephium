use super::*;

pub(super) struct ReadingInteractionPolicy;

#[derive(Clone, Copy, PartialEq)]
enum Interaction {
    Scroll,
    Tab,
    Disclosure,
    Dismiss,
    /// A cookie consent dialog's own dismiss control, clicked only by the
    /// code-owned consent recipe and never offered to a provider.
    Consent,
}

/// Dismiss names in preference order: refusing optional cookies first, then
/// acknowledging. Matched as whole words, English and Polish.
const CONSENT_REFUSE: [&str; 14] = [
    "reject",
    "reject all",
    "decline",
    "refuse",
    "deny",
    "necessary only",
    "only necessary",
    "necessary cookies only",
    "essential only",
    "only essential",
    "continue without accepting",
    "odrzuć",
    "odrzuć wszystkie",
    "tylko niezbędne",
];
const CONSENT_ACKNOWLEDGE: [&str; 12] = [
    "accept",
    "accept all",
    "agree",
    "i agree",
    "allow all",
    "ok",
    "okay",
    "got it",
    "continue",
    "akceptuj",
    "zgadzam się",
    "rozumiem",
];
const CONSENT_TOPIC: [&str; 4] = ["cookie", "cookies", "ciasteczek", "ciasteczka"];
const CONSENT_SETTINGS: [&str; 8] = [
    "settings",
    "manage",
    "customize",
    "preferences",
    "ustawienia",
    "zarządzaj",
    "ustaw",
    "preferencje",
];

/// Disclosure names that may hold a read's values, matched as whole words.
const DETAIL_DISCLOSURES: [&str; 9] = [
    "specifications",
    "specification",
    "specs",
    "details",
    "product details",
    "description",
    "technical",
    "dimensions",
    "features",
];

fn words(text: &str) -> String {
    let words: Vec<_> = text
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    format!(" {} ", words.join(" "))
}

fn names_any(text: &str, phrases: &[&str]) -> bool {
    phrases
        .iter()
        .any(|phrase| text.contains(&format!(" {phrase} ")))
}

/// The consent dialog containing `node`, as (index, end) of its subtree, when
/// that dialog's own text is about cookies and was captured completely.
fn consent_dialog(node: &SemanticNode, snapshot: &SemanticSnapshot) -> Option<(usize, usize)> {
    let mut parent = node.parent();
    while let Some(index) = parent {
        let ancestor = snapshot.nodes().get(usize::from(index))?;
        if ancestor.role() == SemanticRole::Dialog {
            let index = usize::from(index);
            let end = snapshot
                .nodes()
                .iter()
                .enumerate()
                .skip(index + 1)
                .find(|(_, next)| next.depth() <= ancestor.depth())
                .map_or(snapshot.nodes().len(), |(end, _)| end);
            let subtree = &snapshot.nodes()[index..end];
            return (subtree
                .iter()
                .all(|part| part.sensitivity() == SemanticSensitivity::Public)
                && subtree.iter().any(|part| {
                    part.name()
                        .into_iter()
                        .chain(part.text())
                        .any(|text| names_any(&words(text.as_str()), &CONSENT_TOPIC))
                }))
            .then_some((index, end));
        }
        parent = ancestor.parent();
    }
    None
}

/// 0 refuses optional cookies, 1 acknowledges; a settings control is never one.
fn consent_rank(node: &SemanticNode) -> Option<u8> {
    if node.role() != SemanticRole::Button {
        return None;
    }
    let name = words(node.name()?.as_str());
    if names_any(&name, &CONSENT_SETTINGS) {
        return None;
    }
    if names_any(&name, &CONSENT_REFUSE) {
        Some(0)
    } else if names_any(&name, &CONSENT_ACKNOWLEDGE) {
        Some(1)
    } else {
        None
    }
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
    let ordinary = ordinary_interaction(node, observation);
    if ordinary.is_none()
        && consent_rank(node).is_some()
        && consent_dialog(node, snapshot).is_some()
    {
        return Some(Interaction::Consent);
    }
    ordinary
}

fn ordinary_interaction(
    node: &SemanticNode,
    observation: &SemanticObservation,
) -> Option<Interaction> {
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
            Some(Interaction::Dismiss | Interaction::Consent) => (
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

    fn consent_dismissal(&self, observation: &SemanticObservation) -> Option<SemanticReferenceId> {
        let snapshot = observation.frames().first()?;
        snapshot
            .nodes()
            .iter()
            .filter(|node| {
                matches!(
                    interaction(node, observation, SemanticOperationClass::Click),
                    Some(Interaction::Consent | Interaction::Dismiss)
                ) && consent_dialog(node, snapshot).is_some()
            })
            .filter_map(|node| Some((consent_rank(node)?, node.reference())))
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, reference)| reference)
    }

    fn detail_disclosure(&self, observation: &SemanticObservation) -> Option<DecisionOperation> {
        let snapshot = observation.frames().first()?;
        snapshot.nodes().iter().find_map(|node| {
            let named = node.role() == SemanticRole::Button
                && node.activation() == Some(SemanticActivation::Disclosure)
                && !node.states().contains(SemanticState::Expanded)
                && node
                    .name()
                    .is_some_and(|name| names_any(&words(name.as_str()), &DETAIL_DISCLOSURES));
            if !named {
                return None;
            }
            // Only a control in view: bringing one into view was measured to
            // fail natively when a page's layout still shifts.
            (interaction(node, observation, SemanticOperationClass::Click)
                == Some(Interaction::Disclosure))
            .then(|| DecisionOperation::Click(node.reference()))
        })
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
        .filter(|operation| {
            interaction(node, observation, *operation)
                .is_some_and(|interaction| interaction != Interaction::Consent)
        })
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
                Interaction::Dismiss | Interaction::Consent => {
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

/// A signed-in page is read-only for the agent: it may scroll the document,
/// never click, dismiss or consent, since page handlers may persist state.
pub(super) struct SignedInReadingPolicy;

impl AgentWorkLocalActionPolicy for SignedInReadingPolicy {
    fn decision_action_recipe(
        &self,
        operation: &DecisionOperation,
        observation: &SemanticObservation,
    ) -> Result<Option<SemanticActionProposal>, AgentWorkFailure> {
        match operation {
            DecisionOperation::Scroll(_) => {
                ReadingInteractionPolicy.decision_action_recipe(operation, observation)
            }
            _ => Ok(None),
        }
    }

    fn model_action_effect(&self) -> Option<SemanticEffectClass> {
        Some(SemanticEffectClass::Read)
    }

    fn model_action_operations(
        &self,
        node: &SemanticNode,
        observation: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        let scroll = interaction(node, observation, SemanticOperationClass::Scroll)
            == Some(Interaction::Scroll);
        SemanticOperations::try_new(if scroll {
            &[SemanticOperationClass::Scroll]
        } else {
            &[]
        })
        .map_err(|_| AgentWorkFailure::Contract)
    }

    fn assess(
        &self,
        action: &SemanticPreparedAction,
        observation: &SemanticObservation,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        if action.kind() != SemanticActionKind::Scroll {
            return Err(AgentWorkFailure::ActionDenied);
        }
        ReadingInteractionPolicy.assess(action, observation)
    }
}
