//! Cookie and consent banners on a page worked for the person. Choosing on
//! a banner commits nothing the person would confirm: Rust refuses optional
//! cookies itself when the banner offers it, and a choice the page agent
//! makes there is a read, never a held step. Accepting optional cookies is
//! refused while the same banner offers a refusal.
use super::*;

/// What one banner control chooses, by its name.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Choice {
    /// Refuses optional cookies: the privacy-preserving choice.
    Refuse,
    /// Closes or acknowledges a notice without granting anything.
    Acknowledge,
    /// Grants optional cookies.
    Accept,
    /// Opens the banner's own settings.
    Settings,
}

/// Earlier lists win: "Accept only necessary" refuses.
const REFUSE: [&str; 30] = [
    "reject",
    "reject all",
    "reject cookies",
    "reject optional cookies",
    "decline",
    "decline all",
    "decline optional cookies",
    "refuse",
    "refuse all",
    "deny",
    "deny all",
    "necessary only",
    "only necessary",
    "necessary cookies only",
    "only necessary cookies",
    "use necessary cookies only",
    "essential only",
    "only essential",
    "essential cookies only",
    "only essential cookies",
    "strictly necessary only",
    "continue without accepting",
    "odrzuć",
    "odrzuć wszystkie",
    "tylko niezbędne",
    "alle ablehnen",
    "nur notwendige",
    "tout refuser",
    "refuser",
    "rechazar todo",
];
const ACKNOWLEDGE: [&str; 11] = [
    "ok",
    "okay",
    "got it",
    "close",
    "dismiss",
    "continue",
    "i understand",
    "understood",
    "rozumiem",
    "zamknij",
    "kontynuuj",
];
const ACCEPT: [&str; 17] = [
    "accept",
    "accept all",
    "accept cookies",
    "accept all cookies",
    "agree",
    "i agree",
    "agree and continue",
    "allow",
    "allow all",
    "allow cookies",
    "allow all cookies",
    "akceptuj",
    "akceptuję",
    "zgadzam się",
    "alle akzeptieren",
    "tout accepter",
    "aceptar todo",
];
const SETTINGS: [&str; 12] = [
    "settings",
    "cookie settings",
    "manage",
    "manage preferences",
    "manage cookies",
    "customize",
    "customise",
    "preferences",
    "more options",
    "options",
    "ustawienia",
    "zarządzaj",
];
/// Words a banner's own text uses for its topic.
const TOPIC: [&str; 12] = [
    "cookie",
    "cookies",
    "your privacy",
    "privacy choices",
    "privacy settings",
    "ciasteczek",
    "ciasteczka",
    "pliki cookie",
    "plików cookie",
    "consent",
    "tracking technologies",
    "zgody",
];
/// The largest region still read as one banner, in nodes; a dialog, which
/// is plainly one thing, may list its purposes at length.
const BANNER_NODES: usize = 80;
const DIALOG_NODES: usize = 400;

fn words(text: &str) -> String {
    let words: Vec<_> = text
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    format!(" {} ", words.join(" "))
}

fn exactly(name: &str, phrases: &[&str]) -> bool {
    let name = words(name);
    phrases
        .iter()
        .any(|phrase| name.trim() == words(phrase).trim())
}

fn names(name: &str, phrases: &[&str]) -> bool {
    let name = words(name);
    phrases
        .iter()
        .any(|phrase| name.contains(&format!(" {} ", words(phrase).trim())))
}

fn label(node: &SemanticNode) -> String {
    let mut label = String::new();
    for part in node.name().into_iter().chain(node.text()) {
        label.push(' ');
        label.push_str(part.as_str());
    }
    label
}

/// The choice a banner control makes, from its name alone. An
/// acknowledgement must be the whole name; the others may be phrases in it.
pub(crate) fn choice(node: &SemanticNode) -> Option<Choice> {
    if !matches!(node.role(), SemanticRole::Button | SemanticRole::Link) {
        return None;
    }
    let name = label(node);
    if name.trim().is_empty() || name.len() > 80 {
        return None;
    }
    if names(&name, &REFUSE) {
        Some(Choice::Refuse)
    } else if names(&name, &SETTINGS) {
        Some(Choice::Settings)
    } else if names(&name, &ACCEPT) {
        Some(Choice::Accept)
    } else if exactly(&name, &ACKNOWLEDGE) {
        Some(Choice::Acknowledge)
    } else {
        None
    }
}

fn subtree_end(snapshot: &SemanticSnapshot, index: usize) -> usize {
    let depth = snapshot.nodes()[index].depth();
    snapshot
        .nodes()
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, next)| next.depth() <= depth)
        .map_or(snapshot.nodes().len(), |(end, _)| end)
}

