use serde::{Deserialize, Serialize};
pub const KEY: &str = "panel.geometry.v1";
pub const DEFAULT: (f64, f64) = (720.0, 520.0);
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub version: u8,
    pub monitor: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Geometry {
    pub fn decode(value: &str) -> Option<Self> {
        if value.len() > 1024 {
            return None;
        }
        let value: Self = serde_json::from_str(value).ok()?;
        value.valid().then_some(value)
    }
    pub fn valid(&self) -> bool {
        self.version == 1
            && [self.x, self.y, self.width, self.height]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100_000.0)
            && self.width > 0.0
            && self.height > 0.0
            && self.monitor.as_ref().is_none_or(|name| name.len() <= 160)
    }
    pub fn fit(&self, work_width: f64, work_height: f64) -> Self {
        let ((min_w, min_h), (max_w, max_h)) = limits(work_width, work_height);
        let width = self.width.clamp(min_w, max_w);
        let height = self.height.clamp(min_h, max_h);
        let margin_x = 24.0_f64.min((work_width - width).max(0.0) / 2.0);
        let margin_y = 24.0_f64.min((work_height - height).max(0.0) / 2.0);
        Self {
            width,
            height,
            x: self
                .x
                .clamp(margin_x, (work_width - width - margin_x).max(margin_x)),
            y: self
                .y
                .clamp(margin_y, (work_height - height - margin_y).max(margin_y)),
            ..self.clone()
        }
    }
}
pub fn limits(w: f64, h: f64) -> ((f64, f64), (f64, f64)) {
    let max_w = 960.0_f64.min((w * 0.9).min((w - 48.0).max(1.0))).max(1.0);
    let max_h = 760.0_f64.min((h * 0.9).min((h - 48.0).max(1.0))).max(1.0);
    ((560.0_f64.min(max_w), 360.0_f64.min(max_h)), (max_w, max_h))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restored_bounds_fit_small_and_changed_displays() {
        let g = Geometry {
            version: 1,
            monitor: None,
            x: 9000.0,
            y: -9000.0,
            width: 2000.0,
            height: 2000.0,
        };
        let g = g.fit(640.0, 480.0);
        assert!(g.x >= 24.0 && g.y >= 24.0);
        assert!(g.x + g.width <= 616.0);
        assert!(g.y + g.height <= 456.0);
        let tiny = g.fit(100.0, 100.0);
        assert!(tiny.width > 0.0 && tiny.x + tiny.width <= 100.0);
    }
    #[test]
    fn invalid_geometry_is_not_restored() {
        assert!(Geometry::decode(
            r#"{"version":2,"monitor":null,"x":0,"y":0,"width":720,"height":520}"#
        )
        .is_none());
        assert!(Geometry::decode("{}").is_none());
    }
}
