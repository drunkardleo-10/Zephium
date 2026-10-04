//! Bounded supplementary providers. Query replacement aborts network observation;
//! the actor independently checks exact context before admitting any result.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Manager, WebviewWindow};
use zephium_app::{Command, Handle};
use zephium_core::notes::{NoteCall, NoteQuery, NoteResponse};
use zephium_core::ports::store::Store;
use zephium_core::search::{remote_query_allowed, scoped_query, SearchEngine, SearchScope};
use zephium_ipc::{SearchAction, SearchContext, SearchResult};

struct Pending {
    token: u64,
    task: tauri::async_runtime::JoinHandle<()>,
}

/// Keyed by search session, so a New Tab field, the floating launcher and a
/// second window observe the network independently. A single global slot let
/// any one of them abort the others' work.
static PENDING: Mutex<Option<HashMap<String, Pending>>> = Mutex::new(None);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();

/// Delay before touching the note index. Wide enough that ordinary typing
/// replaces the task first, which bounds index work by time rather than by
/// keystroke count. Notes are a local lookup with no round trip to hide.
const NOTES_DELAY: Duration = Duration::from_millis(130);
/// Delay before leaving the machine. Deliberately much shorter than the note
/// window: a suggestion's cost is dominated by its round trip, so waiting here
/// buys little and is felt directly as the list arriving late.
const REMOTE_DELAY: Duration = Duration::from_millis(60);

fn registry() -> std::sync::MutexGuard<'static, Option<HashMap<String, Pending>>> {
    PENDING.lock().unwrap_or_else(|error| error.into_inner())
}

pub(super) fn cancel_all() {
    if let Some(sessions) = registry().take() {
        for (_, work) in sessions {
            work.task.abort();
        }
    }
}

pub(super) fn cancel(session: &str) {
    if let Some(work) = registry().as_mut().and_then(|map| map.remove(session)) {
        work.task.abort();
    }
}

/// Dispatches supplementary completion exactly once, on every exit path
/// including `abort()`, which drops the task's future and therefore this
/// guard. Without it a provider that returned early left the actor's
/// `supplementary_pending` flag set forever, and a query with no local matches
/// never opened its result list at all.
struct Completion {
    shell: Handle,
    context: SearchContext,
    query: String,
    session: String,
    token: u64,
}

impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(map) = registry().as_mut() {
            if map
                .get(&self.session)
                .is_some_and(|work| work.token == self.token)
            {
                map.remove(&self.session);
            }
        }
        self.shell.dispatch(Command::SearchSupplementaryFinished {
            context: Box::new(self.context.clone()),
            query: std::mem::take(&mut self.query),
        });
    }
}

