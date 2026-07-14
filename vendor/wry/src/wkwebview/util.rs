// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use dispatch2::DispatchQueue;
use objc2_foundation::NSProcessInfo;

pub fn operating_system_version() -> (isize, isize, isize) {
  let process_info = NSProcessInfo::processInfo();
  let version = process_info.operatingSystemVersion();
  (
    version.majorVersion,
    version.minorVersion,
    version.patchVersion,
  )
}

/// Transfers ownership of a native-UI closure to Grand Central Dispatch's
/// main queue. `dispatch2` accesses the queue through libdispatch's exported
/// `_dispatch_main_q` object; `dispatch_get_main_queue` is a header-inline API
/// and must not be declared as a dynamically linked symbol.
pub(crate) fn dispatch_main(function: impl FnOnce() + Send + 'static) {
  if objc2_foundation::MainThreadMarker::new().is_some() {
    function();
    return;
  }

  DispatchQueue::main().exec_async(move || {
    // libdispatch callbacks have a C ABI and must never unwind across it.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(function));
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn main_queue_dispatch_path_is_linkable() {
    // The libtest harness does not guarantee that a test body runs on the
    // process main thread. Do not wait for the closure: invoking this path is
    // sufficient to force the supported libdispatch symbols into the linked
    // test binary and catches accidental bindings to header-inline APIs.
    dispatch_main(|| {});
  }
}
