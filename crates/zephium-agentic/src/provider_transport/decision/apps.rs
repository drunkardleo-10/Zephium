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
    /// notion.so
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
            _ if on("notion.so") => Self::Notion,
            _ => return None,
        })
    }
}

/// One record the view shows: its root and its text-bearing nodes.
struct Row {
    nodes: Vec<(SemanticReferenceId, String)>,
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
        let row_of = |root: usize| Row {
            nodes: (root..nodes.len())
                .take_while(|index| *index == root || inside(*index, root))
                .filter_map(|index| readable(index).map(|text| (nodes[index].reference(), text)))
                .collect(),
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

/// The view's records as a findings read: each row's words, cited to its own
/// nodes. None when the task reads into columns, or the view shows no
/// records.
pub fn read_app_view<'a>(
    observation: &'a SemanticObservation,
    account: AgentContextAccountBinding,
    captured_at: SemanticCaptureInstant,
    schema: &SemanticExtractionSchema,
    app: Option<DailyApp>,
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
    let rows = rows(observation, app);
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
    use super::*;

    #[test]
    fn daily_apps_are_known_by_host_and_events_by_their_time() {
        assert_eq!(DailyApp::of("app.slack.com"), Some(DailyApp::Slack));
        assert_eq!(DailyApp::of("acme.linear.app"), Some(DailyApp::Linear));
        assert_eq!(DailyApp::of("www.notion.so"), Some(DailyApp::Notion));
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
