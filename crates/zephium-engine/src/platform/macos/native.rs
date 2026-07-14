use zephium_core::ports::engine::Partition;

pub fn webkit(view: &wry::WebView) -> objc2::rc::Retained<objc2_web_kit::WKWebView> {
    use wry::WebViewExtMacOS;
    // SAFETY: WryWebView is a WKWebView subclass; this is a plain upcast.
    unsafe { objc2::rc::Retained::cast_unchecked(view.webview()) }
}

pub fn configure(
    webview: &wry::WebView,
    radius: f64,
    partition: Partition,
    expected_ephemeral_store: Option<&super::WebsiteDataStore>,
) -> Result<(), String> {
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSColor, NSView};

    let wk = webkit(webview);
    let data_store = unsafe { wk.configuration().websiteDataStore() };
    let persistent = unsafe { data_store.isPersistent() };
    let identifier = unsafe { data_store.identifier() }.map(|identifier| identifier.as_bytes());
    validate_data_store_postcondition(
        partition,
        persistent,
        identifier,
        expected_ephemeral_store.is_some(),
    )?;
    if let Some(expected) = expected_ephemeral_store {
        if objc2::rc::Retained::as_ptr(&data_store) != objc2::rc::Retained::as_ptr(expected) {
            return Err("private WKWebView did not use its profile-owned data store".into());
        }
    }

    unsafe { wk.setInspectable(cfg!(debug_assertions)) };
    let view: &NSView = &wk;
    // Fill the assigned region and follow window resize in AppKit's layout pass.
    view.setTranslatesAutoresizingMaskIntoConstraints(true);
    view.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
        // a hairline keeps the edge readable when page and backdrop are both
        // dark; without it the rounded corners visually vanish
        let border = NSColor::colorWithWhite_alpha(1.0, 0.09);
        layer.setBorderColor(Some(&border.CGColor()));
        layer.setBorderWidth(1.0);
    }

    Ok(())
}

fn validate_data_store_postcondition(
    partition: Partition,
    persistent: bool,
    identifier: Option<[u8; 16]>,
    has_expected_ephemeral_store: bool,
) -> Result<(), String> {
    match partition {
        Partition::Ephemeral(_) => {
            if !has_expected_ephemeral_store {
                return Err("ephemeral WKWebView has no profile-owned data-store proof".into());
            }
            if persistent {
                return Err("ephemeral WKWebView received a persistent website data store".into());
            }
            if identifier.is_some() {
                return Err("ephemeral WKWebView exposed a durable data-store identifier".into());
            }
        }
        Partition::Default(profile) | Partition::Persistent(profile) => {
            if has_expected_ephemeral_store {
                return Err("durable WKWebView received an ephemeral data-store proof".into());
            }
            if !persistent {
                return Err(
                    "durable WKWebView received a non-persistent website data store".into(),
                );
            }
            if identifier != Some(profile.bytes()) {
                return Err(
                    "durable WKWebView data-store identifier does not match its profile".into(),
                );
            }
        }
    }
    Ok(())
}

pub fn stop_loading(view: &wry::WebView) {
    unsafe { webkit(view).stopLoading() };
}

/// Cross-check renderer heuristics with WebKit's public media playback and
/// capture state. Playback is asynchronous; the caller's existing bounded
/// deadline handles a missing native completion without retaining the view.
pub fn query_document_activity(view: &wry::WebView, done: impl FnOnce(bool) + 'static) -> bool {
    use std::cell::RefCell;
    use std::rc::Rc;

    use objc2_web_kit::{WKMediaCaptureState, WKMediaPlaybackState};

    let wk = webkit(view);
    let capturing = unsafe {
        wk.cameraCaptureState() != WKMediaCaptureState::None
            || wk.microphoneCaptureState() != WKMediaCaptureState::None
    };
    if capturing {
        done(false);
        return true;
    }

    let completion = Rc::new(RefCell::new(Some(done)));
    let callback_completion = completion.clone();
    let callback = block2::RcBlock::new(move |state: WKMediaPlaybackState| {
        if let Some(done) = callback_completion.borrow_mut().take() {
            done(state != WKMediaPlaybackState::Playing);
        }
    });
    unsafe { wk.requestMediaPlaybackStateWithCompletionHandler(&callback) };
    true
}

pub fn add_user_script(view: &wry::WebView, script: &zephium_core::ports::engine::UserScript) {
    use objc2::MainThreadOnly;
    use objc2_foundation::{MainThreadMarker, NSString};
    use objc2_web_kit::{WKContentWorld, WKUserScript, WKUserScriptInjectionTime};
    use zephium_core::ports::engine::World;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let wk = webkit(view);
    let source = NSString::from_str(&script.source);
    let time = if script.at_start {
        WKUserScriptInjectionTime::AtDocumentStart
    } else {
        WKUserScriptInjectionTime::AtDocumentEnd
    };
    let user_script = unsafe {
        match script.world {
            World::Page => WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &source,
                time,
                false,
            ),
            World::Isolated => {
                let world = WKContentWorld::worldWithName(&NSString::from_str("zephium"), mtm);
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    WKUserScript::alloc(mtm),
                    &source,
                    time,
                    false,
                    &world,
                )
            }
        }
    };
    unsafe {
        wk.configuration()
            .userContentController()
            .addUserScript(&user_script)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_core::ids::ProfileId;

    #[test]
    fn data_store_postcondition_binds_persistence_and_profile_identity() {
        let profile = ProfileId::from(7);
        let other = ProfileId::from(8);

        assert!(validate_data_store_postcondition(
            Partition::Persistent(profile),
            true,
            Some(profile.bytes()),
            false,
        )
        .is_ok());
        assert!(validate_data_store_postcondition(
            Partition::Default(profile),
            true,
            Some(profile.bytes()),
            false,
        )
        .is_ok());
        assert!(validate_data_store_postcondition(
            Partition::Ephemeral(profile),
            false,
            None,
            true
        )
        .is_ok());

        assert!(validate_data_store_postcondition(
            Partition::Persistent(profile),
            false,
            Some(profile.bytes()),
            false,
        )
        .is_err());
        assert!(validate_data_store_postcondition(
            Partition::Persistent(profile),
            true,
            Some(other.bytes()),
            false,
        )
        .is_err());
        assert!(validate_data_store_postcondition(
            Partition::Persistent(profile),
            true,
            None,
            false
        )
        .is_err());
        assert!(
            validate_data_store_postcondition(Partition::Ephemeral(profile), true, None, true)
                .is_err()
        );
        assert!(validate_data_store_postcondition(
            Partition::Ephemeral(profile),
            false,
            Some(profile.bytes()),
            true,
        )
        .is_err());
        assert!(validate_data_store_postcondition(
            Partition::Ephemeral(profile),
            false,
            None,
            false
        )
        .is_err());
        assert!(validate_data_store_postcondition(
            Partition::Persistent(profile),
            true,
            Some(profile.bytes()),
            true,
        )
        .is_err());
    }
}
