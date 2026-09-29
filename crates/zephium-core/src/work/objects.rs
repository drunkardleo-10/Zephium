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

/// Character and count limits of the lead's objects. The tool schemas the
/// model sees state the same numbers.
pub mod limit {
    pub const REPLY_HEADLINE: usize = 80;
    pub const REPLY_TEXT: usize = 480;
    pub const REPLY_FIGURES: usize = 4;
    pub const FIGURE_LABEL: usize = 24;
    pub const FIGURE_VALUE: usize = 20;
    pub const FIGURE_NOTE: usize = 40;
    pub const REPLY_POINTS: usize = 5;
    pub const REPLY_POINT: usize = 140;
    pub const PICK_NAME: usize = 60;
    pub const PICK_SUBTITLE: usize = 80;
    pub const PICK_IMAGES: usize = 3;
    pub const PICK_PRICE: usize = 24;
    pub const PICK_FACTS: usize = 4;
    pub const FACT_LABEL: usize = 20;
    pub const FACT_VALUE: usize = 40;
    pub const PICK_WHY: usize = 160;
    pub const PICK_TAGS: usize = 3;
    pub const PICK_TAG: usize = 16;
    pub const ROUTE_PLACE: usize = 40;
    pub const ROUTE_TIME: usize = 24;
    pub const ROUTE_DURATION: usize = 16;
    pub const ROUTE_CARRIER: usize = 40;
    pub const ROUTE_STOPS: u8 = 5;
    pub const PICK_WHEN: usize = 40;
    pub const PICK_DURATION: usize = 16;
    pub const STEP_WHEN: usize = 32;
    pub const STEP_TITLE: usize = 80;
    pub const STEP_DETAIL: usize = 280;
    pub const STEP_COST: usize = 24;
    pub const STEP_PLACE: usize = 40;
    pub const PLAN_TOTAL: usize = 24;
    pub const ITEM_TITLE: usize = 90;
    pub const ITEM_DETAIL: usize = 280;
    pub const ITEM_DUE: usize = 32;
    pub const FROM_APP: usize = 24;
    pub const FROM_WHO: usize = 40;
    pub const FROM_WHEN: usize = 32;
    pub const FROM_QUOTE: usize = 200;
    pub const COLUMN_LABEL: usize = 24;
    pub const COLUMN_UNIT: usize = 12;
    pub const CELL_TEXT: usize = 60;
    pub const CELL_DATE: usize = 32;
    pub const CELL_DURATION: usize = 16;
    pub const CELL_ENTITY: usize = 40;
    pub const CELL_TAG: usize = 16;
    pub const SHEET_NOTE: usize = 200;
    pub const PLOT_LABEL: usize = 24;
    pub const PLOT_UNIT: usize = 12;
    pub const PLOT_SERIES_NAME: usize = 24;
    pub const PLOT_X: usize = 24;
    pub const PLOT_HEADLINE_LABEL: usize = 24;
    pub const PLOT_HEADLINE_VALUE: usize = 20;
    pub const PLOT_BASIS: usize = 120;
    pub const DIFF_SUMMARY: usize = 120;
    pub const DIFF_LINE: usize = 500;
    pub const DRAFT_TO: usize = 80;
    pub const DRAFT_SUBJECT: usize = 120;
    pub const MEDIA_TITLE: usize = 80;
    pub const MEDIA_DURATION: usize = 16;
    pub const DIAGRAM_NAME: usize = 28;
    pub const DIAGRAM_NOTE: usize = 60;
    pub const DIAGRAM_EDGE_LABEL: usize = 24;
}
use limit as L;

