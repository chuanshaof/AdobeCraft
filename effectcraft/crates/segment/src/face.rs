//! Trained face models for face tracking, behind one swappable interface.
//!
//! - [`FaceModel`]: what the face tracker (`effectcraft-track`) asks of a model: find a face in a
//!   region of a frame, then follow it from frame to frame. A model reports its own points (a
//!   mesh, for MediaPipe) plus a [`Topology`] saying which of them are the tracker's named
//!   landmarks and which trace the face outline, so the tracker never depends on one model's
//!   layout. The classical tracker stays built in as the fallback when no model is chosen.
//! - [`Roi`] and [`crop`]: the rotated square crops face models look at.

use crate::{ModelInfo, Result};

/// Which of a model's points are the tracker's landmarks and its face outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Topology {
    /// The points tracing the face outline, in order around the face.
    pub outline: &'static [usize],
    /// (landmark id, as the tracker names them, e.g. `leftEyeInner`; point index). "Left" is
    /// image-left. Chin and jaw come from the outline.
    pub landmarks: &'static [(&'static str, usize)],
}

/// A face in a frame: the model's points (frame pixels) and its confidence (0–1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Face {
    pub points: Vec<[f32; 2]>,
    pub score: f32,
}

/// A trained face model (swappable: the face tracker only uses this interface). Frames are
/// straight RGB 0–1, row-major `w×h`, coordinates frame pixels.
pub trait FaceModel: Send + Sync {
    fn info(&self) -> &'static ModelInfo;
    fn topology(&self) -> &'static Topology;
    /// The most confident face whose centre lies in `region` (`[x0, y0, x1, y1]`).
    fn find(&self, rgb: &[[f32; 3]], w: usize, h: usize, region: [f32; 4]) -> Result<Option<Face>>;
    /// The face `prev` was in the previous frame, in this one; `None` when it is lost.
    fn follow(&self, rgb: &[[f32; 3]], w: usize, h: usize, prev: &Face) -> Result<Option<Face>>;
}

/// A rotated square region of a frame: centre, side and angle (radians, the direction of the
/// square's x axis in the frame, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roi {
    pub center: [f32; 2],
    pub size: f32,
    pub angle: f32,
}

impl Roi {
    /// Frame position of the normalised square position `(u, v)` (0–1).
    pub fn to_frame(&self, u: f32, v: f32) -> [f32; 2] {
        let (s, c) = self.angle.sin_cos();
        let (x, y) = ((u - 0.5) * self.size, (v - 0.5) * self.size);
        [self.center[0] + c * x - s * y, self.center[1] + s * x + c * y]
    }
}

/// Sample `roi` into an `n×n` RGB tensor (bilinear; zero outside the frame), each channel mapped
/// from 0–1 to `lo`–`hi`.
pub fn crop(rgb: &[[f32; 3]], w: usize, h: usize, roi: &Roi, n: usize, [lo, hi]: [f32; 2]) -> Vec<f32> {
    let mut out = vec![0.0f32; n * n * 3];
    if rgb.len() < w * h || n == 0 {
        return out;
    }
    let px = |x: i64, y: i64| -> [f32; 3] {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 { [0.0; 3] } else { rgb.get(y as usize * w + x as usize).copied().unwrap_or([0.0; 3]) }
    };
    for j in 0..n {
        for i in 0..n {
            let p = roi.to_frame((i as f32 + 0.5) / n as f32, (j as f32 + 0.5) / n as f32);
            let (x, y) = (p[0] - 0.5, p[1] - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
            for k in 0..3 {
                let v = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
                out[(j * n + i) * 3 + k] = lo + v * (hi - lo);
            }
        }
    }
    out
}

pub(crate) fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roi_maps_corners_and_rotates() {
        let r = Roi { center: [10.0, 20.0], size: 4.0, angle: 0.0 };
        assert_eq!(r.to_frame(0.0, 0.0), [8.0, 18.0]);
        assert_eq!(r.to_frame(1.0, 1.0), [12.0, 22.0]);
        // A quarter turn: the square's x axis points down the frame.
        let q = Roi { angle: std::f32::consts::FRAC_PI_2, ..r };
        let p = q.to_frame(1.0, 0.5);
        assert!((p[0] - 10.0).abs() < 1e-5 && (p[1] - 22.0).abs() < 1e-5);
    }

    #[test]
    fn crop_samples_pixel_centres() {
        // A 4×4 ramp cropped 1:1 reproduces it; outside the frame is zero.
        let rgb: Vec<[f32; 3]> = (0..16).map(|i| [i as f32 / 16.0, 0.0, 1.0]).collect();
        let c = crop(&rgb, 4, 4, &Roi { center: [2.0, 2.0], size: 4.0, angle: 0.0 }, 4, [0.0, 1.0]);
        for i in 0..16 {
            assert!((c[i * 3] - i as f32 / 16.0).abs() < 1e-6);
        }
        let c = crop(&rgb, 4, 4, &Roi { center: [-10.0, -10.0], size: 4.0, angle: 0.0 }, 2, [-1.0, 1.0]);
        assert!(c.iter().all(|v| *v == -1.0));
    }
}
