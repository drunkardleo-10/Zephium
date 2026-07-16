//! Linux adapter. Content views are built into a gtk::Fixed owned by the
//! composition root; the stage positions them and draws the drop indicator.

mod stage;

pub use stage::Stage;

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use gtk::glib::prelude::{ObjectExt, ObjectType};
use gtk::glib::signal::{connect_raw, SignalHandlerId};
use gtk::prelude::WidgetExt as _;
use webkit2gtk::{DownloadExt, WebContextExt, WebViewExt, WebsiteDataManagerExt};
use wry::WebViewExtUnix;
use zephium_core::ports::engine::Partition;

thread_local! {
    static CONTAINER: RefCell<Option<gtk::Fixed>> = const { RefCell::new(None) };
}

pub fn install_container(fixed: gtk::Fixed) -> Result<(), String> {
    CONTAINER.with(|cell| {
        let mut container = cell
            .try_borrow_mut()
            .map_err(|_| "WebKitGTK container is re-entrantly borrowed".to_owned())?;
        if container.is_some() {
            return Err("WebKitGTK container is already installed".to_owned());
        }
        *container = Some(fixed);
        Ok(())
    })
}

// This is deliberately a runtime check because Linux dynamically supplies
// WebKitGTK, so a successful build says nothing about the engine a user's
// machine will actually load. The pure version/deadline policy lives in core
// so CI and runtime cannot silently drift.

pub fn enforce_runtime_security_floor() -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch; cannot enforce the WebKitGTK security review deadline".to_owned())?
        .as_secs();
    enforce_runtime_preconditions(now, || {
        std::env::vars_os().find_map(|(name, _)| {
            let name = name.to_str()?;
            zephium_core::webkitgtk::environment_override_is_security_relevant(name)
                .then(|| name.to_owned())
        })
    })?;
    // SAFETY: These no-argument WebKitGTK ABI functions return immutable
    // library version constants and are safe after the library is loaded.
    let version = unsafe {
        (
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
            webkit2gtk::ffi::webkit_get_micro_version(),
        )
    };
    enforce_runtime_version(version)
}

fn enforce_runtime_preconditions(
    unix_seconds: u64,
    security_override: impl FnOnce() -> Option<String>,
) -> Result<(), String> {
    if let Some(name) = security_override() {
        return Err(format!(
            "security-relevant WebKitGTK/JavaScriptCore environment override {name} is present; unset it before starting Zephium"
        ));
    }
    if unix_seconds < zephium_core::webkitgtk::SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS {
        return Err(format!(
            "system clock predates the reviewed WebKitGTK security release {}; correct the clock before browsing",
            zephium_core::webkitgtk::SECURITY_FLOOR_PUBLISHED_ON,
        ));
    }
    if !zephium_core::webkitgtk::security_floor_review_is_current(unix_seconds) {
        return Err(format!(
            "the embedded WebKitGTK security-floor review expired after {}; update Zephium before browsing",
            zephium_core::webkitgtk::SECURITY_FLOOR_REVIEW_BY,
        ));
    }
    Ok(())
}

fn enforce_runtime_version(version: (u32, u32, u32)) -> Result<(), String> {
    zephium_core::webkitgtk::admit_runtime(version.0, version.1, version.2).map_err(|error| {
        format!(
            "{error}; latest stable {} was reviewed on {} (security floor source: {}; latest release source: {}). Install a supported, reviewed WebKitGTK runtime before starting Zephium",
            zephium_core::webkitgtk::LATEST_REVIEWED_TEXT,
            zephium_core::webkitgtk::LATEST_REVIEWED_PUBLISHED_ON,
            zephium_core::webkitgtk::SECURITY_FLOOR_SOURCE_URL,
            zephium_core::webkitgtk::LATEST_REVIEWED_SOURCE_URL,
        )
    })
}

pub fn container() -> Option<gtk::Fixed> {
    CONTAINER.with(|cell| cell.borrow().clone())
}

