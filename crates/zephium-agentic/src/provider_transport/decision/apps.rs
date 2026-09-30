//! A person's daily apps read as records at once: the view's main list
//! (Slack's messages, Gmail's rows, Linear's issues, GitHub's notifications,
//! Notion's pages) or Calendar's events, each row's own words cited to its
//! nodes. No provider call; a view whose rows cannot be found is left to the
//! page planner.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::projection::document_metadata;
use super::read::{copied_text, DecisionLocatedRead, ReadProjection};
use crate::*;

/// Rows a read keeps, the findings list's own bound.
const MAX_ROWS: usize = 16;
/// Nodes each row cites, the extraction's per-value bound.
const ROW_SOURCES: usize = 4;
/// Text one row keeps, under the list item's byte bound.
const ROW_BYTES: usize = 900;
/// Below this a row is a label (a channel name, a folder), not a record.
const MIN_ROW_CHARS: usize = 16;

/// The daily app a page is on, by its host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DailyApp {
    /// app.slack.com
    Slack,
    /// mail.google.com
    Gmail,
    /// calendar.google.com
    Calendar,
    /// linear.app
    Linear,
    /// app.notion.com, notion.so
    Notion,
    /// github.com
    GitHub,
}

impl DailyApp {
    /// The app a host belongs to, if it is one of them.
    pub fn of(host: &str) -> Option<Self> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let on = |site: &str| host == site || host.ends_with(&format!(".{site}"));
        Some(match host.as_str() {
            "app.slack.com" => Self::Slack,
            "mail.google.com" => Self::Gmail,
            "calendar.google.com" => Self::Calendar,
            "github.com" => Self::GitHub,
            _ if on("linear.app") => Self::Linear,
            _ if on("notion.so") || on("notion.com") => Self::Notion,
            _ => return None,
        })
    }

    /// The app's own lowercase name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Slack => "slack",
            Self::Gmail => "gmail",
            Self::Calendar => "calendar",
            Self::Linear => "linear",
            Self::Notion => "notion",
            Self::GitHub => "github",
        }
    }
}

/// A view of a daily app that lists what is new for the person, opened by
/// its own control in the app's rail or sidebar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppView {
    /// Slack's Activity: mentions, threads and reactions.
    SlackActivity,
    /// Slack's DMs: each conversation with its latest message.
    SlackDms,
    /// Slack's Home: the open channel's latest messages.
    SlackHome,
    /// Linear's Inbox: notifications about the person's issues.
    LinearInbox,
    /// Linear's My issues: the issues assigned to the person.
    LinearMyIssues,
}

impl AppView {
    /// The view's own name, as its app shows it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::SlackActivity => "Activity",
            Self::SlackDms => "DMs",
            Self::SlackHome => "Home",
            Self::LinearInbox => "Inbox",
            Self::LinearMyIssues => "My issues",
        }
    }

    const fn labels(self) -> &'static [&'static str] {
        match self {
            Self::SlackActivity => &["activity"],
            Self::SlackDms => &["dms", "direct messages"],
            Self::SlackHome => &["home"],
            Self::LinearInbox => &["inbox"],
            Self::LinearMyIssues => &["my issues"],
        }
    }

    /// The last view read when the others list nothing: what is there now.
    pub const fn latest(self) -> bool {
        matches!(self, Self::SlackHome | Self::LinearMyIssues)
    }
}

/// The views a view read goes through, in order, for what its goal asks: a
/// person's new messages or issues first, then the latest ones. None when
/// the goal names its own place (a channel), which the page itself shows.
pub fn app_views(app: DailyApp, goal: &str) -> &'static [AppView] {
    use AppView::*;
    let goal = goal.to_ascii_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| goal.contains(word));
    match app {
        DailyApp::Slack if says(&["#", " channel "]) && !says(&["unread", "new", "activity"]) => {
            &[]
        }
        DailyApp::Slack if says(&["dm", "direct message"]) => &[SlackDms, SlackActivity, SlackHome],
        DailyApp::Slack => &[SlackActivity, SlackDms, SlackHome],
        DailyApp::Linear
            if says(&["my issues", "assigned"]) && !says(&["inbox", "notification"]) =>
        {
            &[LinearMyIssues, LinearInbox]
        }
        DailyApp::Linear => &[LinearInbox, LinearMyIssues],
        _ => &[],
    }
}

