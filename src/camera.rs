//! Renderer-independent camera math. Positions are canvas-local screen points.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub zoom: f32,
    pub pan: [f32; 2],
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: [0.0; 2],
        }
    }
}
impl Camera {
    pub fn valid(self) -> bool {
        self.zoom.is_finite()
            && (0.15..=3.0).contains(&self.zoom)
            && self.pan.iter().all(|x| x.is_finite())
    }
    pub fn zoom_at(&mut self, factor: f32, anchor: [f32; 2]) {
        if !factor.is_finite() || factor <= 0.0 || !anchor.iter().all(|x| x.is_finite()) {
            return;
        }
        let zoom = (self.zoom * factor).clamp(0.15, 3.0);
        for (i, point) in anchor.iter().enumerate() {
            self.pan[i] = point - (point - self.pan[i]) * zoom / self.zoom;
        }
        self.zoom = zoom;
    }
    pub fn fit(&mut self, content: [f32; 2], viewport: [f32; 2]) {
        if content
            .iter()
            .chain(viewport.iter())
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return;
        }
        self.zoom = ((viewport[0] - 48.0).max(1.0) / content[0])
            .min((viewport[1] - 48.0).max(1.0) / content[1])
            .clamp(0.15, 1.0);
        self.pan = [
            (viewport[0] - content[0] * self.zoom) / 2.0,
            (viewport[1] - content[1] * self.zoom) / 2.0,
        ];
    }
}
