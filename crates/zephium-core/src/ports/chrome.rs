use crate::geometry::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeFrame {
    pub rect: Rect,
    pub fill_width: bool,
}

pub trait Chrome {
    /// Admit the exact privileged-chrome frame to its native UI queue.
    /// `false` means no task owns the transition, so raw content must remain
    /// concealed and the caller must retry or retire it.
    fn position(&self, frame: ChromeFrame) -> bool;
}