pub fn configure(
    webview: &wry::WebView,
    _radius: f64,
    partition: Partition,
    expected_data_directory: Option<&Path>,
) -> Result<(), String> {
    let view = webview.webview();
    // The host deliberately builds without an initial URL and calls this
    // before its first load. WebKit has therefore not launched a web process,
    // which is the required point for context-wide process policy.
    let context = view
        .context()
        .ok_or_else(|| "content WebKitGTK view has no WebContext".to_owned())?;
    if !context.is_sandbox_enabled() {
        return Err(
            "WebKitGTK content-process sandbox was not enabled at context construction".into(),
        );
    }
    if !context.is_process_swap_on_cross_site_navigation_enabled() {
        return Err("WebKitGTK cross-site Web-process swapping is disabled".into());
    }

    let expected_ephemeral = matches!(partition, Partition::Ephemeral(_));
    if view.is_ephemeral() != expected_ephemeral {
        return Err("WebKitGTK WebView persistence mode does not match its partition".into());
    }
    if context.is_ephemeral() != expected_ephemeral {
        return Err("WebKitGTK WebContext persistence mode does not match its partition".into());
    }
    attest_website_data_manager(&view, &context, partition, expected_data_directory)?;

    // `download-started` belongs to WebContext, not WebView. Persistent tabs
    // share a context, so registering Wry's per-builder callback on every tab
    // would retain one more closure for the lifetime of the profile. Mark the
    // GLib object and install one fail-closed handler before the first load.
    const DOWNLOAD_DENY_MARKER: &str = "zephium-download-deny-installed";
    // SAFETY: this private key is written and read only as `bool` here, and
    // the marker lives exactly as long as the context GLib object.
    let installed = unsafe { context.data::<bool>(DOWNLOAD_DENY_MARKER).is_some() };
    if !installed {
        context.connect_download_started(|_, download| download.cancel());
        // SAFETY: see the typed private-key invariant above.
        unsafe { context.set_data(DOWNLOAD_DENY_MARKER, true) };
    }

    Ok(())
}

fn attest_website_data_manager(
    view: &webkit2gtk::WebView,
    context: &webkit2gtk::WebContext,
    partition: Partition,
    expected_data_directory: Option<&Path>,
) -> Result<(), String> {
    let context_manager = context
        .website_data_manager()
        .ok_or_else(|| "content WebKitGTK context has no WebsiteDataManager".to_owned())?;
    let manager = view
        .website_data_manager()
        .ok_or_else(|| "content WebKitGTK view has no WebsiteDataManager".to_owned())?;
    if context_manager.as_ptr() != manager.as_ptr() {
        return Err("WebKitGTK view and context use different data managers".into());
    }

    let actual_data =
        canonical_reported_directory("base data", manager.base_data_directory().as_deref())?;
    let actual_cache =
        canonical_reported_directory("base cache", manager.base_cache_directory().as_deref())?;
    let expected = expected_data_directory
        .map(|path| direct_canonical_directory("expected profile", path))
        .transpose()?;

    validate_storage_postcondition(
        partition,
        manager.is_ephemeral(),
        expected.as_deref(),
        actual_data.as_deref(),
        actual_cache.as_deref(),
    )
}

fn canonical_reported_directory(
    kind: &str,
    reported: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    reported
        .map(|reported| direct_canonical_directory(kind, Path::new(reported)))
        .transpose()
}

fn direct_canonical_directory(kind: &str, path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!(
            "cannot inspect WebKitGTK {kind} directory {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "WebKitGTK {kind} path is not a direct directory: {}",
            path.display()
        ));
    }
    let canonical = path.canonicalize().map_err(|error| {
        format!(
            "cannot canonicalize WebKitGTK {kind} directory {}: {error}",
            path.display()
        )
    })?;
    // Reject relative spellings, `..`, and symlinked ancestors. A path that
    // merely resolves to the expected profile is not a stable storage
    // boundary because the alias can be retargeted after admission.
    if canonical != path {
        return Err(format!(
            "WebKitGTK {kind} path is not its direct canonical identity: {}",
            path.display()
        ));
    }
    Ok(canonical)
}

