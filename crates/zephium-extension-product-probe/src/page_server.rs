//! Bounded loopback page used to prove a real profile view committed.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SERVER_LIFETIME: Duration = Duration::from_secs(45);
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_LIMIT: usize = 8 * 1024;
const MAX_REQUESTS: usize = 16;
const PAGE_BODY: &[u8] = br#"<!doctype html>
<meta charset="utf-8">
<title>Zephium extension product probe</title>
<script>
(() => {
  "use strict";
  const extensionMarker = "data-zephium-extension-product-probe";
  const signal = "zephium-webkit-same-document-navigation-v1";
  let requested = false;

  document.addEventListener(signal, () => {
    if (location.search === "?zephium-same-document=1") {
      document.documentElement.dataset.zephiumSameDocumentSignal = "observed";
    }
  }, { capture: true });

  const requestSameDocumentNavigation = () => {
    if (requested) return;
    const state = document.documentElement.getAttribute(extensionMarker);
    if (state !== "ready:1" && state !== "ready-brokered:1") return;
    requested = true;
    observer.disconnect();
    history.pushState(null, "", "?zephium-same-document=1");
  };

  const observer = new MutationObserver(requestSameDocumentNavigation);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: [extensionMarker],
  });
  queueMicrotask(requestSameDocumentNavigation);
})();
</script>
<p>probe</p>"#;

pub(crate) struct PageServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    url: String,
}

impl PageServer {
    pub(crate) fn start() -> Result<Self, String> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("cannot bind loopback page server: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot bound page-server acceptance: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read page-server address: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("zephium-extension-product-probe-page".to_owned())
            .spawn(move || serve(listener, worker_stop))
            .map_err(|error| format!("cannot spawn loopback page server: {error}"))?;
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
            url: format!("http://{address}/"),
        })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for PageServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake a platform that retained an accept readiness edge. Failure is
        // harmless because the listener also checks a short nonblocking loop.
        let _ = std::net::TcpStream::connect_timeout(&self.address, Duration::from_millis(50));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(listener: TcpListener, stop: Arc<AtomicBool>) {
    let deadline = Instant::now()
        .checked_add(SERVER_LIFETIME)
        .unwrap_or_else(Instant::now);
    let mut accepted = 0_usize;
    while !stop.load(Ordering::Acquire) && Instant::now() < deadline && accepted < MAX_REQUESTS {
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                accepted += 1;
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let mut request = [0_u8; REQUEST_LIMIT];
                let _ = stream.read(&mut request);
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                    PAGE_BODY.len()
                );
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(PAGE_BODY);
                let _ = stream.flush();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
}
