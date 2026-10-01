// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{collections::HashMap, sync::Mutex};

#[cfg(target_os = "macos")]
use objc2::runtime::ProtocolObject;
use objc2::{define_class, rc::Retained, runtime::Bool, DeclaredClass};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSDraggingDestination, NSEvent, NSMenu};
use objc2_foundation::{NSObjectProtocol, NSUUID};

#[cfg(not(target_os = "macos"))]
use crate::native_admission::InFlightAdmission;
#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use crate::{
  native_admission::InFlightAdmission,
  wkwebview::{drag_drop, synthetic_mouse_events},
  DragDropEvent,
};
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIEvent as NSEvent;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

pub struct WryWebViewIvars {
  pub(crate) webview_id: String,
  pub(crate) is_child: bool,
  #[cfg(target_os = "macos")]
  pub(crate) drag_drop_handler: Option<Box<dyn Fn(DragDropEvent) -> bool>>,
  #[cfg(target_os = "macos")]
  pub(crate) accept_first_mouse: objc2::runtime::Bool,
  #[cfg(target_os = "macos")]
  pub(crate) context_menu_handler:
    Option<Box<dyn Fn(&NSEvent, Option<Retained<NSMenu>>) -> Option<Retained<NSMenu>>>>,
  #[cfg(target_os = "ios")]
  pub(crate) input_accessory_view_builder: Option<Box<crate::InputAccessoryViewBuilder>>,
  pub(crate) custom_protocol_task_ids: Mutex<HashMap<usize, Retained<NSUUID>>>,
  /// One in-flight scope per registered scheme, by protocol index: a slow IPC
  /// call never holds back the app's own assets, nor they it.
  pub(crate) custom_protocol_admission: Vec<InFlightAdmission>,
}

define_class!(
  #[unsafe(super(WKWebView))]
  #[ivars = WryWebViewIvars]
  pub struct WryWebView;

  /// Overridden NSView methods.
  impl WryWebView {
    #[cfg(target_os = "macos")]
    #[unsafe(method(acceptsFirstResponder))]
    fn accepts_first_responder(&self) -> Bool {
      if self.isHidden() {
        Bool::NO
      } else {
        unsafe { objc2::msg_send![super(self), acceptsFirstResponder] }
      }
    }

    #[cfg(target_os = "macos")]
    #[unsafe(method(becomeFirstResponder))]
    fn become_first_responder(&self) -> Bool {
      if self.isHidden() {
        Bool::NO
      } else {
        unsafe { objc2::msg_send![super(self), becomeFirstResponder] }
      }
    }

    #[unsafe(method(performKeyEquivalent:))]
    fn perform_key_equivalent(&self, event: &NSEvent) -> Bool {
      // This is a temporary workaround for https://github.com/tauri-apps/tauri/issues/9426
      // FIXME: When the webview is a child webview, performKeyEquivalent always return YES
      // and stop propagating the event to the window, hence the menu shortcut won't be
      // triggered. However, overriding this method also means the cmd+key event won't be
      // handled in webview, which means the key cannot be listened by JavaScript.
      if self.ivars().is_child {
        Bool::NO
      } else {
        unsafe { objc2::msg_send![super(self), performKeyEquivalent: event] }
      }
    }

    #[cfg(target_os = "macos")]
    #[unsafe(method(acceptsFirstMouse:))]
    fn accept_first_mouse(&self, _event: &NSEvent) -> Bool {
      self.ivars().accept_first_mouse
    }

    #[cfg(target_os = "macos")]
    #[unsafe(method_id(menuForEvent:))]
    fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
      (|| -> Option<Retained<NSMenu>> {
        let default_menu = unsafe { objc2::msg_send![super(self), menuForEvent: event] };
        let Some(handler) = &self.ivars().context_menu_handler else {
          return default_menu;
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          handler(event, default_menu)
        }))
        .unwrap_or(None)
      })()
    }

    #[cfg(target_os = "ios")]
    #[unsafe(method_id(inputAccessoryView))]
    fn input_accessory_view(&self) -> Option<Retained<objc2_ui_kit::UIView>> {
      if let Some(builder) = &self.ivars().input_accessory_view_builder {
        builder(self)
      } else {
        unsafe { objc2::msg_send![super(self), inputAccessoryView] }
      }
    }
  }
  unsafe impl NSObjectProtocol for WryWebView {}

  // Drag & Drop
  #[cfg(target_os = "macos")]
  unsafe impl NSDraggingDestination for WryWebView {
    #[unsafe(method(draggingEntered:))]
    fn dragging_entered(
      &self,
      drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>,
    ) -> objc2_app_kit::NSDragOperation {
      drag_drop::dragging_entered(self, drag_info)
    }

    #[unsafe(method(draggingUpdated:))]
    fn dragging_updated(
      &self,
      drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>,
    ) -> objc2_app_kit::NSDragOperation {
      drag_drop::dragging_updated(self, drag_info)
    }

    #[unsafe(method(performDragOperation:))]
    fn perform_drag_operation(
      &self,
      drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>,
    ) -> Bool {
      drag_drop::perform_drag_operation(self, drag_info)
    }

    #[unsafe(method(draggingExited:))]
    fn dragging_exited(&self, drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>) {
      drag_drop::dragging_exited(self, drag_info)
    }
  }

  // Synthetic mouse events
  #[cfg(target_os = "macos")]
  impl WryWebView {
    #[unsafe(method(otherMouseDown:))]
    fn other_mouse_down(&self, event: &NSEvent) {
      synthetic_mouse_events::other_mouse_down(self, event)
    }

    #[unsafe(method(otherMouseUp:))]
    fn other_mouse_up(&self, event: &NSEvent) {
      synthetic_mouse_events::other_mouse_up(self, event)
    }
  }
);

