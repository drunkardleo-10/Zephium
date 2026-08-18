// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{
  borrow::Cow,
  cell::RefCell,
  collections::HashMap,
  ffi::CString,
  panic::AssertUnwindSafe,
  ptr::NonNull,
  sync::atomic::{AtomicU64, Ordering},
};

use http::{
  header::{CONTENT_LENGTH, CONTENT_TYPE},
  Request, Response as HttpResponse, StatusCode,
};
use objc2::{
  rc::Retained,
  runtime::{AnyClass, AnyObject, ClassBuilder, ProtocolObject},
  AllocAnyThread, ClassType, DeclaredClass, Message,
};
use objc2_foundation::{
  NSData, NSError, NSHTTPURLResponse, NSMutableDictionary, NSObject, NSObjectProtocol, NSString,
  NSURL, NSUUID,
};
use objc2_web_kit::{WKURLSchemeHandler, WKURLSchemeTask};

use crate::{
  native_admission::{InFlightPermit, CUSTOM_PROTOCOL_OVERFLOW_STATUS},
  native_bounds::{
    bounded_nsstring, CustomProtocolRequestBudget, CUSTOM_PROTOCOL_HEADER_NAME_LIMIT,
    CUSTOM_PROTOCOL_HEADER_VALUE_LIMIT, CUSTOM_PROTOCOL_METHOD_LIMIT, PAGE_URL_LIMIT,
  },
  wkwebview::WEBVIEW_STATE,
  RequestAsyncResponder, WryWebView,
};

static NEXT_PENDING_RESPONSE: AtomicU64 = AtomicU64::new(1);

struct PendingResponse {
  task: Retained<ProtocolObject<dyn WKURLSchemeTask>>,
  webview: Retained<WryWebView>,
  task_key: usize,
  task_uuid: Retained<NSUUID>,
  webview_id: String,
  url: Retained<NSURL>,
  _permit: InFlightPermit,
}

thread_local! {
  // WKURLSchemeTask and WryWebView are main-thread-only Objective-C objects.
  // Keep their ownership on that thread and let asynchronous workers carry
  // only the opaque token used to find them again.
  static PENDING_RESPONSES: RefCell<HashMap<u64, PendingResponse>> =
    RefCell::new(HashMap::new());
}

fn fail_task(task: &ProtocolObject<dyn WKURLSchemeTask>) {
  // WebKit requires every started task to complete exactly once. This local
  // error avoids leaving malformed native requests pending forever while also
  // avoiding assumptions about an optional URL or response object.
  let domain = NSString::from_str("org.tauri.wry.custom-protocol");
  // SAFETY: `None` supplies no generically typed user-info dictionary and the
  // domain is a valid process-local NSString.
  let error = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
  let _ = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
    task.didFailWithError(&error);
  }));
}

fn finish_task_with_empty_status(
  task: &ProtocolObject<dyn WKURLSchemeTask>,
  url: &NSURL,
  status: StatusCode,
) {
  let response = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
    NSHTTPURLResponse::alloc(),
    url,
    status.as_u16() as isize,
    Some(&NSString::from_str("HTTP/1.1")),
    None,
  );
  let Some(response) = response else {
    fail_task(task);
    return;
  };
  let result = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
    task.didReceiveResponse(&response);
    task.didFinish();
  }));
  if result.is_err() {
    #[cfg(feature = "tracing")]
    tracing::warn!("WebKit rejected a fail-closed custom protocol response");
  }
}

fn check_webview_id_valid(webview_id: &str) -> crate::Result<()> {
  let state = WEBVIEW_STATE
    .read()
    .map_err(|_| crate::Error::CustomProtocolTaskInvalid)?;
  if !state.contains_key(webview_id) {
    return Err(crate::Error::CustomProtocolTaskInvalid);
  }
  Ok(())
}

fn check_task_is_valid(pending: &PendingResponse) -> crate::Result<()> {
  let latest_task_uuid = pending
    .webview
    .get_custom_task_uuid(pending.task_key)
    .ok_or(crate::Error::CustomProtocolTaskInvalid)?;
  if latest_task_uuid != pending.task_uuid {
    return Err(crate::Error::CustomProtocolTaskInvalid);
  }
  Ok(())
}