fn validate_storage_postcondition(
    partition: Partition,
    manager_is_ephemeral: bool,
    expected_directory: Option<&Path>,
    actual_data_directory: Option<&Path>,
    actual_cache_directory: Option<&Path>,
) -> Result<(), String> {
    match partition {
        Partition::Ephemeral(_) => {
            if !manager_is_ephemeral {
                return Err("ephemeral view received a durable WebKitGTK data manager".into());
            }
            if expected_directory.is_some()
                || actual_data_directory.is_some()
                || actual_cache_directory.is_some()
            {
                return Err(
                    "ephemeral WebKitGTK data manager exposes persistent base directories".into(),
                );
            }
        }
        Partition::Default(_) | Partition::Persistent(_) => {
            if manager_is_ephemeral {
                return Err("durable view received an ephemeral WebKitGTK data manager".into());
            }
            let expected = expected_directory
                .ok_or_else(|| "durable WebKitGTK view has no expected profile path".to_owned())?;
            if actual_data_directory != Some(expected) || actual_cache_directory != Some(expected) {
                return Err(
                    "WebKitGTK data/cache directories do not match the prepared profile path"
                        .into(),
                );
            }
        }
    }
    Ok(())
}

pub fn stop_loading(view: &wry::WebView) {
    view.webview().stop_loading();
}

/// WebKitGTK exposes renderer audio activity as a native property. A discard
/// probe must combine this with the DOM report so page-script tampering cannot
/// make an actually audible document look idle.
pub fn query_document_activity(view: &wry::WebView, done: impl FnOnce(bool) + 'static) -> bool {
    done(!view.webview().is_playing_audio());
    true
}

/// GLib signal registrations owned alongside the Wry WebView. The callbacks
/// capture only the item-id thunk supplied by the host; these strong native
/// handles exist solely so Drop can disconnect before the WebView is released.
pub struct NavigationObserver {
    webview: webkit2gtk::WebView,
    uri_token: Option<SignalHandlerId>,
    history: webkit2gtk::BackForwardList,
    history_token: Option<SignalHandlerId>,
}

pub type InstalledNavigationObserver = NavigationObserver;

pub fn install_navigation_observer(
    webview: &wry::WebView,
    on_change: impl Fn() + 'static,
) -> Result<NavigationObserver, &'static str> {
    let view = webview.webview();
    let history = view
        .back_forward_list()
        .ok_or("WebKitGTK did not provide a back-forward list")?;
    let on_change: Rc<dyn Fn()> = Rc::new(on_change);

    // `notify::uri` is emitted for the active main-frame URI, including
    // fragment and History API source changes.
    let on_uri = on_change.clone();
    let uri_token = view.connect_uri_notify(move |_| on_uri());

    // The generated bindings omit BackForwardList::changed because its GList
    // argument is untyped. Connect to the documented signal ABI directly; we
    // intentionally ignore all three native pointer arguments and query the
    // authoritative WebView state through the host after the callback.
    let history_token = connect_history_changed(&history, move || on_change());

    Ok(NavigationObserver {
        webview: view,
        uri_token: Some(uri_token),
        history,
        history_token: Some(history_token),
    })
}

fn connect_history_changed<F: Fn() + 'static>(
    history: &webkit2gtk::BackForwardList,
    callback: F,
) -> SignalHandlerId {
    unsafe extern "C" fn trampoline<F: Fn() + 'static>(
        _history: *mut c_void,
        _item_added: *mut c_void,
        _items_removed: *mut c_void,
        callback: gtk::glib::ffi::gpointer,
    ) {
        // SAFETY: connect_raw owns this boxed F until it disconnects or the
        // BackForwardList is finalized, and GLib invokes the trampoline with
        // the same user-data pointer.
        let callback = unsafe { &*(callback.cast::<F>()) };
        callback();
    }

    // SAFETY: WebKitBackForwardList::changed has three pointer parameters
    // followed by user_data. Pointer pointee types do not affect the C ABI,
    // and the trampoline never reads them. connect_raw installs the matching
    // destructor for the boxed closure.
    unsafe {
        let callback = Box::new(callback);
        connect_raw(
            history.as_ptr().cast(),
            c"changed".as_ptr(),
            Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
                trampoline::<F> as *const (),
            )),
            Box::into_raw(callback),
        )
    }
}

