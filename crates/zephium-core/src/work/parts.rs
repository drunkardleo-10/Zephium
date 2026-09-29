//! Run facts of the lead agent: the parts a run split into and the inputs it
//! pulled in. Descriptive history for the canvas, never authority.
use super::{validate_text, WorkError, WorkPartId};
use serde::{Deserialize, Serialize};

pub const MAX_WORK_PARTS: usize = 16;
pub const MAX_WORK_INPUTS: usize = 16;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkHelperV1 {
    Browser,
    Computer,
    Connection,
    Research,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkPartStateV1 {
    Planned,
    Running,
    /// Waiting on the person: an entry question, a sign-in or a Confirm.
    Waiting,
    Done,
    Failed,
    Stopped,
}
impl WorkPartStateV1 {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Stopped)
    }
}

/// What a part that could not do its job needs from the person, with the
/// thing it concerns. The fix sits on the part's row.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkPartNeedV1 {
    /// Sign in on the site, then the part can run again.
    SignIn { host: String },
    /// Let the agent work on the site as the person.
    AllowSite { host: String },
    /// Let the agent read a folder on this Mac.
    AllowFolder { path: String },
    /// Use an installed tool or connected service: "gh", "Linear".
    UseConnection { connection: String },
    /// The site or service failed on its side; trying again may work.
    Retry {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
    },
}
impl WorkPartNeedV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        let host_ok = |host: &str| super::artifact::public_host(host);
        let ok = match self {
            Self::SignIn { host } | Self::AllowSite { host } => host_ok(host),
            Self::AllowFolder { path } => super::runtime::validate_file_path(path).is_ok(),
            Self::UseConnection { connection } => words(connection, 40).is_ok(),
            Self::Retry { host } => host.as_deref().is_none_or(host_ok),
        };
        if ok {
            Ok(())
        } else {
            Err(WorkError::Invalid)
        }
    }
}

/// What the part's mark shows: a site's host or a connection's name.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPartServiceV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkPartFactV1 {
    pub id: WorkPartId,
    /// "Stay", "Flights", "Entry".
    pub title: String,
    pub helper: WorkHelperV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<WorkPartServiceV1>,
    pub goal: String,
    pub state: WorkPartStateV1,
    /// Unix epoch milliseconds as decimal text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<String>,
    /// What it found, for people: "3 homes".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What the person can do so the part can do its job. Only a part that
    /// is waiting or ended without doing it carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need: Option<WorkPartNeedV1>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkInputKindV1 {
    Memory,
    Skill,
    History,
    Notes,
    Tabs,
    Files,
    Connection,
    Work,
}

/// Something the run pulled in, drawn left of the request.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkInputFactV1 {
    pub kind: WorkInputKindV1,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u16>,
    /// A skill's name, a note's or work's id: never content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

fn words(value: &str, max_chars: usize) -> Result<(), WorkError> {
    validate_text(value, max_chars * 4)?;
    if value.chars().count() > max_chars || value.contains('\n') {
        return Err(WorkError::Invalid);
    }
    Ok(())
}
fn millis(value: &Option<String>) -> Result<Option<u64>, WorkError> {
    value
        .as_deref()
        .map(|value| {
            value
                .parse::<u64>()
                .ok()
                .filter(|parsed| parsed.to_string() == value)
                .ok_or(WorkError::Invalid)
        })
        .transpose()
}

impl WorkPartFactV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        words(&self.title, 24)?;
        words(&self.goal, 200)?;
        if let Some(summary) = &self.summary {
            words(summary, 80)?;
        }
        if let Some(service) = &self.service {
            if service.host.is_none() == service.connection.is_none()
                || service
                    .host
                    .as_deref()
                    .is_some_and(|host| !super::artifact::public_host(host))
            {
                return Err(WorkError::Invalid);
            }
            if let Some(connection) = &service.connection {
                words(connection, 40)?;
            }
        }
        if let Some(need) = &self.need {
            need.validate()?;
            if matches!(
                self.state,
                WorkPartStateV1::Planned | WorkPartStateV1::Running | WorkPartStateV1::Done
            ) {
                return Err(WorkError::Invalid);
            }
        }
        let started = millis(&self.started_ms)?;
        let ended = millis(&self.ended_ms)?;
        let consistent = match self.state {
            WorkPartStateV1::Planned => started.is_none() && ended.is_none(),
            WorkPartStateV1::Running | WorkPartStateV1::Waiting => {
                started.is_some() && ended.is_none()
            }
            _ => ended.is_some() && started.is_none_or(|s| ended.is_some_and(|e| s <= e)),
        };
        if !consistent {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}

impl WorkInputFactV1 {
    pub fn validate(&self) -> Result<(), WorkError> {
        words(&self.label, 40)?;
        if let Some(reference) = &self.reference {
            words(reference, 256)?;
        }
        Ok(())
    }
}
