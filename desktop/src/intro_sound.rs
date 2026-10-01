//! The one sound Zephium makes: the intro, as the name forms on first run.
//!
//! Played natively so the privileged chrome keeps `autoplay=()` in its
//! permissions policy. The asset is bundled and fixed; chrome can only ask
//! for it to start, never supply audio of its own.

#[cfg(target_os = "macos")]
pub(crate) fn play(app: &tauri::AppHandle) {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;

    static INTRO: &[u8] = include_bytes!("../../assets/sounds/intro.m4a");

    thread_local! {
        // AppKit stops a sound that is deallocated mid-play, so the one
        // playing is held until the next request replaces it.
        static PLAYING: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
    }

    let _ = app.run_on_main_thread(|| {
        let data = NSData::with_bytes(INTRO);
        let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) else {
            return;
        };
        if sound.play() {
            PLAYING.with(|playing| *playing.borrow_mut() = Some(sound));
        }
    });
}

#[cfg(target_os = "windows")]
pub(crate) fn play(_app: &tauri::AppHandle) {
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

    // IMA ADPCM, which the system's own codec decodes; the bytes are static,
    // so they outlive the asynchronous playback that reads them.
    static INTRO: &[u8] = include_bytes!("../../assets/sounds/intro.wav");

    // SAFETY: with SND_MEMORY the first argument is a pointer to a complete
    // WAV image in memory, valid for the life of the process.
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(INTRO.as_ptr().cast()),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn play(_app: &tauri::AppHandle) {}
