//! The objects the lead agent makes: typed, bounded data the canvas sets in
//! designed shapes. Limits count characters, never truncate, and come back to
//! the model as closed faults naming the field so it can correct itself.
use super::artifact::{WorkArtifactField as F, CODE_LANGUAGES};
use serde::{Deserialize, Serialize};

pub const MAX_PICKS: usize = 12;
pub const MAX_PLAN_STEPS: usize = 40;
pub const MAX_LIST_ITEMS: usize = 40;
pub const MAX_SHEET_COLUMNS: usize = 10;
pub const MAX_SHEET_ROWS: usize = 200;
pub const MAX_PLOT_SERIES: usize = 8;
pub const MAX_PLOT_POINTS: usize = 60;
pub const MAX_DIFF_HUNKS: usize = 40;
pub const MAX_DIFF_HUNK_LINES: usize = 400;
pub const MAX_DRAFT_BODY_CHARS: usize = 4000;
pub const MAX_PROJECT_STACK: usize = 16;
pub const MAX_PROJECT_TREE: usize = 80;
pub const MAX_PROJECT_TREE_DEPTH: usize = 3;
pub const MAX_PROJECT_SCRIPTS: usize = 16;
/// Diagram limits for objects a lead run makes; saved diagrams keep theirs.
pub const MAX_LEAD_DIAGRAM_NODES: usize = 24;
pub const MAX_LEAD_DIAGRAM_LAYERS: usize = 6;

macro_rules! closed {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        #[cfg_attr(feature = "ipc-types", derive(specta::Type))]
        #[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),* }
    };
}

