// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::path::PathBuf;

use objc2::{
  runtime::{Bool, ProtocolObject},
  DeclaredClass,
};
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSPasteboardTypeFileURL};
use objc2_foundation::{NSPoint, NSRect, NSString, NSURL};

use crate::{
  native_bounds::{bounded_nsstring, DRAG_DROP_FILE_URL_LIMIT},
  DragDropEvent,
};

use super::WryWebView;

const MAX_DRAG_DROP_FILES: usize = 128;

pub(crate) fn collect_paths(drag_info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<PathBuf> {
  let pb = drag_info.draggingPasteboard();
  let mut drag_drop_paths = Vec::new();

  // `NSFilenamesPboardType` exposes one untyped property-list value and was
  // deprecated in macOS 10.14. Modern file drags carry one file URL per
  // pasteboard item. Treat every object and URL as untrusted native input:
  // malformed or non-file entries are ignored instead of aborting the host.
  let Some(items) = pb.pasteboardItems() else {
    return drag_drop_paths;
  };
  // SAFETY: `NSPasteboardTypeFileURL` is a process-lifetime AppKit constant on
  // every macOS release supported by this crate.
  let file_url_type = unsafe { NSPasteboardTypeFileURL };
  for item in items.iter().take(MAX_DRAG_DROP_FILES) {
    let Some(value) = item.stringForType(file_url_type) else {
      continue;
    };
    let Some(value) = bounded_nsstring(&value, DRAG_DROP_FILE_URL_LIMIT) else {
      continue;
    };
    let value = NSString::from_str(&value);
    let Some(url) = NSURL::URLWithString(&value) else {
      continue;
    };
    if !url.isFileURL() {
      continue;
    }
    if let Some(path) = url.to_file_path() {
      drag_drop_paths.push(path);
    }
  }
  drag_drop_paths
}

// Validate the complete native cohort before falling back to WebKit. A
// truncated Rust path list must never authorize an unbounded original drop.
fn native_drop_is_bounded(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> bool {
  if this.isHiddenOrHasHiddenAncestor() {
    return false;
  }
  let Some(items) = drag_info.draggingPasteboard().pasteboardItems() else {
    return true;
  };
  if items.len() > MAX_DRAG_DROP_FILES {
    return false;
  }
  let file_url_type = unsafe { NSPasteboardTypeFileURL };
  for item in items.iter() {
    if let Some(value) = item.stringForType(file_url_type) {
      let Some(value) = bounded_nsstring(&value, DRAG_DROP_FILE_URL_LIMIT) else {
        return false;
      };
      if !NSURL::URLWithString(&NSString::from_str(&value)).is_some_and(|url| url.isFileURL()) {
        return false;
      }
    }
  }
  true
}

pub(crate) fn dragging_entered(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
  let Some(listener) = this.ivars().drag_drop_handler.as_ref() else {
    // With no explicit embedder policy, deny before inspecting or copying any
    // pasteboard item. This also prevents an unbrokered file-input upload.
    return NSDragOperation::None;
  };
  let paths = collect_paths(drag_info);
  let dl: NSPoint = drag_info.draggingLocation();
  let frame: NSRect = this.frame();
  let position = (dl.x as i32, (frame.size.height - dl.y) as i32);

  if !listener(DragDropEvent::Enter { paths, position }) {
    if !native_drop_is_bounded(this, drag_info) {
      return NSDragOperation::None;
    }
    // Reject the Wry file drop (invoke the OS default behaviour)
    unsafe { objc2::msg_send![super(this), draggingEntered: drag_info] }
  } else {
    NSDragOperation::Copy
  }
}

pub(crate) fn dragging_updated(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> NSDragOperation {
  let Some(listener) = this.ivars().drag_drop_handler.as_ref() else {
    return NSDragOperation::None;
  };
  let dl: NSPoint = drag_info.draggingLocation();
  let frame: NSRect = this.frame();
  let position = (dl.x as i32, (frame.size.height - dl.y) as i32);

  if !listener(DragDropEvent::Over { position }) {
    if this.isHiddenOrHasHiddenAncestor() {
      return NSDragOperation::None;
    }
    unsafe { objc2::msg_send![super(this), draggingUpdated: drag_info] }
  } else {
    NSDragOperation::Copy
  }
}

pub(crate) fn perform_drag_operation(
  this: &WryWebView,
  drag_info: &ProtocolObject<dyn NSDraggingInfo>,
) -> Bool {
  let Some(listener) = this.ivars().drag_drop_handler.as_ref() else {
    return Bool::NO;
  };
  let paths = collect_paths(drag_info);
  let dl: NSPoint = drag_info.draggingLocation();
  let frame: NSRect = this.frame();
  let position = (dl.x as i32, (frame.size.height - dl.y) as i32);

  if !listener(DragDropEvent::Drop { paths, position }) {
    if !native_drop_is_bounded(this, drag_info) {
      return Bool::NO;
    }
    // Reject the Wry drop (invoke the OS default behaviour)
    unsafe { objc2::msg_send![super(this), performDragOperation: drag_info] }
  } else {
    Bool::YES
  }
}

pub(crate) fn dragging_exited(this: &WryWebView, drag_info: &ProtocolObject<dyn NSDraggingInfo>) {
  let Some(listener) = this.ivars().drag_drop_handler.as_ref() else {
    return;
  };
  if !listener(DragDropEvent::Leave) {
    // Reject the Wry drop (invoke the OS default behaviour)
    unsafe { objc2::msg_send![super(this), draggingExited: drag_info] }
  }
}