/// The control that opens a view: a rail button, sidebar link, tab or
/// tree row named for it, outside any dialog, that can be clicked.
pub fn app_view_control(
    observation: &SemanticObservation,
    view: AppView,
) -> Option<SemanticReferenceId> {
    let frame = observation.frames().first()?;
    let nodes = frame.nodes();
    let in_dialog = |index: usize| {
        let mut cursor = nodes[index].parent().map(usize::from);
        for _ in 0..nodes.len() {
            let Some(at) = cursor.filter(|at| *at < nodes.len()) else {
                return false;
            };
            if nodes[at].role() == SemanticRole::Dialog {
                return true;
            }
            cursor = nodes[at].parent().map(usize::from);
        }
        false
    };
    nodes
        .iter()
        .enumerate()
        .find(|(index, node)| {
            matches!(
                node.role(),
                SemanticRole::Button
                    | SemanticRole::Link
                    | SemanticRole::Tab
                    | SemanticRole::MenuItem
                    | SemanticRole::Option
                    | SemanticRole::ListItem
            ) && node.operations().contains(SemanticOperationClass::Click)
                && !node.states().contains(SemanticState::Disabled)
                && node.sensitivity() == SemanticSensitivity::Public
                && [node.name(), node.text()]
                    .into_iter()
                    .flatten()
                    .any(|words| names_view(words.as_str(), view))
                && !in_dialog(*index)
        })
        .map(|(_, node)| node.reference())
}

/// "Activity", "Inbox 3", "DMs, 2 unread": a control named for the view.
fn names_view(words: &str, view: AppView) -> bool {
    let words = words.trim().to_ascii_lowercase();
    view.labels().iter().any(|label| {
        words == *label
            || words
                .strip_prefix(label)
                .is_some_and(|rest| rest.starts_with([' ', ',', '(', '·']))
                && words.len() <= label.len() + 24
    })
}

/// One record the view shows: its root and its text-bearing nodes.
struct Row {
    nodes: Vec<(SemanticReferenceId, String)>,
    /// Some node of the row names when it happened.
    timed: bool,
    /// Some node of the row names an issue by its key (ENG-142).
    keyed: bool,
}

impl Row {
    fn text(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for (_, text) in &self.nodes {
            let text = text.trim();
            if !text.is_empty() && !parts.iter().any(|seen| seen.contains(text)) {
                parts.retain(|seen| !text.contains(seen));
                parts.push(text);
            }
        }
        clip(&parts.join(" · "), ROW_BYTES)
    }
}

fn clip(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_owned();
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end().to_owned()
}

