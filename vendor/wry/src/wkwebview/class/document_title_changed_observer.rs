// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{ffi::c_void, ptr::null_mut};

use objc2::{
  define_class, msg_send,
  rc::Retained,
  runtime::{AnyObject, NSObject},
  AllocAnyThread, DefinedClass,
};
use objc2_foundation::{
  ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions,
  NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSString,
};

use crate::{
  native_bounds::{bounded_nsstring, PAGE_TITLE_LIMIT},
  WryWebView,
};
pub struct DocumentTitleChangedObserverIvars {
  pub object: Retained<WryWebView>,
  pub handler: Box<dyn Fn(String)>,
}

define_class!(
  #[unsafe(super(NSObject))]
  #[ivars = DocumentTitleChangedObserverIvars]
  pub struct DocumentTitleChangedObserver;

  /// NSKeyValueObserving.
  impl DocumentTitleChangedObserver {
    #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
    fn observe_value_for_key_path(
      &self,
      key_path: Option<&NSString>,
      _of_object: Option<&AnyObject>,
      _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
      _context: *mut c_void,
    ) {
      if let Some(key_path) = key_path {
        unsafe {
          if key_path.isEqualToString(ns_string!("title")) {
            // `WKWebView.title` is explicitly nullable during navigation and
            // teardown. Read it through the typed API on the retained object;
            // never dereference a fabricated raw NSString pointer from KVO.
            let Some(title) = self.ivars().object.title() else {
              return;
            };
            // Check NSString's UTF-16 length before constructing a Rust String.
            // An over-limit title is an untrusted update, not a process error.
            let Some(title) = bounded_nsstring(&title, PAGE_TITLE_LIMIT) else {
              return;
            };
            let handler = &self.ivars().handler;
            handler(title);
          }
        }
      }
    }
  }

  unsafe impl NSObjectProtocol for DocumentTitleChangedObserver {}
);

impl DocumentTitleChangedObserver {
  pub fn new(webview: Retained<WryWebView>, handler: Box<dyn Fn(String)>) -> Retained<Self> {
    let observer = Self::alloc().set_ivars(DocumentTitleChangedObserverIvars {
      object: webview,
      handler,
    });

    let observer: Retained<Self> = unsafe { msg_send![super(observer), init] };

    unsafe {
      observer
        .ivars()
        .object
        .addObserver_forKeyPath_options_context(
          &observer,
          ns_string!("title"),
          NSKeyValueObservingOptions::New,
          null_mut(),
        );
    }

    observer
  }
}

impl Drop for DocumentTitleChangedObserver {
  fn drop(&mut self) {
    unsafe {
      self
        .ivars()
        .object
        .removeObserver_forKeyPath(self, ns_string!("title"));
    }
  }
}
