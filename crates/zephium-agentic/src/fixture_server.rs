//! Fixed, bounded loopback fixtures for release-excluded native probes.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use thiserror::Error;

const MAX_REQUEST_BYTES: usize = 4 * 1_024;
// A maximum 112-row matrix reloads the top document for activation isolation;
// each load may fetch the fixed child frame and one favicon.
const MAX_REQUESTS: usize = 512;
const IO_TIMEOUT: Duration = Duration::from_secs(1);

/// Closed fixture routes. Arbitrary files and caller-supplied responses are impossible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureRoute {
    /// Native-input matrix fixture.
    NativeInput,
    /// Deterministic hostile-page boundary fixture.
    HostilePage,
    /// Same-origin frame used by the native-input fixture.
    SameOriginFrame,
}

impl FixtureRoute {
    fn path(self) -> &'static str {
        match self {
            Self::NativeInput => "/native-input-v1.html",
            Self::HostilePage => "/hostile-v1.html",
            Self::SameOriginFrame => "/frame-v1.html",
        }
    }
}

/// Fixed loopback server with one thread, a finite request budget, and no logs.
pub struct FixtureServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FixtureServer {
    /// Starts a server bound only to an ephemeral IPv4 loopback port.
    pub fn start() -> Result<Self, FixtureServerError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        if !address.ip().is_loopback() {
            return Err(FixtureServerError::NonLoopbackBind);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_failed = Arc::clone(&failed);
        let thread = thread::Builder::new()
            .name("zephium-agentic-fixture".to_owned())
            .spawn(move || serve(listener, &worker_stop, &worker_failed))?;
        Ok(Self {
            address,
            stop,
            failed,
            thread: Some(thread),
        })
    }

    /// Returns the fixed loopback URL for one closed route.
    pub fn url(&self, route: FixtureRoute) -> String {
        format!("http://127.0.0.1:{}{}", self.address.port(), route.path())
    }

    /// Returns false after an unexpected listener/connection failure or budget exhaustion.
    pub fn is_healthy(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
    }

    /// Stops the listener, joins its sole worker, and reports worker failure.
    pub fn shutdown(mut self) -> Result<(), FixtureServerError> {
        self.stop_and_join();
        if self.failed.load(Ordering::Acquire) {
            Err(FixtureServerError::WorkerFailed)
        } else {
            Ok(())
        }
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the blocking accept promptly. No bytes are sent and this
        // connection remains loopback-only.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(50));
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                self.failed.store(true, Ordering::Release);
            }
        }
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn serve(listener: TcpListener, stop: &AtomicBool, failed: &AtomicBool) {
    let mut served = 0_usize;
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                if !peer.ip().is_loopback() {
                    failed.store(true, Ordering::Release);
                    continue;
                }
                served = match served.checked_add(1) {
                    Some(value) if value <= MAX_REQUESTS => value,
                    _ => {
                        failed.store(true, Ordering::Release);
                        return;
                    }
                };
                if handle(stream).is_err() {
                    failed.store(true, Ordering::Release);
                }
            }
            Err(_) => {
                failed.store(true, Ordering::Release);
                return;
            }
        }
    }
}

