use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const HTML: &str = r#"<!doctype html><title>Animation gated load</title>
<img src="/pending.svg" alt="Fixture">
<button style="font:24px system-ui;padding:20px" type="button" onclick="location.assign('/verified')">Verify local handoff</button>
<script>requestAnimationFrame(() => fetch('/release'));</script>"#;

pub(super) struct Fixture {
    url: String,
    stop: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    #[cfg(feature = "native-agentic-semantic-probe")]
    verified: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Fixture {
    pub(super) fn start() -> Result<Self, &'static str> {
        Self::with_release(true)
    }

    #[cfg(feature = "native-agentic-semantic-probe")]
    pub(super) fn stalled() -> Result<Self, &'static str> {
        Self::with_release(false)
    }

    fn with_release(release_load: bool) -> Result<Self, &'static str> {
        let listener =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|_| "fixture_bind")?;
        let port = listener.local_addr().map_err(|_| "fixture_address")?.port();
        listener.set_nonblocking(true).map_err(|_| "fixture_mode")?;
        let stop = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_released = released.clone();
        let verified = Arc::new(AtomicBool::new(false));
        let worker_verified = verified.clone();
        let worker = thread::Builder::new()
            .name("liveness-fixture".into())
            .spawn(move || {
                serve(
                    listener,
                    worker_stop,
                    worker_released,
                    worker_verified,
                    release_load,
                )
            })
            .map_err(|_| "fixture_worker")?;
        Ok(Self {
            url: format!("http://127.0.0.1:{port}/"),
            stop,
            released,
            #[cfg(feature = "native-agentic-semantic-probe")]
            verified,
            worker: Some(worker),
        })
    }

    pub(super) fn url(&self) -> &str {
        &self.url
    }

    pub(super) fn released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }
    #[cfg(feature = "native-agentic-semantic-probe")]
    pub(super) fn verified(&self) -> bool {
        self.verified.load(Ordering::Acquire)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Request {
    stream: TcpStream,
    bytes: [u8; 4096],
    length: usize,
    deadline: Instant,
}

fn serve(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    verified: Arc<AtomicBool>,
    release_load: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut pending = None;
    let mut incoming: Vec<Request> = Vec::new();
    let mut requests = 0u8;
    while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
        if release_load && released.load(Ordering::Acquire) {
            if let Some(stream) = pending.take() {
                respond(
                    stream,
                    "image/svg+xml",
                    "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
                );
            }
        }
        if requests < 32 {
            match listener.accept() {
                Ok((stream, _)) => {
                    requests += 1;
                    if stream.set_nonblocking(true).is_ok() {
                        incoming.push(Request {
                            stream,
                            bytes: [0; 4096],
                            length: 0,
                            deadline: Instant::now() + Duration::from_secs(5),
                        });
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => break,
            }
        }
        for index in (0..incoming.len()).rev() {
            let request = &mut incoming[index];
            let remove =
                if Instant::now() >= request.deadline || request.length == request.bytes.len() {
                    true
                } else {
                    match request.stream.read(&mut request.bytes[request.length..]) {
                        Ok(0) => true,
                        Ok(count) => {
                            request.length += count;
                            false
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
                        Err(_) => true,
                    }
                };
            if remove {
                incoming.swap_remove(index);
                continue;
            }
            if !request.bytes[..request.length]
                .windows(4)
                .any(|v| v == b"\r\n\r\n")
            {
                continue;
            }
            let Request {
                stream,
                bytes,
                length,
                ..
            } = incoming.swap_remove(index);
            if bytes[..length].starts_with(b"GET / HTTP/1.") {
                eprintln!("liveness_fixture route=Document request_bytes={length}");
                respond(stream, "text/html", HTML);
            } else if bytes[..length].starts_with(b"GET /pending.svg HTTP/1.") && pending.is_none()
            {
                eprintln!("liveness_fixture route=Pending request_bytes={length}");
                pending = Some(stream);
            } else if bytes[..length].starts_with(b"GET /release HTTP/1.") {
                eprintln!("liveness_fixture route=Release request_bytes={length}");
                released.store(true, Ordering::Release);
                respond(stream, "text/plain", "");
            } else if bytes[..length].starts_with(b"GET /verified HTTP/1.") {
                let cookie = bytes[..length]
                    .windows(b"zephium_fixture=present".len())
                    .any(|value| value == b"zephium_fixture=present");
                verified.store(cookie, Ordering::Release);
                eprintln!("liveness_fixture route=Verified session_cookie={cookie}");
                respond(
                    stream,
                    "text/html",
                    "<!doctype html><title>Verified fixture</title><h1>Retained page verified</h1>",
                );
            } else {
                eprintln!("liveness_fixture route=Other request_bytes={length}");
                respond(stream, "text/plain", "");
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn respond(mut stream: TcpStream, kind: &str, body: &str) {
    if stream.set_nonblocking(false).is_ok()
        && stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .is_ok()
    {
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nSet-Cookie: zephium_fixture=present; Path=/; SameSite=Lax; HttpOnly\r\nConnection: close\r\n\r\n{body}", body.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_preconnection_does_not_block_a_complete_document_request() {
        let fixture = Fixture::start().unwrap();
        let address = fixture
            .url()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/');
        let _idle = TcpStream::connect(address).unwrap();
        let mut incomplete = TcpStream::connect(address).unwrap();
        incomplete.write_all(b"GET / HTTP/1.1\r\n").unwrap();
        let mut complete = TcpStream::connect(address).unwrap();
        complete
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        complete
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        complete.read_to_string(&mut response).unwrap();
        assert!(response.ends_with(HTML));
        assert!(!fixture.released());
    }
}
