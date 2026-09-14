//! Objective-independent Work state. Existing objective and execution identities
//! remain unchanged; attaching one here creates a relationship, not authority.
use super::*;
use crate::ids::{ItemId, ResourceId, SpaceId};

pub const MAX_ENVIRONMENT_ELEMENTS: usize = 500;
pub const MAX_ENVIRONMENT_AREAS: usize = 64;
pub const MAX_ENVIRONMENT_RELATIONS: usize = 2000;
pub const MAX_ENVIRONMENT_TITLE_BYTES: usize = 512;
pub const MAX_ENVIRONMENT_BODY_BYTES: usize = 262_144;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEnvironmentReference {
    Browser {
        tab: ItemId,
    },
    Resource {
        resource: ResourceId,
    },
    Objective {
        objective: WorkId,
    },
    /// A retained result from one historical execution, not a mutable copy or
    /// a capability to rerun its producer.
    Artifact {
        objective: WorkId,
        execution: WorkExecutionId,
        artifact: WorkArtifactId,
    },
    /// One subject inside an immutable artifact, addressed by index.
    Subject {
        objective: WorkId,
        execution: WorkExecutionId,
        artifact: WorkArtifactId,
        index: u16,
    },
    /// One finding inside an immutable artifact, addressed by index.
    Finding {
        objective: WorkId,
        execution: WorkExecutionId,
        artifact: WorkArtifactId,
        index: u16,
    },
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum WorkRelationKind {
    Supports,
    Uses,
    DependsOn,
    SameAs,
    Contradicts,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkRelationOrigin {
    User,
    Agent { execution: WorkExecutionId },
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkRelation {
    pub id: WorkRelationId,
    pub from: WorkElementId,
    pub to: WorkElementId,
    pub kind: WorkRelationKind,
    pub origin: WorkRelationOrigin,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkEnvironmentElement {
    pub id: WorkElementId,
    pub reference: WorkEnvironmentReference,
    pub area: Option<WorkAreaId>,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkArea {
    pub id: WorkAreaId,
    pub title: String,
}

/// Integer canvas coordinates avoid non-finite or precision-dependent values.
/// They are descriptive geometry, never native-page geometry or authority.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkElementPlacement {
    pub element: WorkElementId,
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}

/// Area geometry is presentation; membership stays on the element.
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkAreaPlacement {
    pub area: WorkAreaId,
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkEnvironmentView {
    pub revision: WorkRevision,
    pub x: i32,
    pub y: i32,
    pub zoom_milli: u16,
    pub placements: Vec<WorkElementPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub areas: Vec<WorkAreaPlacement>,
}
impl Default for WorkEnvironmentView {
    fn default() -> Self {
        Self {
            revision: WorkRevision::INITIAL,
            x: 0,
            y: 0,
            zoom_milli: 1000,
            placements: vec![],
            areas: vec![],
        }
    }
}
impl WorkEnvironmentView {
    pub fn validate(&self) -> Result<(), WorkError> {
        let coordinate = |value: i32| (-1_000_000..=1_000_000).contains(&value);
        if !coordinate(self.x)
            || !coordinate(self.y)
            || !(100..=4000).contains(&self.zoom_milli)
            || self.placements.len() > MAX_ENVIRONMENT_ELEMENTS
            || self.areas.len() > MAX_ENVIRONMENT_AREAS
        {
            return Err(WorkError::Invalid);
        }
        let mut ids = BTreeSet::new();
        for p in &self.placements {
            if !ids.insert(p.element)
                || !coordinate(p.x)
                || !coordinate(p.y)
                || !(120..=4096).contains(&p.width)
                || !(80..=4096).contains(&p.height)
            {
                return Err(WorkError::Invalid);
            }
        }
        let mut areas = BTreeSet::new();
        for a in &self.areas {
            if !areas.insert(a.area)
                || !coordinate(a.x)
                || !coordinate(a.y)
                || !(240..=8192).contains(&a.width)
                || !(160..=8192).contains(&a.height)
            {
                return Err(WorkError::Invalid);
            }
        }
        Ok(())
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkEnvironmentSnapshot {
    pub version: u16,
    pub id: WorkEnvironmentId,
    pub profile: ProfileId,
    pub space: SpaceId,
    pub title: String,
    pub lifecycle: WorkLifecycle,
    pub revision: WorkRevision,
    pub elements: Vec<WorkEnvironmentElement>,
    pub areas: Vec<WorkArea>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<WorkRelation>,
    pub view: WorkEnvironmentView,
}
impl WorkEnvironmentSnapshot {
    pub fn create(
        id: WorkEnvironmentId,
        profile: ProfileId,
        space: SpaceId,
        title: String,
    ) -> Result<Self, WorkError> {
        validate_text(&title, MAX_ENVIRONMENT_TITLE_BYTES)?;
        Ok(Self {
            version: 1,
            id,
            profile,
            space,
            title,
            lifecycle: WorkLifecycle::Active,
            revision: WorkRevision::INITIAL,
            elements: vec![],
            areas: vec![],
            relations: vec![],
            view: Default::default(),
        })
    }
    pub fn validate(&self) -> Result<(), WorkError> {
        validate_text(&self.title, MAX_ENVIRONMENT_TITLE_BYTES)?;
        if self.version != 1
            || self.elements.len() > MAX_ENVIRONMENT_ELEMENTS
            || self.areas.len() > MAX_ENVIRONMENT_AREAS
            || self.relations.len() > MAX_ENVIRONMENT_RELATIONS
        {
            return Err(WorkError::Invalid);
        }
        let mut areas = BTreeSet::new();
        for area in &self.areas {
            validate_text(&area.title, MAX_ENVIRONMENT_TITLE_BYTES)?;
            if !areas.insert(area.id) {
                return Err(WorkError::Invalid);
            }
        }
        let mut elements = BTreeSet::new();
        let mut references = BTreeSet::new();
        for element in &self.elements {
            if !elements.insert(element.id)
                || !references.insert(&element.reference)
                || element.area.is_some_and(|id| !areas.contains(&id))
            {
                return Err(WorkError::Invalid);
            }
        }
        let mut relation_ids = BTreeSet::new();
        let mut relation_keys = BTreeSet::new();
        for relation in &self.relations {
            if !relation_ids.insert(relation.id)
                || relation.from == relation.to
                || !elements.contains(&relation.from)
                || !elements.contains(&relation.to)
                || !relation_keys.insert((relation.from, relation.to, relation.kind))
            {
                return Err(WorkError::Invalid);
            }
        }
        self.view.validate()?;
        if self
            .view
            .placements
            .iter()
            .any(|p| !elements.contains(&p.element))
            || self.view.areas.iter().any(|a| !areas.contains(&a.area))
        {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
    /// Pure edit: the imperative caller supplies freshly minted identities.
    pub fn edit(
        &self,
        edit: WorkEnvironmentEdit,
        element_id: WorkElementId,
        area_id: WorkAreaId,
        relation_id: WorkRelationId,
    ) -> Result<Self, WorkError> {
        edit.validate()?;
        let mut next = self.clone();
        if next.lifecycle == WorkLifecycle::Archived
            && !matches!(edit, WorkEnvironmentEdit::SetLifecycle { .. })
        {
            return Err(WorkError::Conflict);
        }
        match edit {
            WorkEnvironmentEdit::Rename { title } => next.title = title,
            WorkEnvironmentEdit::SetLifecycle { lifecycle } => next.lifecycle = lifecycle,
            WorkEnvironmentEdit::Add { reference, area } => {
                if next.elements.iter().any(|e| e.reference == reference) {
                    return Err(WorkError::Conflict);
                }
                if next.elements.len() >= MAX_ENVIRONMENT_ELEMENTS {
                    return Err(WorkError::Capacity);
                }
                next.elements.push(WorkEnvironmentElement {
                    id: element_id,
                    reference,
                    area,
                });
            }
            WorkEnvironmentEdit::Remove { element } => {
                let count = next.elements.len();
                next.elements.retain(|e| e.id != element);
                if count == next.elements.len() {
                    return Err(WorkError::NotFound);
                }
                next.view.placements.retain(|p| p.element != element);
                next.relations
                    .retain(|r| r.from != element && r.to != element);
                next.view.revision = next.view.revision.next()?;
            }
            WorkEnvironmentEdit::Relate { from, to, relation } => {
                if next.relations.len() >= MAX_ENVIRONMENT_RELATIONS {
                    return Err(WorkError::Capacity);
                }
                if next
                    .relations
                    .iter()
                    .any(|r| r.from == from && r.to == to && r.kind == relation)
                {
                    return Err(WorkError::Conflict);
                }
                next.relations.push(WorkRelation {
                    id: relation_id,
                    from,
                    to,
                    kind: relation,
                    origin: WorkRelationOrigin::User,
                });
            }
            WorkEnvironmentEdit::Unrelate { relation } => {
                let count = next.relations.len();
                next.relations.retain(|r| r.id != relation);
                if count == next.relations.len() {
                    return Err(WorkError::NotFound);
                }
            }
            WorkEnvironmentEdit::CreateArea { title } => {
                if next.areas.len() >= MAX_ENVIRONMENT_AREAS {
                    return Err(WorkError::Capacity);
                }
                next.areas.push(WorkArea { id: area_id, title });
            }
            WorkEnvironmentEdit::RenameArea { area, title } => {
                next.areas
                    .iter_mut()
                    .find(|a| a.id == area)
                    .ok_or(WorkError::NotFound)?
                    .title = title;
            }
            WorkEnvironmentEdit::RemoveArea { area } => {
                let count = next.areas.len();
                next.areas.retain(|a| a.id != area);
                if count == next.areas.len() {
                    return Err(WorkError::NotFound);
                }
                for element in &mut next.elements {
                    if element.area == Some(area) {
                        element.area = None;
                    }
                }
                if next.view.areas.iter().any(|a| a.area == area) {
                    next.view.areas.retain(|a| a.area != area);
                    next.view.revision = next.view.revision.next()?;
                }
            }
            WorkEnvironmentEdit::AssignArea { element, area } => {
                next.elements
                    .iter_mut()
                    .find(|e| e.id == element)
                    .ok_or(WorkError::NotFound)?
                    .area = area;
            }
        }
        next.revision = self.revision.next()?;
        next.validate()?;
        Ok(next)
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEnvironmentEdit {
    Rename {
        title: String,
    },
    SetLifecycle {
        lifecycle: WorkLifecycle,
    },
    Add {
        reference: WorkEnvironmentReference,
        area: Option<WorkAreaId>,
    },
    Remove {
        element: WorkElementId,
    },
    CreateArea {
        title: String,
    },
    RenameArea {
        area: WorkAreaId,
        title: String,
    },
    RemoveArea {
        area: WorkAreaId,
    },
    AssignArea {
        element: WorkElementId,
        area: Option<WorkAreaId>,
    },
    Relate {
        from: WorkElementId,
        to: WorkElementId,
        relation: WorkRelationKind,
    },
    Unrelate {
        relation: WorkRelationId,
    },
}
impl WorkEnvironmentEdit {
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::Rename { title }
            | Self::CreateArea { title }
            | Self::RenameArea { title, .. } => validate_text(title, MAX_ENVIRONMENT_TITLE_BYTES),
            Self::Relate { from, to, .. } if from == to => Err(WorkError::Invalid),
            _ => Ok(()),
        }
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEnvironmentIntent {
    Create {
        space: SpaceId,
        title: String,
    },
    Edit {
        id: WorkEnvironmentId,
        expected: WorkRevision,
        edit: WorkEnvironmentEdit,
    },
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEnvironmentCall {
    /// Revision-scoped presentation save; stale identities never execute again.
    Checkpoint {
        id: WorkEnvironmentId,
        expected: WorkRevision,
        view: WorkEnvironmentView,
    },

    /// Idempotent user selection; updates only the Space's last-opened Work.
    Open {
        id: WorkEnvironmentId,
    },
    Read {
        id: WorkEnvironmentId,
    },
    List {
        space: SpaceId,
        after: Option<WorkEnvironmentId>,
        limit: u16,
    },
    Command {
        command: WorkCommandId,
        intent: WorkEnvironmentIntent,
    },
}
impl WorkEnvironmentCall {
    pub fn validate(&self) -> Result<(), WorkError> {
        match self {
            Self::Checkpoint { expected, view, .. } => {
                view.validate()?;
                if view.revision != *expected {
                    return Err(WorkError::Conflict);
                }
                Ok(())
            }
            Self::List { limit, .. } if *limit == 0 || usize::from(*limit) > MAX_WORK_PAGE_SIZE => {
                Err(WorkError::Invalid)
            }
            Self::Command { intent, .. } => match intent {
                WorkEnvironmentIntent::Create { title, .. } => {
                    validate_text(title, MAX_ENVIRONMENT_TITLE_BYTES)
                }
                WorkEnvironmentIntent::Edit { edit, .. } => edit.validate(),
            },
            _ => Ok(()),
        }
    }
    pub fn space(&self) -> Option<SpaceId> {
        match self {
            Self::List { space, .. }
            | Self::Command {
                intent: WorkEnvironmentIntent::Create { space, .. },
                ..
            } => Some(*space),
            _ => None,
        }
    }
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkEnvironmentSummary {
    pub id: WorkEnvironmentId,
    pub space: SpaceId,
    pub title: String,
    pub lifecycle: WorkLifecycle,
    pub revision: WorkRevision,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkEnvironmentReply {
    Checkpointed {
        expected: WorkRevision,
        applied_view_revision: WorkRevision,
        replayed: bool,
        snapshot: Box<WorkEnvironmentSnapshot>,
    },
    Snapshot {
        snapshot: Box<WorkEnvironmentSnapshot>,
    },
    Applied {
        command: WorkCommandId,
        applied_revision: WorkRevision,
        applied_view_revision: WorkRevision,
        replayed: bool,
        snapshot: Box<WorkEnvironmentSnapshot>,
    },
    Page {
        works: Vec<WorkEnvironmentSummary>,
        next: Option<WorkEnvironmentId>,
        selected: Option<WorkEnvironmentId>,
    },
}

macro_rules! redacted_debug {
    ($($kind:ty),+) => { $(impl std::fmt::Debug for $kind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(concat!(stringify!($kind), "([content redacted])")) }
    })+ };
}
redacted_debug!(
    WorkEnvironmentCall,
    WorkEnvironmentReply,
    WorkEnvironmentSnapshot
);

#[cfg(test)]
mod tests;
