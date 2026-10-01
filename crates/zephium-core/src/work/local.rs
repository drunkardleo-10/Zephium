//! Durable local work contracts; execution authority remains in the application.
use super::*;
pub const MAX_WORK_RUNNING_COMMANDS: usize = 2;
pub const MAX_WORK_COMMAND_BYTES: usize = 4096;
pub const MAX_WORK_COMMAND_MEMORY_BYTES: usize = 256 * 1024;
pub const MAX_WORK_FILE_REPLACEMENTS: usize = 8;

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFileReplacementV1 {
    pub old: String,
    pub new: String,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFileLinesV1 {
    pub first: u32,
    pub last: u32,
    pub total: u32,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum WorkCommandClassV1 {
    Read,
    Write,
    Ask,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkCommandReasonV1 {
    Inspection,
    ProjectExecution,
    FileChange,
    UnknownProgram,
    Network,
    Destructive,
    Privilege,
    OutsideRoots,
    ShellSyntax,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkCommandApprovalScopeV1 {
    None,
    Folder,
    Command,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandPolicyV1 {
    pub class: WorkCommandClassV1,
    pub reason: WorkCommandReasonV1,
    pub scope: WorkCommandApprovalScopeV1,
    pub root: String,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFolderApprovalV1 {
    pub root: String,
    /// Unix seconds as decimal text, without a JavaScript integer precision loss.
    pub at: String,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandOutputV1 {
    /// Present with output only after the process starts; excludes approval time.
    #[serde(default)]
    pub elapsed_ms: u32,
    pub text: String,
    pub bytes: u32,
    pub truncated: bool,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct WorkLocalStepV1 {
    /// Observed public title for a read step's URL, retained after settlement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<WorkCommandPolicyV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<WorkCommandOutputV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_digest: Option<String>,
    /// The folder a `folder` question asks to read, as an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}
impl WorkLocalStepV1 {
    pub fn validate(&self, kind: &WorkStepKindV1) -> Result<(), WorkError> {
        if let Some(title) = &self.page_title {
            if !matches!(kind, WorkStepKindV1::Read { .. }) {
                return Err(WorkError::Invalid);
            }
            validate_page_title(title)?;
        }
        if let Some(policy) = &self.policy {
            if !matches!(kind, WorkStepKindV1::RunCommand { .. }) {
                return Err(WorkError::Invalid);
            }
            validate_file_path(&policy.root)?;
            if !matches!(
                (policy.class, policy.scope),
                (WorkCommandClassV1::Read, WorkCommandApprovalScopeV1::None)
                    | (
                        WorkCommandClassV1::Write,
                        WorkCommandApprovalScopeV1::Folder | WorkCommandApprovalScopeV1::None
                    )
                    | (WorkCommandClassV1::Ask, WorkCommandApprovalScopeV1::Command)
            ) {
                return Err(WorkError::Invalid);
            }
        }
        if let Some(output) = &self.output {
            validate_local_text(&output.text)?;
            if self.policy.is_none() {
                return Err(WorkError::Invalid);
            }
        }
        if let Some(proposal) = &self.proposal {
            validate_local_text(proposal)?;
            if !kind.files() {
                return Err(WorkError::Invalid);
            }
        }
        if let Some(digest) = &self.before_digest {
            validate_digest(digest)?;
        }
        let folder_ask = matches!(
            kind,
            WorkStepKindV1::Ask {
                purpose: Some(super::runtime::WorkAskPurposeV1::Folder),
                ..
            }
        );
        match &self.folder {
            Some(folder) if folder_ask => validate_file_path(folder)?,
            None if !folder_ask => {}
            _ => return Err(WorkError::Invalid),
        }
        Ok(())
    }
}

pub fn validate_page_title(title: &str) -> Result<(), WorkError> {
    if title.is_empty()
        || title.len() > crate::work::environment::MAX_ENVIRONMENT_TITLE_BYTES
        || title.chars().any(char::is_control)
    {
        return Err(WorkError::Invalid);
    }
    Ok(())
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandEvidenceV1 {
    pub cwd: String,
    pub command: String,
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub elapsed_ms: u32,
    pub bytes: u32,
    pub digest: String,
    pub text: String,
    pub truncated: bool,
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandOutcomeV1 {
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub elapsed_ms: u32,
    pub bytes: u32,
    pub digest: String,
}
impl WorkCommandEvidenceV1 {
    pub fn outcome(&self) -> WorkCommandOutcomeV1 {
        WorkCommandOutcomeV1 {
            exit: self.exit,
            signal: self.signal,
            elapsed_ms: self.elapsed_ms,
            bytes: self.bytes,
            digest: self.digest.clone(),
        }
    }
    pub fn validate(&self) -> Result<(), WorkError> {
        validate_file_path(&self.cwd)?;
        validate_command(&self.command)?;
        validate_digest(&self.digest)?;
        validate_local_text(&self.text)?;
        if self.exit.is_some() && self.signal.is_some() {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}
#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkCommandRecordV1 {
    pub id: WorkArtifactId,
    pub node: WorkPlanNodeId,
    pub attempt: WorkAttemptId,
    pub command: WorkCommandEvidenceV1,
}
pub fn validate_command(command: &str) -> Result<(), WorkError> {
    if command.trim().is_empty()
        || command.len() > MAX_WORK_COMMAND_BYTES
        || command.chars().any(|c| c.is_control() && c != '\t')
    {
        return Err(WorkError::Invalid);
    }
    Ok(())
}
pub fn validate_digest(digest: &str) -> Result<(), WorkError> {
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(WorkError::Invalid);
    }
    Ok(())
}
pub fn validate_local_text(text: &str) -> Result<(), WorkError> {
    if text.len() > MAX_WORK_FILE_TEXT_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(WorkError::Invalid);
    }
    Ok(())
}
pub fn validate_replacements(
    old: &str,
    new: &str,
    replacements: &[WorkFileReplacementV1],
) -> Result<(), WorkError> {
    if replacements.is_empty() {
        if old.is_empty()
            || old.len() > MAX_WORK_FILE_CONTENT_BYTES
            || new.len() > MAX_WORK_FILE_CONTENT_BYTES
            || old.contains('\0')
            || new.contains('\0')
        {
            return Err(WorkError::Invalid);
        }
    } else {
        if !old.is_empty() || !new.is_empty() || replacements.len() > MAX_WORK_FILE_REPLACEMENTS {
            return Err(WorkError::Invalid);
        }
        let mut seen = BTreeSet::new();
        let mut old_bytes = 0;
        let mut new_bytes = 0;
        for r in replacements {
            validate_replacements(&r.old, &r.new, &[])?;
            if !seen.insert(&r.old) {
                return Err(WorkError::Invalid);
            }
            old_bytes += r.old.len();
            new_bytes += r.new.len();
        }
        if old_bytes > MAX_WORK_FILE_CONTENT_BYTES || new_bytes > MAX_WORK_FILE_CONTENT_BYTES {
            return Err(WorkError::Invalid);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_live_metadata_keeps_legacy_journals_readable() {
        let output: WorkCommandOutputV1 =
            serde_json::from_str(r#"{"text":"","bytes":0,"truncated":false}"#).unwrap();
        assert_eq!(output.elapsed_ms, 0);
        let legacy: WorkLocalStepV1 = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.page_title, None);
        let titled = WorkLocalStepV1 {
            page_title: Some("Read title".into()),
            ..Default::default()
        };
        assert!(titled
            .validate(&WorkStepKindV1::Read {
                url: "https://example.com".into(),
                collection: None,
                goal: None,
            })
            .is_ok());
        assert!(titled.validate(&WorkStepKindV1::Turn).is_err());
        assert!(validate_page_title(&"a".repeat(513)).is_err());
    }
    #[test]
    fn work_local_legacy_edits_and_command_bounds() {
        let legacy = serde_json::from_str::<WorkStepKindV1>(r#"{"kind":"edit_file","path":"/Users/a/project/a","old":"a","new":"b","decision":null}"#).unwrap();
        assert!(legacy.validate().is_ok());
        for command in ["", "a\n", "a\0", "a\r"] {
            assert!(validate_command(command).is_err());
        }
        assert!(validate_command(&"a".repeat(4096)).is_ok());
        assert!(validate_command(&"a".repeat(4097)).is_err());
        assert!(validate_command("echo\thi").is_ok());
        for timeout in [0, 601] {
            assert!(WorkStepKindV1::RunCommand {
                cwd: "/Users/a/project".into(),
                command: "pwd".into(),
                timeout_secs: Some(timeout),
                decision: None
            }
            .validate()
            .is_err());
        }
        let repeated = vec![
            WorkFileReplacementV1 {
                old: "a".into(),
                new: "b".into()
            };
            2
        ];
        assert!(validate_replacements("", "", &repeated).is_err());
    }
}