/// The records of an app's view, read from the observation alone: its main
/// list, or a calendar's events when the view has no list of records.
fn rows(observation: &SemanticObservation, app: Option<DailyApp>) -> Vec<Row> {
    let mut lists: Option<(usize, Vec<Row>)> = None;
    let mut events: Option<(usize, Vec<Row>)> = None;
    for frame in observation.frames() {
        let nodes = frame.nodes();
        let parents: Vec<Option<usize>> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                node.parent()
                    .map(usize::from)
                    .filter(|parent| *parent < nodes.len() && *parent != index)
            })
            .collect();
        let inside = |index: usize, root: usize| {
            let mut cursor = Some(index);
            for _ in 0..nodes.len() {
                match cursor {
                    Some(at) if at == root => return true,
                    Some(at) => cursor = parents[at],
                    None => return false,
                }
            }
            false
        };
        let readable = |index: usize| {
            let node = &nodes[index];
            (node.sensitivity() == SemanticSensitivity::Public && !document_metadata(node))
                .then(|| copied_text(node))
                .flatten()
                .map(str::to_owned)
        };
        let row_of = |root: usize| {
            let members: Vec<usize> = (root..nodes.len())
                .take_while(|index| *index == root || inside(*index, root))
                .collect();
            let words = || {
                members.iter().flat_map(|index| {
                    let node = &nodes[*index];
                    [node.name(), node.text()]
                        .into_iter()
                        .flatten()
                        .map(SemanticText::as_str)
                        .filter(move |_| node.sensitivity() == SemanticSensitivity::Public)
                })
            };
            Row {
                nodes: members
                    .iter()
                    .filter_map(|index| {
                        readable(*index).map(|text| (nodes[*index].reference(), text))
                    })
                    .collect(),
                timed: words().any(|text| names_a_time(text) || names_a_day(text)),
                keyed: words().any(names_an_issue),
            }
        };
        let in_dialog = |index: usize| {
            let mut cursor = parents[index];
            while let Some(at) = cursor {
                if nodes[at].role() == SemanticRole::Dialog {
                    return true;
                }
                cursor = parents[at];
            }
            false
        };
        let timed: Vec<Row> = nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| {
                matches!(node.role(), SemanticRole::Button | SemanticRole::Link)
                    && !in_dialog(*index)
                    && readable(*index).is_some_and(|text| names_a_time(&text))
            })
            .map(|(index, _)| row_of(index))
            .collect();
        if timed.len() > events.as_ref().map_or(0, |(count, _)| *count) {
            events = Some((timed.len(), timed));
        }
        let mut groups: BTreeMap<(usize, u8), Vec<usize>> = BTreeMap::new();
        for (index, node) in nodes.iter().enumerate() {
            let role = node.role();
            if !matches!(
                role,
                SemanticRole::ListItem
                    | SemanticRole::Row
                    | SemanticRole::Document
                    | SemanticRole::Option
                    | SemanticRole::Link
            ) || in_dialog(index)
            {
                continue;
            }
            if let Some(parent) = parents[index] {
                groups.entry((parent, role as u8)).or_default().push(index);
            }
        }
        for group in groups.into_values().filter(|rows| rows.len() >= 2) {
            let rows: Vec<Row> = group
                .into_iter()
                .map(row_of)
                .filter(|row| row.text().chars().count() >= MIN_ROW_CHARS)
                .filter(|row| record_of(app, row))
                .collect();
            if rows.len() < 2 {
                continue;
            }
            let score: usize = rows.iter().map(|row| row.text().len().min(300)).sum();
            if score > lists.as_ref().map_or(0, |(best, _)| *best) {
                lists = Some((score, rows));
            }
        }
    }
    let mut rows = match (app, lists, events) {
        (Some(DailyApp::Calendar), _, Some((_, events))) | (_, None, Some((_, events))) => events,
        (_, Some((_, rows)), _) => rows,
        _ => Vec::new(),
    };
    // A chat's newest messages are at the bottom of its view.
    if app == Some(DailyApp::Slack) && rows.len() > MAX_ROWS {
        rows.drain(..rows.len() - MAX_ROWS);
    }
    rows.truncate(MAX_ROWS);
    rows
}

/// A row the app shows as a record: a Slack message or activity names when
/// it happened (a welcome or promo card does not); a Linear row names its
/// issue's key. Other apps' rows stand as they are.
fn record_of(app: Option<DailyApp>, row: &Row) -> bool {
    match app {
        Some(DailyApp::Slack) => row.timed,
        Some(DailyApp::Linear) => row.keyed,
        _ => true,
    }
}

