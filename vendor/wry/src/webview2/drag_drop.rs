// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// A silly implementation of file drop handling for Windows!

use crate::DragDropEvent;

use std::{
  cell::UnsafeCell,
  ffi::OsString,
  os::{raw::c_void, windows::ffi::OsStringExt},
  path::PathBuf,
  ptr,
  rc::Rc,
};

use windows::{
  core::{implement, BOOL},
  Win32::{
    Foundation::{DRAGDROP_E_NOTREGISTERED, E_POINTER, HWND, LPARAM, POINT, POINTL},
    Graphics::Gdi::ScreenToClient,
    System::{
      Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL},
      Ole::{
        IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
        CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
      },
      SystemServices::MODIFIERKEYS_FLAGS,
    },
    UI::{
      Shell::{DragQueryFileW, HDROP},
      WindowsAndMessaging::EnumChildWindows,
    },
  },
};

#[derive(Default)]
pub(crate) struct DragDropController {
  drop_targets: Vec<(HWND, IDropTarget)>,
}

impl Drop for DragDropController {
  fn drop(&mut self) {
    for (hwnd, _) in self.drop_targets.drain(..) {
      let _ = unsafe { RevokeDragDrop(hwnd) };
    }
  }
}

impl DragDropController {
  #[inline]
  pub(crate) fn new(hwnd: HWND, handler: Box<dyn Fn(DragDropEvent) -> bool>) -> Self {
    let mut controller = DragDropController::default();

    let handler = Rc::new(handler);

    // Enumerate child windows to find the WebView2 "window" and override!
    {
      let mut callback = |hwnd| controller.inject_in_hwnd(hwnd, handler.clone());
      let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut callback;
      let closure_pointer_pointer: *mut c_void = unsafe { std::mem::transmute(&mut trait_obj) };
      let lparam = LPARAM(closure_pointer_pointer as _);
      unsafe extern "system" fn enumerate_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let closure = &mut *(lparam.0 as *mut c_void as *mut &mut dyn FnMut(HWND) -> bool);
        closure(hwnd).into()
      }
      let _ = unsafe { EnumChildWindows(Some(hwnd), Some(enumerate_callback), lparam) };
    }

    controller
  }

  #[inline]
  fn inject_in_hwnd(&mut self, hwnd: HWND, handler: Rc<dyn Fn(DragDropEvent) -> bool>) -> bool {
    let drag_drop_target: IDropTarget = DragDropTarget::new(hwnd, handler).into();
    match unsafe { RevokeDragDrop(hwnd) } {
      Ok(()) => {}
      Err(error) if error.code() == DRAGDROP_E_NOTREGISTERED => {}
      Err(_) => return true,
    }
    if unsafe { RegisterDragDrop(hwnd, &drag_drop_target) }.is_ok() {
      self.drop_targets.push((hwnd, drag_drop_target));
    }

    true
  }
}

#[implement(IDropTarget)]
pub struct DragDropTarget {
  hwnd: HWND,
  listener: Rc<dyn Fn(DragDropEvent) -> bool>,
  cursor_effect: UnsafeCell<DROPEFFECT>,
  enter_is_valid: UnsafeCell<bool>, /* If the currently hovered item is not valid there must not be any `HoveredFileCancelled` emitted */
}

impl DragDropTarget {
  pub fn new(hwnd: HWND, listener: Rc<dyn Fn(DragDropEvent) -> bool>) -> DragDropTarget {
    Self {
      hwnd,
      listener,
      cursor_effect: DROPEFFECT_NONE.into(),
      enter_is_valid: false.into(),
    }
  }

  unsafe fn iterate_filenames<F>(
    data_obj: windows_core::Ref<'_, IDataObject>,
    mut callback: F,
  ) -> bool
  where
    F: FnMut(PathBuf),
  {
    let drop_format = FORMATETC {
      cfFormat: CF_HDROP.0,
      ptd: ptr::null_mut(),
      dwAspect: DVASPECT_CONTENT.0,
      lindex: -1,
      tymed: TYMED_HGLOBAL.0 as u32,
    };

    let Some(data_obj) = data_obj.as_ref() else {
      return false;
    };
    match data_obj.GetData(&drop_format) {
      Ok(medium) => {
        struct StgMediumGuard(STGMEDIUM);
        impl Drop for StgMediumGuard {
          fn drop(&mut self) {
            unsafe { ReleaseStgMedium(&mut self.0) };
          }
        }
        let medium = StgMediumGuard(medium);
        let hdrop = HDROP(medium.0.u.hGlobal.0 as _);
        if hdrop.0.is_null() {
          return false;
        }

        // The second parameter (0xFFFFFFFF) instructs the function to return the item count
        const MAX_DROPPED_FILES: u32 = 4_096;
        const MAX_PATH_CODE_UNITS: usize = 32_767;
        let item_count = DragQueryFileW(hdrop, 0xFFFFFFFF, None).min(MAX_DROPPED_FILES);

        for i in 0..item_count {
          // Get the length of the path string NOT including the terminating null character.
          // Previously, this was using a fixed size array of MAX_PATH length, but the
          // Windows API allows longer paths under certain circumstances.
          let character_count = DragQueryFileW(hdrop, i, None) as usize;
          if character_count == 0 || character_count > MAX_PATH_CODE_UNITS {
            continue;
          }

          // Fill path_buf with the null-terminated file name
          let Some(str_len) = character_count.checked_add(1) else {
            continue;
          };
          let mut path_buf = vec![0; str_len];
          let copied = DragQueryFileW(hdrop, i, Some(&mut path_buf)) as usize;
          if copied == 0 || copied > character_count {
            continue;
          }
          callback(OsString::from_wide(&path_buf[..copied]).into());
        }

        true
      }
      Err(_error) => {
        #[cfg(feature = "tracing")]
        tracing::warn!(
          "{}",
          match _error.code() {
            windows::Win32::Foundation::DV_E_FORMATETC => {
              // If the dropped item is not a file this error will occur.
              // In this case it is OK to return without taking further action.
              "Error occurred while processing dropped/hovered item: item is not a file."
            }
            _ => "Unexpected error occurred while processing dropped/hovered item.",
          }
        );
        false
      }
    }
  }

