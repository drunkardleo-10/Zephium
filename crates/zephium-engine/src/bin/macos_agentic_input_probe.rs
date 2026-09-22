//! One-process JSONL controller for the release-excluded native-input probe.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos-agentic-input-probe: unsupported platform");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    macos::main();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::io::{self, Write as _};
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use zephium_agentic::{
        encode_response_line, CancelledReply, CaseOutcome, FixtureCase, HelloReply, InputBackend,
        InputEventKind, PresentationState, ProbeAdmissionError, ProbeCommand, ProbeFailure,
        ProbeFailureCode, ProbeGate, ProbeReply, ProbeRequest, ProbeResponse, ProbeStage,
        RunMatrixRequest, ShutdownReply, MAX_PROTOCOL_INPUT_BYTES, PROBE_PROTOCOL_VERSION,
    };

    const INGRESS_CAPACITY: usize = 16;
    const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

    pub(super) fn main() {
        let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
        if arguments.len() == 1 && arguments[0] == "--ci-hidden-fixed-dom" {
            run_ci_hidden_fixed_dom();
            return;
        }
        let (allow_visible_focused, owned_view) = match arguments.as_slice() {
            [] => (false, false),
            [argument] if argument == "--allow-visible-focused" => (true, false),
            [argument] if argument == "--owned-view-control" => (true, true),
            _ => fail("expected no arguments, --allow-visible-focused, --owned-view-control, or --ci-hidden-fixed-dom"),
        };
        let mut reader = match ControllerReader::start() {
            Ok(reader) => reader,
            Err(()) => fail("controller input initialization failed"),
        };
        let gate = ProbeGate::new();
        let (run_request_id, matrix, permit) = loop {
            let request = match reader.recv_timeout(FIRST_REQUEST_TIMEOUT) {
                Ok(request) => request,
                Err(_) => fail("controller request deadline expired"),
            };
            match request.command {
                ProbeCommand::Hello(_) => {
                    if !emit(ProbeResponse {
                        protocol_version: PROBE_PROTOCOL_VERSION,
                        request_id: request.request_id,
                        reply: ProbeReply::Hello(HelloReply::current()),
                    }) {
                        fail("controller output failed");
                    }
                }
                ProbeCommand::Shutdown(_) => {
                    if !emit(ProbeResponse {
                        protocol_version: PROBE_PROTOCOL_VERSION,
                        request_id: request.request_id,
                        reply: ProbeReply::Shutdown(ShutdownReply { drained: true }),
                    }) {
                        fail("controller output failed");
                    }
                    reader.stop_and_join();
                    return;
                }
                ProbeCommand::Cancel(_) => {
                    emit_rejection(
                        request.request_id,
                        ProbeFailureCode::InvalidRequest,
                        ProbeStage::Admit,
                        false,
                    );
                }
                ProbeCommand::RunMatrix(matrix) => {
                    if !presentation_is_authorized(&matrix, allow_visible_focused) {
                        emit_rejection(
                            request.request_id,
                            ProbeFailureCode::FocusPolicyViolation,
                            ProbeStage::Admit,
                            false,
                        );
                        continue;
                    }
                    let permit = match gate.try_start(request.request_id) {
                        Ok(permit) => permit,
                        Err(ProbeAdmissionError::Busy) => {
                            emit_rejection(
                                request.request_id,
                                ProbeFailureCode::ResourceExhausted,
                                ProbeStage::Admit,
                                true,
                            );
                            continue;
                        }
                        Err(ProbeAdmissionError::ZeroRequestId) => {
                            emit_rejection(
                                request.request_id,
                                ProbeFailureCode::InvalidRequest,
                                ProbeStage::Admit,
                                false,
                            );
                            continue;
                        }
                    };
                    break (request.request_id, matrix, permit);
                }
            }
        };

        let output_failed = std::cell::Cell::new(false);
        let shutdown_request = std::cell::Cell::new(None::<u64>);
        let run = if owned_view {
            zephium_engine::run_macos_owned_input_matrix
        } else {
            zephium_engine::run_macos_agentic_input_matrix
        };
        let result = run(run_request_id, &matrix, &permit, || {
            while let Ok(request) = reader.try_recv() {
                match request.command {
                    ProbeCommand::Cancel(cancel) => {
                        if gate.cancel(cancel.target_request_id) {
                            output_failed.set(
                                output_failed.get()
                                    || !emit(ProbeResponse {
                                        protocol_version: PROBE_PROTOCOL_VERSION,
                                        request_id: request.request_id,
                                        reply: ProbeReply::Cancelled(CancelledReply {
                                            target_request_id: cancel.target_request_id,
                                        }),
                                    }),
                            );
                        } else {
                            emit_rejection(
                                request.request_id,
                                ProbeFailureCode::InvalidRequest,
                                ProbeStage::Admit,
                                false,
                            );
                        }
                    }
                    ProbeCommand::Hello(_) => {
                        output_failed.set(
                            output_failed.get()
                                || !emit(ProbeResponse {
                                    protocol_version: PROBE_PROTOCOL_VERSION,
                                    request_id: request.request_id,
                                    reply: ProbeReply::Hello(HelloReply::current()),
                                }),
                        );
                    }
                    ProbeCommand::Shutdown(_) => {
                        if shutdown_request.get().is_none() {
                            shutdown_request.set(Some(request.request_id));
                            let _ = gate.cancel(run_request_id);
                        } else {
                            emit_rejection(
                                request.request_id,
                                ProbeFailureCode::ResourceExhausted,
                                ProbeStage::Teardown,
                                true,
                            );
                        }
                    }
                    ProbeCommand::RunMatrix(_) => {
                        emit_rejection(
                            request.request_id,
                            ProbeFailureCode::ResourceExhausted,
                            ProbeStage::Admit,
                            true,
                        );
                    }
                }
            }
            if reader.failed() || output_failed.get() {
                let _ = gate.cancel(run_request_id);
            }
        });
        drop(permit);

        let response = ProbeResponse {
            protocol_version: PROBE_PROTOCOL_VERSION,
            request_id: run_request_id,
            reply: match result {
                Ok(evidence) => ProbeReply::RunCompleted(evidence),
                Err(failure) => ProbeReply::Rejected(failure),
            },
        };
        if !emit(response) {
            output_failed.set(true);
        }
        if let Some(request_id) = shutdown_request.get() {
            if !emit(ProbeResponse {
                protocol_version: PROBE_PROTOCOL_VERSION,
                request_id,
                reply: ProbeReply::Shutdown(ShutdownReply { drained: true }),
            }) {
                output_failed.set(true);
            }
        }
        reader.stop_and_join();
        if reader.failed() || output_failed.get() {
            fail("controller transport failed closed");
        }
    }

    fn run_ci_hidden_fixed_dom() {
        let gate = ProbeGate::new();
        let permit = gate
            .try_start(1)
            .unwrap_or_else(|_| fail("CI permit admission failed"));
        let matrix = RunMatrixRequest {
            cases: vec![
                FixtureCase::Button,
                FixtureCase::Link,
                FixtureCase::TextInput,
                FixtureCase::ContentEditable,
                FixtureCase::Select,
                FixtureCase::PointerMouse,
                FixtureCase::Keyboard,
                FixtureCase::TransientActivation,
                FixtureCase::Popup,
                FixtureCase::ClipboardGate,
                FixtureCase::Drag,
                FixtureCase::Iframe,
                FixtureCase::OpenShadow,
                FixtureCase::ClosedShadow,
            ],
            backends: vec![InputBackend::FixedDomRecipe],
            presentation: PresentationState::Hidden,
        };
        let evidence = zephium_engine::run_macos_agentic_input_matrix(1, &matrix, &permit, || {})
            .unwrap_or_else(|failure| fail_probe("CI hidden fixed-DOM matrix failed", failure));
        if !evidence.teardown.view_closed
            || !evidence.teardown.work_drained
            || evidence.teardown.retained_native_views != 0
        {
            fail("CI hidden fixed-DOM teardown invariant failed");
        }
        if evidence.cases.len() != matrix.cases.len() {
            fail("CI hidden fixed-DOM row-count invariant failed");
        }
        if evidence
            .cases
            .iter()
            .any(|case| case.focus.browse_focus_was_stolen)
        {
            fail("CI hidden fixed-DOM focus invariant failed");
        }
        if let Some((case, event)) = evidence
            .cases
            .iter()
            .flat_map(|case| case.events.iter().map(move |event| (case.case, event)))
            .find(|(_, event)| {
                event.is_trusted
                    && !matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
            })
        {
            eprintln!(
                "macos-agentic-input-probe: CI hidden fixed-DOM trusted-effect invariant failed; case={case:?}; event={:?}; target={:?}",
                event.kind, event.target
            );
            std::process::exit(2);
        }
        if let Some(case) = evidence.cases.iter().find(|case| {
            case.activation.active_before
                || case.activation.active_during_event
                || case.activation.active_after_event
                || case.activation.active_after_settle
                || case.activation.has_been_active
        }) {
            eprintln!(
                "macos-agentic-input-probe: CI hidden fixed-DOM activation invariant failed; case={:?}; before={}; during={}; after_event={}; after_settle={}; sticky={}",
                case.case,
                case.activation.active_before,
                case.activation.active_during_event,
                case.activation.active_after_event,
                case.activation.active_after_settle,
                case.activation.has_been_active
            );
            std::process::exit(2);
        }
        for case in &evidence.cases {
            let accepted = match case.case {
                FixtureCase::Popup => {
                    matches!(
                        case.outcome,
                        CaseOutcome::Verified | CaseOutcome::Unsupported
                    ) && !case.target.popup_observed
                }
                FixtureCase::ClipboardGate | FixtureCase::ClosedShadow => {
                    case.outcome == CaseOutcome::Unsupported
                }
                _ => case.outcome == CaseOutcome::Verified && case.target.target_verified,
            };
            if !accepted {
                fail("CI hidden fixed-DOM fixture result drifted");
            }
        }
        let trusted_focus_events = evidence
            .cases
            .iter()
            .flat_map(|case| &case.events)
            .filter(|event| {
                event.is_trusted
                    && matches!(event.kind, InputEventKind::Focus | InputEventKind::Blur)
            })
            .count();
        println!(
            "macos-agentic-input-probe: passed; profile=ephemeral; extensions=absent; page_world_bridge=absent; isolated_messages=bounded_one_way; os={}; engine={}; engine_version={}; presentation=hidden; backend=fixed-dom-recipe; cases=14; trusted_effect_events=0; trusted_focus_events={trusted_focus_events}; activation=0; focus_theft=0; retained_views=0",
            evidence.runtime.os_version.as_str(),
            evidence.runtime.engine.as_str(),
            evidence.runtime.engine_version.as_str(),
        );
    }

    fn emit(response: ProbeResponse) -> bool {
        let Ok(encoded) = encode_response_line(&response) else {
            return false;
        };
        let mut stdout = io::stdout().lock();
        stdout.write_all(&encoded).is_ok() && stdout.flush().is_ok()
    }

    fn presentation_is_authorized(matrix: &RunMatrixRequest, allow_visible_focused: bool) -> bool {
        matrix.presentation != PresentationState::VisibleFocused || allow_visible_focused
    }

    fn emit_rejection(request_id: u64, code: ProbeFailureCode, stage: ProbeStage, retryable: bool) {
        if !emit(ProbeResponse {
            protocol_version: PROBE_PROTOCOL_VERSION,
            request_id,
            reply: ProbeReply::Rejected(ProbeFailure {
                code,
                stage,
                backend: None,
                case: None,
                retryable,
            }),
        }) {
            fail("controller output failed");
        }
    }

    fn fail(message: &'static str) -> ! {
        eprintln!("macos-agentic-input-probe: {message}");
        std::process::exit(2);
    }

    fn fail_probe(message: &'static str, failure: ProbeFailure) -> ! {
        eprintln!(
            "macos-agentic-input-probe: {message}; code={:?}; stage={:?}; case={:?}; backend={:?}; retryable={}",
            failure.code, failure.stage, failure.case, failure.backend, failure.retryable
        );
        std::process::exit(2);
    }

    struct ControllerReader {
        receiver: mpsc::Receiver<ProbeRequest>,
        stop: Arc<AtomicBool>,
        failed: Arc<AtomicBool>,
        stop_waker: Option<OwnedFd>,
        worker: Option<JoinHandle<()>>,
    }

    impl ControllerReader {
        fn start() -> Result<Self, ()> {
            let stop = Arc::new(AtomicBool::new(false));
            let failed = Arc::new(AtomicBool::new(false));
            let (sender, receiver) = mpsc::sync_channel(INGRESS_CAPACITY);
            let (stop_reader, stop_waker) = controller_stop_pipe()?;
            let worker_stop = Arc::clone(&stop);
            let worker_failed = Arc::clone(&failed);
            let worker = std::thread::Builder::new()
                .name("zephium-agentic-probe-control".to_owned())
                .spawn(move || read_stdin(sender, &worker_stop, &worker_failed, stop_reader))
                .map_err(|_| ())?;
            Ok(Self {
                receiver,
                stop,
                failed,
                stop_waker: Some(stop_waker),
                worker: Some(worker),
            })
        }

        fn recv_timeout(&self, duration: Duration) -> Result<ProbeRequest, mpsc::RecvTimeoutError> {
            self.receiver.recv_timeout(duration)
        }

        fn try_recv(&self) -> Result<ProbeRequest, mpsc::TryRecvError> {
            self.receiver.try_recv()
        }

        fn failed(&self) -> bool {
            self.failed.load(Ordering::Acquire)
        }

        fn stop_and_join(&mut self) {
            self.stop.store(true, Ordering::Release);
            // Closing the sole write end wakes the worker's blocking poll via
            // POLLHUP without a timer or a SIGPIPE-prone wake write.
            drop(self.stop_waker.take());
            if self
                .worker
                .take()
                .is_some_and(|worker| worker.join().is_err())
            {
                self.failed.store(true, Ordering::Release);
            }
        }
    }

    impl Drop for ControllerReader {
        fn drop(&mut self) {
            self.stop_and_join();
        }
    }

    fn read_stdin(
        sender: mpsc::SyncSender<ProbeRequest>,
        stop: &AtomicBool,
        failed: &AtomicBool,
        stop_reader: OwnedFd,
    ) {
        let Ok(descriptor) = duplicate_controller_input() else {
            failed.store(true, Ordering::Release);
            return;
        };
        let mut buffered = Vec::with_capacity(MAX_PROTOCOL_INPUT_BYTES);
        let mut chunk = [0_u8; 1_024];
        while !stop.load(Ordering::Acquire) {
            let mut readiness = [
                libc::pollfd {
                    fd: descriptor.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop_reader.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: readiness contains two initialized pollfd records and
            // both OwnedFd values remain live for the entire blocking call.
            let ready = unsafe { libc::poll(readiness.as_mut_ptr(), readiness.len() as _, -1) };
            if ready < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                failed.store(true, Ordering::Release);
                break;
            }
            if readiness[1].revents
                & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                != 0
            {
                break;
            }
            if readiness[0].revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                failed.store(true, Ordering::Release);
                break;
            }
            if readiness[0].revents & (libc::POLLIN | libc::POLLHUP) == 0 {
                continue;
            }
            // SAFETY: chunk is writable for its full declared length and the
            // descriptor has just reported readable or terminal state.
            let count = unsafe {
                libc::read(
                    descriptor.as_raw_fd(),
                    chunk.as_mut_ptr().cast::<c_void>(),
                    chunk.len(),
                )
            };
            if count > 0 {
                let count = usize::try_from(count).unwrap_or(0);
                buffered.extend_from_slice(&chunk[..count]);
                if !drain_lines(&mut buffered, &sender, failed) {
                    break;
                }
            } else if count == 0 {
                if !buffered.is_empty() && !send_line(&buffered, &sender, failed) {
                    break;
                }
                buffered.clear();
                break;
            } else {
                failed.store(true, Ordering::Release);
                break;
            }
            if buffered.len() > MAX_PROTOCOL_INPUT_BYTES {
                failed.store(true, Ordering::Release);
                break;
            }
        }
    }

    fn duplicate_controller_input() -> Result<OwnedFd, ()> {
        // SAFETY: F_DUPFD_CLOEXEC creates an independently closeable
        // descriptor atomically excluded from WebKit helper inheritance.
        let descriptor = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 0) };
        if descriptor < 0 {
            Err(())
        } else {
            // SAFETY: fcntl returned one newly owned descriptor.
            Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
        }
    }

    fn controller_stop_pipe() -> Result<(OwnedFd, OwnedFd), ()> {
        let mut descriptors = [-1; 2];
        // SAFETY: descriptors points to writable storage for two descriptors.
        if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
            return Err(());
        }
        for descriptor in descriptors {
            // SAFETY: both descriptors are live and exclusively owned here.
            let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
            if flags < 0
                || unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
            {
                // SAFETY: each descriptor is still raw and closed exactly once.
                unsafe {
                    libc::close(descriptors[0]);
                    libc::close(descriptors[1]);
                }
                return Err(());
            }
        }
        // SAFETY: successful pipe returned two distinct owned descriptors.
        Ok(unsafe {
            (
                OwnedFd::from_raw_fd(descriptors[0]),
                OwnedFd::from_raw_fd(descriptors[1]),
            )
        })
    }

    fn drain_lines(
        buffered: &mut Vec<u8>,
        sender: &mpsc::SyncSender<ProbeRequest>,
        failed: &AtomicBool,
    ) -> bool {
        while let Some(newline) = buffered.iter().position(|byte| *byte == b'\n') {
            let line = buffered.drain(..=newline).collect::<Vec<_>>();
            if !send_line(&line, sender, failed) {
                return false;
            }
        }
        true
    }

    fn send_line(
        line: &[u8],
        sender: &mpsc::SyncSender<ProbeRequest>,
        failed: &AtomicBool,
    ) -> bool {
        let request = match zephium_agentic::decode_request_line(line) {
            Ok(request) => request,
            Err(_) => {
                failed.store(true, Ordering::Release);
                return false;
            }
        };
        if sender.try_send(request).is_err() {
            failed.store(true, Ordering::Release);
            false
        } else {
            true
        }
    }

    use std::ffi::c_void;

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn duplicated_controller_input_is_close_on_exec() {
            let descriptor = duplicate_controller_input().expect("duplicate input");
            // SAFETY: descriptor is a live duplicate owned by this test.
            let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
            assert_ne!(flags, -1);
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }

        #[test]
        fn close_on_exec_stop_pipe_wakes_without_polling() {
            let (reader, writer) = controller_stop_pipe().expect("stop pipe");
            for descriptor in [&reader, &writer] {
                // SAFETY: both descriptors are live for this inspection.
                let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
                assert_ne!(flags, -1);
                assert_ne!(flags & libc::FD_CLOEXEC, 0);
            }
            drop(writer);
            let mut readiness = libc::pollfd {
                fd: reader.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: readiness is one initialized pollfd and reader is live.
            assert_eq!(unsafe { libc::poll(&mut readiness, 1, 100) }, 1);
            assert_ne!(readiness.revents & libc::POLLHUP, 0);
        }

        #[test]
        fn fragmented_jsonl_records_are_drained_in_order() {
            let (sender, receiver) = mpsc::sync_channel(2);
            let failed = AtomicBool::new(false);
            let mut buffered = br#"{"protocol_version":2,"request_id":1,"command":{"hello":{}}}
{"protocol_version":2,"request_id":2,"command":{"shutdown":{}}}
"#
            .to_vec();
            assert!(drain_lines(&mut buffered, &sender, &failed));
            assert!(buffered.is_empty());
            assert_eq!(receiver.recv().expect("hello").request_id, 1);
            assert_eq!(receiver.recv().expect("shutdown").request_id, 2);
            assert!(!failed.load(Ordering::Acquire));
        }

        #[test]
        fn malformed_controller_record_fails_closed_without_queueing() {
            let (sender, receiver) = mpsc::sync_channel(1);
            let failed = AtomicBool::new(false);
            assert!(!send_line(b"{}\n", &sender, &failed));
            assert!(failed.load(Ordering::Acquire));
            assert!(receiver.try_recv().is_err());
        }

        #[test]
        fn visible_focus_requires_the_explicit_process_gate() {
            let matrix = RunMatrixRequest {
                cases: vec![FixtureCase::Button],
                backends: vec![InputBackend::FixedDomRecipe],
                presentation: PresentationState::VisibleFocused,
            };
            assert!(!presentation_is_authorized(&matrix, false));
            assert!(presentation_is_authorized(&matrix, true));

            let hidden = RunMatrixRequest {
                presentation: PresentationState::Hidden,
                ..matrix
            };
            assert!(presentation_is_authorized(&hidden, false));
        }
    }
}