/// "Today", "Yesterday", "Sep 28", "Monday", "2h", "5 minutes ago": the
/// words of a day or an age.
fn names_a_day(text: &str) -> bool {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    const DAYS: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words.iter().enumerate().any(|(at, word)| {
        let next_is_day = words
            .get(at + 1)
            .is_some_and(|next| next.len() <= 2 && next.bytes().all(|b| b.is_ascii_digit()));
        matches!(*word, "today" | "yesterday" | "ago")
            || (*word == "just" && words.get(at + 1) == Some(&"now"))
            || DAYS.contains(word)
            || (MONTHS.iter().any(|month| word.starts_with(month))
                && word.len() <= 9
                && next_is_day)
            || (word.len() >= 2
                && word.len() <= 3
                && matches!(word.as_bytes()[word.len() - 1], b'm' | b'h' | b'd' | b'w')
                && word[..word.len() - 1].bytes().all(|b| b.is_ascii_digit()))
    })
}

/// "ENG-142", "LIN-7": an issue tracker's key.
fn names_an_issue(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .any(|word| {
            let Some((team, number)) = word.split_once('-') else {
                return false;
            };
            (2..=7).contains(&team.len())
                && team.as_bytes()[0].is_ascii_uppercase()
                && team
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                && (1..=6).contains(&number.len())
                && number.bytes().all(|b| b.is_ascii_digit())
        })
}

/// "10:30", "10am", "3:15 PM", "14.00": the words of an event's time.
fn names_a_time(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(at, byte)| {
        if !byte.is_ascii_digit() || (at > 0 && bytes[at - 1].is_ascii_digit()) {
            return false;
        }
        let rest = &text[at..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 2 {
            return false;
        }
        let after = rest[digits..].trim_start().to_ascii_lowercase();
        let clock = after.len() >= 3
            && matches!(after.as_bytes()[0], b':' | b'.')
            && after.as_bytes()[1].is_ascii_digit()
            && after.as_bytes()[2].is_ascii_digit();
        clock || after.starts_with("am") || after.starts_with("pm")
    })
}

/// Words an app shows when a view has nothing in it.
const EMPTY_VIEW: [&str; 14] = [
    "all caught up",
    "caught up",
    "nothing new",
    "nothing here",
    "nothing to see",
    "no new",
    "no unread",
    "no issues",
    "no notifications",
    "no messages",
    "no results",
    "inbox zero",
    "quiet for now",
    "you have no",
];

/// The one line a view shows when it lists nothing ("All caught up"), as a
/// record of its own, so an empty view is an answer.
fn empty_view(observation: &SemanticObservation) -> Option<Row> {
    let frame = observation.frames().first()?;
    let nodes = frame.nodes();
    nodes.iter().enumerate().find_map(|(index, node)| {
        if node.sensitivity() != SemanticSensitivity::Public
            || !matches!(
                node.role(),
                SemanticRole::ListItem
                    | SemanticRole::Paragraph
                    | SemanticRole::Heading
                    | SemanticRole::Status
                    | SemanticRole::Group
            )
            || document_metadata(node)
        {
            return None;
        }
        let mut cursor = node.parent().map(usize::from);
        while let Some(at) = cursor.filter(|at| *at < index) {
            if nodes[at].role() == SemanticRole::Dialog {
                return None;
            }
            cursor = nodes[at].parent().map(usize::from);
        }
        let text = copied_text(node)?;
        let lower = text.to_ascii_lowercase();
        (text.len() <= ROW_BYTES && EMPTY_VIEW.iter().any(|words| lower.contains(words))).then(
            || Row {
                nodes: vec![(node.reference(), text.to_owned())],
                timed: false,
                keyed: false,
            },
        )
    })
}

