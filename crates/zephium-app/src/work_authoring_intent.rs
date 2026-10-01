//! User-facing authoring grammar. New durable identities and attribution are
//! selected by Rust, while existing identities are references checked by Store.
use zephium_core::work::{port::WorkRequest, *};

#[derive(Clone)]
pub enum WorkIntent {
    Create {
        objective: String,
    },
    Read {
        id: WorkId,
    },
    List {
        after: Option<WorkId>,
        limit: usize,
    },
    ListPlans {
        id: WorkId,
    },
    ReadPlan {
        id: WorkId,
        revision: WorkRevision,
    },
    Delete {
        id: WorkId,
        expected: WorkRevision,
    },
    Edit {
        id: WorkId,
        expected: WorkRevision,
        edit: WorkUserEdit,
    },
}
pub use zephium_core::work::authoring::WorkUserEdit;
impl WorkIntent {
    pub(crate) fn into_request(self) -> Result<WorkRequest, WorkError> {
        let author = WorkAuthor::User;
        let request = match self {
            Self::Create { objective } => WorkRequest::Create {
                id: WorkId::generate(),
                objective,
                author,
            },
            Self::Read { id } => WorkRequest::Read { id },
            Self::List { after, limit } => WorkRequest::List { after, limit },
            Self::ListPlans { id } => WorkRequest::ListPlans { id },
            Self::ReadPlan { id, revision } => WorkRequest::ReadPlan { id, revision },
            Self::Delete { id, expected } => WorkRequest::Delete { id, expected },
            Self::Edit { id, expected, edit } => WorkRequest::Edit {
                id,
                expected,
                author,
                edit: edit.into_edit()?,
            },
        };
        request.validate()?;
        Ok(request)
    }
}
impl std::fmt::Debug for WorkIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkIntent([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::work::proposal::{WorkNodeProposal, WorkPlanProposal};

    #[test]
    fn work_document_intents_mint_new_identity_graphs_and_fix_user_attribution() {
        let intent = WorkIntent::Create {
            objective: "A user objective".into(),
        };
        let WorkRequest::Create {
            id: first, author, ..
        } = intent.clone().into_request().unwrap()
        else {
            panic!()
        };
        let WorkRequest::Create { id: second, .. } = intent.into_request().unwrap() else {
            panic!()
        };
        assert_ne!(first, second);
        assert_eq!(author, WorkAuthor::User);
        let outputs = vec![WorkExpectedOutput {
            name: "findings".into(),
            description: "Cited evidence".into(),
            review: WorkOutputReview::SourceMappedNeedsReview,
        }];
        let proposal = WorkPlanProposal {
            nodes: vec![
                WorkNodeProposal {
                    key: 8,
                    objective: "Read".into(),
                    dependencies: vec![],
                    outputs: outputs.clone(),
                },
                WorkNodeProposal {
                    key: 2,
                    objective: "Compare".into(),
                    dependencies: vec![8],
                    outputs,
                },
            ],
        };
        let request = WorkIntent::Edit {
            id: first,
            expected: WorkRevision::INITIAL,
            edit: WorkUserEdit::ReplaceDraft { proposal },
        }
        .into_request()
        .unwrap();
        let WorkRequest::Edit {
            author,
            edit: WorkEdit::ReplaceDraft { draft },
            ..
        } = request
        else {
            panic!()
        };
        assert_eq!(author, WorkAuthor::User);
        assert_eq!(draft.nodes[1].dependencies, vec![draft.nodes[0].id]);
        assert_ne!(draft.nodes[0].id, WorkPlanNodeId::from(8));
    }

    #[test]
    fn work_document_lost_create_reply_retains_minted_identity_for_reconciliation() {
        let queue = crate::actor::CommandQueue::new();
        let handle = crate::actor::Handle::new(queue.clone());
        let request = handle
            .work_document(WorkIntent::Create {
                objective: "Persist this goal".into(),
            })
            .unwrap();
        let id = request.work_id().unwrap();
        assert_eq!(request.profile(), None);
        drop(queue.try_recv().unwrap()); // accepted command lost before a settlement
        assert!(matches!(
            request.try_recv(),
            Some(Err(WorkError::OutcomeUnknown))
        ));
        assert_eq!(request.work_id(), Some(id));
        assert!(request.try_recv().is_none());
    }
}