  unsafe fn initialize_effect(effect: *mut DROPEFFECT) -> windows::core::Result<()> {
    let Some(effect) = effect.as_mut() else {
      return Err(windows::core::Error::from(E_POINTER));
    };
    *effect = DROPEFFECT_NONE;
    Ok(())
  }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for DragDropTarget_Impl {
  fn DragEnter(
    &self,
    pDataObj: windows_core::Ref<'_, IDataObject>,
    _grfKeyState: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    pdwEffect: *mut DROPEFFECT,
  ) -> windows::core::Result<()> {
    unsafe { DragDropTarget::initialize_effect(pdwEffect)? };
    unsafe {
      *self.enter_is_valid.get() = false;
      *self.cursor_effect.get() = DROPEFFECT_NONE;
    }
    let mut pt = POINT { x: pt.x, y: pt.y };
    if !unsafe { ScreenToClient(self.hwnd, &mut pt) }.as_bool() {
      return Ok(());
    }

    let mut paths = Vec::new();
    let enter_is_valid =
      unsafe { DragDropTarget::iterate_filenames(pDataObj, |path| paths.push(path)) }
        && !paths.is_empty();

    if !enter_is_valid {
      return Ok(());
    };

    unsafe {
      *self.enter_is_valid.get() = enter_is_valid;
    }

    (self.listener)(DragDropEvent::Enter {
      paths,
      position: (pt.x as _, pt.y as _),
    });

    let cursor_effect = DROPEFFECT_COPY;

    unsafe {
      *pdwEffect = cursor_effect;
      *self.cursor_effect.get() = cursor_effect;
    }

    Ok(())
  }

  fn DragOver(
    &self,
    _grfKeyState: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    pdwEffect: *mut DROPEFFECT,
  ) -> windows::core::Result<()> {
    unsafe { DragDropTarget::initialize_effect(pdwEffect)? };
    if unsafe { *self.enter_is_valid.get() } {
      let mut pt = POINT { x: pt.x, y: pt.y };
      if !unsafe { ScreenToClient(self.hwnd, &mut pt) }.as_bool() {
        return Ok(());
      }
      (self.listener)(DragDropEvent::Over {
        position: (pt.x as _, pt.y as _),
      });
    }

    unsafe { *pdwEffect = *self.cursor_effect.get() };
    Ok(())
  }

  fn DragLeave(&self) -> windows::core::Result<()> {
    if unsafe { *self.enter_is_valid.get() } {
      (self.listener)(DragDropEvent::Leave);
    }
    unsafe {
      *self.enter_is_valid.get() = false;
      *self.cursor_effect.get() = DROPEFFECT_NONE;
    }
    Ok(())
  }

  fn Drop(
    &self,
    pDataObj: windows_core::Ref<'_, IDataObject>,
    _grfKeyState: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    pdwEffect: *mut DROPEFFECT,
  ) -> windows::core::Result<()> {
    unsafe { DragDropTarget::initialize_effect(pdwEffect)? };
    if unsafe { *self.enter_is_valid.get() } {
      let mut pt = POINT { x: pt.x, y: pt.y };
      if !unsafe { ScreenToClient(self.hwnd, &mut pt) }.as_bool() {
        return Ok(());
      }

      let mut paths = Vec::new();
      if unsafe { DragDropTarget::iterate_filenames(pDataObj, |path| paths.push(path)) }
        && !paths.is_empty()
      {
        (self.listener)(DragDropEvent::Drop {
          paths,
          position: (pt.x as _, pt.y as _),
        });
        unsafe { *pdwEffect = DROPEFFECT_COPY };
      }
    }

    unsafe {
      *self.enter_is_valid.get() = false;
      *self.cursor_effect.get() = DROPEFFECT_NONE;
    }

    Ok(())
  }
}
