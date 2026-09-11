use crate::work_authoring::{WorkDocumentProjection, WorkDocumentSubmission};
use zephium_core::work::WorkError;
impl crate::Shell {
    pub(crate) fn work_document(&self, submission: WorkDocumentSubmission) {
        let Some(crate::work_authoring::Payload {
            request,
            owner,
            reply,
            permit,
        }) = submission.take()
        else {
            return;
        };
        let profile = self
            .windows
            .focused()
            .and_then(|w| self.profiles.get(w.profile))
            .filter(|p| p.kind != zephium_core::profiles::ProfileKind::Incognito)
            .filter(|p| !self.profile_deletion_quarantines(p.id))
            .map(|p| p.id);
        let Some(profile) = profile else {
            let _ = reply.try_send(Err(WorkError::ProfileUnavailable));
            return;
        };
        let _ = owner.set(profile);
        let refused = reply.clone();
        let result = self.store.work_document(
            profile,
            request,
            Box::new(move |result| {
                let _permit = permit;
                let _ =
                    reply.try_send(result.map(|reply| WorkDocumentProjection { profile, reply }));
            }),
        );
        if let Err(error) = result {
            let _ = refused.try_send(Err(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Shell, WorkIntent, WorkUserEdit};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use zephium_core::work::{port::*, *};

    #[test]
    fn work_document_product_intent_uses_selected_profile_and_real_store_without_execution() {
        let store = Arc::new(zephium_store::SqliteStore::in_memory().unwrap());
        let mut shell = Shell::new(
            Arc::new(crate::shell::tests::FakeEngine::default()),
            store.clone(),
            Arc::new(crate::shell::tests::FakeChrome),
            Box::new(|_| {}),
        );
        shell.handle(Command::Bootstrap);
        assert!(store.flush());
        let profile = shell.windows.focused().unwrap().profile;
        let queue = crate::actor::CommandQueue::new();
        let handle = crate::actor::Handle::new(queue.clone());
        let request = handle
            .work_document(WorkIntent::Create {
                objective: "Compare local model deployment options".into(),
            })
            .unwrap();
        let minted_id = request.work_id().unwrap();
        let command = queue.try_recv().unwrap();
        let duplicate = command.clone();
        shell.handle(command);
        shell.handle(duplicate); // cloned Command owns only the same one-shot payload
        let projection = wait(&request).unwrap();
        assert!(
            request.try_recv().is_none(),
            "a known outcome cannot become uncertain later"
        );
        assert_eq!(projection.profile, profile);
        assert_eq!(request.profile(), Some(profile));
        let WorkReply::Snapshot(work) = projection.reply else {
            panic!()
        };
        let id = work.id;
        assert_eq!(id, minted_id);
        assert_eq!(work.objective_author, WorkAuthor::User);
        assert_eq!(work.status, WorkAuthoringStatus::Draft);
        assert!(work.plan.is_none());
        let request = handle
            .work_document(WorkIntent::Edit {
                id,
                expected: work.revision,
                edit: WorkUserEdit::OpenQuestion {
                    prompt: "Which hardware?".into(),
                    options: vec![],
                },
            })
            .unwrap();
        shell.handle(queue.try_recv().unwrap());
        let WorkReply::Snapshot(questioned) = wait(&request).unwrap().reply else {
            panic!()
        };
        assert_eq!(questioned.status, WorkAuthoringStatus::NeedsInput);
        // Private selection is refused before the Store adapter sees content.
        let mut private = shell.profiles.remove(profile).unwrap();
        private.kind = zephium_core::profiles::ProfileKind::Incognito;
        assert!(shell.profiles.insert(private));
        let request = handle.work_document(WorkIntent::Read { id }).unwrap();
        shell.handle(queue.try_recv().unwrap());
        assert!(matches!(wait(&request), Err(WorkError::ProfileUnavailable)));
        assert_eq!(
            store.shutdown_until(Instant::now() + Duration::from_secs(5)),
            zephium_core::ports::store::StoreShutdownOutcome::Clean
        );
    }
    fn wait(request: &crate::WorkDocumentRequest) -> Result<WorkDocumentProjection, WorkError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(reply) = request.try_recv() {
                return reply;
            }
            assert!(Instant::now() < deadline, "Work Store callback deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