/// The view's records as a findings read: each row's words, cited to its own
/// nodes. The last view a read goes through answers with its empty-state
/// line when it lists nothing. None when the task reads into columns, or the
/// view shows no records.
pub fn read_app_view<'a>(
    observation: &'a SemanticObservation,
    account: AgentContextAccountBinding,
    captured_at: SemanticCaptureInstant,
    schema: &SemanticExtractionSchema,
    app: Option<DailyApp>,
    last: bool,
) -> Option<DecisionLocatedRead<'a>> {
    let projection = ReadProjection::for_schema(schema)?;
    let [field] = projection.columns.as_slice() else {
        return None;
    };
    if projection.row.is_some()
        || field.kind() != SemanticExtractionValueKind::TextList
        || field.verbatim_text()
    {
        return None;
    }
    let mut rows = rows(observation, app);
    if rows.is_empty() && last {
        rows.extend(empty_view(observation));
    }
    if rows.is_empty() {
        return None;
    }
    let cited: Vec<Vec<SemanticReferenceId>> = rows
        .iter()
        .map(|row| {
            let mut nodes: Vec<&(SemanticReferenceId, String)> = row.nodes.iter().collect();
            nodes.sort_by_key(|(_, text)| std::cmp::Reverse(text.len()));
            nodes
                .into_iter()
                .take(ROW_SOURCES)
                .map(|(reference, _)| *reference)
                .collect()
        })
        .collect();
    let all: BTreeSet<SemanticReferenceId> = cited.iter().flatten().copied().collect();
    let baseline = SemanticObservationAcknowledgement::from_fingerprint(
        crate::semantic_diff::SemanticObservationFingerprint::from_observation(observation),
    );
    let read = crate::semantic_read::read_located_semantic_observation_at(
        observation,
        &baseline,
        captured_at,
        &projection.schema,
        &all,
        None,
    )
    .ok()?;
    let token = |reference: SemanticReferenceId| {
        read.fragments()
            .iter()
            .find(|fragment| {
                fragment.provenance().reference() == reference
                    && matches!(
                        fragment.field(),
                        SemanticReadField::VisibleText | SemanticReadField::AccessibleName
                    )
            })
            .map(|fragment| fragment.id().model_token().to_string())
    };
    let limit = field.max_text_bytes().unwrap_or(ROW_BYTES).min(ROW_BYTES);
    let mut items: Vec<Value> = Vec::new();
    let mut list_sources: Vec<String> = Vec::new();
    for (row, nodes) in rows.iter().zip(&cited) {
        let sources: Vec<String> = nodes.iter().filter_map(|node| token(*node)).collect();
        let text = clip(&row.text(), limit);
        if sources.is_empty() || text.is_empty() {
            continue;
        }
        if list_sources.len() < ROW_SOURCES && !list_sources.contains(&sources[0]) {
            list_sources.push(sources[0].clone());
        }
        items.push(json!({"value": text, "sources": sources}));
        if items.len() == field.max_list_items().unwrap_or(MAX_ROWS) {
            break;
        }
    }
    if items.is_empty() {
        return None;
    }
    let mut copied = BTreeMap::new();
    copied.insert(
        field.name().to_owned(),
        json!({"k": "text_list", "items": items, "sources": list_sources}),
    );
    Some(DecisionLocatedRead {
        projection,
        baseline,
        account,
        read,
        copied,
        generation: None,
        rows: None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::capture::observation_of;
    use super::*;

    fn texts(rows: &[Row]) -> Vec<String> {
        rows.iter().map(Row::text).collect()
    }

    /// A real app view the QA build captured, names and words replaced.
    fn captured(fixture: &str) -> SemanticObservation {
        let record: Value = serde_json::from_str(fixture).unwrap();
        observation_of(record["url"].as_str().unwrap(), &record["snapshot"])
    }

    #[test]
    fn a_real_slack_channel_reads_its_messages_under_its_welcome_card() {
        let look = captured(include_str!("fixtures/slack-channel.json"));
        let rows = rows(&look, Some(DailyApp::Slack));
        let read = texts(&rows);
        assert_eq!(read.len(), 3, "{read:?}");
        assert!(read[0].contains("joined #general"), "{read:?}");
        assert!(read[1].contains("Message text one"), "{read:?}");
        assert!(read[2].contains("Message text two"), "{read:?}");
        assert!(!read.iter().any(|row| row.contains("handbook")), "{read:?}");
        for view in [
            AppView::SlackActivity,
            AppView::SlackDms,
            AppView::SlackHome,
        ] {
            assert!(app_view_control(&look, view).is_some(), "{view:?}");
        }
    }

    fn read(look: &SemanticObservation, last: bool) -> Option<usize> {
        let schema = SemanticExtractionSchema::try_new(
            SemanticExtractionSchemaId::new(7).unwrap(),
            vec![
                SemanticExtractionFieldSchema::try_text_list("output_0".into(), true, 16, 1024)
                    .unwrap(),
            ],
        )
        .unwrap();
        let account = AgentContextAccountBinding::new(
            AgentAccountAttestationId::from_raw(14),
            look.request().context(),
            AgentAccountScope::Anonymous,
            AgentPolicyInstant::from_millis(100),
        );
        read_app_view(
            look,
            account,
            SemanticCaptureInstant::from_millis(200),
            &schema,
            Some(DailyApp::Slack),
            last,
        )
        .map(|located| located.rows_read())
    }

    #[test]
    fn a_real_empty_slack_activity_is_all_caught_up_only_when_it_is_the_last_view() {
        let look = captured(include_str!("fixtures/slack-activity.json"));
        assert!(rows(&look, Some(DailyApp::Slack)).is_empty());
        let empty = empty_view(&look).expect("the empty state");
        assert!(
            empty.text().starts_with("All caught up"),
            "{}",
            empty.text()
        );
        assert_eq!(read(&look, false), None);
        assert_eq!(read(&look, true), Some(1));
        let channel = captured(include_str!("fixtures/slack-channel.json"));
        assert_eq!(read(&channel, false), Some(3));
    }

    #[test]
    fn a_real_linear_home_opens_its_inbox_and_my_issues_from_the_sidebar() {
        let look = captured(include_str!("fixtures/linear-home.json"));
        assert!(rows(&look, Some(DailyApp::Linear)).is_empty());
        for view in [AppView::LinearInbox, AppView::LinearMyIssues] {
            assert!(app_view_control(&look, view).is_some(), "{view:?}");
        }
        let booting = captured(include_str!("fixtures/linear-loading.json"));
        assert_eq!(app_view_control(&booting, AppView::LinearInbox), None);
    }

    #[test]
    fn slack_reads_messages_and_leaves_welcome_cards() {
        let page = json!({"v": SEMANTIC_WIRE_VERSION, "c": "complete", "n": [
            {"k": 1, "r": "document", "n": "general (Channel) - Acme - Slack"},
            {"k": 2, "p": 0, "r": "button", "n": "Activity", "o": 1},
            {"k": 3, "p": 0, "r": "list", "n": "Get started"},
            {"k": 4, "p": 2, "r": "list_item"},
            {"k": 5, "p": 3, "r": "paragraph", "t": "Add your company handbook to a canvas"},
            {"k": 6, "p": 2, "r": "list_item"},
            {"k": 7, "p": 5, "r": "paragraph", "t": "Invite your teammates to collaborate here"},
            {"k": 8, "p": 2, "r": "list_item"},
            {"k": 9, "p": 7, "r": "paragraph", "t": "Connect the tools your team already uses daily"},
            {"k": 10, "p": 0, "r": "list", "n": "general (channel)"},
            {"k": 11, "p": 9, "r": "list_item"},
            {"k": 12, "p": 10, "r": "button", "t": "Person A", "o": 1},
            {"k": 13, "p": 10, "r": "link", "n": "Today at 9:14:02 AM", "t": "9:14 AM", "o": 1},
            {"k": 14, "p": 10, "r": "paragraph", "t": "Message text one"},
            {"k": 15, "p": 9, "r": "list_item"},
            {"k": 16, "p": 14, "r": "button", "t": "Person B", "o": 1},
            {"k": 17, "p": 14, "r": "link", "n": "Yesterday at 5:48:10 PM", "t": "5:48 PM", "o": 1},
            {"k": 18, "p": 14, "r": "paragraph", "t": "Message text two"},
        ]});
        let look = observation_of("https://app.slack.com/client/T1/C2", &page);
        let rows = rows(&look, Some(DailyApp::Slack));
        assert_eq!(
            texts(&rows),
            [
                "Person A · 9:14 AM · Message text one",
                "Person B · 5:48 PM · Message text two"
            ]
        );
        assert!(app_view_control(&look, AppView::SlackActivity).is_some());
        assert_eq!(app_view_control(&look, AppView::SlackDms), None);
    }

    #[test]
    fn linear_reads_issue_rows_by_their_key() {
        let page = json!({"v": SEMANTIC_WIRE_VERSION, "c": "complete", "n": [
            {"k": 1, "r": "document", "n": "My issues"},
            {"k": 2, "p": 0, "r": "link", "n": "Inbox 3", "o": 1},
            {"k": 3, "p": 0, "r": "link", "n": "My issues", "o": 1},
            {"k": 4, "p": 0, "r": "group"},
            {"k": 5, "p": 3, "r": "link", "t": "ENG-142 Offline sync loses edits after a conflict", "o": 1},
            {"k": 6, "p": 3, "r": "link", "t": "ENG-151 Settings page flickers on resize", "o": 1},
            {"k": 7, "p": 3, "r": "link", "t": "Try Linear Asks for your whole team", "o": 1},
        ]});
        let look = observation_of("https://linear.app/acme/my-issues/assigned", &page);
        assert_eq!(texts(&rows(&look, Some(DailyApp::Linear))).len(), 2);
        assert!(app_view_control(&look, AppView::LinearInbox).is_some());
    }

    #[test]
    fn a_goal_chooses_the_views_it_reads_first() {
        use AppView::*;
        assert_eq!(
            app_views(DailyApp::Slack, "What's new in my Slack"),
            [SlackActivity, SlackDms, SlackHome]
        );
        assert_eq!(
            app_views(DailyApp::Slack, "Unread direct messages")[0],
            SlackDms
        );
        assert!(app_views(DailyApp::Slack, "Summarise #launch today").is_empty());
        assert_eq!(
            app_views(DailyApp::Linear, "What's in my Linear")[0],
            LinearInbox
        );
        assert_eq!(
            app_views(DailyApp::Linear, "Issues assigned to me")[0],
            LinearMyIssues
        );
        assert!(app_views(DailyApp::Gmail, "Unread mail").is_empty());
        for day in [
            "Today",
            "Yesterday at 5:48 PM",
            "Sep 28",
            "Monday",
            "2h",
            "5 minutes ago",
        ] {
            assert!(names_a_day(day), "{day}");
        }
        for other in ["Add company handbook", "Say hi", "Home", "Channel 2"] {
            assert!(!names_a_day(other), "{other}");
        }
        assert!(names_an_issue("ENG-142 Offline sync"));
        assert!(!names_an_issue("Wi-Fi e-mail COVID-19x"));
    }

    #[test]
    fn daily_apps_are_known_by_host_and_events_by_their_time() {
        assert_eq!(DailyApp::of("app.slack.com"), Some(DailyApp::Slack));
        assert_eq!(DailyApp::of("acme.linear.app"), Some(DailyApp::Linear));
        assert_eq!(DailyApp::of("www.notion.so"), Some(DailyApp::Notion));
        assert_eq!(DailyApp::of("app.notion.com"), Some(DailyApp::Notion));
        assert_eq!(DailyApp::of("slack.com"), None);
        for event in [
            "10:30 – 11:00, Standup",
            "3pm Design review",
            "14.00 Gym",
            "9 AM, Call",
        ] {
            assert!(names_a_time(event), "{event}");
        }
        for other in ["Tuesday 3", "Room 101", "2026 plans", "Create"] {
            assert!(!names_a_time(other), "{other}");
        }
    }
}
