//! A page agent working on one site in the person's session: it navigates,
//! searches, filters, opens items and fills drafts. Rust classifies every
//! proposed step from the observed page; a step that would commit something
//! (send, post, pay, book, delete, share, save, submit) is held back and
//! recorded for the person. Credentials are never typed.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// What one proposed step would do, read from the page, never from the model.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SiteEffect {
    Read,
    Draft,
    Commit,
}

pub(crate) struct SiteWorkPolicy {
    /// Set once a committing step was held back for the person.
    pub(crate) held: Arc<AtomicBool>,
}

/// Words on a control or its dialog that commit something for the person.
const COMMIT_WORDS: [&str; 52] = [
    "buy",
    "purchase",
    "pay",
    "checkout",
    "order",
    "book",
    "reserve",
    "subscribe",
    "donate",
    "bid",
    "send",
    "post",
    "publish",
    "reply",
    "comment",
    "share",
    "invite",
    "tweet",
    "submit",
    "join",
    "follow",
    "like",
    "react",
    "accept",
    "decline",
    "approve",
    "reject",
    "confirm",
    "rsvp",
    "delete",
    "remove",
    "archive",
    "trash",
    "erase",
    "unsubscribe",
    "leave",
    "deactivate",
    "disconnect",
    "revoke",
    "logout",
    "signout",
    "save",
    "update",
    "transfer",
    "wyślij",
    "opublikuj",
    "kup",
    "zapłać",
    "zamów",
    "zarezerwuj",
    "usuń",
    "zapisz",
];
const COMMIT_PHRASES: [&str; 12] = [
    "request to book",
    "place order",
    "sign up",
    "sign out",
    "log out",
    "change password",
    "cancel subscription",
    "cancel order",
    "cancel booking",
    "cancel reservation",
    "cancel plan",
    "potwierdź",
];
/// Controls that close or step back and so commit nothing.
const DISMISS_WORDS: [&str; 8] = [
    "close",
    "dismiss",
    "cancel",
    "not now",
    "back",
    "later",
    "skip",
    "no thanks",
];
const SIGN_IN_PHRASES: [&str; 8] = [
    "sign in",
    "log in",
    "login",
    "signin",
    "continue with google",
    "continue with apple",
    "zaloguj",
    "sign in with",
];
const CONFIRM_CONTEXT: [&str; 5] = [
    "are you sure",
    "cannot be undone",
    "confirm",
    "permanently",
    "czy na pewno",
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
    let text = words(text);
    phrases
        .iter()
        .any(|phrase| text.contains(&format!(" {} ", words(phrase).trim())))
}

fn label(node: &SemanticNode) -> String {
    let mut label = String::new();
    for part in node.name().into_iter().chain(node.text()) {
        label.push(' ');
        label.push_str(part.as_str());
    }
    label
}

fn ancestors<'a>(
    node: &SemanticNode,
    snapshot: &'a SemanticSnapshot,
) -> impl Iterator<Item = (usize, &'a SemanticNode)> {
    let mut parent = node.parent();
    std::iter::from_fn(move || {
        let index = usize::from(parent?);
        let ancestor = snapshot.nodes().get(index)?;
        parent = ancestor.parent();
        Some((index, ancestor))
    })
}

/// The nodes of the subtree rooted at `index`.
fn subtree(snapshot: &SemanticSnapshot, index: usize) -> &[SemanticNode] {
    let Some(root) = snapshot.nodes().get(index) else {
        return &[];
    };
    let end = snapshot
        .nodes()
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, next)| next.depth() <= root.depth())
        .map_or(snapshot.nodes().len(), |(end, _)| end);
    &snapshot.nodes()[index..end]
}

fn in_search(node: &SemanticNode, snapshot: &SemanticSnapshot) -> bool {
    node.role() == SemanticRole::Searchbox
        || ancestors(node, snapshot).any(|(index, ancestor)| {
            ancestor.landmark_kind() == Some(SemanticLandmarkKind::Search)
                || (ancestor.landmark_kind() == Some(SemanticLandmarkKind::Form)
                    && subtree(snapshot, index)
                        .iter()
                        .any(|part| part.role() == SemanticRole::Searchbox))
        })
}

fn dialog_commits(node: &SemanticNode, snapshot: &SemanticSnapshot) -> bool {
    ancestors(node, snapshot)
        .find(|(_, ancestor)| ancestor.role() == SemanticRole::Dialog)
        .is_some_and(|(index, _)| {
            subtree(snapshot, index)
                .iter()
                .any(|part| names_any(&label(part), &CONFIRM_CONTEXT))
        })
}

fn composer_has_send(snapshot: &SemanticSnapshot) -> bool {
    snapshot.nodes().iter().any(|node| {
        node.role() == SemanticRole::Button
            && names_any(
                &label(node),
                &["send", "post", "reply", "comment", "wyślij"],
            )
    })
}

