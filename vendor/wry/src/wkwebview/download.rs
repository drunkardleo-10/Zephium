use std::{env::current_dir, path::Path, ptr::null_mut};

use objc2::{rc::Retained, runtime::ProtocolObject, DeclaredClass};
use objc2_foundation::{NSData, NSError, NSString, NSURLResponse, NSURL};
use objc2_web_kit::{WKDownload, WKNavigationAction, WKNavigationResponse};

use crate::native_bounds::{bounded_nsstring, DOWNLOAD_FILENAME_LIMIT, PAGE_URL_LIMIT};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

use super::class::{
  wry_download_delegate::WryDownloadDelegate, wry_navigation_delegate::WryNavigationDelegate,
};

// Download action handler
pub(crate) fn navigation_download_action(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _action: &WKNavigationAction,
  download: &WKDownload,
) {
  unsafe {
    if let Some(delegate) = &this.ivars().download_delegate {
      #[cfg(target_os = "macos")]
      if let Some(native) = &delegate.ivars().native {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native(download))).is_err() {
          download.cancel(None);
        }
        return;
      }
      let proto_delegate = ProtocolObject::from_ref(&**delegate);
      download.setDelegate(Some(proto_delegate));
    }
  }
}

// Download response handler
pub(crate) fn navigation_download_response(
  this: &WryNavigationDelegate,
  _webview: &WKWebView,
  _response: &WKNavigationResponse,
  download: &WKDownload,
) {
  unsafe {
    if let Some(delegate) = &this.ivars().download_delegate {
      #[cfg(target_os = "macos")]
      if let Some(native) = &delegate.ivars().native {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native(download))).is_err() {
          download.cancel(None);
        }
        return;
      }
      let proto_delegate = ProtocolObject::from_ref(&**delegate);
      download.setDelegate(Some(proto_delegate));
    }
  }
}

pub(crate) fn download_policy(
  this: &WryDownloadDelegate,
  download: &WKDownload,
  _response: &NSURLResponse,
  suggested_filename: &NSString,
  completion_handler: &block2::Block<dyn Fn(*const NSURL)>,
) {
  let Some(url) = download_url(download) else {
    (*completion_handler).call((null_mut(),));
    return;
  };
  // Treat WebKit's name as untrusted input. In particular, an absolute or
  // parent-relative suggestion must never escape the downloads directory.
  let suggested_filename = bounded_nsstring(suggested_filename, DOWNLOAD_FILENAME_LIMIT)
    .unwrap_or_else(|| "download".to_owned());
  let suggested_filename = Path::new(&suggested_filename)
    .file_name()
    .and_then(|name| name.to_str())
    .filter(|name| !name.is_empty() && *name != "." && *name != ".." && name.len() <= 240)
    .unwrap_or("download")
    .to_string();
  let mut download_destination =
    dirs::download_dir().unwrap_or_else(|| current_dir().unwrap_or_default());

  download_destination.push(&suggested_filename);

  let (suggested_filename, ext) = suggested_filename
    .split_once('.')
    .map(|(base, ext)| (base, format!(".{ext}")))
    .unwrap_or((suggested_filename.as_str(), "".to_string()));

  // WebView2 does not overwrite files but appends numbers
  if download_destination.exists() {
    const MAX_COLLISION_ATTEMPTS: u32 = 10_000;
    let mut available = false;
    for counter in 1..=MAX_COLLISION_ATTEMPTS {
      download_destination.set_file_name(format!("{suggested_filename} ({counter}){ext}"));
      if !download_destination.exists() {
        available = true;
        break;
      }
    }
    if !available {
      (*completion_handler).call((null_mut(),));
      return;
    }
  }

  let started_fn = &this.ivars().started;
  if let Some(started_fn) = started_fn {
    let Ok(mut started_fn) = started_fn.try_borrow_mut() else {
      (*completion_handler).call((null_mut(),));
      return;
    };
    match started_fn(url, &mut download_destination) {
      true => {
        let path = NSString::from_str(&download_destination.display().to_string());
        let ns_url = NSURL::fileURLWithPath_isDirectory(&path, false);
        (*completion_handler).call((Retained::as_ptr(&ns_url),))
      }
      false => (*completion_handler).call((null_mut(),)),
    };
  } else {
    #[cfg(feature = "tracing")]
    tracing::warn!("WebView instance is dropped! This navigation handler shouldn't be called.");
    (*completion_handler).call((null_mut(),));
  }
}

pub(crate) fn download_did_finish(this: &WryDownloadDelegate, download: &WKDownload) {
  if let (Some(url), Some(completed_fn)) = (download_url(download), this.ivars().completed.clone())
  {
    completed_fn(url, None, true);
  }
}

pub(crate) fn download_did_fail(
  this: &WryDownloadDelegate,
  download: &WKDownload,
  _error: &NSError,
  _resume_data: Option<&NSData>,
) {
  #[cfg(debug_assertions)]
  {
    let description = _error.localizedDescription().to_string();
    eprintln!("Download failed with error: {description}");
  }

  if let (Some(url), Some(completed_fn)) = (download_url(download), this.ivars().completed.clone())
  {
    completed_fn(url, None, false);
  }
}

fn download_url(download: &WKDownload) -> Option<String> {
  unsafe {
    download
      .originalRequest()?
      .URL()?
      .absoluteString()
      .and_then(|url| bounded_nsstring(&url, PAGE_URL_LIMIT))
  }
}