closed!(WorkPickFacetV1 {
    Stay,
    Flight,
    Product,
    Place,
    Restaurant,
    Job,
    Course,
    Video,
    Repo,
    Service,
    Company,
    Person,
    Event,
    Article,
    Other,
});
closed!(WorkFactKindV1 {
    Text,
    Yes,
    No,
    Partial,
    Rating
});
closed!(WorkPlanStepKindV1 {
    Travel,
    Stay,
    Event,
    Task,
    Milestone,
    Note
});
closed!(WorkListStyleV1 {
    Todo,
    Messages,
    Reading,
    Requirements
});
closed!(WorkListPriorityV1 { High });
closed!(WorkSheetColumnKindV1 {
    Text,
    Number,
    Money,
    Percent,
    Date,
    Duration,
    YesNo,
    Rating,
    Link,
    Entity,
    Tag,
});
closed!(WorkSheetBestV1 { Max, Min });
closed!(WorkPlotStyleV1 {
    Bar,
    BarHorizontal,
    BarStacked,
    BarGrouped,
    Line,
    Area,
    AreaStacked,
    Donut,
    Radial,
    Radar,
    Range,
});
closed!(WorkPlotAxisKindV1 {
    Category,
    Time,
    Linear
});
closed!(WorkPlotFormatV1 {
    Number,
    Money,
    Duration,
    Percent,
    Bytes
});
closed!(WorkDiffOpV1 { Ctx, Add, Del });
closed!(WorkDraftDestinationV1 {
    Slack,
    Email,
    Linkedin,
    X,
    Github,
    Message
});
closed!(WorkMediaKindV1 {
    Image,
    Video,
    Audio
});
closed!(WorkProjectEntryKindV1 { Folder, File });
closed!(WorkMediaProviderV1 {
    Youtube,
    Vimeo,
    File
});

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFigureV1 {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPickV1 {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Public HTTPS pictures of the subject itself, from its sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_candidates: Vec<String>,
    /// A bare public host whose logo stands for the subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<WorkPriceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<WorkPickFactV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<WorkRatingV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recommended: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<WorkRouteV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    /// Index into the artifact's evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u16>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPriceV1 {
    /// As a person reads it: "$1,240 total", "€89 / night".
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPickFactV1 {
    pub label: String,
    pub value: String,
    pub kind: WorkFactKindV1,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkRatingV1 {
    pub value: String,
    pub max: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkRouteV1 {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depart: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrive: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    pub stops: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_host: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlanStepV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: WorkPlanStepKindV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// A pick this step stands for, in a picks object of the same work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<WorkPickRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u16>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPickRefV1 {
    pub artifact: super::WorkArtifactId,
    pub index: u16,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkLabelledV1 {
    pub label: String,
    pub value: String,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkListItemV1 {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<WorkListPriorityV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<WorkListFromV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u16>,
}
/// Where an item came from: the app or site, who, when, their words.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkListFromV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSheetColumnV1 {
    pub label: String,
    pub kind: WorkSheetColumnKindV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best: Option<WorkSheetBestV1>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSheetRowV1 {
    /// One per column, typed by it; empty reads as unknown.
    pub cells: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<WorkSheetEntityV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u16>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkSheetEntityV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlotXV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub kind: WorkPlotAxisKindV1,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlotYV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub format: WorkPlotFormatV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlotSeriesV1 {
    pub name: String,
    pub points: Vec<WorkPlotPointV1>,
}
/// Decimal strings keep values exact; a missing `y` is a gap.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPlotPointV1 {
    pub x: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y2: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDiffHunkV1 {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<WorkDiffLineV1>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkDiffLineV1 {
    pub op: WorkDiffOpV1,
    pub text: String,
}

/// One technology the project uses, read from a manifest.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProjectStackV1 {
    /// "SvelteKit", "Rust", "Tauri".
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// What it is for here: "Frontend", "Desktop shell", "Language".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// A bare public host whose logo marks it: svelte.dev.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The manifest it was read from, relative to the project root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
}
/// One folder or file of the project's structure, parents before children.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProjectEntryV1 {
    /// Relative to the root, `/`-separated, at most three names deep.
    pub path: String,
    pub kind: WorkProjectEntryKindV1,
    /// Entries the folder holds beyond those listed under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more: Option<u32>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProjectScriptV1 {
    /// "dev", "test", "build".
    pub name: String,
    /// What it runs, exactly as the project defines it.
    pub command: String,
    /// Where it is defined: "package.json", "Makefile".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkProjectGitV1 {
    /// Absent on a detached head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Files changed, staged or untracked.
    pub changed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
}

// Content types print no content: a log line never carries page or model text.
macro_rules! redacted {
    ($($name:ident),+ $(,)?) => { $(impl std::fmt::Debug for $name {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(concat!(stringify!($name), "([content redacted])"))
        }
    })+ };
}
redacted!(
    WorkFigureV1,
    WorkPickV1,
    WorkPriceV1,
    WorkPickFactV1,
    WorkRatingV1,
    WorkRouteV1,
    WorkPlanStepV1,
    WorkPickRefV1,
    WorkLabelledV1,
    WorkListItemV1,
    WorkListFromV1,
    WorkSheetColumnV1,
    WorkSheetRowV1,
    WorkSheetEntityV1,
    WorkPlotXV1,
    WorkPlotYV1,
    WorkPlotSeriesV1,
    WorkPlotPointV1,
    WorkDiffHunkV1,
    WorkDiffLineV1,
    WorkProjectStackV1,
    WorkProjectEntryV1,
    WorkProjectScriptV1,
    WorkProjectGitV1,
);

/// The first part of an object that failed, and the item it sits in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkObjectFault {
    pub field: F,
    /// Zero-based position of the item, row, step, series or hunk.
    pub index: Option<u16>,
    /// What the refused text measured: its characters, or a line break.
    pub found: Option<WorkTextFound>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkTextFound {
    Characters(u32),
    LineBreak,
    Empty,
}
impl WorkObjectFault {
    /// Closed words for the model: the rule, where it broke and by how much.
    pub fn describe(&self) -> String {
        let mut out = self.field.phrase().to_owned();
        let found = match self.found {
            Some(WorkTextFound::Characters(n)) => Some(format!("it has {n} characters")),
            Some(WorkTextFound::LineBreak) => Some("it has a line break".to_owned()),
            Some(WorkTextFound::Empty) => Some("it is empty".to_owned()),
            None => None,
        };
        match (self.index, found) {
            (Some(index), Some(found)) => {
                out.push_str(&format!(" (at position {}: {found})", index + 1))
            }
            (Some(index), None) => out.push_str(&format!(" (at position {})", index + 1)),
            (None, Some(found)) => out.push_str(&format!(" ({found})")),
            (None, None) => {}
        }
        out
    }
}

pub(super) type Checked = Result<(), WorkObjectFault>;

fn fault(field: F) -> WorkObjectFault {
    WorkObjectFault {
        field,
        index: None,
        found: None,
    }
}
fn at(field: F, index: usize) -> WorkObjectFault {
    WorkObjectFault {
        field,
        index: Some(u16::try_from(index).unwrap_or(u16::MAX)),
        found: None,
    }
}
fn measured(value: &str) -> WorkTextFound {
    if value.trim().is_empty() {
        WorkTextFound::Empty
    } else if value.contains('\n') {
        WorkTextFound::LineBreak
    } else {
        WorkTextFound::Characters(u32::try_from(value.chars().count()).unwrap_or(u32::MAX))
    }
}

/// Bytes of person-visible text one object may hold in total.
pub const MAX_OBJECT_TEXT_BYTES: usize = 32 * 1024;
pub(super) struct Budget(usize);
impl Budget {
    pub(super) fn new() -> Self {
        Self(0)
    }
    fn add(&mut self, value: &str) -> Checked {
        self.0 += value.len();
        if self.0 > MAX_OBJECT_TEXT_BYTES {
            return Err(fault(F::Text));
        }
        Ok(())
    }
}

/// One line of 1..=max characters without control characters.
fn line_ok(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}
fn line(budget: &mut Budget, value: &str, max: usize, field: F, index: Option<usize>) -> Checked {
    if !line_ok(value, max) {
        let mut refused = match index {
            Some(index) => at(field, index),
            None => fault(field),
        };
        refused.found = Some(measured(value));
        return Err(refused);
    }
    budget.add(value)
}
fn optional(
    budget: &mut Budget,
    value: &Option<String>,
    max: usize,
    field: F,
    index: Option<usize>,
) -> Checked {
    match value {
        Some(value) => line(budget, value, max, field, index),
        None => Ok(()),
    }
}
fn https(value: &str) -> bool {
    super::runtime::validate_public_reference_url(value).is_ok()
}
fn host(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(super::artifact::public_host)
}
pub(super) fn decimal(value: &str) -> bool {
    value.len() <= 64
        && !value.trim().is_empty()
        && value.trim() == value
        && value.parse::<f64>().is_ok_and(f64::is_finite)
}
fn currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|b| b.is_ascii_uppercase())
}
fn source(index: Option<u16>, evidence: usize, at_index: usize) -> Checked {
    if index.is_some_and(|index| usize::from(index) >= evidence) {
        return Err(at(F::ItemSource, at_index));
    }
    Ok(())
}
fn count(len: usize, max: usize, field: F) -> Checked {
    if len == 0 || len > max {
        return Err(fault(field));
    }
    Ok(())
}

/// Block syntax, links, images and HTML are outside inline text.
fn inline_only(text: &str) -> bool {
    let faults = super::artifact::answer_faults(text);
    faults.is_empty()
        && text.lines().all(|line| {
            let line = line.trim_start();
            !(line.starts_with('#')
                || line.starts_with("- ")
                || line.starts_with("* ")
                || line.starts_with("> ")
                || line.starts_with("```")
                || line
                    .split_once(". ")
                    .is_some_and(|(number, _)| number.bytes().all(|b| b.is_ascii_digit())))
        })
}

pub(super) fn check_reply(
    budget: &mut Budget,
    headline: &str,
    text: &str,
    figures: &[WorkFigureV1],
    points: &[String],
) -> Checked {
    line(budget, headline, 80, F::ReplyHeadline, None)?;
    if text.trim().is_empty()
        || text.chars().count() > 480
        || text.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(fault(F::ReplyText));
    }
    if !inline_only(text) {
        return Err(fault(F::ReplyMarkup));
    }
    budget.add(text)?;
    if figures.len() > 4 {
        return Err(fault(F::ReplyFigures));
    }
    for (index, figure) in figures.iter().enumerate() {
        line(budget, &figure.label, 24, F::ReplyFigure, Some(index))?;
        line(budget, &figure.value, 20, F::ReplyFigure, Some(index))?;
        optional(budget, &figure.note, 40, F::ReplyFigure, Some(index))?;
    }
    if points.len() > 5 {
        return Err(fault(F::ReplyPoints));
    }
    for (index, point) in points.iter().enumerate() {
        line(budget, point, 110, F::ReplyPoints, Some(index))?;
    }
    Ok(())
}

pub(super) fn check_picks(budget: &mut Budget, evidence: usize, items: &[WorkPickV1]) -> Checked {
    count(items.len(), MAX_PICKS, F::PicksItems)?;
    if items.iter().filter(|item| item.recommended).count() > 1 {
        return Err(fault(F::PickRecommended));
    }
    for (index, item) in items.iter().enumerate() {
        let i = Some(index);
        line(budget, &item.name, 60, F::PickName, i)?;
        optional(budget, &item.subtitle, 60, F::PickSubtitle, i)?;
        if item.image_candidates.len() > 3 || !item.image_candidates.iter().all(|url| https(url)) {
            return Err(at(F::PickImages, index));
        }
        for url in &item.image_candidates {
            budget.add(url)?;
        }
        if !host(&item.logo_host) {
            return Err(at(F::PickLogo, index));
        }
        if item.url.as_deref().is_some_and(|url| !https(url)) {
            return Err(at(F::PickUrl, index));
        }
        if let Some(price) = &item.price {
            line(budget, &price.display, 24, F::PickPrice, i)?;
            if price.amount.as_deref().is_some_and(|a| !decimal(a))
                || price.currency.as_deref().is_some_and(|c| !currency(c))
            {
                return Err(at(F::PickPrice, index));
            }
        }
        if item.facts.len() > 4 {
            return Err(at(F::PickFacts, index));
        }
        for fact in &item.facts {
            line(budget, &fact.label, 18, F::PickFacts, i)?;
            line(budget, &fact.value, 32, F::PickFacts, i)?;
        }
        if let Some(rating) = &item.rating {
            let value = rating.value.parse::<f64>().ok();
            if !decimal(&rating.value)
                || !matches!(rating.max, 5 | 10)
                || value.is_none_or(|v| v < 0.0 || v > f64::from(rating.max))
            {
                return Err(at(F::PickRating, index));
            }
        }
        optional(budget, &item.why, 120, F::PickWhy, i)?;
        if item.tags.len() > 3 {
            return Err(at(F::PickTags, index));
        }
        for tag in &item.tags {
            line(budget, tag, 16, F::PickTags, i)?;
        }
        if let Some(route) = &item.route {
            line(budget, &route.from, 40, F::PickRoute, i)?;
            line(budget, &route.to, 40, F::PickRoute, i)?;
            optional(budget, &route.depart, 24, F::PickRoute, i)?;
            optional(budget, &route.arrive, 24, F::PickRoute, i)?;
            optional(budget, &route.duration, 16, F::PickRoute, i)?;
            optional(budget, &route.carrier, 40, F::PickRoute, i)?;
            if route.stops > 5 || !host(&route.carrier_host) {
                return Err(at(F::PickRoute, index));
            }
        }
        optional(budget, &item.when, 40, F::PickWhen, i)?;
        optional(budget, &item.duration, 16, F::PickWhen, i)?;
        source(item.source, evidence, index)?;
    }
    Ok(())
}
pub(super) fn picks_link(items: &[WorkPickV1]) -> bool {
    items
        .iter()
        .any(|item| item.url.is_some() || !item.image_candidates.is_empty())
}

pub(super) fn check_plan(
    budget: &mut Budget,
    evidence: usize,
    steps: &[WorkPlanStepV1],
    total: &Option<WorkLabelledV1>,
) -> Checked {
    count(steps.len(), MAX_PLAN_STEPS, F::PlanSteps)?;
    for (index, step) in steps.iter().enumerate() {
        let i = Some(index);
        optional(budget, &step.when, 32, F::PlanStepWhen, i)?;
        line(budget, &step.title, 70, F::PlanStepTitle, i)?;
        optional(budget, &step.detail, 140, F::PlanStepDetail, i)?;
        optional(budget, &step.cost, 24, F::PlanStepCost, i)?;
        optional(budget, &step.place, 40, F::PlanStepPlace, i)?;
        if step
            .pick
            .as_ref()
            .is_some_and(|pick| usize::from(pick.index) >= MAX_PICKS)
        {
            return Err(at(F::PlanStepPick, index));
        }
        source(step.source, evidence, index)?;
    }
    if let Some(total) = &total {
        line(budget, &total.label, 24, F::PlanTotal, None)?;
        line(budget, &total.value, 24, F::PlanTotal, None)?;
    }
    Ok(())
}

pub(super) fn check_list(
    budget: &mut Budget,
    evidence: usize,
    items: &[WorkListItemV1],
) -> Checked {
    count(items.len(), MAX_LIST_ITEMS, F::ListItems)?;
    for (index, item) in items.iter().enumerate() {
        let i = Some(index);
        line(budget, &item.title, 90, F::ListItemTitle, i)?;
        optional(budget, &item.detail, 160, F::ListItemDetail, i)?;
        optional(budget, &item.due, 32, F::ListItemDue, i)?;
        if let Some(from) = &item.from {
            optional(budget, &from.app, 24, F::ListItemFrom, i)?;
            optional(budget, &from.who, 40, F::ListItemFrom, i)?;
            optional(budget, &from.when, 32, F::ListItemFrom, i)?;
            if let Some(quote) = &from.quote {
                if quote.trim().is_empty()
                    || quote.chars().count() > 200
                    || quote.chars().any(|c| c.is_control() && c != '\n')
                {
                    return Err(at(F::ListItemFrom, index));
                }
                budget.add(quote)?;
            }
            if !host(&from.host) || from.url.as_deref().is_some_and(|url| !https(url)) {
                return Err(at(F::ListItemFrom, index));
            }
        }
        source(item.source, evidence, index)?;
    }
    Ok(())
}
pub(super) fn list_links(items: &[WorkListItemV1]) -> bool {
    items
        .iter()
        .any(|item| item.from.as_ref().is_some_and(|from| from.url.is_some()))
}

pub(super) fn check_sheet(
    budget: &mut Budget,
    evidence: usize,
    columns: &[WorkSheetColumnV1],
    rows: &[WorkSheetRowV1],
    note: &Option<String>,
) -> Checked {
    count(columns.len(), MAX_SHEET_COLUMNS, F::SheetColumns)?;
    let mut labels = std::collections::BTreeSet::new();
    for (index, column) in columns.iter().enumerate() {
        let i = Some(index);
        line(budget, &column.label, 24, F::SheetColumn, i)?;
        optional(budget, &column.unit, 12, F::SheetColumn, i)?;
        if !labels.insert(column.label.trim().to_lowercase())
            || column.currency.as_deref().is_some_and(|c| !currency(c))
            || (column.kind == WorkSheetColumnKindV1::Money && column.currency.is_none())
            || (column.best.is_some()
                && !matches!(
                    column.kind,
                    WorkSheetColumnKindV1::Number
                        | WorkSheetColumnKindV1::Money
                        | WorkSheetColumnKindV1::Percent
                        | WorkSheetColumnKindV1::Rating
                        | WorkSheetColumnKindV1::YesNo
                        | WorkSheetColumnKindV1::Duration
                        | WorkSheetColumnKindV1::Date
                ))
        {
            return Err(at(F::SheetColumn, index));
        }
    }
    count(rows.len(), MAX_SHEET_ROWS, F::SheetRows)?;
    for (index, row) in rows.iter().enumerate() {
        if row.cells.len() != columns.len() {
            return Err(at(F::SheetRows, index));
        }
        for (cell, column) in row.cells.iter().zip(columns) {
            if !sheet_cell(cell, column.kind) {
                return Err(at(F::SheetCell, index));
            }
            budget.add(cell)?;
        }
        if let Some(entity) = &row.entity {
            if !host(&entity.logo_host) || entity.image.as_deref().is_some_and(|u| !https(u)) {
                return Err(at(F::SheetEntity, index));
            }
        }
        source(row.source, evidence, index)?;
    }
    optional(budget, note, 120, F::SheetNote, None)
}
pub(super) fn sheet_links(columns: &[WorkSheetColumnV1], rows: &[WorkSheetRowV1]) -> bool {
    rows.iter()
        .any(|row| row.entity.as_ref().is_some_and(|e| e.image.is_some()))
        || columns.iter().enumerate().any(|(index, column)| {
            column.kind == WorkSheetColumnKindV1::Link
                && rows.iter().any(|row| !row.cells[index].is_empty())
        })
}

/// A cell typed by its column; empty is an unknown value.
fn sheet_cell(cell: &str, kind: WorkSheetColumnKindV1) -> bool {
    use WorkSheetColumnKindV1 as K;
    if cell.is_empty() {
        return true;
    }
    if cell.chars().any(char::is_control) || cell.trim() != cell {
        return false;
    }
    let chars = cell.chars().count();
    match kind {
        K::Text => chars <= 60,
        K::Number | K::Money | K::Percent => decimal(cell),
        K::Date => chars <= 32,
        K::Duration => chars <= 16,
        K::YesNo => matches!(cell, "yes" | "no" | "partial" | "unknown"),
        K::Rating => cell.split_once('/').is_some_and(|(value, max)| {
            let max = max.parse::<u8>().ok().filter(|m| (1..=10).contains(m));
            decimal(value)
                && max.is_some_and(|max| {
                    value
                        .parse::<f64>()
                        .is_ok_and(|v| (0.0..=f64::from(max)).contains(&v))
                })
        }),
        K::Link => https(cell),
        K::Entity => chars <= 40,
        K::Tag => chars <= 16,
    }
}

pub(super) struct Plot<'a> {
    pub style: WorkPlotStyleV1,
    pub x: &'a WorkPlotXV1,
    pub y: &'a WorkPlotYV1,
    pub series: &'a [WorkPlotSeriesV1],
    pub headline: &'a Option<WorkLabelledV1>,
    pub basis: &'a str,
}
impl Plot<'_> {
    pub(super) fn check(&self, budget: &mut Budget) -> Checked {
        optional(budget, &self.x.label, 24, F::PlotAxis, None)?;
        optional(budget, &self.y.label, 24, F::PlotAxis, None)?;
        optional(budget, &self.y.unit, 12, F::PlotAxis, None)?;
        if self.y.currency.as_deref().is_some_and(|c| !currency(c))
            || (self.y.format == WorkPlotFormatV1::Money && self.y.currency.is_none())
        {
            return Err(fault(F::PlotAxis));
        }
        count(self.series.len(), MAX_PLOT_SERIES, F::PlotSeries)?;
        let single = matches!(self.style, WorkPlotStyleV1::Donut | WorkPlotStyleV1::Radial);
        if single && self.series.len() != 1 {
            return Err(fault(F::PlotShape));
        }
        let mut values: Vec<f64> = Vec::new();
        for (index, series) in self.series.iter().enumerate() {
            line(budget, &series.name, 24, F::PlotSeries, Some(index))?;
            if series.points.is_empty() || series.points.len() > MAX_PLOT_POINTS {
                return Err(at(F::PlotSeries, index));
            }
            if self.style == WorkPlotStyleV1::Radar && series.points.len() < 3 {
                return Err(at(F::PlotShape, index));
            }
            for point in &series.points {
                line(budget, &point.x, 24, F::PlotPoint, Some(index))?;
                let range = self.style == WorkPlotStyleV1::Range;
                if point.y.as_deref().is_some_and(|y| !decimal(y))
                    || point.y2.as_deref().is_some_and(|y| !decimal(y))
                    || (range && point.y.is_some() != point.y2.is_some())
                    || (!range && point.y2.is_some())
                {
                    return Err(at(F::PlotPoint, index));
                }
                if let Some(y) = point.y.as_deref().and_then(|y| y.parse::<f64>().ok()) {
                    values.push(y);
                }
            }
        }
        let first = values.first().copied();
        if first.is_none() || (values.len() > 1 && values.iter().all(|v| Some(*v) == first)) {
            return Err(fault(F::PlotValues));
        }
        if let Some(headline) = &self.headline {
            line(budget, &headline.label, 24, F::PlotHeadline, None)?;
            line(budget, &headline.value, 20, F::PlotHeadline, None)?;
        }
        line(budget, self.basis, 120, F::PlotBasis, None)
    }
}

