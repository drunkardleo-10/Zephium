use super::*;
use crate::work_files::WorkFileGrant;
impl Driver {
    pub(super) async fn command_step(
        &mut self,
        mut kind: WorkStepKindV1,
        files: &WorkFileGrant,
    ) -> Result<Option<WorkAttemptStatus>, WorkError> {
        let WorkStepKindV1::RunCommand {
            cwd,
            command,
            timeout_secs,
            ..
        } = &mut kind
        else {
            return Err(WorkError::Invalid);
        };
        let directory = match files.resolve(cwd, false) {
            Ok(path) if path.is_dir() => path,
            _ => {
                self.notice("The command folder is not an existing granted folder.");
                return Ok(None);
            }
        };
        *cwd = directory.to_string_lossy().into_owned();
        let root = files
            .roots()
            .iter()
            .filter(|r| directory.starts_with(r))
            .max_by_key(|r| r.components().count())
            .ok_or(WorkError::Invalid)?
            .to_string_lossy()
            .into_owned();
        let (class, reason) =
            crate::work_commands::policy::classify(command, &directory, files.roots());
        let state = self.probe.runtime_projection().await?;
        let execution = state
            .executions
            .iter()
            .find(|e| e.id == self.probe.execution())
            .ok_or(WorkError::NotFound)?;
        let approved = execution.folder_approvals.iter().any(|a| a.root == root);
        let scope = match class {
            WorkCommandClassV1::Read => WorkCommandApprovalScopeV1::None,
            WorkCommandClassV1::Write if approved => WorkCommandApprovalScopeV1::None,
            WorkCommandClassV1::Write => WorkCommandApprovalScopeV1::Folder,
            WorkCommandClassV1::Ask => WorkCommandApprovalScopeV1::Command,
        };
        let command = command.clone();
        let timeout = timeout_secs.unwrap_or(120);
        let mut step = self.step(kind, WorkStepStatus::Running);
        step.local = Some(Box::new(WorkLocalStepV1 {
            policy: Some(WorkCommandPolicyV1 {
                class,
                reason,
                scope,
                root,
            }),
            ..Default::default()
        }));
        let id = self.begin(step, vec![], None).await?;
        if scope != WorkCommandApprovalScopeV1::None {
            self.probe.record_activity(WorkActivityV1::WaitingForHuman);
            let since = self.wait();
            loop {
                let state = self.probe.runtime_projection().await?;
                let decision = state
                    .executions
                    .iter()
                    .find(|e| e.id == self.probe.execution())
                    .and_then(|e| e.steps.iter().find(|s| s.id == id))
                    .and_then(|s| s.kind.file_decision());
                if let Some(approved) = decision {
                    self.resume(since);
                    if !approved {
                        self.notice("The command was declined. Do not repeat it unchanged.");
                        self.settle_file(id, WorkStepStatus::Failed, None, Some("Declined".into()))
                            .await?;
                        return Ok(None);
                    }
                    break;
                }
                if self.cancelled().await || since.elapsed() >= WAIT_PATIENCE {
                    self.resume(since);
                    self.settle_file(id, WorkStepStatus::Failed, None, Some("Stopped".into()))
                        .await?;
                    return Ok(Some(WorkAttemptStatus::Cancelled));
                }
                tokio::time::sleep(ASK_POLL).await;
            }
        }
        if self.cancelled().await {
            self.settle_file(id, WorkStepStatus::Failed, None, Some("Stopped".into()))
                .await?;
            return Ok(Some(WorkAttemptStatus::Cancelled));
        }
        let state = self.probe.runtime_projection().await?;
        if state
            .executions
            .iter()
            .find(|e| e.id == self.probe.execution())
            .is_some_and(|e| {
                e.steps.iter().any(|s| {
                    s.id != id
                        && s.status == WorkStepStatus::Running
                        && s.kind.files()
                        && s.kind.proposes_write()
                        && s.kind.file_decision().is_none()
                })
            })
        {
            return Err(WorkError::Conflict);
        }
        // Revalidate cwd after an approval wait; a replaced symlink cannot redirect it.
        if files
            .resolve(&directory.to_string_lossy(), false)
            .ok()
            .as_ref()
            != Some(&directory)
        {
            self.settle_file(
                id,
                WorkStepStatus::Failed,
                None,
                Some("The command folder changed".into()),
            )
            .await?;
            return Ok(None);
        }
        self.probe.record_activity(WorkActivityV1::Interacting);
        let driver: &Driver = self;
        let result = crate::work_commands::run(
            &directory,
            &command,
            timeout,
            || async {
                tokio::time::timeout(Duration::from_millis(100), driver.cancelled())
                    .await
                    .unwrap_or(false)
            },
            |output| async move {
                let _ = tokio::time::timeout(
                    Duration::from_millis(100),
                    driver
                        .probe
                        .commit_step(WorkRuntimeUpdate::CommandProgress {
                            execution: driver.probe.execution(),
                            attempt: driver.probe.attempt(),
                            step: id,
                            output,
                        }),
                )
                .await;
            },
        )
        .await;
        match result {
            Ok(result) => {
                let record = WorkCommandRecordV1 {
                    id: WorkArtifactId::generate(),
                    node: self.probe.node(),
                    attempt: self.probe.attempt(),
                    command: result.evidence,
                };
                self.probe
                    .commit_step(WorkRuntimeUpdate::SettleCommand {
                        execution: self.probe.execution(),
                        attempt: self.probe.attempt(),
                        step: id,
                        status: if result.succeeded {
                            WorkStepStatus::Succeeded
                        } else {
                            WorkStepStatus::Failed
                        },
                        record: Box::new(record.clone()),
                        note: result.note,
                    })
                    .await?;
                self.keep_command_preview(&record);
            }
            Err(note) => {
                self.settle_file(id, WorkStepStatus::Failed, None, Some(note.into()))
                    .await?
            }
        }
        if self.cancelled().await {
            Ok(Some(WorkAttemptStatus::Cancelled))
        } else {
            Ok(None)
        }
    }
    fn keep_command_preview(&mut self, record: &WorkCommandRecordV1) {
        let command = &record.command;
        self.keep_preview(WorkEvidencePreviewV1 {
            version: 1,
            link: WorkEvidenceLink {
                extraction_id: record.id,
                source_id: 1,
            },
            link_destination: None,
            origin: format!("file://{}", command.cwd),
            role: "command".into(),
            truncated: command.truncated,
            text: command.text.clone(),
            source_bytes: command.bytes.to_string(),
            source: WorkEvidenceSourceV1::Command {
                cwd: command.cwd.clone(),
                command: command.command.clone(),
                outcome: command.outcome(),
            },
        });
    }
}