/// Reads what a step would do from the observed target and its surroundings.
pub(crate) fn classify(
    kind: SemanticActionKind,
    key: Option<SemanticPressKey>,
    node: &SemanticNode,
    snapshot: &SemanticSnapshot,
) -> SiteEffect {
    let text = label(node);
    let commits = names_any(&text, &COMMIT_WORDS) || names_any(&text, &COMMIT_PHRASES);
    match kind {
        SemanticActionKind::Scroll => SiteEffect::Read,
        SemanticActionKind::Click => {
            let dismiss = names_any(&text, &DISMISS_WORDS) && !commits;
            if matches!(node.role(), SemanticRole::Tab)
                || node.activation() == Some(SemanticActivation::Disclosure)
                || (node.role() == SemanticRole::Link && !commits)
                || node.activation() == Some(SemanticActivation::Navigation) && !commits
                || dismiss
            {
                SiteEffect::Read
            } else if commits || dialog_commits(node, snapshot) {
                SiteEffect::Commit
            } else if node.activation() == Some(SemanticActivation::Submit) {
                if in_search(node, snapshot) {
                    SiteEffect::Read
                } else {
                    SiteEffect::Commit
                }
            } else {
                SiteEffect::Draft
            }
        }
        SemanticActionKind::Fill | SemanticActionKind::Select => {
            if in_search(node, snapshot) {
                SiteEffect::Read
            } else if node.editable_structure().is_some()
                && !ancestors(node, snapshot)
                    .any(|(_, a)| a.landmark_kind() == Some(SemanticLandmarkKind::Form))
                && !composer_has_send(snapshot)
            {
                // A document that saves as it is typed commits on the first key.
                SiteEffect::Commit
            } else {
                SiteEffect::Draft
            }
        }
        SemanticActionKind::Press => match key {
            Some(SemanticPressKey::Enter) => {
                if in_search(node, snapshot)
                    || matches!(
                        node.role(),
                        SemanticRole::Combobox | SemanticRole::Listbox | SemanticRole::Option
                    )
                {
                    SiteEffect::Read
                } else {
                    SiteEffect::Commit
                }
            }
            Some(SemanticPressKey::Backspace | SemanticPressKey::Delete) => SiteEffect::Draft,
            Some(SemanticPressKey::Space) => match node.role() {
                SemanticRole::Button | SemanticRole::Link => {
                    classify(SemanticActionKind::Click, None, node, snapshot)
                }
                _ => SiteEffect::Draft,
            },
            _ => SiteEffect::Read,
        },
    }
}

/// A sign-in wall on this page: a visible password field, or a small page
/// asking to sign in with an account field.
pub(crate) fn sign_in_wall(observation: &SemanticObservation) -> bool {
    let Some(snapshot) = observation.frames().first() else {
        return false;
    };
    let nodes = snapshot.nodes();
    let asks = nodes.iter().any(|node| {
        matches!(node.role(), SemanticRole::Heading | SemanticRole::Button)
            && names_any(&label(node), &SIGN_IN_PHRASES)
    });
    // A visible password field is a wall; one without layout facts counts
    // only beside a sign-in heading or button.
    if nodes
        .iter()
        .any(|node| node.role() == SemanticRole::Password && (node.geometry().is_some() || asks))
    {
        return true;
    }
    nodes.len() <= 160
        && asks
        && nodes.iter().any(|node| {
            node.role() == SemanticRole::Textbox
                && (node.sensitivity() != SemanticSensitivity::Public
                    || names_any(&label(node), &["email", "username", "phone", "e-mail"]))
        })
}

fn credential(node: &SemanticNode) -> bool {
    node.role() == SemanticRole::Password || node.sensitivity() == SemanticSensitivity::Secret
}

impl AgentWorkLocalActionPolicy for SiteWorkPolicy {
    fn consent_dismissal(&self, observation: &SemanticObservation) -> Option<SemanticReferenceId> {
        read_interactions::ReadingInteractionPolicy.consent_dismissal(observation)
    }

    fn human_wall(&self, observation: &SemanticObservation) -> Option<AgentBrowserHumanReason> {
        sign_in_wall(observation).then_some(AgentBrowserHumanReason::SignIn)
    }