impl Drop for NavigationObserver {
    fn drop(&mut self) {
        if let Some(token) = self.uri_token.take() {
            self.webview.disconnect(token);
        }
        if let Some(token) = self.history_token.take() {
            self.history.disconnect(token);
        }
    }
}

pub fn current_url(view: &wry::WebView) -> Option<String> {
    const PAGE_URL_UTF8_LIMIT: usize = 8 * 1_024;
    let uri = view.webview().uri()?;
    (uri.as_str().len() <= PAGE_URL_UTF8_LIMIT).then(|| uri.to_string())
}

pub fn enforce_navigation_pending(view: &wry::WebView) -> bool {
    // Keep WebKitGTK mapped so its compositing surface survives the gate;
    // opacity is the native non-painting primitive used by the stage and by
    // Wry's synchronous commit guard.
    let widget = view.webview();
    widget.set_opacity(0.0);
    widget.opacity() == 0.0
}

/// Strong native storage handles that must outlive every view created from
/// the context. A successful Wry build is not enough proof: later hardening,
/// observer, or first-load steps can still fail after WebKitGTK has created a
/// WebsiteDataManager and touched profile storage.
pub(crate) struct WebsiteDataManagerObligation {
    pub(crate) managers: Vec<webkit2gtk::WebsiteDataManager>,
    pub(crate) provenance_complete: bool,
}

pub(crate) fn website_data_manager_obligation(view: &wry::WebView) -> WebsiteDataManagerObligation {
    let view = view.webview();
    let context_manager = view
        .context()
        .and_then(|context| context.website_data_manager());
    let view_manager = view.website_data_manager();
    let provenance_complete = context_manager
        .as_ref()
        .zip(view_manager.as_ref())
        .is_some_and(|(context, view)| context.as_ptr() == view.as_ptr());

    // Preserve every native object even when the view/context relationship is
    // malformed. The host marks that profile unverifiable and will not delete
    // its directory, but retaining the objects prevents a subsequent empty
    // retry from fabricating absence.
    let mut managers = Vec::with_capacity(2);
    for manager in [context_manager, view_manager].into_iter().flatten() {
        if !managers
            .iter()
            .any(|existing: &webkit2gtk::WebsiteDataManager| existing.as_ptr() == manager.as_ptr())
        {
            managers.push(manager);
        }
    }
    WebsiteDataManagerObligation {
        managers,
        provenance_complete,
    }
}

/// Clear every native data manager retained from this profile after its views
/// and WebContexts have been released, then remove and verify the owned disk
/// directory on a worker. Incognito WebKitGTK views each own an independent
/// ephemeral manager, so all managers must acknowledge the clear operation.
pub(crate) fn erase_profile_data(
    mut managers: Vec<webkit2gtk::WebsiteDataManager>,
    manager_provenance_valid: bool,
    roots: Vec<std::path::PathBuf>,
    profile: zephium_core::ids::ProfileId,
    completion: Arc<crate::erasure::Completion>,
) {
    use webkit2gtk::{WebsiteDataManagerExt, WebsiteDataManagerExtManual, WebsiteDataTypes};

    let mut seen = std::collections::HashSet::new();
    managers.retain(|manager| seen.insert(manager.as_ptr() as usize));
    if managers.is_empty() {
        if manager_provenance_valid {
            remove_linux_profile_directories_async(roots, profile, completion);
        } else {
            completion.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
        }
        return;
    }

    let remaining = Arc::new(AtomicUsize::new(managers.len()));
    // Missing/mismatched native manager provenance is sticky. Known managers
    // are still cleared and fetched best-effort, but disk deletion is denied.
    let failed = Arc::new(AtomicBool::new(!manager_provenance_valid));
    for manager in managers {
        // clear() requires a Send callback even though WebKit invokes it on
        // this GTK main context. SendWeakRef is the binding's thread-checked
        // bridge; upgrading it in the callback also proves the native source
        // object survived long enough to start verification.
        let manager_ref: gtk::glib::SendWeakRef<webkit2gtk::WebsiteDataManager> =
            manager.downgrade().into();
        let remaining = remaining.clone();
        let failed = failed.clone();
        let roots = roots.clone();
        let completion = completion.clone();
        manager.clear(
            WebsiteDataTypes::ALL,
            gtk::glib::TimeSpan::from_seconds(0),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if result.is_err() {
                    failed.store(true, Ordering::Release);
                    complete_linux_manager_erasure(&remaining, &failed, roots, profile, completion);
                    return;
                }
                let Some(manager) = manager_ref.upgrade() else {
                    failed.store(true, Ordering::Release);
                    complete_linux_manager_erasure(&remaining, &failed, roots, profile, completion);
                    return;
                };
                let retained_manager = manager.clone();
                manager.fetch(
                    WebsiteDataTypes::ALL,
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        // Keep a strong native manager reference through the
                        // fetch callback; dropping it earlier could turn an
                        // ephemeral-context verification into a use-after-
                        // release race hidden by an empty directory.
                        drop(retained_manager);
                        if !matches!(result, Ok(ref records) if records.is_empty()) {
                            failed.store(true, Ordering::Release);
                        }
                        complete_linux_manager_erasure(
                            &remaining, &failed, roots, profile, completion,
                        );
                    },
                );
            },
        );
    }
}

