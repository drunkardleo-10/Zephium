//! A stdio server as a child process in its own process group: only the
//! environment the caller gives it, stderr drained and dropped, and stdout
//! read through a bound on any one message.
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout};

use super::{McpError, MAX_MESSAGE_BYTES};

pub struct StdioServer {
    /// An absolute path to the program.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The whole environment the server gets.
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

/// The running child; dropping it ends the server and its group.
pub struct Process {
    child: Child,
    group: Option<i32>,
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(group) = self.group {
            // SAFETY: a negative pid addresses the group this process created
            // for its own child; no other process is named.
            unsafe {
                libc::kill(-group, libc::SIGTERM);
            }
        }
        let _ = self.child.start_kill();
    }
}

impl StdioServer {
    pub(crate) fn spawn(&self) -> Result<(Process, Bounded<ChildStdout>, ChildStdin), McpError> {
        if !self.program.is_absolute() || self.args.len() > 64 {
            return Err(McpError::Spawn);
        }
        let mut command = tokio::process::Command::new(&self.program);
        command
            .args(&self.args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|_| McpError::Spawn)?;
        let group = child.id().and_then(|id| i32::try_from(id).ok());
        let stdout = child.stdout.take().ok_or(McpError::Spawn)?;
        let stdin = child.stdin.take().ok_or(McpError::Spawn)?;
        if let Some(mut stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            });
        }
        Ok((Process { child, group }, Bounded::new(stdout), stdin))
    }
}

/// Fails the stream once a line runs past `MAX_MESSAGE_BYTES`, so a server
/// cannot make the client buffer without end.
pub struct Bounded<R> {
    inner: R,
    line: usize,
    limit: usize,
}
impl<R> Bounded<R> {
    pub fn new(inner: R) -> Self {
        Self::with_limit(inner, MAX_MESSAGE_BYTES)
    }
    pub fn with_limit(inner: R, limit: usize) -> Self {
        Self {
            inner,
            line: 0,
            limit,
        }
    }
}
impl<R: AsyncRead + Unpin> AsyncRead for Bounded<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = polled {
            let fresh = &buf.filled()[before..];
            let mut line = self.line;
            for byte in fresh {
                if *byte == b'\n' {
                    line = 0;
                } else {
                    line += 1;
                }
            }
            if line > self.limit {
                buf.set_filled(before);
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "message too long",
                )));
            }
            self.line = line;
        }
        polled
    }
}
