//! Connections: services the agent reaches through a tool the person already
//! has (a CLI such as `gh`) or an MCP server they added. The first use in a
//! work asks "Use GitHub (gh)?"; reading is free after that; anything that
//! writes as the person stops for a Confirm with a preview. What a service
//! returns is data for the agent, never instructions.
pub mod cli;
pub mod gh;
pub mod helper;
pub mod mcp;
pub mod store;

use zephium_core::work::runtime::WorkConfirmV1;
use zephium_core::work::WorkError;

pub use crate::work_computer::{Decision, HostFuture};

/// One call as the canvas shows it: "Read issue #123", "Listed 4 PRs".
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CallFact {
    /// "github", or an MCP server's id.
    pub service: String,
    /// The namespaced tool: `github_issue`, `notion__search`.
    pub tool: String,
    /// What the frame writes the row from: `issue`, `issues`, `pr`, `comment`.
    pub verb: String,
    /// What it touched: "#123", "octo/app".
    pub target: Option<String>,
    /// A title it read, such as the issue's.
    pub title: Option<String>,
    /// Items it listed.
    pub count: Option<u32>,
    /// The page the call is about, when it has one.
    pub url: Option<String>,
}

/// Where a connection's calls go: the run that owns the part.
pub trait ConnectionHost: Send + Sync {
    /// Records a finished call on the part. `source` is the bounded result,
    /// kept with the step and out of the lead's context.
    fn record(
        &self,
        fact: CallFact,
        ok: bool,
        source: Option<String>,
    ) -> HostFuture<'_, Result<(), WorkError>>;
    /// Records a Confirm step and waits for the person.
    fn confirm(&self, confirm: WorkConfirmV1) -> HostFuture<'_, Result<Decision, WorkError>>;
    /// Asks once per work; an earlier answer in the same work is reused.
    fn ask<'a>(
        &'a self,
        prompt: &'a str,
        options: &'a [&'a str],
    ) -> HostFuture<'a, Result<Option<String>, WorkError>>;
}

/// Names that commit something for the person, when a server says nothing.
pub fn consequential_name(name: &str) -> bool {
    const WORDS: [&str; 30] = [
        "send", "post", "create", "delete", "remove", "update", "write", "edit", "merge", "close",
        "reopen", "publish", "comment", "reply", "invite", "share", "pay", "purchase", "book",
        "order", "submit", "archive", "move", "rename", "set", "add", "assign", "approve",
        "upload", "execute",
    ];
    words(name)
        .iter()
        .any(|word| WORDS.contains(&word.as_str()))
}

/// `sendMessage`, `send_message` and `send-message` alike become words.
fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous_lower = false;
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() {
            previous_lower = false;
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if c.is_ascii_uppercase() && previous_lower && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        word.push(c.to_ascii_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Text from a service, bounded for the agent and cleaned of control bytes.
pub fn bounded(text: &str, max: usize) -> (String, bool) {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    if cleaned.len() <= max {
        return (cleaned, false);
    }
    let mut end = max;
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    (cleaned[..end].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consequence_by_name() {
        for name in [
            "send_message",
            "create-issue",
            "notion_delete_page",
            "post",
            "addComment",
        ] {
            assert!(consequential_name(name), "{name}");
        }
        for name in ["search", "get_issue", "list_channels", "read_page", "fetch"] {
            assert!(!consequential_name(name), "{name}");
        }
        assert_eq!(bounded("a\u{1}b", 10), ("a b".into(), false));
        assert_eq!(bounded("héllo", 2), ("h".into(), true));
    }
}