    fn model_action_operations(
        &self,
        node: &SemanticNode,
        _: &SemanticObservation,
    ) -> Result<SemanticOperations, AgentWorkFailure> {
        if node.states().contains(SemanticState::Disabled) || node.geometry().is_none() {
            return Ok(SemanticOperations::NONE);
        }
        let operations = [
            SemanticOperationClass::Click,
            SemanticOperationClass::Fill,
            SemanticOperationClass::Select,
            SemanticOperationClass::Press,
            SemanticOperationClass::Scroll,
        ]
        .into_iter()
        .filter(|operation| node.operations().contains(*operation))
        .filter(|operation| {
            !credential(node)
                || matches!(
                    operation,
                    SemanticOperationClass::Click | SemanticOperationClass::Scroll
                )
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
        if credential(node)
            && matches!(
                action.kind(),
                SemanticActionKind::Fill | SemanticActionKind::Press | SemanticActionKind::Select
            )
        {
            return Err(AgentWorkFailure::ActionDenied);
        }
        let observed = classify(
            action.kind(),
            action.bound_action().press_key(),
            node,
            snapshot,
        );
        if observed == SiteEffect::Commit {
            self.held.store(true, Ordering::Relaxed);
            return Err(AgentWorkFailure::ActionDenied);
        }
        // Reading and drafting both proceed; the declared class stands.
        Ok(AgentEffectAssessment::new(
            action,
            action.frame().origin().clone(),
            action.effect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page(nodes: Vec<serde_json::Value>) -> SemanticObservation {
        super::super::tests::reading_observation(json!(nodes), "complete")
    }

    fn effect(observation: &SemanticObservation, key: u64, kind: SemanticActionKind) -> SiteEffect {
        effect_key(observation, key, kind, None)
    }
    fn effect_key(
        observation: &SemanticObservation,
        key: u64,
        kind: SemanticActionKind,
        press: Option<SemanticPressKey>,
    ) -> SiteEffect {
        let snapshot = observation.frames().first().unwrap();
        let node = &snapshot.nodes()[usize::try_from(key - 1).unwrap()];
        classify(kind, press, node, snapshot)
    }

    #[test]
    fn committing_controls_are_held_and_drafting_proceeds() {
        let slack = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"textbox","n":"Message #design","o":10}),
            json!({"k":3,"p":0,"r":"button","n":"Send now","o":1,"ak":1}),
            json!({"k":4,"p":0,"r":"button","n":"Emoji","o":1,"ak":1}),
            json!({"k":5,"p":0,"r":"link","n":"#general","o":1,"ak":4}),
            json!({"k":6,"p":0,"r":"tab","n":"Threads","o":1}),
        ]);
        assert_eq!(
            effect(&slack, 3, SemanticActionKind::Click),
            SiteEffect::Commit
        );
        assert_eq!(
            effect(&slack, 4, SemanticActionKind::Click),
            SiteEffect::Draft
        );
        assert_eq!(
            effect(&slack, 5, SemanticActionKind::Click),
            SiteEffect::Read
        );
        assert_eq!(
            effect(&slack, 6, SemanticActionKind::Click),
            SiteEffect::Read
        );
        assert_eq!(
            effect(&slack, 2, SemanticActionKind::Fill),
            SiteEffect::Draft
        );
        assert_eq!(
            effect_key(
                &slack,
                2,
                SemanticActionKind::Press,
                Some(SemanticPressKey::Enter)
            ),
            SiteEffect::Commit
        );
        let booking = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"button","n":"Request to book","o":1,"ak":1}),
            json!({"k":3,"p":0,"r":"button","n":"Apply filters","o":1,"ak":1}),
            json!({"k":4,"p":0,"r":"dialog","n":"Delete this page?"}),
            json!({"k":5,"p":3,"r":"paragraph","t":"This cannot be undone."}),
            json!({"k":6,"p":3,"r":"button","n":"Yes","o":1,"ak":1}),
            json!({"k":7,"p":3,"r":"button","n":"Cancel","o":1,"ak":1}),
        ]);
        assert_eq!(
            effect(&booking, 2, SemanticActionKind::Click),
            SiteEffect::Commit
        );
        assert_eq!(
            effect(&booking, 3, SemanticActionKind::Click),
            SiteEffect::Draft
        );
        assert_eq!(
            effect(&booking, 6, SemanticActionKind::Click),
            SiteEffect::Commit
        );
        assert_eq!(
            effect(&booking, 7, SemanticActionKind::Click),
            SiteEffect::Read
        );
        let search = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"landmark","lm":"search"}),
            json!({"k":3,"p":1,"r":"searchbox","n":"Where","o":2}),
            json!({"k":4,"p":1,"r":"button","n":"Search","o":1,"ak":2}),
        ]);
        assert_eq!(
            effect(&search, 3, SemanticActionKind::Fill),
            SiteEffect::Read
        );
        assert_eq!(
            effect(&search, 4, SemanticActionKind::Click),
            SiteEffect::Read
        );
        assert_eq!(
            effect_key(
                &search,
                3,
                SemanticActionKind::Press,
                Some(SemanticPressKey::Enter)
            ),
            SiteEffect::Read
        );
    }

    #[test]
    fn a_password_or_a_small_sign_in_form_is_a_wall() {
        assert!(sign_in_wall(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"password","n":"Password","q":"secret","o":2,"b":{"x":10,"y":10,"w":200,"h":30}}),
        ])));
        assert!(sign_in_wall(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Sign in to your account","l":1}),
            json!({"k":3,"p":0,"r":"password","n":"Password","q":"secret","o":2}),
        ])));
        assert!(!sign_in_wall(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"password","n":"Password","q":"secret","o":2}),
        ])));
        assert!(sign_in_wall(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Sign in to Notion","l":1}),
            json!({"k":3,"p":0,"r":"textbox","n":"Email","q":"sensitive","o":2,"b":{"x":10,"y":50,"w":200,"h":30}}),
        ])));
        assert!(!sign_in_wall(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Inbox","l":1}),
            json!({"k":3,"p":0,"r":"button","n":"Log in to another account","o":1,"ak":1}),
        ])));
    }
}
