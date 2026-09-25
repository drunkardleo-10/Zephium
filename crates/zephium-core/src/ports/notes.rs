//! The notes folder service, as the shell sees it.
use std::sync::Arc;

use crate::ids::ProfileId;
use crate::notes::{NoteCall, NoteDone};

pub trait Notes: Send + Sync {
    /// Bounded asynchronous access to a profile's notes. Caller owns authorization.
    fn call(&self, profile: ProfileId, call: NoteCall, done: NoteDone);
    /// Closes a profile's notes for the rest of the process, ahead of erasing them.
    fn release(&self, profile: ProfileId, done: Box<dyn FnOnce() + Send>);
}

pub type SharedNotes = Arc<dyn Notes>;
