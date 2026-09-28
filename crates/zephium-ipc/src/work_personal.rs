//! Settings and the canvas over the person's memory and skills.
use super::*;
pub use zephium_core::work::personal::{WorkMemoryKindV1, WorkMemoryV1};

/// Which memories to list: the words to look for, or one work's.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkMemoryQueryV1 {
    pub query: Option<String>,
    pub work: Option<WorkId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkMemoryChangeV1 {
    /// A fact the person writes themselves.
    Add {
        text: String,
        memory: WorkMemoryKindV1,
    },
    Edit {
        id: String,
        text: String,
        memory: WorkMemoryKindV1,
    },
    Forget {
        id: String,
    },
    ForgetAll,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkMemoryResponseV1 {
    pub version: u16,
    pub profile: String,
    pub memories: Vec<WorkMemoryV1>,
    /// Why a fact was refused: it looks like a secret, or is too long.
    pub refused: Option<WorkMemoryRefusalV1>,
    pub error: Option<WorkFailureV1>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkMemoryRefusalV1 {
    Empty,
    TooLong,
    Lines,
    Secret,
}

/// One skill as Settings lists it.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkSkillRowV1 {
    pub name: String,
    pub description: String,
    /// Ships with the app, unchanged.
    pub builtin: bool,
    /// The person's own version of a built-in.
    pub customized: bool,
    pub enabled: bool,
    pub tools: Vec<String>,
    /// lead, page or light.
    pub role: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkSkillChangeV1 {
    /// Writes the person's skill from its `SKILL.md` text; `previous` is the
    /// name it had when it is renamed.
    Save {
        previous: Option<String>,
        text: String,
    },
    Delete {
        name: String,
    },
    SetEnabled {
        name: String,
        enabled: bool,
    },
}

/// Why a skill change was refused, in closed words.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkSkillFaultV1 {
    NoFrontmatter,
    Name,
    Description,
    Role,
    TooLarge,
    Empty,
    NotFound,
    BuiltIn,
    Taken,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkSkillsResponseV1 {
    pub version: u16,
    pub profile: String,
    pub skills: Vec<WorkSkillRowV1>,
    /// A skill's `SKILL.md`, when one was read.
    pub text: Option<String>,
    pub fault: Option<WorkSkillFaultV1>,
    pub error: Option<WorkFailureV1>,
}