/// A region is a banner when its own text is about cookies and it holds no
/// field the person types into.
fn banner(nodes: &[SemanticNode]) -> bool {
    nodes.iter().any(|part| names(&label(part), &TOPIC))
        && !nodes.iter().any(|part| {
            matches!(
                part.role(),
                SemanticRole::Textbox
                    | SemanticRole::Searchbox
                    | SemanticRole::Password
                    | SemanticRole::Spinbutton
            ) || part.sensitivity() == SemanticSensitivity::Secret
        })
}

/// The consent banner holding `node`, as its subtree's bounds: the nearest
/// small enclosing region whose own text is about cookies, or a whole small
/// page that is itself a consent page.
pub(crate) fn region(node: &SemanticNode, snapshot: &SemanticSnapshot) -> Option<(usize, usize)> {
    enclosing(node, snapshot).or_else(|| nearby(node, snapshot))
}

/// A fitted look may flatten a banner into the page: its text then sits
/// right beside its controls, in page order.
fn nearby(node: &SemanticNode, snapshot: &SemanticSnapshot) -> Option<(usize, usize)> {
    const REACH: usize = 12;
    // Only a control named exactly as a banner's is read this loosely.
    let name = label(node);
    if ![&REFUSE[..], &ACCEPT[..], &SETTINGS[..], &ACKNOWLEDGE[..]]
        .iter()
        .any(|phrases| exactly(&name, phrases))
    {
        return None;
    }
    let nodes = snapshot.nodes();
    let at = nodes
        .iter()
        .position(|candidate| candidate.reference() == node.reference())?;
    // The nearest cookie text before the control, or just after it, with no
    // field the person types into between them.
    let before = (at.saturating_sub(REACH)..at)
        .rev()
        .find(|index| names(&label(&nodes[*index]), &TOPIC));
    let after =
        (at + 1..(at + 4).min(nodes.len())).find(|index| names(&label(&nodes[*index]), &TOPIC));
    let (start, end) = match (before, after) {
        (Some(start), _) => (start, at + 1),
        (None, Some(end)) => (at, end + 1),
        (None, None) => return None,
    };
    banner(&nodes[start..end]).then_some((start, end))
}

fn enclosing(node: &SemanticNode, snapshot: &SemanticSnapshot) -> Option<(usize, usize)> {
    let mut parent = node.parent();
    while let Some(index) = parent {
        let index = usize::from(index);
        let ancestor = snapshot.nodes().get(index)?;
        let end = subtree_end(snapshot, index);
        let limit = if ancestor.role() == SemanticRole::Dialog {
            DIALOG_NODES
        } else {
            BANNER_NODES
        };
        if end - index <= limit && banner(&snapshot.nodes()[index..end]) {
            return Some((index, end));
        }
        if ancestor.role() == SemanticRole::Document {
            // A consent page: the document itself asks, and says so in a
            // heading, beside at most a few hundred nodes.
            let page = &snapshot.nodes()[index..end];
            let asks = page
                .iter()
                .any(|part| part.role() == SemanticRole::Heading && names(&label(part), &TOPIC))
                || snapshot
                    .frame()
                    .origin()
                    .as_url()
                    .host_str()
                    .is_some_and(|host| host.starts_with("consent."));
            return (asks && page.len() <= 400 && banner(page)).then_some((index, end));
        }
        parent = ancestor.parent();
    }
    None
}

/// A banner control's choice when it sits in a consent banner.
pub(crate) fn banner_choice(node: &SemanticNode, snapshot: &SemanticSnapshot) -> Option<Choice> {
    let choice = choice(node)?;
    region(node, snapshot).map(|_| choice)
}

/// Whether the banner holding `node` also offers a refusal.
pub(crate) fn offers_refusal(node: &SemanticNode, snapshot: &SemanticSnapshot) -> bool {
    region(node, snapshot).is_some_and(|(start, end)| {
        snapshot.nodes()[start..end]
            .iter()
            .any(|part| choice(part) == Some(Choice::Refuse))
    })
}