fn insert_pending_response(pending: PendingResponse) -> Option<u64> {
  PENDING_RESPONSES
    .try_with(|responses| {
      let mut responses = responses.try_borrow_mut().ok()?;
      loop {
        let token = NEXT_PENDING_RESPONSE.fetch_add(1, Ordering::Relaxed);
        if token != 0 && !responses.contains_key(&token) {
          responses.insert(token, pending);
          return Some(token);
        }
      }
    })
    .ok()
    .flatten()
}

fn take_pending_response(token: u64) -> Option<PendingResponse> {
  PENDING_RESPONSES
    .try_with(|responses| responses.try_borrow_mut().ok()?.remove(&token))
    .ok()
    .flatten()
}

fn cancel_pending_task(webview_id: &str, task_key: usize) {
  let _ = PENDING_RESPONSES.try_with(|responses| {
    if let Ok(mut responses) = responses.try_borrow_mut() {
      responses
        .retain(|_, pending| pending.webview_id != webview_id || pending.task_key != task_key);
    }
  });
}

pub(crate) fn cancel_pending_for_webview(webview_id: &str) {
  let _ = PENDING_RESPONSES.try_with(|responses| {
    if let Ok(mut responses) = responses.try_borrow_mut() {
      responses.retain(|_, pending| pending.webview_id != webview_id);
    }
  });
}

fn respond_to_pending(
  pending: &PendingResponse,
  sent_response: HttpResponse<Cow<'static, [u8]>>,
) -> crate::Result<()> {
  check_webview_id_valid(&pending.webview_id)?;
  check_task_is_valid(pending)?;

  let content_len = sent_response.body().len();
  let wanted_status_code = sent_response.status().as_u16() as isize;
  let wanted_version = format!("{:#?}", sent_response.version());

  let headers = NSMutableDictionary::new();
  if let Some(mime) = sent_response.headers().get(CONTENT_TYPE) {
    if let Ok(mime) = mime.to_str() {
      headers.insert(
        &*NSString::from_str(CONTENT_TYPE.as_str()),
        &*NSString::from_str(mime),
      );
    }
  }
  headers.insert(
    &*NSString::from_str(CONTENT_LENGTH.as_str()),
    &*NSString::from_str(&content_len.to_string()),
  );
  for (name, value) in sent_response.headers() {
    if let Ok(value) = value.to_str() {
      headers.insert(
        &*NSString::from_str(name.as_str()),
        &*NSString::from_str(value),
      );
    }
  }

  let response = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
    NSHTTPURLResponse::alloc(),
    &pending.url,
    wanted_status_code,
    Some(&NSString::from_str(&wanted_version)),
    Some(&headers),
  );
  let Some(response) = response else {
    fail_task(&pending.task);
    return Err(crate::Error::CustomProtocolTaskInvalid);
  };

  check_task_is_valid(pending)?;
  if objc2::exception::catch(AssertUnwindSafe(|| unsafe {
    pending.task.didReceiveResponse(&response);
  }))
  .is_err()
  {
    fail_task(&pending.task);
    return Err(crate::Error::CustomProtocolTaskInvalid);
  }

  let data = match sent_response.into_body() {
    Cow::Owned(content) => NSData::from_vec(content),
    Cow::Borrowed(content) => NSData::with_bytes(content),
  };

  check_task_is_valid(pending)?;
  if objc2::exception::catch(AssertUnwindSafe(|| unsafe {
    pending.task.didReceiveData(&data);
  }))
  .is_err()
  {
    fail_task(&pending.task);
    return Err(crate::Error::CustomProtocolTaskInvalid);
  }

  check_task_is_valid(pending)?;
  objc2::exception::catch(AssertUnwindSafe(|| unsafe {
    pending.task.didFinish();
  }))
  .map_err(|_| crate::Error::CustomProtocolTaskInvalid)?;
  Ok(())
}

fn finish_pending_response(token: u64, sent_response: HttpResponse<Cow<'static, [u8]>>) {
  let Some(pending) = take_pending_response(token) else {
    return;
  };
  let result = respond_to_pending(&pending, sent_response);
  pending.webview.remove_custom_task_key(pending.task_key);
  if let Err(_error) = result {
    #[cfg(feature = "tracing")]
    tracing::warn!("failed to complete custom protocol task: {_error:?}");
  }
}