pub(super) struct Diff<'a> {
    pub path: &'a str,
    pub language: &'a str,
    pub summary: &'a str,
    pub hunks: &'a [WorkDiffHunkV1],
}
impl Diff<'_> {
    pub(super) fn check(&self, budget: &mut Budget) -> Checked {
        if self.path.trim().is_empty()
            || self.path.len() > 512
            || self.path.chars().any(char::is_control)
        {
            return Err(fault(F::DiffPath));
        }
        budget.add(self.path)?;
        if !CODE_LANGUAGES.contains(&self.language) {
            return Err(fault(F::DiffLanguage));
        }
        line(budget, self.summary, 120, F::DiffSummary, None)?;
        count(self.hunks.len(), MAX_DIFF_HUNKS, F::DiffHunks)?;
        for (index, hunk) in self.hunks.iter().enumerate() {
            if hunk.lines.is_empty()
                || hunk.lines.len() > MAX_DIFF_HUNK_LINES
                || hunk.old_start == 0 && hunk.new_start == 0
            {
                return Err(at(F::DiffHunks, index));
            }
            for line in &hunk.lines {
                if line.text.chars().count() > 500
                    || line.text.chars().any(|c| c.is_control() && c != '\t')
                {
                    return Err(at(F::DiffLines, index));
                }
                budget.add(&line.text)?;
            }
        }
        Ok(())
    }
}

