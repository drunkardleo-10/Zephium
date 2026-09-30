//! One bounded worker for document-policy lookup and script encoding.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

type MainJob = Box<dyn FnOnce() + Send>;
type Job = Box<dyn FnOnce() -> Option<MainJob> + Send>;
pub(super) struct StyleWorker {
    sender: Option<mpsc::SyncSender<Job>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl StyleWorker {
    pub(super) fn new(dispatch: crate::MainThreadDispatch) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(32);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("blocker-styles".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    if let Ok(Some(completion)) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                    {
                        let _ = dispatch(completion);
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            stop,
            thread: Some(thread),
        })
    }
    pub(super) fn submit(&self, job: impl FnOnce() -> Option<MainJob> + Send + 'static) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(Box::new(job));
        }
    }
}
impl Drop for StyleWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.sender.take();
        // Jobs contain pure bounded lookup/encoding and never wait for the UI.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_and_native_completion_have_different_thread_owners() {
        let owner = std::thread::current().id();
        let (send, receive) = mpsc::sync_channel::<MainJob>(1);
        let worker = StyleWorker::new(Arc::new(move |job| send.try_send(job).is_ok())).unwrap();
        let completed = Arc::new(AtomicBool::new(false));
        let observed = completed.clone();
        worker.submit(move || {
            assert_ne!(std::thread::current().id(), owner);
            Some(Box::new(move || {
                assert_eq!(std::thread::current().id(), owner);
                observed.store(true, Ordering::Release);
            }))
        });
        let completion = receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(!completed.load(Ordering::Acquire));
        completion();
        assert!(completed.load(Ordering::Acquire));
    }
}