pub fn create(name: &str) -> crate::Result<&AnyClass> {
  unsafe {
    // Include the address of WEBVIEW_STATE in the class name so that each dylib in the process
    // gets its own ObjC class with method pointers into its own code and data segments.
    let unique_id = std::ptr::addr_of!(WEBVIEW_STATE) as usize;
    let scheme_name = CString::new(format!("{name}URLSchemeHandler_{unique_id:x}"))?;
    let cls = ClassBuilder::new(&scheme_name, NSObject::class());
    match cls {
      Some(mut cls) => {
        cls.add_ivar::<usize>(c"protocol_index");
        cls.add_method(
          objc2::sel!(webView:startURLSchemeTask:),
          start_task as extern "C" fn(_, _, _, _),
        );
        cls.add_method(
          objc2::sel!(webView:stopURLSchemeTask:),
          stop_task as extern "C" fn(_, _, _, _),
        );
        Ok(cls.register())
      }
      None => AnyClass::get(&scheme_name).ok_or(crate::Error::NativeObjectUnavailable(
        "custom URL scheme Objective-C class",
      )),
    }
  }
}

// Task handler for custom protocol
extern "C" fn start_task(
  this: &AnyObject,
  _sel: objc2::runtime::Sel,
  webview: &WryWebView,
  task: &ProtocolObject<dyn WKURLSchemeTask>,
) {
  #[cfg(feature = "tracing")]
  let span =
    tracing::info_span!(parent: None, "wry::custom_protocol::handle", uri = tracing::field::Empty)
      .entered();

  let task_key = task.hash(); // hash by task object address

  // Store the identifier in Wry's Rust-managed WebView ivars. The old
  // dynamically allocated C string had no matching Objective-C deallocator
  // and gave asynchronous responders a fabricated process-long lifetime.
  let webview_id = webview.ivars().webview_id.clone();

  let Some(ivar) = this.class().instance_variable(c"protocol_index") else {
    fail_task(task);
    return;
  };
  let protocol_index: usize = unsafe { *ivar.load(this) };

  let function = WEBVIEW_STATE.read().ok().and_then(|state| {
    state
      .get(&webview_id)
      .and_then(|v| v.protocol_ptrs.get(protocol_index))
      .cloned()
  });

  if let Some(function) = function {
    // Get url request
    let request = unsafe { task.request() };
    let Some(url) = request.URL() else {
      fail_task(task);
      return;
    };

    let Some(uri) = url
      .absoluteString()
      .and_then(|uri| bounded_nsstring(&uri, PAGE_URL_LIMIT))
    else {
      fail_task(task);
      return;
    };

    let Some(permit) = webview.ivars().custom_protocol_admission.try_acquire() else {
      finish_task_with_empty_status(task, &url, CUSTOM_PROTOCOL_OVERFLOW_STATUS);
      return;
    };

    #[cfg(feature = "tracing")]
    span.record("uri", uri.clone());

    let mut budget = CustomProtocolRequestBudget::default();

    // Get request method (GET, POST, PUT etc...) without first duplicating an
    // attacker-sized NSString into Rust.
    let method = match request.HTTPMethod() {
      Some(method) => {
        let Some(method) = bounded_nsstring(&method, CUSTOM_PROTOCOL_METHOD_LIMIT) else {
          fail_task(task);
          return;
        };
        method
      }
      None => "GET".to_owned(),
    };

    // Prepare our HttpRequest
    let mut http_request = Request::builder().uri(uri).method(method.as_str());

    // Get body
    let mut sent_form_body = Vec::new();
    let body = request.HTTPBody();
    let body_stream = request.HTTPBodyStream();
    if let Some(body) = body {
      if !budget.admit_body_chunk(body.length()) {
        fail_task(task);
        return;
      }
      sent_form_body = body.to_vec();
    } else if let Some(body_stream) = body_stream {
      body_stream.open();

      let mut buf = [0u8; 128];
      while body_stream.hasBytesAvailable() {
        // `buf.as_mut_ptr()` is non-null because the backing array is non-empty.
        let buf_ptr = unsafe { NonNull::new_unchecked(buf.as_mut_ptr()) };
        let count = unsafe { body_stream.read_maxLength(buf_ptr, buf.len()) };
        if count <= 0 {
          break;
        }
        let count = usize::try_from(count).unwrap_or_default().min(buf.len());
        if !budget.admit_body_chunk(count) {
          body_stream.close();
          fail_task(task);
          return;
        }
        sent_form_body.extend_from_slice(&buf[..count]);
      }

      body_stream.close();
    }

    // Extract all headers fields
    let all_headers = request.allHTTPHeaderFields();

    // get all our headers values and inject them in our request
    if let Some(all_headers) = all_headers {
      for current_header in all_headers.allKeys().iter() {
        let Some(header_value) = all_headers.valueForKey(&current_header) else {
          continue;
        };
        let (Some(current_header), Some(header_value)) = (
          bounded_nsstring(&current_header, CUSTOM_PROTOCOL_HEADER_NAME_LIMIT),
          bounded_nsstring(&header_value, CUSTOM_PROTOCOL_HEADER_VALUE_LIMIT),
        ) else {
          fail_task(task);
          return;
        };
        if !budget.admit_header(current_header.len(), header_value.len()) {
          fail_task(task);
          return;
        }
        // inject the header into the request
        http_request = http_request.header(current_header, header_value);
      }
    }

    let respond_with_404 = || finish_task_with_empty_status(task, &url, StatusCode::NOT_FOUND);

    match http_request.body(sent_form_body) {
      Ok(final_request) => {
        let Some(task_uuid) = webview.add_custom_task_key(task_key) else {
          fail_task(task);
          return;
        };

        let Some(pending_token) = insert_pending_response(PendingResponse {
          task: task.retain(),
          webview: webview.retain(),
          task_key,
          task_uuid,
          webview_id: webview_id.clone(),
          url: url.clone(),
          _permit: permit,
        }) else {
          webview.remove_custom_task_key(task_key);
          fail_task(task);
          return;
        };

        let responder: Box<dyn FnOnce(HttpResponse<Cow<'static, [u8]>>) + Send> =
          Box::new(move |sent_response| {
            crate::wkwebview::util::dispatch_main(move || {
              #[cfg(feature = "tracing")]
              let _span = tracing::info_span!("wry::custom_protocol::call_handler").entered();
              finish_pending_response(pending_token, sent_response);
            });
          });

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("wry::custom_protocol::call_handler").entered();

        function(
          &webview_id,
          final_request,
          RequestAsyncResponder {
            responder: Some(responder),
          },
        );
      }
      Err(_) => respond_with_404(),
    };
  } else {
    #[cfg(feature = "tracing")]
    tracing::warn!(
      "Either WebView or WebContext instance is dropped! This handler shouldn't be called."
    );
    fail_task(task);
  };
}

extern "C" fn stop_task(
  _this: &ProtocolObject<dyn WKURLSchemeHandler>,
  _sel: objc2::runtime::Sel,
  webview: &WryWebView,
  task: &ProtocolObject<dyn WKURLSchemeTask>,
) {
  let task_key = task.hash();
  cancel_pending_task(&webview.ivars().webview_id, task_key);
  webview.remove_custom_task_key(task_key);
}

#[cfg(test)]
mod tests {
  use super::*;

  struct CancelAfterPendingResponsesDrops;

  impl Drop for CancelAfterPendingResponsesDrops {
    fn drop(&mut self) {
      cancel_pending_for_webview("already-dropped");
    }
  }

  thread_local! {
    static CANCEL_AFTER: CancelAfterPendingResponsesDrops = const { CancelAfterPendingResponsesDrops };
  }

  #[test]
  fn webview_cleanup_is_safe_during_thread_local_destruction() {
    std::thread::spawn(|| {
      // TLS destructors run in reverse initialization order. Initialize the
      // caller first so PENDING_RESPONSES is gone when its destructor calls
      // the public cleanup path.
      CANCEL_AFTER.with(|_| {});
      PENDING_RESPONSES.with(|_| {});
    })
    .join()
    .unwrap();
  }
}