pub(super) struct Draft<'a> {
    pub destination: WorkDraftDestinationV1,
    pub to: &'a Option<String>,
    pub subject: &'a Option<String>,
    pub body: &'a str,
    pub target_url: &'a Option<String>,
}
impl Draft<'_> {
    pub(super) fn check(&self, budget: &mut Budget) -> Checked {
        optional(budget, self.to, 80, F::DraftTo, None)?;
        optional(budget, self.subject, 120, F::DraftSubject, None)?;
        if self.subject.is_some() && self.destination != WorkDraftDestinationV1::Email {
            return Err(fault(F::DraftSubject));
        }
        if self.body.trim().is_empty()
            || self.body.chars().count() > MAX_DRAFT_BODY_CHARS
            || self
                .body
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(fault(F::DraftBody));
        }
        if super::artifact::answer_faults(self.body)
            .iter()
            .any(|field| *field != F::AnswerLink)
        {
            return Err(fault(F::DraftMarkup));
        }
        budget.add(self.body)?;
        if self.target_url.as_deref().is_some_and(|url| !https(url)) {
            return Err(fault(F::DraftTarget));
        }
        Ok(())
    }
}

pub(super) struct Project<'a> {
    pub name: &'a str,
    pub summary: &'a str,
    pub root: &'a str,
    pub stack: &'a [WorkProjectStackV1],
    pub tree: &'a [WorkProjectEntryV1],
    pub scripts: &'a [WorkProjectScriptV1],
    pub git: &'a Option<WorkProjectGitV1>,
}
/// A relative path of 1..=`depth` plain names.
fn relative(path: &str, depth: usize) -> bool {
    let names: Vec<&str> = path.split('/').collect();
    path.len() <= 512
        && names.len() <= depth
        && names.iter().all(|name| {
            !name.is_empty()
                && *name != "."
                && *name != ".."
                && name.chars().count() <= 120
                && !name.chars().any(char::is_control)
        })
}
impl Project<'_> {
    pub(super) fn check(&self, budget: &mut Budget) -> Checked {
        line(budget, self.name, 60, F::ProjectName, None)?;
        line(budget, self.summary, 160, F::ProjectSummary, None)?;
        if super::runtime::validate_file_path(self.root).is_err() {
            return Err(fault(F::ProjectRoot));
        }
        budget.add(self.root)?;
        if self.stack.len() > MAX_PROJECT_STACK {
            return Err(fault(F::ProjectStack));
        }
        let mut names = std::collections::BTreeSet::new();
        for (index, item) in self.stack.iter().enumerate() {
            let i = Some(index);
            line(budget, &item.name, 32, F::ProjectStack, i)?;
            optional(budget, &item.version, 24, F::ProjectStack, i)?;
            optional(budget, &item.role, 24, F::ProjectStack, i)?;
            if !names.insert(item.name.trim().to_lowercase())
                || !host(&item.host)
                || item.manifest.as_deref().is_some_and(|m| !relative(m, 8))
            {
                return Err(at(F::ProjectStack, index));
            }
        }
        count(self.tree.len(), MAX_PROJECT_TREE, F::ProjectTree)?;
        let mut seen: Vec<(&str, WorkProjectEntryKindV1)> = Vec::new();
        for (index, entry) in self.tree.iter().enumerate() {
            let parent_ok = match entry.path.rsplit_once('/') {
                Some((parent, _)) => seen
                    .iter()
                    .any(|(path, kind)| *path == parent && *kind == WorkProjectEntryKindV1::Folder),
                None => true,
            };
            if !relative(&entry.path, MAX_PROJECT_TREE_DEPTH)
                || !parent_ok
                || seen.iter().any(|(path, _)| *path == entry.path)
                || (entry.more.is_some() && entry.kind != WorkProjectEntryKindV1::Folder)
            {
                return Err(at(F::ProjectTree, index));
            }
            budget.add(&entry.path)?;
            seen.push((&entry.path, entry.kind));
        }
        if self.scripts.len() > MAX_PROJECT_SCRIPTS {
            return Err(fault(F::ProjectScripts));
        }
        for (index, script) in self.scripts.iter().enumerate() {
            let i = Some(index);
            line(budget, &script.name, 32, F::ProjectScripts, i)?;
            line(budget, &script.command, 160, F::ProjectScripts, i)?;
            optional(budget, &script.source, 24, F::ProjectScripts, i)?;
        }
        if let Some(git) = self.git {
            optional(budget, &git.branch, 80, F::ProjectGit, None)?;
        }
        Ok(())
    }
}