pub(super) fn schedule(
    caller: WebviewWindow,
    app: tauri::AppHandle,
    shell: Handle,
    query: String,
    context: SearchContext,
) {
    let session = context.session_id.clone();
    if let Some(work) = registry().get_or_insert_with(HashMap::new).remove(&session) {
        work.task.abort();
    }
    let (scope, term) = scoped_query(&query);
    let notes_possible = matches!(scope, SearchScope::All | SearchScope::Notes)
        && !term.is_empty()
        && term.len() <= 512;
    let term = term.to_owned();
    let remote_possible = remote_query_allowed(&query);
    if !notes_possible && !remote_possible {
        // Nothing supplementary can run for this input. Settle now rather than
        // spending a task to discover it, so the list stops waiting at once.
        shell.dispatch(Command::SearchSupplementaryFinished {
            context: Box::new(context),
            query,
        });
        return;
    }
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let task = tauri::async_runtime::spawn(async move {
        let _completion = Completion {
            shell: shell.clone(),
            context: context.clone(),
            query: query.clone(),
            session: context.session_id.clone(),
            token,
        };
        // Ownership is cheap to confirm and both branches need it, so it is
        // checked before either one waits.
        let Some(private) = app
            .try_state::<crate::overlay::ContextCache>()
            .and_then(|cache| cache.search_private(&context))
        else {
            return;
        };
        if !context.session_id.starts_with("newtab:") {
            let Some(overlay) = app.try_state::<crate::overlay::Overlay>() else {
                return;
            };
            let owner = overlay.snapshot();
            if !owner.visible || owner.session_id != context.session_id {
                return;
            }
        }
        // The two branches hold their own windows. Sharing one made every
        // suggestion wait out the note window as well.
        let notes = async {
            if notes_possible {
                tokio::time::sleep(NOTES_DELAY).await;
                let reply = crate::notes::note_call(
                    caller,
                    app.clone(),
                    context.profile_id.clone(),
                    NoteCall::List {
                        query: NoteQuery {
                            search: term.clone(),
                            trashed: false,
                            after: None,
                            // Search shows at most two notes; reading more
                            // only builds previews that are dropped.
                            limit: 3,
                        },
                    },
                )
                .await;
                if reply.profile.as_deref() == Some(&context.profile_id) {
                    if let NoteResponse::Page { items, .. } = reply.response {
                        let results = items
                            .into_iter()
                            .take(3)
                            .map(|item| SearchResult {
                                kind: "note".into(),
                                title: item.title,
                                // Matches come from the body too; the preview
                                // shows why a note was found.
                                detail: item.preview,
                                icon: None,
                                action: SearchAction::OpenNote { id: item.id },
                            })
                            .collect();
                        shell.dispatch(Command::SearchAdditional {
                            context: Box::new(context.clone()),
                            query: query.clone(),
                            results,
                        });
                    }
                }
            }
        };
        let remote = async {
            tokio::time::sleep(REMOTE_DELAY).await;
            if private || !remote_possible {
                return;
            }
            let settings = tauri::async_runtime::spawn_blocking(|| {
                let store = crate::APP_STORE.get()?;
                Some((
                    store
                        .app_setting("search.engine")
                        .unwrap_or_else(|| "duckduckgo".into()),
                    store.app_setting("search.suggestions").as_deref() != Some("false"),
                ))
            })
            .await
            .ok()
            .flatten();
            let Some((engine, true)) = settings else {
                return;
            };
            // DuckDuckGo publishes this browser suggestion endpoint. Other engines
            // remain search destinations until their suggestion access is qualified.
            if engine != "duckduckgo" {
                return;
            }
            let client = CLIENT.get_or_init(|| {
                reqwest::Client::builder()
                    .https_only(true)
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(1))
                    .timeout(Duration::from_secs(2))
                    .pool_max_idle_per_host(1)
                    .build()
                    .ok()
            });
            let Some(client) = client else {
                return;
            };
            let mut endpoint =
                reqwest::Url::parse("https://duckduckgo.com/ac/").expect("fixed HTTPS endpoint");
            endpoint
                .query_pairs_mut()
                .append_pair("q", query.trim())
                .append_pair("type", "list");
            let response = client
                .get(endpoint)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await;
            let Ok(mut response) = response else {
                return;
            };
            if !response.status().is_success()
                || response
                    .content_length()
                    .is_some_and(|length| length > 32768)
            {
                return;
            }
            let mut bytes = Vec::new();
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) if bytes.len() + chunk.len() <= 32768 => {
                        bytes.extend_from_slice(&chunk)
                    }
                    Ok(None) => break,
                    _ => return,
                }
            }
            let results = parse_suggestions(&bytes, query.trim());
            shell.dispatch(Command::SearchAdditional {
                context: Box::new(context.clone()),
                query: query.clone(),
                results,
            });
        };
        tokio::join!(notes, remote);
    });
    registry()
        .get_or_insert_with(HashMap::new)
        .insert(session, Pending { token, task });
}

fn parse_suggestions(bytes: &[u8], query: &str) -> Vec<SearchResult> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    if value.get(0).and_then(|value| value.as_str()) != Some(query) {
        return Vec::new();
    }
    let Some(phrases) = value.get(1).and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    phrases
        .iter()
        .take(10)
        .filter_map(|value| {
            let phrase = value.as_str()?.trim();
            if phrase.is_empty()
                || phrase.len() > 512
                || phrase.chars().any(char::is_control)
                || !seen.insert(phrase.to_owned())
            {
                return None;
            }
            let url = SearchEngine::DuckDuckGo.search(phrase)?;
            Some(SearchResult {
                kind: "suggestion".into(),
                title: phrase.into(),
                detail: "DuckDuckGo".into(),
                icon: None,
                action: SearchAction::OpenUrl {
                    url: url.to_string(),
                },
            })
        })
        .take(4)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_suggestions_are_bounded_and_cannot_supply_actions() {
        let rows = parse_suggestions(
            br#"["rust",["rust guide","rust guide","file:///secret",42,"rust book"]]"#,
            "rust",
        );
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| matches!(&row.action, SearchAction::OpenUrl { url } if url.starts_with("https://duckduckgo.com/?q="))));
        assert!(parse_suggestions(br#"["old",["rust guide"]]"#, "rust").is_empty());
    }
}
