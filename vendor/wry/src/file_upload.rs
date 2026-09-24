// Copyright 2026 Zephium contributors
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use objc2_foundation::{NSArray, NSURL};

use crate::PermissionOrigin;

pub(crate) type FileUploadHandler =
  dyn Fn(&objc2_web_kit::WKWebView, FileUploadRequest, FileUploadResponder);
type Completion = Box<dyn FnOnce(Option<&NSArray<NSURL>>)>;

/// Native upload-control metadata. Selected file URLs never enter page IPC.
pub struct FileUploadRequest {
  /// Structured origin supplied by WebKit for the initiating frame.
  pub origin: PermissionOrigin,
  /// Whether this input accepts multiple selected entries.
  pub allows_multiple_selection: bool,
  /// Whether this input selects directories instead of ordinary files.
  pub allows_directories: bool,
}

/// Single-use, main-thread upload completion. Dropping it cancels selection.
///
/// This type intentionally is neither Clone nor Send. Only return URLs selected
/// by the user in a native picker; never synthesize selections from page data.
pub struct FileUploadResponder {
  completion: Option<Completion>,
}

impl FileUploadResponder {
  pub(crate) fn new(completion: impl FnOnce(Option<&NSArray<NSURL>>) + 'static) -> Self {
    Self {
      completion: Some(Box::new(completion)),
    }
  }

  /// Complete the original input request with native picker URLs, or cancel it.
  pub fn respond(mut self, urls: Option<&NSArray<NSURL>>) {
    if let Some(completion) = self.completion.take() {
      completion(urls);
    }
  }
}

impl Drop for FileUploadResponder {
  fn drop(&mut self) {
    if let Some(completion) = self.completion.take() {
      completion(None);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{cell::Cell, rc::Rc};

  #[test]
  fn selection_is_returned_once_without_copying_file_data() {
    let count = Rc::new(Cell::new(0));
    let observed = count.clone();
    let responder = FileUploadResponder::new(move |urls| {
      assert_eq!(urls.unwrap().len(), 1);
      observed.set(observed.get() + 1);
    });
    let url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(
      "/tmp/zephium-generated-fixture.txt",
    ));
    let urls = NSArray::from_retained_slice(&[url]);
    responder.respond(Some(&urls));
    assert_eq!(count.get(), 1);
  }

  #[test]
  fn file_selection_requires_explicit_construction_opt_in() {
    use crate::{WebViewBuilder, WebViewBuilderExtMacos};
    let denied = WebViewBuilder::new();
    assert!(denied.platform_specific.file_upload_handler.is_none());
    let allowed = WebViewBuilder::new().with_file_upload_handler(|_, _, _| {});
    assert!(allowed.platform_specific.file_upload_handler.is_some());
  }

  #[test]
  fn explicit_cancel_and_abandoned_request_each_settle_once() {
    for explicit in [false, true] {
      let count = Rc::new(Cell::new(0));
      let observed = count.clone();
      let responder = FileUploadResponder::new(move |urls| {
        assert!(urls.is_none());
        observed.set(observed.get() + 1);
      });
      if explicit {
        responder.respond(None);
      } else {
        drop(responder);
      }
      assert_eq!(count.get(), 1);
    }
  }

  #[test]
  fn unwinding_a_broker_cancels_the_native_request() {
    let count = Rc::new(Cell::new(0));
    let observed = count.clone();
    let responder = FileUploadResponder::new(move |urls| {
      assert!(urls.is_none());
      observed.set(observed.get() + 1);
    });
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
      let _retained = responder;
      panic!("broker failed");
    }));
    assert_eq!(count.get(), 1);
  }
}