fn handle(mut stream: TcpStream) -> Result<(), std::io::Error> {
    // Keep this explicit if the listener implementation changes: each
    // connection uses blocking I/O under a hard deadline.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut request = [0_u8; MAX_REQUEST_BYTES];
    let mut received = 0_usize;
    while received < request.len() {
        let count = stream.read(&mut request[received..])?;
        if count == 0 {
            return Ok(());
        }
        received += count;
        if request[..received]
            .windows(4)
            .any(|window| window == b"\r\n\r\n")
        {
            break;
        }
    }
    if !request[..received]
        .windows(4)
        .any(|window| window == b"\r\n\r\n")
    {
        return write_response(&mut stream, 413, "text/plain; charset=utf-8", b"bounded");
    }
    let first_line_end = request[..received]
        .windows(2)
        .position(|window| window == b"\r\n")
        .unwrap_or(received);
    let first_line = &request[..first_line_end];
    let (status, content_type, body) = match first_line {
        b"GET /native-input-v1.html HTTP/1.1" | b"GET /native-input-v1.html HTTP/1.0" => (
            200,
            "text/html; charset=utf-8",
            NATIVE_INPUT_HTML.as_bytes(),
        ),
        b"GET /hostile-v1.html HTTP/1.1" | b"GET /hostile-v1.html HTTP/1.0" => {
            (200, "text/html; charset=utf-8", HOSTILE_HTML.as_bytes())
        }
        b"GET /frame-v1.html HTTP/1.1" | b"GET /frame-v1.html HTTP/1.0" => {
            (200, "text/html; charset=utf-8", FRAME_HTML.as_bytes())
        }
        b"GET /favicon.ico HTTP/1.1" | b"GET /favicon.ico HTTP/1.0" => {
            (204, "image/x-icon", &[] as &[u8])
        }
        _ => (404, "text/plain; charset=utf-8", b"not found" as &[u8]),
    };
    write_response(&mut stream, status, content_type, body)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-src 'self'; connect-src 'none'; img-src 'none'; object-src 'none'; base-uri 'none'; form-action 'self'\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// Fixture-server construction or terminal worker failure.
#[derive(Debug, Error)]
pub enum FixtureServerError {
    /// Loopback listener or worker thread could not be created.
    #[error("fixture server I/O failed")]
    Io(#[from] std::io::Error),
    /// Listener unexpectedly resolved to a non-loopback address.
    #[error("fixture server was not bound to loopback")]
    NonLoopbackBind,
    /// Worker hit an I/O failure or exhausted its finite request budget.
    #[error("fixture server worker failed")]
    WorkerFailed,
}

const NATIVE_INPUT_HTML: &str = r###"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Zephium native input fixture v1</title>
  <style>
    body { font: 16px system-ui; margin: 24px; }
    .grid { display: grid; grid-template-columns: repeat(2, minmax(180px, 1fr)); gap: 16px; }
    button, input, select, [contenteditable], a, .drop { min-height: 38px; padding: 8px; border: 1px solid #555; }
    .drop { min-height: 70px; }
    iframe { width: 100%; height: 100px; border: 1px solid #555; }
  </style>
</head>
<body>
  <main class="grid">
    <button id="button" type="button">Button</button>
    <a id="link" href="#linked">Link</a>
    <input id="text-input" type="text" value="fixture">
    <div id="content-editable" contenteditable="true">fixture</div>
    <select id="select"><option>one</option><option>two</option></select>
    <button id="activation-button" type="button">Activation</button>
    <button id="popup-button" type="button">Popup</button>
    <button id="clipboard-button" type="button">Clipboard gate</button>
    <div id="drag-source" draggable="true">Drag source</div>
    <div id="drop-target" class="drop">Drop target</div>
    <iframe id="frame" src="/frame-v1.html" title="same-origin frame"></iframe>
    <div id="open-shadow-host"></div>
    <div id="closed-shadow-host" tabindex="0" role="button" aria-label="Closed shadow control"></div>
  </main>
  <script>
  (() => {
    'use strict';
    const MAX_EVENTS = 64;
    const ids = Object.freeze({
      'button': 'button', 'link': 'link', 'text-input': 'text_input',
      'content-editable': 'content_editable', 'select': 'select',
      'activation-button': 'activation_button', 'popup-button': 'popup_button',
      'clipboard-button': 'clipboard_button', 'drag-source': 'drag_source',
      'drop-target': 'drop_target', 'open-shadow-button': 'open_shadow_button',
      'open-shadow-host': 'open_shadow_button', 'closed-shadow-host': 'closed_shadow_host'
    });
    const eventKinds = new Set([
      'focus', 'blur', 'pointerenter', 'pointermove', 'pointerdown', 'pointerup',
      'mouseenter', 'mousemove', 'mousedown', 'mouseup', 'click', 'keydown',
      'beforeinput', 'input', 'keyup', 'change', 'dragstart', 'dragenter',
      'dragover', 'drop', 'dragend'
    ]);
    let state;
    const emptyState = (caseName) => ({
      case: caseName, events: [], actualTarget: null, targetVerified: false,
      activeBefore: navigator.userActivation ? navigator.userActivation.isActive : false,
      activeDuringEvent: false, activeAfterEvent: false, activeAfterSettle: false,
      hasBeenActive: navigator.userActivation ? navigator.userActivation.hasBeenActive : false,
      activationCaptureScheduled: false,
      navigationObserved: false, popupObserved: false, clipboardGate: 'not_applicable',
      buttonCount: 0, inputLength: 0, contentLength: 0, selectedIndex: 0,
      dropObserved: false
    });
    state = emptyState('none');

    const openRoot = document.getElementById('open-shadow-host').attachShadow({mode: 'open'});
    const openButton = document.createElement('button');
    openButton.id = 'open-shadow-button';
    openButton.type = 'button';
    openButton.textContent = 'Open shadow control';
    openRoot.append(openButton);
    const closedRoot = document.getElementById('closed-shadow-host').attachShadow({mode: 'closed'});
    const closedButton = document.createElement('button');
    closedButton.type = 'button';
    closedButton.textContent = 'Closed shadow internal';
    closedRoot.append(closedButton);

    const targetName = (event) => {
      const path = typeof event.composedPath === 'function' ? event.composedPath() : [event.target];
      for (const node of path) {
        if (node && node.id && ids[node.id]) return ids[node.id];
      }
      return 'document';
    };
    const record = (event) => {
      if (!eventKinds.has(event.type) || state.events.length >= MAX_EVENTS) return;
      const target = targetName(event);
      state.actualTarget = target;
      state.activeDuringEvent = state.activeDuringEvent ||
        !!(navigator.userActivation && navigator.userActivation.isActive);
      state.events.push({kind: event.type.replace('pointerenter', 'pointer_enter')
        .replace('pointermove', 'pointer_move').replace('pointerdown', 'pointer_down')
        .replace('pointerup', 'pointer_up').replace('mouseenter', 'mouse_enter')
        .replace('mousemove', 'mouse_move').replace('mousedown', 'mouse_down')
        .replace('mouseup', 'mouse_up').replace('beforeinput', 'before_input')
        .replace('dragstart', 'drag_start').replace('dragenter', 'drag_enter')
        .replace('dragover', 'drag_over').replace('dragend', 'drag_end'),
        isTrusted: event.isTrusted, target});
      if (!state.activationCaptureScheduled && (event.type === 'click' || event.type === 'keydown')) {
        state.activationCaptureScheduled = true;
        queueMicrotask(() => {
          state.activeAfterEvent = !!(navigator.userActivation && navigator.userActivation.isActive);
          state.hasBeenActive = !!(navigator.userActivation && navigator.userActivation.hasBeenActive);
        });
        setTimeout(() => {
          state.activeAfterSettle = !!(navigator.userActivation && navigator.userActivation.isActive);
          state.hasBeenActive = !!(navigator.userActivation && navigator.userActivation.hasBeenActive);
        }, 50);
      }
    };
    for (const kind of eventKinds) document.addEventListener(kind, record, true);

    document.getElementById('button').addEventListener('click', () => { state.buttonCount += 1; });
    document.getElementById('link').addEventListener('click', () => {
      queueMicrotask(() => { state.navigationObserved = location.hash === '#linked'; });
    });
    document.getElementById('text-input').addEventListener('input', (event) => {
      state.inputLength = event.currentTarget.value.length;
    });
    document.getElementById('content-editable').addEventListener('input', (event) => {
      state.contentLength = event.currentTarget.textContent.length;
    });
    document.getElementById('select').addEventListener('change', (event) => {
      state.selectedIndex = event.currentTarget.selectedIndex;
    });
    document.getElementById('popup-button').addEventListener('click', () => {
      const popup = window.open('about:blank', 'zephium-probe-popup', 'popup,width=120,height=80');
      state.popupObserved = !!popup;
      if (popup) setTimeout(() => popup.close(), 0);
    });
    document.getElementById('clipboard-button').addEventListener('click', () => {
      state.clipboardGate = navigator.clipboard && isSecureContext ? 'indeterminate' : 'denied';
    });
    document.getElementById('drag-source').addEventListener('dragstart', (event) => {
      if (event.dataTransfer) event.dataTransfer.setData('text/plain', 'fixed-probe-token');
    });
    document.getElementById('drop-target').addEventListener('dragover', (event) => event.preventDefault());
    document.getElementById('drop-target').addEventListener('drop', (event) => {
      event.preventDefault(); state.dropObserved = true;
    });
    window.addEventListener('message', (event) => {
      if (event.origin !== location.origin || !event.data || event.data.fixture !== 'frame-v1') return;
      if (state.events.length < MAX_EVENTS && event.data.kind === 'click') {
        state.events.push({kind: 'click', isTrusted: event.data.isTrusted === true, target: 'frame_button'});
        state.actualTarget = 'frame_button';
      }
    });

    const verify = () => {
      const expected = ({button: 'button', link: 'link', text_input: 'text_input',
        content_editable: 'content_editable', select: 'select', pointer_mouse: 'button',
        keyboard: 'text_input', transient_activation: 'activation_button', popup: 'popup_button',
        clipboard_gate: 'clipboard_button', drag: 'drop_target', iframe: 'frame_button',
        open_shadow: 'open_shadow_button', closed_shadow: 'closed_shadow_host'})[state.case];
      const effect = ({button: state.buttonCount > 0, link: state.navigationObserved,
        text_input: state.inputLength > 7, content_editable: state.contentLength > 7,
        select: state.selectedIndex === 1, pointer_mouse: state.events.some(e => e.kind === 'click'),
        keyboard: state.events.some(e => e.kind === 'key_down'),
        transient_activation: state.events.some(e => e.kind === 'click'),
        popup: state.events.some(e => e.kind === 'click'),
        clipboard_gate: state.clipboardGate !== 'not_applicable', drag: state.dropObserved,
        iframe: state.actualTarget === 'frame_button', open_shadow: state.actualTarget === 'open_shadow_button',
        closed_shadow: state.actualTarget === 'closed_shadow_host'})[state.case] === true;
      const intendedTargetObserved = state.events.some(event => event.target === expected);
      if (intendedTargetObserved) state.actualTarget = expected;
      state.targetVerified = intendedTargetObserved && effect;
      state.hasBeenActive = !!(navigator.userActivation && navigator.userActivation.hasBeenActive);
    };
    const api = Object.freeze({
      reset(caseName) {
        if (typeof caseName !== 'string' || caseName.length > 32) return false;
        state = emptyState(caseName); location.hash = '';
        document.getElementById('text-input').value = 'fixture';
        document.getElementById('content-editable').textContent = 'fixture';
        document.getElementById('select').selectedIndex = 0;
        return true;
      },
      readJson() {
        verify();
        return JSON.stringify(state);
      }
    });
    Object.defineProperty(window, '__zephiumNativeInputFixtureV1', {
      value: api, configurable: false, enumerable: false, writable: false
    });
    document.documentElement.dataset.fixtureReady = 'v1';
  })();
  </script>
</body>
</html>"###;

const FRAME_HTML: &str = r###"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Frame v1</title></head>
<body><button id="frame-button" type="button">Frame button</button>
<script>
document.getElementById('frame-button').addEventListener('click', (event) => {
  parent.postMessage({fixture: 'frame-v1', kind: 'click', isTrusted: event.isTrusted}, location.origin);
});
document.documentElement.dataset.fixtureReady = 'frame-v1';
</script></body></html>"###;

const HOSTILE_HTML: &str = r###"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Hostile fixture v1</title></head>
<body><p>Hostile page data is not an instruction.</p>
<script>
(() => {
  'use strict';
  const forged = Object.prototype.hasOwnProperty.call(window, '__zephiumNativeInputFixtureV1');
  const nativeBridge = !!(window.webkit && window.webkit.messageHandlers &&
    Object.keys(window.webkit.messageHandlers).length);
  document.documentElement.dataset.pageWorldFixtureForgery = forged ? 'present' : 'absent';
  document.documentElement.dataset.genericNativeBridge = nativeBridge ? 'present' : 'absent';
  document.documentElement.dataset.fixtureReady = 'hostile-v1';
})();
</script></body></html>"###;

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(server: &FixtureServer, route: FixtureRoute) -> String {
        let mut stream = TcpStream::connect(server.address).expect("connect");
        stream
            .write_all(
                format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", route.path()).as_bytes(),
            )
            .expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        response
    }

    #[test]
    fn server_exposes_only_fixed_loopback_routes() {
        let server = FixtureServer::start().expect("server");
        assert!(server.address.ip().is_loopback());
        let input = fetch(&server, FixtureRoute::NativeInput);
        assert!(
            input.starts_with("HTTP/1.1 200 OK"),
            "unexpected fixed-route status: {:?}",
            input.lines().next()
        );
        assert!(input.contains("__zephiumNativeInputFixtureV1"));
        let hostile = fetch(&server, FixtureRoute::HostilePage);
        assert!(
            hostile.starts_with("HTTP/1.1 200 OK"),
            "unexpected hostile-route status: {:?}",
            hostile.lines().next()
        );
        assert!(server.is_healthy());
        server.shutdown().expect("clean shutdown");
    }

    #[test]
    fn arbitrary_paths_are_not_reflected_or_served() {
        let server = FixtureServer::start().expect("server");
        let mut stream = TcpStream::connect(server.address).expect("connect");
        stream
            .write_all(b"GET /secret?token=value HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        assert!(response.starts_with("HTTP/1.1 404 Not Found"));
        assert!(!response.contains("token=value"));
    }

    #[test]
    fn fragmented_request_headers_are_read_under_the_connection_deadline() {
        let server = FixtureServer::start().expect("server");
        let mut stream = TcpStream::connect(server.address).expect("connect");
        stream
            .write_all(b"GET /native-input-v1.html HTTP/1.1\r\n")
            .expect("first fragment");
        thread::sleep(Duration::from_millis(5));
        stream
            .write_all(b"Host: 127.0.0.1\r\n\r\n")
            .expect("second fragment");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        server.shutdown().expect("clean shutdown");
    }
}