fn complete_linux_manager_erasure(
    remaining: &AtomicUsize,
    failed: &AtomicBool,
    roots: Vec<std::path::PathBuf>,
    profile: zephium_core::ids::ProfileId,
    completion: Arc<crate::erasure::Completion>,
) {
    if remaining.fetch_sub(1, Ordering::AcqRel) != 1 {
        return;
    }
    if failed.load(Ordering::Acquire) {
        completion.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
    } else {
        remove_linux_profile_directories_async(roots, profile, completion);
    }
}

fn remove_linux_profile_directories_async(
    roots: Vec<std::path::PathBuf>,
    profile: zephium_core::ids::ProfileId,
    completion: Arc<crate::erasure::Completion>,
) {
    let attempt = completion.attempt_flag();
    // Capture the GTK owner's thread-default context while still inside the
    // native callback. A process-global default source could be dispatched by
    // the wrong loop in embedded/multi-context hosts.
    let owner_context = gtk::glib::MainContext::ref_thread_default();
    let failed = completion.clone();
    let task = move || {
        let verified = crate::erasure::remove_profile_directories_verified(&roots, profile);
        completion.finish(if verified {
            zephium_core::ports::engine::ProfileDataErasureOutcome::Verified
        } else {
            zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
        });
        if verified {
            // WebsiteDataManager is main-thread-bound. Ask the GTK owner to
            // release the retained strong handles only after this exact
            // attempt verified disk absence. Generation matching in the host
            // prevents a late callback from erasing a newer retry's proof.
            owner_context.invoke(move || {
                crate::host::release_linux_erasure_obligations(profile, attempt);
            });
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("zephium-linux-profile-delete".into())
        .spawn(task)
    {
        eprintln!("privacy: cannot start Linux profile-directory deletion: {error}");
        failed.finish(zephium_core::ports::engine::ProfileDataErasureOutcome::Failed);
    }
}

#[cfg(test)]
mod tests {
    use gtk::prelude::{ContainerExt, WidgetExt};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};
    use webkit2gtk::{WebContextExt, WebViewExt};
    use wry::{WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};
    use zephium_core::ids::ProfileId;

    fn proc_parent_map() -> HashMap<u32, u32> {
        let mut parents = HashMap::new();
        for entry in std::fs::read_dir("/proc").expect("Linux procfs is required") {
            let Ok(entry) = entry else { continue };
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
                continue;
            };
            let Some(parent) = status.lines().find_map(|line| {
                line.strip_prefix("PPid:")
                    .and_then(|value| value.trim().parse::<u32>().ok())
            }) else {
                continue;
            };
            parents.insert(pid, parent);
        }
        parents
    }

    fn descendants_of(root: u32) -> HashSet<u32> {
        let parents = proc_parent_map();
        let mut descendants = HashSet::from([root]);
        let mut changed = true;
        while changed {
            changed = false;
            for (&pid, &parent) in &parents {
                if descendants.contains(&parent) && descendants.insert(pid) {
                    changed = true;
                }
            }
        }
        descendants.remove(&root);
        descendants
    }

    fn web_process_descendant(root: u32) -> Option<u32> {
        descendants_of(root).into_iter().find(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .ok()
                .map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .replace('\0', " ")
                        .contains("WebKitWebProcess")
                })
                .unwrap_or(false)
        })
    }

    fn wait_for_web_process(root: u32) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            if let Some(pid) = web_process_descendant(root) {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "WebKitGTK did not spawn a descendant WebKitWebProcess"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn proc_status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key).map(str::trim))
    }

    fn prove_web_process_confinement(pid: u32, host_only_file: &Path) {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status"))
            .expect("read WebKitWebProcess status");
        assert_eq!(
            proc_status_value(&status, "NoNewPrivs:"),
            Some("1"),
            "WebKitWebProcess must forbid privilege escalation"
        );
        assert_eq!(
            proc_status_value(&status, "Seccomp:"),
            Some("2"),
            "WebKitWebProcess must run under a seccomp filter"
        );

        for namespace in ["mnt", "user", "pid"] {
            let host_namespace = std::fs::read_link(format!("/proc/self/ns/{namespace}"))
                .unwrap_or_else(|error| panic!("read host {namespace} namespace: {error}"));
            let renderer_namespace = std::fs::read_link(format!("/proc/{pid}/ns/{namespace}"))
                .unwrap_or_else(|error| {
                    panic!("read WebKitWebProcess {namespace} namespace: {error}")
                });
            assert_ne!(
                host_namespace, renderer_namespace,
                "WebKitWebProcess must not share the browser's {namespace} namespace"
            );
        }

        let relative = host_only_file
            .strip_prefix("/")
            .expect("host-only test path must be absolute");
        let renderer_path = PathBuf::from(format!("/proc/{pid}/root")).join(relative);
        assert!(
            std::fs::read(&renderer_path).is_err(),
            "WebKitWebProcess can read a host-only path through its sandbox root: {}",
            renderer_path.display()
        );
    }

    #[test]
    fn storage_postcondition_rejects_mode_and_path_mismatches() {
        let profile = ProfileId::from(7);
        let expected = Path::new("/owned/profile");
        let other = Path::new("/other/profile");

        assert!(validate_storage_postcondition(
            Partition::Persistent(profile),
            false,
            Some(expected),
            Some(expected),
            Some(expected),
        )
        .is_ok());
        assert!(validate_storage_postcondition(
            Partition::Ephemeral(profile),
            true,
            None,
            None,
            None,
        )
        .is_ok());

        assert!(validate_storage_postcondition(
            Partition::Persistent(profile),
            true,
            Some(expected),
            Some(expected),
            Some(expected),
        )
        .is_err());
        assert!(validate_storage_postcondition(
            Partition::Persistent(profile),
            false,
            Some(expected),
            Some(expected),
            Some(other),
        )
        .is_err());
        assert!(validate_storage_postcondition(
            Partition::Persistent(profile),
            false,
            None,
            Some(expected),
            Some(expected),
        )
        .is_err());
        assert!(validate_storage_postcondition(
            Partition::Ephemeral(profile),
            false,
            None,
            None,
            None,
        )
        .is_err());
        assert!(validate_storage_postcondition(
            Partition::Ephemeral(profile),
            true,
            None,
            Some(expected),
            None,
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn reported_storage_directory_rejects_alias_spellings() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().join("expected");
        let alias = temp.path().join("alias");
        let parent_alias = temp.path().join("parent-alias");
        let parent_child = expected.join("child");
        std::fs::create_dir(&expected).unwrap();
        std::fs::create_dir(&parent_child).unwrap();
        symlink(&expected, &alias).unwrap();
        symlink(&expected, &parent_alias).unwrap();

        assert!(direct_canonical_directory("base data", &expected).is_ok());
        assert!(direct_canonical_directory("base data", &alias).is_err());
        assert!(direct_canonical_directory("base data", &parent_alias.join("child")).is_err());
        assert!(
            direct_canonical_directory("base data", &parent_child.join("..").join("child"))
                .is_err()
        );
    }

    #[test]
    fn webkitgtk_floor_matches_the_reviewed_security_advisory() {
        assert!(enforce_runtime_version((2, 52, 3)).is_err());
        assert!(enforce_runtime_version((2, 52, 4)).is_err());
        assert!(enforce_runtime_version((2, 52, 5)).is_ok());
        assert!(enforce_runtime_version((2, 53, 0)).is_err());
        assert!(enforce_runtime_version((2, 54, 0)).is_err());
        assert!(enforce_runtime_version((3, 0, 0)).is_err());
    }

    #[test]
    fn runtime_preconditions_reject_sandbox_override_and_expired_review() {
        assert!(enforce_runtime_preconditions(
            zephium_core::webkitgtk::SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS - 1,
            || None,
        )
        .unwrap_err()
        .contains("system clock predates"));
        assert!(enforce_runtime_preconditions(
            zephium_core::webkitgtk::SECURITY_FLOOR_PUBLISHED_UNIX_SECONDS,
            || None,
        )
        .is_ok());
        let before_deadline =
            zephium_core::webkitgtk::SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS - 1;
        assert!(enforce_runtime_preconditions(before_deadline, || None).is_ok());
        assert!(enforce_runtime_preconditions(before_deadline, || Some(
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS".into()
        ))
        .unwrap_err()
        .contains("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS"));
        assert!(enforce_runtime_preconditions(before_deadline, || Some(
            "WEBKIT_FORCE_SANDBOX".into()
        ))
        .unwrap_err()
        .contains("WEBKIT_FORCE_SANDBOX"));
        assert!(enforce_runtime_preconditions(before_deadline, || Some(
            "WEBKIT_INSPECTOR_SERVER".into()
        ))
        .unwrap_err()
        .contains("WEBKIT_INSPECTOR_SERVER"));
        assert!(enforce_runtime_preconditions(
            zephium_core::webkitgtk::SECURITY_FLOOR_REVIEW_DEADLINE_EXCLUSIVE_UNIX_SECONDS,
            || None,
        )
        .unwrap_err()
        .contains("review expired"));
    }

    #[test]
    fn invalid_manager_provenance_cannot_be_forgotten_by_empty_retry() {
        let temp = tempfile::tempdir().unwrap();
        let root = crate::erasure::canonical_owned_root(&temp.path().join("profiles")).unwrap();
        let profile = ProfileId::from(70);
        let path = crate::erasure::prepare_profile_directory(&root, profile).unwrap();
        std::fs::write(path.join("retained"), b"must-not-delete").unwrap();

        for _ in 0..2 {
            let (tx, rx) = mpsc::channel();
            let completion = crate::erasure::Completion::start(
                Box::new(move |outcome| tx.send(outcome).unwrap()),
                Arc::new(AtomicBool::new(true)),
            );
            erase_profile_data(Vec::new(), false, vec![root.clone()], profile, completion);
            assert_eq!(
                rx.recv_timeout(std::time::Duration::from_millis(100))
                    .unwrap(),
                zephium_core::ports::engine::ProfileDataErasureOutcome::Failed
            );
            assert_eq!(
                std::fs::read(path.join("retained")).unwrap(),
                b"must-not-delete"
            );
        }
    }

    #[test]
    #[ignore = "requires Xvfb and WebKitGTK at the supported security floor; CI runs this explicitly"]
    fn wry_contexts_enable_sandbox_and_cross_site_process_swap() {
        enforce_runtime_security_floor().expect("test runner must use supported WebKitGTK");
        gtk::init().expect("GTK requires an Xvfb/Wayland display for native WebView tests");

        let host_only = tempfile::Builder::new()
            .prefix("zephium-host-only-")
            .tempfile()
            .expect("host-only sandbox probe");
        std::fs::write(host_only.path(), b"renderer sandbox boundary")
            .expect("write host-only sandbox probe");

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        let container = gtk::Fixed::new();
        window.add(&container);
        window.realize();

        let data = tempfile::tempdir().expect("temporary profile directory");
        let data_path = data
            .path()
            .canonicalize()
            .expect("canonical temporary profile directory");
        let profile = ProfileId::from(7);
        let mut web_context = wry::WebContext::try_new(Some(data_path.clone()))
            .expect("secure persistent WebContext");
        let (ipc_tx, ipc_rx) = mpsc::channel();
        let persistent = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_ipc_handler(move |request| {
                let _ = ipc_tx.send(request.into_body());
            })
            .build_gtk(&container)
            .expect("persistent Wry WebView");
        configure(
            &persistent,
            0.0,
            Partition::Persistent(profile),
            Some(&data_path),
        )
        .expect("persistent storage and process postconditions");
        let persistent_obligation = website_data_manager_obligation(&persistent);
        assert!(persistent_obligation.provenance_complete);
        assert_eq!(persistent_obligation.managers.len(), 1);
        let persistent_context = persistent
            .webview()
            .context()
            .expect("persistent native WebContext");
        assert!(!persistent_context.is_ephemeral());
        assert!(persistent_context.is_sandbox_enabled());
        assert!(persistent_context.is_process_swap_on_cross_site_navigation_enabled());
        window.show_all();
        persistent
            .load_url(
                "data:text/html,<title>hostile renderer probe</title><script>\
                 window.ipc.postMessage(window.webkit?.messageHandlers?.wryIpc \
                   ? 'page-native-handler-visible' : 'bounded-ipc-ok');\
                 document.dispatchEvent(new CustomEvent('wry-ipc-message-v1', \
                   {detail: 'x'.repeat(70000)}));\
                 </script>",
            )
            .expect("start a real WebKitWebProcess");
        let web_process = wait_for_web_process(std::process::id());
        prove_web_process_confinement(web_process, host_only.path());

        let ipc_deadline = Instant::now() + Duration::from_secs(5);
        let first_ipc = loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            match ipc_rx.try_recv() {
                Ok(message) => break message,
                Err(mpsc::TryRecvError::Empty) if Instant::now() < ipc_deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("bounded isolated-world IPC did not arrive: {error}"),
            }
        };
        assert_eq!(first_ipc, "bounded-ipc-ok");
        let oversize_deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < oversize_deadline {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            assert!(
                matches!(ipc_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
                "an over-limit page-world event reached the native IPC handler"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let durable_related_view = persistent.webview();
        drop(persistent);
        assert!(
            WebViewBuilder::new_with_web_context(&mut web_context)
                .with_incognito(true)
                .build_gtk(&container)
                .is_err(),
            "incognito construction must reject a durable supplied context"
        );

        let mut private_context =
            wry::WebContext::new_ephemeral().expect("secure ephemeral WebContext");
        assert!(
            WebViewBuilder::new_with_web_context(&mut private_context)
                .with_incognito(true)
                .with_related_view(durable_related_view)
                .build_gtk(&container)
                .is_err(),
            "a durable related view must not override an ephemeral incognito context"
        );
        let incognito = WebViewBuilder::new_with_web_context(&mut private_context)
            .with_incognito(true)
            .build_gtk(&container)
            .expect("incognito Wry WebView");
        configure(
            &incognito,
            0.0,
            Partition::Ephemeral(ProfileId::from(8)),
            None,
        )
        .expect("ephemeral storage and process postconditions");
        let incognito_obligation = website_data_manager_obligation(&incognito);
        assert!(incognito_obligation.provenance_complete);
        assert_eq!(incognito_obligation.managers.len(), 1);
        let incognito_context = incognito
            .webview()
            .context()
            .expect("incognito native WebContext");
        assert!(incognito_context.is_ephemeral());
        assert!(incognito_context.is_sandbox_enabled());
        assert!(incognito_context.is_process_swap_on_cross_site_navigation_enabled());

        let second_incognito = WebViewBuilder::new_with_web_context(&mut private_context)
            .with_incognito(true)
            .with_related_view(incognito.webview())
            .build_gtk(&container)
            .expect("second incognito Wry WebView");
        configure(
            &second_incognito,
            0.0,
            Partition::Ephemeral(ProfileId::from(8)),
            None,
        )
        .expect("shared ephemeral storage and process postconditions");
        let second_obligation = website_data_manager_obligation(&second_incognito);
        assert!(second_obligation.provenance_complete);
        assert_eq!(second_obligation.managers.len(), 1);
        assert_eq!(
            incognito_obligation.managers[0].as_ptr(),
            second_obligation.managers[0].as_ptr(),
            "one private profile must retain exactly one ephemeral native manager"
        );
    }
}