// Custom Protocol Task Checker
impl WryWebView {
  pub(crate) fn add_custom_task_key(&self, task_id: usize) -> Option<Retained<NSUUID>> {
    let task_uuid = NSUUID::new();
    self
      .ivars()
      .custom_protocol_task_ids
      .lock()
      .ok()?
      .insert(task_id, task_uuid.clone());
    Some(task_uuid)
  }
  pub(crate) fn remove_custom_task_key(&self, task_id: usize) {
    if let Ok(mut task_ids) = self.ivars().custom_protocol_task_ids.lock() {
      task_ids.remove(&task_id);
    }
  }
  pub(crate) fn get_custom_task_uuid(&self, task_id: usize) -> Option<Retained<NSUUID>> {
    self
      .ivars()
      .custom_protocol_task_ids
      .lock()
      .ok()?
      .get(&task_id)
      .cloned()
  }
}

#[cfg(test)]
mod tests {
  #[cfg(target_os = "macos")]
  #[test]
  fn hidden_webview_refuses_both_responder_admission_routes() {
    let source = include_str!("wry_web_view.rs");
    for method in ["fn accepts_first_responder", "fn become_first_responder"] {
      let body = source
        .split(method)
        .nth(1)
        .and_then(|source| source.split("    #[").next())
        .expect("bounded responder override");
      assert!(body.contains("if self.isHidden()"));
      assert!(body.contains("Bool::NO"));
      assert!(body.contains("super(self)"));
    }
  }

  #[test]
  fn native_context_menu_preserves_default_before_bounded_embedder_merge() {
    let source = include_str!("wry_web_view.rs");
    let body = source
      .split("fn menu_for_event")
      .nth(1)
      .and_then(|source| source.split("fn accept_first_mouse").next())
      .expect("menuForEvent override");
    let native = body.find("menuForEvent: event").expect("super menu");
    let handler = body
      .find("handler(event, default_menu)")
      .expect("embedder merge");
    assert!(native < handler);
    assert!(body.contains("catch_unwind"));
    assert!(body.contains("unwrap_or(None)"));
  }
}