/// The first part of an object that failed: where it sits, what it
/// measured against which limit, and how it can be left out when it is
/// optional.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkObjectFault {
    pub field: F,
    /// Zero-based position of the item, row, step, series or hunk.
    pub index: Option<u16>,
    /// What the refused text measured: its characters, or a line break.
    pub found: Option<WorkTextFound>,
    /// Where it broke in the object's data, with `[]` for each position:
    /// `steps[].detail`, `items[].facts[].value`.
    pub path: Option<&'static str>,
    /// Zero-based position inside the item: a fact, a tag, a cell.
    pub sub: Option<u16>,
    /// The limit the value broke.
    pub limit: Option<u32>,
    /// How the object stands without the offending part, when it is
    /// optional; required text is never cut.
    pub drop: Option<WorkFaultDrop>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkTextFound {
    Characters(u32),
    LineBreak,
    Empty,
    Items(u32),
}
/// What leaving out an optional part means, at a path in the fault's form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkFaultDrop {
    /// Remove the member or array element the path names.
    Remove(&'static str),
    /// Empty the sheet cell the path names: it reads as unknown.
    Empty(&'static str),
    /// Keep only the first `limit` elements of the array the path names.
    Keep(&'static str),
}
impl WorkFaultDrop {
    pub fn path(self) -> &'static str {
        match self {
            Self::Remove(path) | Self::Empty(path) | Self::Keep(path) => path,
        }
    }
}
impl WorkObjectFault {
    /// A fault of the whole object, with no place in it.
    pub fn of(field: F) -> Self {
        fault(field)
    }
    /// A path template with its positions filled: `steps[3].detail`.
    pub fn fill(&self, template: &str) -> String {
        let mut positions = [self.index, self.sub].into_iter().flatten();
        let mut out = String::new();
        let mut rest = template;
        while let Some(at) = rest.find("[]") {
            out.push_str(&rest[..at]);
            match positions.next() {
                Some(position) => out.push_str(&format!("[{position}]")),
                None => out.push_str("[]"),
            }
            rest = &rest[at + 2..];
        }
        out.push_str(rest);
        out
    }
    /// Closed words for the model: where it broke, by how much against which
    /// limit, then the rule.
    pub fn describe(&self) -> String {
        let limit = self.limit.map(|limit| format!("; the limit is {limit}"));
        let found = match (self.found, &limit) {
            (Some(WorkTextFound::Characters(n)), Some(limit)) => {
                Some(format!("is {n} characters{limit}"))
            }
            (Some(WorkTextFound::Characters(n)), None) => Some(format!("is {n} characters")),
            (Some(WorkTextFound::Items(n)), Some(limit)) => Some(format!("has {n} items{limit}")),
            (Some(WorkTextFound::Items(n)), None) => Some(format!("has {n} items")),
            (Some(WorkTextFound::LineBreak), _) => Some("has a line break; it is one line".into()),
            (Some(WorkTextFound::Empty), _) => Some("is empty".into()),
            (None, _) => None,
        };
        match (self.path, found) {
            (Some(path), Some(found)) => {
                format!("{} {found}. Rule: {}", self.fill(path), self.field.phrase())
            }
            (Some(path), None) => format!("{}: {}", self.fill(path), self.field.phrase()),
            (None, Some(found)) => format!("{} (it {found})", self.field.phrase()),
            (None, None) => self.field.phrase().to_owned(),
        }
    }
}

pub(super) type Checked = Result<(), WorkObjectFault>;

fn fault(field: F) -> WorkObjectFault {
    WorkObjectFault {
        field,
        index: None,
        found: None,
        path: None,
        sub: None,
        limit: None,
        drop: None,
    }
}
fn position(value: Option<usize>) -> Option<u16> {
    value.map(|value| u16::try_from(value).unwrap_or(u16::MAX))
}
/// Where a value sits: its path template and its positions.
#[derive(Clone, Copy)]
struct Place {
    path: &'static str,
    index: Option<usize>,
    sub: Option<usize>,
}
fn place(path: &'static str, index: Option<usize>) -> Place {
    Place {
        path,
        index,
        sub: None,
    }
}
fn inner(path: &'static str, index: usize, sub: usize) -> Place {
    Place {
        path,
        index: Some(index),
        sub: Some(sub),
    }
}
impl Place {
    fn fault(self, field: F) -> WorkObjectFault {
        WorkObjectFault {
            index: position(self.index),
            sub: position(self.sub),
            path: Some(self.path),
            ..fault(field)
        }
    }
    fn dropping(self, field: F, drop: WorkFaultDrop) -> WorkObjectFault {
        WorkObjectFault {
            drop: Some(drop),
            ..self.fault(field)
        }
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
fn limit(max: usize) -> Option<u32> {
    u32::try_from(max).ok()
}
/// Required text: one line within its limit.
fn line(budget: &mut Budget, value: &str, max: usize, field: F, at: Place) -> Checked {
    if !line_ok(value, max) {
        return Err(WorkObjectFault {
            found: Some(measured(value)),
            limit: limit(max),
            ..at.fault(field)
        });
    }
    budget.add(value)
}
/// Text that can go as `drop` when it breaks its rule.
fn droppable(
    budget: &mut Budget,
    value: &str,
    max: usize,
    field: F,
    at: Place,
    drop: WorkFaultDrop,
) -> Checked {
    line(budget, value, max, field, at).map_err(|fault| WorkObjectFault {
        drop: Some(drop),
        ..fault
    })
}
/// An optional member: out of its rule, the object stands without it.
fn optional(
    budget: &mut Budget,
    value: &Option<String>,
    max: usize,
    field: F,
    at: Place,
) -> Checked {
    match value {
        Some(value) => droppable(
            budget,
            value,
            max,
            field,
            at,
            WorkFaultDrop::Remove(at.path),
        ),
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
fn source(index: Option<u16>, evidence: usize, at: Place) -> Checked {
    if index.is_some_and(|index| usize::from(index) >= evidence) {
        return Err(at.dropping(F::ItemSource, WorkFaultDrop::Remove(at.path)));
    }
    Ok(())
}
fn items(len: usize) -> Option<WorkTextFound> {
    Some(WorkTextFound::Items(u32::try_from(len).unwrap_or(u32::MAX)))
}
/// A required array of 1..=max elements.
fn count(len: usize, max: usize, field: F, at: Place) -> Checked {
    if len == 0 || len > max {
        return Err(WorkObjectFault {
            found: items(len),
            limit: limit(max),
            ..at.fault(field)
        });
    }
    Ok(())
}
/// An optional array of at most max elements: past it, the first max stay.
fn extra(len: usize, max: usize, field: F, at: Place) -> Checked {
    if len > max {
        return Err(WorkObjectFault {
            found: items(len),
            limit: limit(max),
            ..at.dropping(field, WorkFaultDrop::Keep(at.path))
        });
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
    line(
        budget,
        headline,
        L::REPLY_HEADLINE,
        F::ReplyHeadline,
        place("headline", None),
    )?;
    if text.trim().is_empty()
        || text.chars().count() > L::REPLY_TEXT
        || text.chars().any(|c| c.is_control() && c != '\n')
    {
        let found = if text.trim().is_empty() {
            WorkTextFound::Empty
        } else {
            WorkTextFound::Characters(u32::try_from(text.chars().count()).unwrap_or(u32::MAX))
        };
        return Err(WorkObjectFault {
            found: Some(found),
            limit: limit(L::REPLY_TEXT),
            ..place("text", None).fault(F::ReplyText)
        });
    }
    if !inline_only(text) {
        return Err(place("text", None).fault(F::ReplyMarkup));
    }
    budget.add(text)?;
    extra(
        figures.len(),
        L::REPLY_FIGURES,
        F::ReplyFigures,
        place("figures", None),
    )?;
    let figure = WorkFaultDrop::Remove("figures[]");
    for (index, item) in figures.iter().enumerate() {
        let i = Some(index);
        droppable(
            budget,
            &item.label,
            L::FIGURE_LABEL,
            F::ReplyFigure,
            place("figures[].label", i),
            figure,
        )?;
        droppable(
            budget,
            &item.value,
            L::FIGURE_VALUE,
            F::ReplyFigure,
            place("figures[].value", i),
            figure,
        )?;
        optional(
            budget,
            &item.note,
            L::FIGURE_NOTE,
            F::ReplyFigure,
            place("figures[].note", i),
        )?;
    }
    extra(
        points.len(),
        L::REPLY_POINTS,
        F::ReplyPoints,
        place("points", None),
    )?;
    for (index, point) in points.iter().enumerate() {
        let at = place("points[]", Some(index));
        droppable(
            budget,
            point,
            L::REPLY_POINT,
            F::ReplyPoints,
            at,
            WorkFaultDrop::Remove(at.path),
        )?;
    }
    Ok(())
}

pub(super) fn check_picks(budget: &mut Budget, evidence: usize, items: &[WorkPickV1]) -> Checked {
    use WorkFaultDrop::Remove;
    count(items.len(), MAX_PICKS, F::PicksItems, place("items", None))?;
    if let Some(second) = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.recommended)
        .nth(1)
    {
        return Err(place("items[].recommended", Some(second.0))
            .dropping(F::PickRecommended, Remove("items[].recommended")));
    }
    for (index, item) in items.iter().enumerate() {
        let i = Some(index);
        line(
            budget,
            &item.name,
            L::PICK_NAME,
            F::PickName,
            place("items[].name", i),
        )?;
        optional(
            budget,
            &item.subtitle,
            L::PICK_SUBTITLE,
            F::PickSubtitle,
            place("items[].subtitle", i),
        )?;
        extra(
            item.image_candidates.len(),
            L::PICK_IMAGES,
            F::PickImages,
            place("items[].image_candidates", i),
        )?;
        for (sub, url) in item.image_candidates.iter().enumerate() {
            if !https(url) {
                return Err(inner("items[].image_candidates[]", index, sub)
                    .dropping(F::PickImages, Remove("items[].image_candidates[]")));
            }
            budget.add(url)?;
        }
        if !host(&item.logo_host) {
            return Err(
                place("items[].logo_host", i).dropping(F::PickLogo, Remove("items[].logo_host"))
            );
        }
        if item.url.as_deref().is_some_and(|url| !https(url)) {
            return Err(place("items[].url", i).dropping(F::PickUrl, Remove("items[].url")));
        }
        if let Some(price) = &item.price {
            droppable(
                budget,
                &price.display,
                L::PICK_PRICE,
                F::PickPrice,
                place("items[].price.display", i),
                Remove("items[].price"),
            )?;
            if price.amount.as_deref().is_some_and(|a| !decimal(a)) {
                return Err(place("items[].price.amount", i)
                    .dropping(F::PickPrice, Remove("items[].price.amount")));
            }
            if price.currency.as_deref().is_some_and(|c| !currency(c)) {
                return Err(place("items[].price.currency", i)
                    .dropping(F::PickPrice, Remove("items[].price.currency")));
            }
        }
        extra(
            item.facts.len(),
            L::PICK_FACTS,
            F::PickFacts,
            place("items[].facts", i),
        )?;
        for (sub, fact) in item.facts.iter().enumerate() {
            let whole = Remove("items[].facts[]");
            droppable(
                budget,
                &fact.label,
                L::FACT_LABEL,
                F::PickFacts,
                inner("items[].facts[].label", index, sub),
                whole,
            )?;
            droppable(
                budget,
                &fact.value,
                L::FACT_VALUE,
                F::PickFacts,
                inner("items[].facts[].value", index, sub),
                whole,
            )?;
        }
        if let Some(rating) = &item.rating {
            let value = rating.value.parse::<f64>().ok();
            if !decimal(&rating.value)
                || !matches!(rating.max, 5 | 10)
                || value.is_none_or(|v| v < 0.0 || v > f64::from(rating.max))
            {
                return Err(
                    place("items[].rating", i).dropping(F::PickRating, Remove("items[].rating"))
                );
            }
        }
        optional(
            budget,
            &item.why,
            L::PICK_WHY,
            F::PickWhy,
            place("items[].why", i),
        )?;
        extra(
            item.tags.len(),
            L::PICK_TAGS,
            F::PickTags,
            place("items[].tags", i),
        )?;
        for (sub, tag) in item.tags.iter().enumerate() {
            droppable(
                budget,
                tag,
                L::PICK_TAG,
                F::PickTags,
                inner("items[].tags[]", index, sub),
                Remove("items[].tags[]"),
            )?;
        }
        if let Some(route) = &item.route {
            let whole = Remove("items[].route");
            droppable(
                budget,
                &route.from,
                L::ROUTE_PLACE,
                F::PickRoute,
                place("items[].route.from", i),
                whole,
            )?;
            droppable(
                budget,
                &route.to,
                L::ROUTE_PLACE,
                F::PickRoute,
                place("items[].route.to", i),
                whole,
            )?;
            optional(
                budget,
                &route.depart,
                L::ROUTE_TIME,
                F::PickRoute,
                place("items[].route.depart", i),
            )?;
            optional(
                budget,
                &route.arrive,
                L::ROUTE_TIME,
                F::PickRoute,
                place("items[].route.arrive", i),
            )?;
            optional(
                budget,
                &route.duration,
                L::ROUTE_DURATION,
                F::PickRoute,
                place("items[].route.duration", i),
            )?;
            optional(
                budget,
                &route.carrier,
                L::ROUTE_CARRIER,
                F::PickRoute,
                place("items[].route.carrier", i),
            )?;
            if route.stops > L::ROUTE_STOPS {
                return Err(place("items[].route.stops", i).dropping(F::PickRoute, whole));
            }
            if !host(&route.carrier_host) {
                return Err(place("items[].route.carrier_host", i)
                    .dropping(F::PickRoute, Remove("items[].route.carrier_host")));
            }
        }
        optional(
            budget,
            &item.when,
            L::PICK_WHEN,
            F::PickWhen,
            place("items[].when", i),
        )?;
        optional(
            budget,
            &item.duration,
            L::PICK_DURATION,
            F::PickWhen,
            place("items[].duration", i),
        )?;
        source(item.source, evidence, place("items[].source", i))?;
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
    count(
        steps.len(),
        MAX_PLAN_STEPS,
        F::PlanSteps,
        place("steps", None),
    )?;
    for (index, step) in steps.iter().enumerate() {
        let i = Some(index);
        optional(
            budget,
            &step.when,
            L::STEP_WHEN,
            F::PlanStepWhen,
            place("steps[].when", i),
        )?;
        line(
            budget,
            &step.title,
            L::STEP_TITLE,
            F::PlanStepTitle,
            place("steps[].title", i),
        )?;
        optional(
            budget,
            &step.detail,
            L::STEP_DETAIL,
            F::PlanStepDetail,
            place("steps[].detail", i),
        )?;
        optional(
            budget,
            &step.cost,
            L::STEP_COST,
            F::PlanStepCost,
            place("steps[].cost", i),
        )?;
        optional(
            budget,
            &step.place,
            L::STEP_PLACE,
            F::PlanStepPlace,
            place("steps[].place", i),
        )?;
        if step
            .pick
            .as_ref()
            .is_some_and(|pick| usize::from(pick.index) >= MAX_PICKS)
        {
            return Err(place("steps[].pick", i)
                .dropping(F::PlanStepPick, WorkFaultDrop::Remove("steps[].pick")));
        }
        source(step.source, evidence, place("steps[].source", i))?;
    }
    if let Some(total) = &total {
        let whole = WorkFaultDrop::Remove("total");
        droppable(
            budget,
            &total.label,
            L::PLAN_TOTAL,
            F::PlanTotal,
            place("total.label", None),
            whole,
        )?;
        droppable(
            budget,
            &total.value,
            L::PLAN_TOTAL,
            F::PlanTotal,
            place("total.value", None),
            whole,
        )?;
    }
    Ok(())
}

pub(super) fn check_list(
    budget: &mut Budget,
    evidence: usize,
    items: &[WorkListItemV1],
) -> Checked {
    use WorkFaultDrop::Remove;
    count(
        items.len(),
        MAX_LIST_ITEMS,
        F::ListItems,
        place("items", None),
    )?;
    for (index, item) in items.iter().enumerate() {
        let i = Some(index);
        line(
            budget,
            &item.title,
            L::ITEM_TITLE,
            F::ListItemTitle,
            place("items[].title", i),
        )?;
        optional(
            budget,
            &item.detail,
            L::ITEM_DETAIL,
            F::ListItemDetail,
            place("items[].detail", i),
        )?;
        optional(
            budget,
            &item.due,
            L::ITEM_DUE,
            F::ListItemDue,
            place("items[].due", i),
        )?;
        if let Some(from) = &item.from {
            optional(
                budget,
                &from.app,
                L::FROM_APP,
                F::ListItemFrom,
                place("items[].from.app", i),
            )?;
            optional(
                budget,
                &from.who,
                L::FROM_WHO,
                F::ListItemFrom,
                place("items[].from.who", i),
            )?;
            optional(
                budget,
                &from.when,
                L::FROM_WHEN,
                F::ListItemFrom,
                place("items[].from.when", i),
            )?;
            if let Some(quote) = &from.quote {
                if quote.trim().is_empty()
                    || quote.chars().count() > L::FROM_QUOTE
                    || quote.chars().any(|c| c.is_control() && c != '\n')
                {
                    return Err(WorkObjectFault {
                        found: Some(measured(&quote.replace('\n', " "))),
                        limit: limit(L::FROM_QUOTE),
                        ..place("items[].from.quote", i)
                            .dropping(F::ListItemFrom, Remove("items[].from.quote"))
                    });
                }
                budget.add(quote)?;
            }
            if !host(&from.host) {
                return Err(place("items[].from.host", i)
                    .dropping(F::ListItemFrom, Remove("items[].from.host")));
            }
            if from.url.as_deref().is_some_and(|url| !https(url)) {
                return Err(place("items[].from.url", i)
                    .dropping(F::ListItemFrom, Remove("items[].from.url")));
            }
        }
        source(item.source, evidence, place("items[].source", i))?;
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
    use WorkFaultDrop::Remove;
    count(
        columns.len(),
        MAX_SHEET_COLUMNS,
        F::SheetColumns,
        place("columns", None),
    )?;
    let mut labels = std::collections::BTreeSet::new();
    for (index, column) in columns.iter().enumerate() {
        let i = Some(index);
        line(
            budget,
            &column.label,
            L::COLUMN_LABEL,
            F::SheetColumn,
            place("columns[].label", i),
        )?;
        optional(
            budget,
            &column.unit,
            L::COLUMN_UNIT,
            F::SheetColumn,
            place("columns[].unit", i),
        )?;
        if !labels.insert(column.label.trim().to_lowercase()) {
            return Err(place("columns[].label", i).fault(F::SheetColumn));
        }
        let money = column.kind == WorkSheetColumnKindV1::Money;
        if column.currency.as_deref().is_some_and(|c| !currency(c))
            || (money && column.currency.is_none())
        {
            let at = place("columns[].currency", i);
            return Err(if money {
                at.fault(F::SheetColumn)
            } else {
                at.dropping(F::SheetColumn, Remove("columns[].currency"))
            });
        }
        if column.best.is_some()
            && !matches!(
                column.kind,
                WorkSheetColumnKindV1::Number
                    | WorkSheetColumnKindV1::Money
                    | WorkSheetColumnKindV1::Percent
                    | WorkSheetColumnKindV1::Rating
                    | WorkSheetColumnKindV1::YesNo
                    | WorkSheetColumnKindV1::Duration
                    | WorkSheetColumnKindV1::Date
            )
        {
            return Err(
                place("columns[].best", i).dropping(F::SheetColumn, Remove("columns[].best"))
            );
        }
    }
    count(
        rows.len(),
        MAX_SHEET_ROWS,
        F::SheetRows,
        place("rows", None),
    )?;
    for (index, row) in rows.iter().enumerate() {
        if row.cells.len() != columns.len() {
            return Err(WorkObjectFault {
                found: items(row.cells.len()),
                limit: limit(columns.len()),
                ..place("rows[].cells", Some(index)).fault(F::SheetRows)
            });
        }
        for (sub, (cell, column)) in row.cells.iter().zip(columns).enumerate() {
            if !sheet_cell(cell, column.kind) {
                let at = inner("rows[].cells[]", index, sub);
                let mut refused = if sub > 0 {
                    at.dropping(F::SheetCell, WorkFaultDrop::Empty("rows[].cells[]"))
                } else {
                    at.fault(F::SheetCell)
                };
                if let Some(max) = text_limit(column.kind) {
                    refused.found = Some(measured(cell));
                    refused.limit = limit(max);
                }
                return Err(refused);
            }
            budget.add(cell)?;
        }
        if let Some(entity) = &row.entity {
            if !host(&entity.logo_host) || entity.image.as_deref().is_some_and(|u| !https(u)) {
                return Err(place("rows[].entity", Some(index))
                    .dropping(F::SheetEntity, Remove("rows[].entity")));
            }
        }
        source(row.source, evidence, place("rows[].source", Some(index)))?;
    }
    optional(
        budget,
        note,
        L::SHEET_NOTE,
        F::SheetNote,
        place("note", None),
    )
}
pub(super) fn sheet_links(columns: &[WorkSheetColumnV1], rows: &[WorkSheetRowV1]) -> bool {
    rows.iter()
        .any(|row| row.entity.as_ref().is_some_and(|e| e.image.is_some()))
        || columns.iter().enumerate().any(|(index, column)| {
            column.kind == WorkSheetColumnKindV1::Link
                && rows.iter().any(|row| !row.cells[index].is_empty())
        })
}

/// The character limit of a text-like column's cells.
fn text_limit(kind: WorkSheetColumnKindV1) -> Option<usize> {
    use WorkSheetColumnKindV1 as K;
    match kind {
        K::Text => Some(L::CELL_TEXT),
        K::Date => Some(L::CELL_DATE),
        K::Duration => Some(L::CELL_DURATION),
        K::Entity => Some(L::CELL_ENTITY),
        K::Tag => Some(L::CELL_TAG),
        _ => None,
    }
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
    if let Some(max) = text_limit(kind) {
        return cell.chars().count() <= max;
    }
    match kind {
        K::Number | K::Money | K::Percent => decimal(cell),
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
        _ => false,
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
        optional(
            budget,
            &self.x.label,
            L::PLOT_LABEL,
            F::PlotAxis,
            place("x.label", None),
        )?;
        optional(
            budget,
            &self.y.label,
            L::PLOT_LABEL,
            F::PlotAxis,
            place("y.label", None),
        )?;
        optional(
            budget,
            &self.y.unit,
            L::PLOT_UNIT,
            F::PlotAxis,
            place("y.unit", None),
        )?;
        if self.y.currency.as_deref().is_some_and(|c| !currency(c))
            || (self.y.format == WorkPlotFormatV1::Money && self.y.currency.is_none())
        {
            return Err(place("y.currency", None).fault(F::PlotAxis));
        }
        count(
            self.series.len(),
            MAX_PLOT_SERIES,
            F::PlotSeries,
            place("series", None),
        )?;
        let single = matches!(self.style, WorkPlotStyleV1::Donut | WorkPlotStyleV1::Radial);
        if single && self.series.len() != 1 {
            return Err(place("series", None).fault(F::PlotShape));
        }
        let mut values: Vec<f64> = Vec::new();
        for (index, series) in self.series.iter().enumerate() {
            let i = Some(index);
            line(
                budget,
                &series.name,
                L::PLOT_SERIES_NAME,
                F::PlotSeries,
                place("series[].name", i),
            )?;
            count(
                series.points.len(),
                MAX_PLOT_POINTS,
                F::PlotSeries,
                place("series[].points", i),
            )?;
            if self.style == WorkPlotStyleV1::Radar && series.points.len() < 3 {
                return Err(place("series[].points", i).fault(F::PlotShape));
            }
            for (sub, point) in series.points.iter().enumerate() {
                line(
                    budget,
                    &point.x,
                    L::PLOT_X,
                    F::PlotPoint,
                    inner("series[].points[].x", index, sub),
                )?;
                let range = self.style == WorkPlotStyleV1::Range;
                if point.y.as_deref().is_some_and(|y| !decimal(y))
                    || point.y2.as_deref().is_some_and(|y| !decimal(y))
                    || (range && point.y.is_some() != point.y2.is_some())
                    || (!range && point.y2.is_some())
                {
                    return Err(inner("series[].points[]", index, sub).fault(F::PlotPoint));
                }
                if let Some(y) = point.y.as_deref().and_then(|y| y.parse::<f64>().ok()) {
                    values.push(y);
                }
            }
        }
        let first = values.first().copied();
        if first.is_none() || (values.len() > 1 && values.iter().all(|v| Some(*v) == first)) {
            return Err(place("series", None).fault(F::PlotValues));
        }
        if let Some(headline) = &self.headline {
            let whole = WorkFaultDrop::Remove("headline");
            droppable(
                budget,
                &headline.label,
                L::PLOT_HEADLINE_LABEL,
                F::PlotHeadline,
                place("headline.label", None),
                whole,
            )?;
            droppable(
                budget,
                &headline.value,
                L::PLOT_HEADLINE_VALUE,
                F::PlotHeadline,
                place("headline.value", None),
                whole,
            )?;
        }
        line(
            budget,
            self.basis,
            L::PLOT_BASIS,
            F::PlotBasis,
            place("basis", None),
        )
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
            return Err(place("path", None).fault(F::DiffPath));
        }
        budget.add(self.path)?;
        if !CODE_LANGUAGES.contains(&self.language) {
            return Err(place("language", None).fault(F::DiffLanguage));
        }
        line(
            budget,
            self.summary,
            L::DIFF_SUMMARY,
            F::DiffSummary,
            place("summary", None),
        )?;
        count(
            self.hunks.len(),
            MAX_DIFF_HUNKS,
            F::DiffHunks,
            place("hunks", None),
        )?;
        for (index, hunk) in self.hunks.iter().enumerate() {
            if hunk.lines.is_empty()
                || hunk.lines.len() > MAX_DIFF_HUNK_LINES
                || hunk.old_start == 0 && hunk.new_start == 0
            {
                return Err(place("hunks[]", Some(index)).fault(F::DiffHunks));
            }
            for (sub, line) in hunk.lines.iter().enumerate() {
                if line.text.chars().count() > L::DIFF_LINE
                    || line.text.chars().any(|c| c.is_control() && c != '\t')
                {
                    return Err(WorkObjectFault {
                        found: Some(measured(&line.text)),
                        limit: limit(L::DIFF_LINE),
                        ..inner("hunks[].lines[].text", index, sub).fault(F::DiffLines)
                    });
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
        optional(budget, self.to, L::DRAFT_TO, F::DraftTo, place("to", None))?;
        optional(
            budget,
            self.subject,
            L::DRAFT_SUBJECT,
            F::DraftSubject,
            place("subject", None),
        )?;
        if self.subject.is_some() && self.destination != WorkDraftDestinationV1::Email {
            return Err(
                place("subject", None).dropping(F::DraftSubject, WorkFaultDrop::Remove("subject"))
            );
        }
        if self.body.trim().is_empty()
            || self.body.chars().count() > MAX_DRAFT_BODY_CHARS
            || self
                .body
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(WorkObjectFault {
                found: Some(if self.body.trim().is_empty() {
                    WorkTextFound::Empty
                } else {
                    WorkTextFound::Characters(
                        u32::try_from(self.body.chars().count()).unwrap_or(u32::MAX),
                    )
                }),
                limit: limit(MAX_DRAFT_BODY_CHARS),
                ..place("body", None).fault(F::DraftBody)
            });
        }
        if super::artifact::answer_faults(self.body)
            .iter()
            .any(|field| *field != F::AnswerLink)
        {
            return Err(place("body", None).fault(F::DraftMarkup));
        }
        budget.add(self.body)?;
        if self.target_url.as_deref().is_some_and(|url| !https(url)) {
            return Err(place("target_url", None)
                .dropping(F::DraftTarget, WorkFaultDrop::Remove("target_url")));
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
        line(budget, self.name, 60, F::ProjectName, place("name", None))?;
        line(
            budget,
            self.summary,
            160,
            F::ProjectSummary,
            place("summary", None),
        )?;
        if super::runtime::validate_file_path(self.root).is_err() {
            return Err(place("root", None).fault(F::ProjectRoot));
        }
        budget.add(self.root)?;
        if self.stack.len() > MAX_PROJECT_STACK {
            return Err(place("stack", None).fault(F::ProjectStack));
        }
        let mut names = std::collections::BTreeSet::new();
        for (index, item) in self.stack.iter().enumerate() {
            let i = Some(index);
            line(
                budget,
                &item.name,
                32,
                F::ProjectStack,
                place("stack[].name", i),
            )?;
            optional(
                budget,
                &item.version,
                24,
                F::ProjectStack,
                place("stack[].version", i),
            )?;
            optional(
                budget,
                &item.role,
                24,
                F::ProjectStack,
                place("stack[].role", i),
            )?;
            if !names.insert(item.name.trim().to_lowercase())
                || !host(&item.host)
                || item.manifest.as_deref().is_some_and(|m| !relative(m, 8))
            {
                return Err(place("stack[]", i).fault(F::ProjectStack));
            }
        }
        count(
            self.tree.len(),
            MAX_PROJECT_TREE,
            F::ProjectTree,
            place("tree", None),
        )?;
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
                return Err(place("tree[]", Some(index)).fault(F::ProjectTree));
            }
            budget.add(&entry.path)?;
            seen.push((&entry.path, entry.kind));
        }
        if self.scripts.len() > MAX_PROJECT_SCRIPTS {
            return Err(place("scripts", None).fault(F::ProjectScripts));
        }
        for (index, script) in self.scripts.iter().enumerate() {
            let i = Some(index);
            line(
                budget,
                &script.name,
                32,
                F::ProjectScripts,
                place("scripts[].name", i),
            )?;
            line(
                budget,
                &script.command,
                160,
                F::ProjectScripts,
                place("scripts[].command", i),
            )?;
            optional(
                budget,
                &script.source,
                24,
                F::ProjectScripts,
                place("scripts[].source", i),
            )?;
        }
        if let Some(git) = self.git {
            optional(
                budget,
                &git.branch,
                80,
                F::ProjectGit,
                place("git.branch", None),
            )?;
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
        use WorkFaultDrop::Remove;
        if !https(self.url) {
            return Err(place("url", None).fault(F::MediaUrl));
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
            return Err(place("provider", None).dropping(F::MediaProvider, Remove("provider")));
        }
        optional(
            budget,
            self.title,
            L::MEDIA_TITLE,
            F::MediaTitle,
            place("title", None),
        )?;
        if self.poster.as_deref().is_some_and(|url| !https(url)) {
            return Err(place("poster", None).dropping(F::MediaPoster, Remove("poster")));
        }
        optional(
            budget,
            self.duration,
            L::MEDIA_DURATION,
            F::MediaDuration,
            place("duration", None),
        )?;
        if self.start_secs.is_some_and(|secs| secs > 86_400) {
            return Err(place("start_secs", None).dropping(F::MediaDuration, Remove("start_secs")));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "objects_tests.rs"]
mod tests;
