//! Closed probe vocabulary shared by requests and evidence.

use serde::{Deserialize, Serialize};

/// Deterministic control classes exercised by the native-input spike.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureCase {
    /// Ordinary button activation.
    Button,
    /// Same-document link activation.
    Link,
    /// Single-line text editing.
    TextInput,
    /// Contenteditable text editing.
    ContentEditable,
    /// Native select control interaction.
    Select,
    /// Pointer and mouse event ordering.
    PointerMouse,
    /// Keyboard event ordering.
    Keyboard,
    /// Transient user-activation lifetime.
    TransientActivation,
    /// Popup admission under transient activation.
    Popup,
    /// Clipboard-gated behavior without recording clipboard contents.
    ClipboardGate,
    /// Drag-and-drop event ordering.
    Drag,
    /// Same-origin child-frame targeting.
    Iframe,
    /// Open shadow-root targeting.
    OpenShadow,
    /// Closed shadow-root public-semantics limitation.
    ClosedShadow,
}

impl FixtureCase {
    /// Returns the one closed fixture target that proves this case's effect.
    pub const fn target(self) -> FixtureTarget {
        match self {
            Self::Button | Self::PointerMouse => FixtureTarget::Button,
            Self::Link => FixtureTarget::Link,
            Self::TextInput | Self::Keyboard => FixtureTarget::TextInput,
            Self::ContentEditable => FixtureTarget::ContentEditable,
            Self::Select => FixtureTarget::Select,
            Self::TransientActivation => FixtureTarget::ActivationButton,
            Self::Popup => FixtureTarget::PopupButton,
            Self::ClipboardGate => FixtureTarget::ClipboardButton,
            Self::Drag => FixtureTarget::DropTarget,
            Self::Iframe => FixtureTarget::FrameButton,
            Self::OpenShadow => FixtureTarget::OpenShadowButton,
            Self::ClosedShadow => FixtureTarget::ClosedShadowHost,
        }
    }
}

/// Candidate execution mechanisms. A model can never select this enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputBackend {
    /// Audited fixed semantic page recipe in an isolated content world.
    FixedDomRecipe,
    /// AppKit event delivery directly to the owned WKWebView's window.
    MacosAppKitEvent,
    /// AppKit accessibility action on a public accessibility element.
    MacosAccessibility,
    /// Visible and focused macOS operating-system input baseline.
    MacosFocusedOsInput,
    /// Input routed through an ordinary WebView2 child HWND.
    WindowsHwndInput,
    /// `ICoreWebView2CompositionController` spatial input.
    WindowsCompositionInput,
    /// Fixed diagnostics-only WebView2 DevTools input dispatch.
    WindowsCdpInput,
    /// Explicit visible human-control baseline.
    HumanBaseline,
}

/// Visibility/focus condition under which one route is evaluated.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationState {
    /// Visible, key, and explicitly dedicated to the probe.
    VisibleFocused,
    /// Visible without becoming the active/key surface.
    VisibleBackground,
    /// Native view retained but not visible.
    Hidden,
}

/// Fixed target identities used only by deterministic fixtures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureTarget {
    /// Ordinary button.
    Button,
    /// Same-document link.
    Link,
    /// Single-line input.
    TextInput,
    /// Contenteditable region.
    ContentEditable,
    /// Select control.
    Select,
    /// Transient-activation control.
    ActivationButton,
    /// Popup control.
    PopupButton,
    /// Clipboard-gate control.
    ClipboardButton,
    /// Draggable source.
    DragSource,
    /// Drop target.
    DropTarget,
    /// Same-origin frame button.
    FrameButton,
    /// Open-shadow-root button.
    OpenShadowButton,
    /// Closed-shadow-root host, whose internals are intentionally not named.
    ClosedShadowHost,
    /// Document body or other non-control fixture surface.
    Document,
}

/// Event classes retained by the bounded fixture recorder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputEventKind {
    /// Focus entered a target.
    Focus,
    /// Focus left a target.
    Blur,
    /// Pointer entered a target.
    PointerEnter,
    /// Pointer moved over a target.
    PointerMove,
    /// Pointer button depressed.
    PointerDown,
    /// Pointer button released.
    PointerUp,
    /// Mouse entered a target.
    MouseEnter,
    /// Mouse moved over a target.
    MouseMove,
    /// Mouse button depressed.
    MouseDown,
    /// Mouse button released.
    MouseUp,
    /// Click activation.
    Click,
    /// Keyboard key depressed.
    KeyDown,
    /// Editable input is about to change.
    BeforeInput,
    /// Editable state changed.
    Input,
    /// Keyboard key released.
    KeyUp,
    /// Committed form-control change.
    Change,
    /// Drag operation began.
    DragStart,
    /// Drag entered a target.
    DragEnter,
    /// Drag moved over a target.
    DragOver,
    /// Drop occurred.
    Drop,
    /// Drag operation ended.
    DragEnd,
}

/// Coarse focus identity; native handles and page identifiers never cross the contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusOwner {
    /// No focus owner was observable.
    None,
    /// Zephium Browse/chrome retained focus.
    Browse,
    /// The dedicated probe host window owned focus.
    ProbeHost,
    /// A fixture control owned DOM focus.
    FixtureTarget,
    /// Focus belonged to another application or an unclassified surface.
    External,
}

/// Privacy-preserving outcome for a gated capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    /// Gate was not exercised for this case.
    NotApplicable,
    /// Gate admitted the operation.
    Allowed,
    /// Gate rejected the operation.
    Denied,
    /// Platform behavior could not be determined safely.
    Indeterminate,
}
