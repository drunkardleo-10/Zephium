use super::*;

impl Downloads {
    pub(in crate::host) fn is_retired(&self, profile: ProfileId) -> bool {
        self.retired.borrow().contains(&profile)
    }

    pub(in crate::host) fn quiesce(
        self: &Rc<Self>,
        profile: Option<ProfileId>,
        done: Box<dyn FnOnce(bool) + Send>,
    ) {
        if let Some(profile) = profile {
            self.retired.borrow_mut().insert(profile);
        } else {
            self.stopping.set(true);
        }
        self.waiters.borrow_mut().push((profile, done));
        let ids: Vec<_> = self
            .active
            .borrow()
            .iter()
            .filter(|(_, transfer)| {
                profile.is_none_or(|profile| transfer.partition.profile() == profile)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.cancel(id, None);
        }
        let tokens: Vec<_> = self
            .calls
            .borrow()
            .iter()
            .filter(|(_, call)| profile.is_none_or(|profile| call.partition.profile() == profile))
            .map(|(token, _)| *token)
            .collect();
        for token in tokens {
            let request = self.calls.borrow_mut().remove(&token);
            let panel = self.directory_panels.borrow_mut().remove(&token);
            if let Some((panel, _lease)) = panel {
                unsafe { panel.cancel(None) };
                panel.orderOut(None);
            }
            drop(request);
        }
        self.recent
            .borrow_mut()
            .retain(|(owner, _)| profile.is_some_and(|profile| owner.profile() != profile));
        self.preferences
            .borrow_mut()
            .retain(|owner, _| profile.is_some_and(|profile| *owner != profile));
        self.forgotten
            .borrow_mut()
            .retain(|(owner, _)| profile.is_some_and(|profile| *owner != profile));
        self.recovered.borrow_mut().clear();
        self.ensure_timer();
    }
}
