//! Fixed local responder experiment, excluded from production.

use super::*;
use objc2::Message;

fn supports_click(case: FixtureCase) -> bool {
    matches!(
        case,
        FixtureCase::Button
            | FixtureCase::Link
            | FixtureCase::PointerMouse
            | FixtureCase::TransientActivation
            | FixtureCase::Popup
            | FixtureCase::ClipboardGate
            | FixtureCase::Iframe
            | FixtureCase::OpenShadow
            | FixtureCase::ClosedShadow
    )
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "ZephiumOwnedInputProbeChrome"]
    #[thread_kind = MainThreadOnly]
    struct ProbeChrome;

    impl ProbeChrome {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { true }
    }
);

pub(super) struct OwnedInputSurface {
    chrome: Retained<ProbeChrome>,
    page: Retained<WryWebView>,
    window: Retained<NSWindow>,
    frame: NSRect,
}

impl OwnedInputSurface {
    pub(super) fn new(
        mtm: MainThreadMarker,
        host: &NSView,
        window: &NSWindow,
        page: &Retained<WryWebView>,
    ) -> Result<Self, AdapterError> {
        // SAFETY: main-thread NSView initializer with the exact retained host bounds.
        let chrome: Retained<ProbeChrome> =
            unsafe { msg_send![ProbeChrome::alloc(mtm), initWithFrame: host.bounds()] };
        if !super::super::passive_page::register(page) {
            return Err(AdapterError::NativeConstruction);
        }
        host.addSubview(&chrome);
        Ok(Self {
            chrome,
            page: page.clone(),
            window: window.retain(),
            frame: page.frame(),
        })
    }

    pub(super) fn focus_chrome(&self) -> bool {
        self.window.makeFirstResponder(Some(&self.chrome))
    }

    pub(super) fn current(&self) -> bool {
        self.page.frame() == self.frame
            && self.window.isKeyWindow()
            && self
                .page
                .window()
                .is_some_and(|window| std::ptr::eq(&*window, &*self.window))
            && self.window.firstResponder().is_some_and(|responder| {
                std::ptr::from_ref(&*responder).addr() == Retained::as_ptr(&self.chrome).addr()
            })
            && !self.page.isHiddenOrHasHiddenAncestor()
    }

    pub(super) fn verify_refusals(
        &self,
        case: FixtureCase,
        geometry: Geometry,
        control: &mut NativeDispatchControl<'_>,
    ) -> Result<(), AdapterError> {
        if !supports_click(case) {
            return Ok(());
        }
        if !self.current() || self.window.makeFirstResponder(Some(&self.page)) || !self.current() {
            return Err(AdapterError::FocusPolicy);
        }
        let mut shifted = self.frame;
        shifted.origin.x += 1.0;
        self.page.setFrame(shifted);
        let changed_frame = self.dispatch_click(case, geometry, control);
        self.page.setFrame(self.frame);
        if changed_frame != Err(AdapterError::FocusPolicy) {
            return Err(AdapterError::InvalidEvidence);
        }
        if !self.window.makeFirstResponder(None) {
            return Err(AdapterError::FocusPolicy);
        }
        let changed_responder = self.dispatch_click(case, geometry, control);
        let restored = self.focus_chrome();
        if changed_responder != Err(AdapterError::FocusPolicy) || !restored || !self.current() {
            return Err(AdapterError::InvalidEvidence);
        }
        Ok(())
    }

    pub(super) fn dispatch_click(
        &self,
        case: FixtureCase,
        geometry: Geometry,
        control: &mut NativeDispatchControl<'_>,
    ) -> Result<bool, AdapterError> {
        if !supports_click(case) {
            return Ok(false);
        }
        control.check()?;
        if !self.current() {
            return Err(AdapterError::FocusPolicy);
        }
        let point = window_point(
            &self.page,
            geometry.x + geometry.width / 2.0,
            geometry.y + geometry.height / 2.0,
        )?;
        // SAFETY: the owned page remains attached to this retained main-thread window.
        let parent = unsafe { self.page.superview() }.ok_or(AdapterError::InvalidEvidence)?;
        let parent_point = parent.convertPoint_fromView(point, None);
        if !parent.hitTest(parent_point).is_some_and(|hit| {
            Retained::as_ptr(&hit).addr() == Retained::as_ptr(&self.chrome).addr()
        }) {
            return Err(AdapterError::InvalidEvidence);
        }
        let target = self
            .page
            .hitTest(parent_point)
            .ok_or(AdapterError::InvalidEvidence)?;
        if !target.isDescendantOf(&self.page) {
            return Err(AdapterError::InvalidEvidence);
        }
        let chrome_point = self.chrome.convertPoint_fromView(point, None);
        if !self.chrome.mouse_inRect(chrome_point, self.chrome.bounds()) {
            return Err(AdapterError::InvalidEvidence);
        }

        for event_type in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                event_type, point, NSEventModifierFlags::empty(),
                NSProcessInfo::processInfo().systemUptime(), self.window.windowNumber(),
                None, 0, 1, if event_type == NSEventType::LeftMouseDown { 1.0 } else { 0.0 },
            ).ok_or(AdapterError::NativeConstruction)?;
            control.check()?;
            if !self.current()
                || !self
                    .page
                    .hitTest(parent_point)
                    .is_some_and(|fresh| std::ptr::eq(&*fresh, &*target))
            {
                return Err(AdapterError::FocusPolicy);
            }
            if event_type == NSEventType::LeftMouseDown {
                target.mouseDown(&event);
            } else {
                target.mouseUp(&event);
            }
        }
        if !self.current() {
            return Err(AdapterError::FocusPolicy);
        }
        Ok(true)
    }
}

impl Drop for OwnedInputSurface {
    fn drop(&mut self) {
        let _ = self.window.makeFirstResponder(None);
        self.chrome.removeFromSuperview();
        super::super::passive_page::unregister(&self.page);
    }
}
