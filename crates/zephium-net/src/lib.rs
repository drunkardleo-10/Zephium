//! Outbound http(s) fetches on a dedicated worker thread. Everything else in
//! the workspace stays network-free.

use std::io::Read;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use zephium_core::ports::net::{Fetched, Net};

struct Job {
    url: String,
    max_bytes: usize,
    done: Box<dyn FnOnce(Option<Fetched>) + Send>,
}

pub struct HttpNet {
    tx: Sender<Job>,
}

impl Default for HttpNet {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpNet {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("zephium-net".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(10))
                    .user_agent("Zephium/0.1")
                    .build();
                for job in rx {
                    (job.done)(fetch(&agent, &job.url, job.max_bytes));
                }
            })
            .expect("spawn net thread");
        Self { tx }
    }
}

impl Net for HttpNet {
    fn fetch(&self, url: String, max_bytes: usize, done: Box<dyn FnOnce(Option<Fetched>) + Send>) {
        let _ = self.tx.send(Job {
            url,
            max_bytes,
            done,
        });
    }
}

fn fetch(agent: &ureq::Agent, url: &str, max_bytes: usize) -> Option<Fetched> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let response = agent.get(url).call().ok()?;
    let content_type = Some(response.content_type().to_string());
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return None;
    }
    Some(Fetched {
        content_type,
        bytes,
    })
}