pub(super) struct Media<'a> {
    pub medium: WorkMediaKindV1,
    pub url: &'a str,
    pub title: &'a Option<String>,
    pub provider: Option<WorkMediaProviderV1>,
    pub poster: &'a Option<String>,
    pub duration: &'a Option<String>,
    pub start_secs: Option<u32>,
}
impl Media<'_> {
    pub(super) fn check(&self, budget: &mut Budget) -> Checked {
        if !https(self.url) {
            return Err(fault(F::MediaUrl));
        }
        budget.add(self.url)?;
        let host = url::Url::parse(self.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .unwrap_or_default();
        let on = |domains: &[&str]| {
            domains.iter().any(|domain| {
                host == *domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|rest| rest.ends_with('.'))
            })
        };
        let provider_ok = match self.provider {
            Some(WorkMediaProviderV1::Youtube) => {
                self.medium == WorkMediaKindV1::Video
                    && on(&["youtube.com", "youtu.be", "youtube-nocookie.com"])
            }
            Some(WorkMediaProviderV1::Vimeo) => {
                self.medium == WorkMediaKindV1::Video && on(&["vimeo.com"])
            }
            Some(WorkMediaProviderV1::File) | None => true,
        };
        if !provider_ok {
            return Err(fault(F::MediaProvider));
        }
        optional(budget, self.title, 80, F::MediaTitle, None)?;
        if self.poster.as_deref().is_some_and(|url| !https(url)) {
            return Err(fault(F::MediaPoster));
        }
        optional(budget, self.duration, 16, F::MediaDuration, None)?;
        if self.start_secs.is_some_and(|secs| secs > 86_400) {
            return Err(fault(F::MediaDuration));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "objects_tests.rs"]
mod tests;