/// The control Rust clicks on the first view: a visible, enabled refusal in
/// a consent banner, else a plain acknowledgement there. Never an accept.
pub(crate) fn dismissal(observation: &SemanticObservation) -> Option<SemanticReferenceId> {
    let snapshot = observation.frames().first()?;
    snapshot
        .nodes()
        .iter()
        .filter(|node| {
            node.geometry().is_some()
                && !node.states().contains(SemanticState::Disabled)
                && node.operations().contains(SemanticOperationClass::Click)
                && node.sensitivity() == SemanticSensitivity::Public
        })
        .filter_map(|node| {
            let choice = banner_choice(node, snapshot)?;
            matches!(choice, Choice::Refuse | Choice::Acknowledge)
                .then_some((choice, node.reference()))
        })
        .min_by_key(|(choice, _)| *choice)
        .map(|(_, reference)| reference)
}

/// A look that may have cut a consent banner's controls off: the page is a
/// consent host, or a heading, dialog or short text is about cookies while no
/// refusal or acknowledgement shows.
pub(crate) fn suspected(observation: &SemanticObservation) -> bool {
    let Some(snapshot) = observation.frames().first() else {
        return false;
    };
    let host = snapshot
        .frame()
        .origin()
        .as_url()
        .host_str()
        .is_some_and(|host| host.starts_with("consent."));
    let topic = snapshot.nodes().iter().any(|node| {
        matches!(
            node.role(),
            SemanticRole::Heading | SemanticRole::Dialog | SemanticRole::Paragraph
        ) && label(node).len() <= 400
            && names(&label(node), &TOPIC)
    });
    let control = snapshot
        .nodes()
        .iter()
        .any(|node| matches!(choice(node), Some(Choice::Refuse | Choice::Acknowledge)));
    (host || topic) && !control
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page(nodes: Vec<serde_json::Value>) -> SemanticObservation {
        super::super::tests::reading_observation(json!(nodes), "complete")
    }
    fn node(observation: &SemanticObservation, key: usize) -> &SemanticNode {
        &observation.frames()[0].nodes()[key - 1]
    }
    fn at(x: u32) -> serde_json::Value {
        json!({"x": x, "y": 700, "w": 120, "h": 32})
    }

    fn airbnb() -> SemanticObservation {
        let mut nodes = vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Homes in San Francisco","l":1}),
            json!({"k":3,"p":0,"r":"group"}),
            json!({"k":4,"p":2,"r":"paragraph","t":"We use cookies and other technologies to personalize content."}),
            json!({"k":5,"p":2,"r":"group"}),
            json!({"k":6,"p":4,"r":"button","n":"Accept all","o":1,"ak":1,"b":at(10)}),
            json!({"k":7,"p":4,"r":"button","n":"Only necessary","o":1,"ak":1,"b":at(140)}),
            json!({"k":8,"p":4,"r":"button","n":"Manage preferences","o":1,"ak":1,"b":at(270)}),
            json!({"k":9,"p":0,"r":"button","n":"Accept invitation","o":1,"ak":1,"b":at(400)}),
        ];
        // A real results page: the banner is a small part of it.
        nodes.extend((10..100).map(|k| json!({"k":k,"p":0,"r":"paragraph","t":"Entire home"})));
        page(nodes)
    }

    #[test]
    fn a_cookie_banner_is_refused_by_rust_never_accepted() {
        let banner = airbnb();
        let snapshot = &banner.frames()[0];
        assert_eq!(dismissal(&banner), Some(node(&banner, 7).reference()));
        assert_eq!(
            banner_choice(node(&banner, 6), snapshot),
            Some(Choice::Accept)
        );
        assert_eq!(
            banner_choice(node(&banner, 8), snapshot),
            Some(Choice::Settings)
        );
        assert!(offers_refusal(node(&banner, 6), snapshot));
        // An accept outside any banner is no banner choice.
        assert_eq!(banner_choice(node(&banner, 9), snapshot), None);

        // A banner with only an accept is left alone.
        let accept_only = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"dialog","n":"Cookies"}),
            json!({"k":3,"p":1,"r":"paragraph","t":"This site uses cookies."}),
            json!({"k":4,"p":1,"r":"button","n":"Accept all cookies","o":1,"ak":1,"b":at(10)}),
        ]);
        assert_eq!(dismissal(&accept_only), None);
        // A notice acknowledged with OK is closed.
        let notice = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"group"}),
            json!({"k":3,"p":1,"r":"paragraph","t":"We are placing cookies on your device."}),
            json!({"k":4,"p":1,"r":"button","n":"Continue","o":1,"ak":1,"b":at(10)}),
        ]);
        assert_eq!(dismissal(&notice), Some(node(&notice, 4).reference()));
    }

    #[test]
    fn a_long_privacy_dialog_is_a_banner() {
        let mut nodes = vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"dialog"}),
            json!({"k":3,"p":1,"r":"heading","n":"We value your privacy","l":2}),
            json!({"k":4,"p":1,"r":"button","n":"Accept all","o":1,"ak":1,"b":at(10)}),
            json!({"k":5,"p":1,"r":"button","n":"Reject all","o":1,"ak":1,"b":at(140)}),
        ];
        nodes.extend((6..95).map(|k| json!({"k":k,"p":1,"r":"paragraph","t":"Purpose"})));
        nodes.extend((95..100).map(|k| json!({"k":k,"p":0,"r":"paragraph","t":"Flight"})));
        let kayak = page(nodes);
        assert_eq!(dismissal(&kayak), Some(node(&kayak, 5).reference()));
    }

    #[test]
    fn a_flattened_banner_is_read_from_its_neighbours() {
        let mut nodes = vec![json!({"k":1,"r":"document","o":16})];
        nodes.extend((2..40).map(|k| json!({"k":k,"p":0,"r":"paragraph","t":"Flight"})));
        nodes.push(json!({"k":40,"p":0,"r":"textbox","n":"From","o":2}));
        nodes.extend((41..70).map(|k| json!({"k":k,"p":0,"r":"paragraph","t":"Flight"})));
        nodes.push(
            json!({"k":70,"p":0,"r":"paragraph","t":"KAYAK and its partners wish to use cookies."}),
        );
        nodes.push(json!({"k":71,"p":0,"r":"button","n":"Accept all","o":1,"ak":1,"b":at(10)}));
        nodes.push(json!({"k":72,"p":0,"r":"button","n":"Reject all","o":1,"ak":1,"b":at(140)}));
        nodes.push(json!({"k":73,"p":0,"r":"textbox","n":"","o":2}));
        let kayak = page(nodes);
        assert_eq!(dismissal(&kayak), Some(node(&kayak, 72).reference()));
        // A control far from any cookie text is no banner's.
        assert_eq!(banner_choice(node(&kayak, 40), &kayak.frames()[0]), None);
    }

    #[test]
    fn a_consent_page_is_a_banner_and_a_form_with_fields_is_not() {
        let google = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Before you continue to Google","l":1}),
            json!({"k":3,"p":0,"r":"paragraph","t":"We use cookies and data to deliver and maintain Google services."}),
            json!({"k":4,"p":0,"r":"button","n":"Reject all","o":1,"ak":1,"b":at(10)}),
            json!({"k":5,"p":0,"r":"button","n":"Accept all","o":1,"ak":1,"b":at(140)}),
            json!({"k":6,"p":0,"r":"button","n":"More options","o":1,"ak":1,"b":at(270)}),
        ]);
        assert_eq!(dismissal(&google), Some(node(&google, 4).reference()));
        let google = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"We use cookies","l":1}),
            json!({"k":3,"p":0,"r":"button","n":"Reject all","o":1,"ak":1,"b":at(10)}),
            json!({"k":4,"p":0,"r":"button","n":"Accept all","o":1,"ak":1,"b":at(140)}),
        ]);
        assert_eq!(dismissal(&google), Some(node(&google, 3).reference()));
        let newsletter = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"group"}),
            json!({"k":3,"p":1,"r":"paragraph","t":"Get cookies recipes weekly."}),
            json!({"k":4,"p":1,"r":"textbox","n":"Email","o":2}),
            json!({"k":5,"p":1,"r":"button","n":"Continue","o":1,"ak":1,"b":at(10)}),
        ]);
        assert_eq!(dismissal(&newsletter), None);
    }

    #[test]
    fn a_look_that_cut_off_a_consent_banners_buttons_is_suspected() {
        // Google's consent page, fitted: the text, not yet the buttons.
        let cut = page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Before you continue to Google","l":1}),
            json!({"k":3,"p":0,"r":"paragraph","t":"We use cookies and data to deliver and maintain Google services."}),
            json!({"k":4,"p":0,"r":"paragraph","t":"Track outages and protect against spam."}),
        ]);
        assert_eq!(dismissal(&cut), None);
        assert!(suspected(&cut));
        // Its refusal in view: the ordinary dismissal takes it.
        assert!(!suspected(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"paragraph","t":"We use cookies to improve this site."}),
            json!({"k":3,"p":0,"r":"button","n":"Reject all","o":1,"ak":1,"b":at(10)}),
        ])));
        // A page about something else is not.
        assert!(!suspected(&page(vec![
            json!({"k":1,"r":"document","o":16}),
            json!({"k":2,"p":0,"r":"heading","n":"Homes in San Francisco","l":1}),
            json!({"k":3,"p":0,"r":"paragraph","t":"Entire home"}),
        ])));
    }
}
