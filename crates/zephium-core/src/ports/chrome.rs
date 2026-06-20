use crate::geometry::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeFrame {
    pub rect: Rect,
    pub fill_width: bool,
}

pub trait Chrome {
    fn position(&self, frame: ChromeFrame);
}
