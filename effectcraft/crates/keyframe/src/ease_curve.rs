//! Ease curves: the temporal ease of one keyframe segment, independent of its duration and values,
//! so it can be kept as a preset and applied to any pair of keyframes.
//!
//! A curve is the out side of the first key and the in side of the second, in the Keyframe Velocity
//! dialog's terms: influence in percent of the segment, and speed relative to the segment's average
//! speed (1 = linear, 0 = at rest). In the normalised value graph (time and value 0 → 1) its handles
//! are the points (x1, y1) = (out influence, out speed × out influence) and
//! (x2, y2) = (1 − in influence, 1 − in speed × in influence).

use serde::{Deserialize, Serialize};

use crate::{Ease, Interp, Keyframe, Value, effective_ease, secs, spatial_segment_length};

/// Smallest influence (percent), as in the Keyframe Velocity dialog.
const MIN_INFLUENCE: f64 = 0.1;
/// Relative speeds are kept within ± this (a handle 100 times steeper than linear).
const MAX_SPEED: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EaseCurve {
    pub out_influence: f64,
    pub out_speed: f64,
    pub in_influence: f64,
    pub in_speed: f64,
}

impl EaseCurve {
    /// Constant speed: both handles a third of the way along the straight line.
    pub const LINEAR: EaseCurve = EaseCurve { out_influence: 100.0 / 3.0, out_speed: 1.0, in_influence: 100.0 / 3.0, in_speed: 1.0 };

    /// The curve with influences in 0.1–100 % and speeds within ±100; `None` when a number isn't
    /// finite.
    pub fn sanitized(self) -> Option<EaseCurve> {
        if ![self.out_influence, self.out_speed, self.in_influence, self.in_speed].iter().all(|v| v.is_finite()) {
            return None;
        }
        let inf = |v: f64| v.clamp(MIN_INFLUENCE, 100.0);
        let speed = |v: f64| v.clamp(-MAX_SPEED, MAX_SPEED);
        Some(EaseCurve {
            out_influence: inf(self.out_influence),
            out_speed: speed(self.out_speed),
            in_influence: inf(self.in_influence),
            in_speed: speed(self.in_speed),
        })
    }

    /// Handles in the normalised value graph: `[x1, y1, x2, y2]`.
    pub fn handles(&self) -> [f64; 4] {
        let (o, i) = (self.out_influence / 100.0, self.in_influence / 100.0);
        [o, self.out_speed * o, 1.0 - i, 1.0 - self.in_speed * i]
    }

    /// The curve with handles `[x1, y1, x2, y2]` (each influence at least 0.1 %); `None` when a
    /// number isn't finite.
    pub fn from_handles([x1, y1, x2, y2]: [f64; 4]) -> Option<EaseCurve> {
        let o = x1.clamp(MIN_INFLUENCE / 100.0, 1.0);
        let i = (1.0 - x2).clamp(MIN_INFLUENCE / 100.0, 1.0);
        EaseCurve { out_influence: o * 100.0, out_speed: y1 / o, in_influence: i * 100.0, in_speed: (1.0 - y2) / i }.sanitized()
    }

    /// Ease segment `i` → `i + 1` with this curve: the out side of key `i` and the in side of key
    /// `i + 1` become Bezier, scaled to the segment per dimension (along the motion path for
    /// spatial values, as overall progress for paths and gradients), as Easy Ease shapes them.
    /// Auto-Bezier keys keep their other side as it plays. False when there is no such segment or
    /// its values don't interpolate.
    pub fn apply(&self, keys: &mut [Keyframe], i: usize, spatial: bool) -> bool {
        let Some(slopes) = segment_slopes(keys, i, spatial) else { return false };
        let side = |influence: f64, speed: f64| -> Vec<Ease> { slopes.iter().map(|s| Ease { speed: speed * s, influence: influence / 100.0 }).collect() };
        let (out, inn) = (side(self.out_influence, self.out_speed), side(self.in_influence, self.in_speed));
        freeze_auto(keys, i, spatial);
        freeze_auto(keys, i + 1, spatial);
        if let Some(a) = keys.get_mut(i) {
            a.out_interp = Interp::Bezier;
            a.out_ease = out;
        }
        if let Some(b) = keys.get_mut(i + 1) {
            b.in_interp = Interp::Bezier;
            b.in_ease = inn;
        }
        true
    }

    /// The curve segment `i` → `i + 1` plays (a linear side counts as speed 1 at 33.3 %), measured
    /// on the dimension that changes most. `None` for hold segments, values that don't
    /// interpolate and segments whose value doesn't change.
    pub fn measure(keys: &[Keyframe], i: usize, spatial: bool) -> Option<EaseCurve> {
        let slopes = segment_slopes(keys, i, spatial)?;
        let (a, b) = (keys.get(i)?, keys.get(i + 1)?);
        if a.out_interp == Interp::Hold {
            return None;
        }
        let (d, slope) = slopes.iter().copied().enumerate().max_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))?;
        if slope.abs() < 1e-12 {
            return None;
        }
        let is_spatial = is_spatial(&a.value, spatial);
        let d = if is_spatial { 0 } else { d };
        let side =
            |k: usize, bezier: bool, out: bool| if bezier { effective_ease(keys, k, d, is_spatial, out) } else { Ease { speed: slope, influence: 1.0 / 3.0 } };
        let out = side(i, a.out_interp == Interp::Bezier, true);
        let inn = side(i + 1, b.in_interp == Interp::Bezier, false);
        EaseCurve { out_influence: out.influence * 100.0, out_speed: out.speed / slope, in_influence: inn.influence * 100.0, in_speed: inn.speed / slope }
            .sanitized()
    }
}

fn is_spatial(v: &Value, spatial: bool) -> bool {
    spatial && matches!(v, Value::Vec2(_) | Value::Vec3(_))
}

/// Average speed of segment `i` → `i + 1` for each ease entry of its keys: per dimension, along
/// the motion path (spatial), or 1 / duration (paths and gradients ease their overall progress).
fn segment_slopes(keys: &[Keyframe], i: usize, spatial: bool) -> Option<Vec<f64>> {
    let (a, b) = (keys.get(i)?, keys.get(i.checked_add(1)?)?);
    let dur = secs(b.time) - secs(a.time);
    if !a.value.interpolates() || !dur.is_finite() || dur <= 0.0 {
        return None;
    }
    let n = a.value.dims().max(1);
    let (ca, cb) = (a.value.components(), b.value.components());
    Some(if is_spatial(&a.value, spatial) {
        vec![spatial_segment_length(keys, i) / dur; n]
    } else if ca.is_empty() || ca.len() != cb.len() {
        vec![1.0 / dur; n]
    } else {
        ca.iter().zip(&cb).map(|(x, y)| (y - x) / dur).collect()
    })
}

/// Store the eases an Auto-Bezier key `k` plays and turn Auto-Bezier off, so editing one side
/// leaves the other as it was.
fn freeze_auto(keys: &mut [Keyframe], k: usize, spatial: bool) {
    let Some(key) = keys.get(k).filter(|key| key.auto_bezier) else { return };
    let is_spatial = is_spatial(&key.value, spatial);
    let n = key.value.dims().max(1);
    let side = |out: bool| -> Vec<Ease> { (0..n).map(|d| effective_ease(keys, k, if is_spatial { 0 } else { d }, is_spatial, out)).collect() };
    let (inn, out) = (side(false), side(true));
    if let Some(key) = keys.get_mut(k) {
        key.in_ease = inn;
        key.out_ease = out;
        key.auto_bezier = false;
    }
}

#[cfg(test)]
mod tests {
    use effectcraft_time::Tick;

    use super::*;
    use crate::{ease_progress, evaluate};

    fn s(x: f64) -> Tick {
        Tick::from_seconds_f64(x)
    }

    const CURVE: EaseCurve = EaseCurve { out_influence: 60.0, out_speed: 0.0, in_influence: 20.0, in_speed: 2.0 };

    #[test]
    fn handles_round_trip_and_reject_non_finite_numbers() {
        let h = CURVE.handles();
        assert!(h.iter().zip([0.6, 0.0, 0.8, 0.6]).all(|(a, b)| (a - b).abs() < 1e-12), "{h:?}");
        let back = EaseCurve::from_handles(h).unwrap();
        for (a, b) in [(back.out_influence, 60.0), (back.out_speed, 0.0), (back.in_influence, 20.0), (back.in_speed, 2.0)] {
            assert!((a - b).abs() < 1e-9, "{back:?}");
        }
        // Influences stay ≥ 0.1 % even with a handle on the key.
        assert!((EaseCurve::from_handles([0.0, 0.0, 1.0, 1.0]).unwrap().out_influence - MIN_INFLUENCE).abs() < 1e-12);
        assert!(EaseCurve::from_handles([f64::NAN, 0.0, 0.5, 1.0]).is_none());
        assert!(EaseCurve { out_speed: f64::INFINITY, ..CURVE }.sanitized().is_none());
        assert_eq!(EaseCurve { in_speed: 1e9, ..CURVE }.sanitized().unwrap().in_speed, MAX_SPEED);
    }

    #[test]
    fn applied_curve_scales_to_each_dimension_and_measures_back() {
        let mut keys = vec![Keyframe::new(s(1.0), Value::Vec2([0.0, 100.0])), Keyframe::new(s(3.0), Value::Vec2([200.0, 50.0]))];
        assert!(CURVE.apply(&mut keys, 0, false));
        assert_eq!((keys[0].out_interp, keys[1].in_interp), (Interp::Bezier, Interp::Bezier));
        // X goes 200 in 2 s (100/s), Y −50 (−25/s): speeds are relative to those.
        assert_eq!(keys[1].in_ease, vec![Ease { speed: 200.0, influence: 0.2 }, Ease { speed: -50.0, influence: 0.2 }]);
        // Every dimension follows the curve's progress.
        let p = evaluate(&keys, s(2.0), false).unwrap().as_vec2();
        let f = ease_progress(0.0, 0.6, 2.0, 0.2, 0.5);
        assert!((p[0] - 200.0 * f).abs() < 1e-6 && (p[1] - (100.0 - 50.0 * f)).abs() < 1e-6, "{p:?}");
        let m = EaseCurve::measure(&keys, 0, false).unwrap();
        assert!((m.out_influence - 60.0).abs() < 1e-9 && (m.in_speed - 2.0).abs() < 1e-9, "{m:?}");
        // No such segment / nothing to ease.
        assert!(!CURVE.apply(&mut keys, 1, false));
        let mut held = vec![Keyframe::new(s(0.0), Value::Bool(false)), Keyframe::new(s(1.0), Value::Bool(true))];
        assert!(!CURVE.apply(&mut held, 0, false));
    }

    #[test]
    fn spatial_curve_follows_the_motion_path() {
        let mut keys = vec![Keyframe::new(s(0.0), Value::Vec2([0.0, 0.0])), Keyframe::new(s(1.0), Value::Vec2([300.0, 400.0]))];
        assert!(CURVE.apply(&mut keys, 0, true));
        // 500 px along the path in 1 s.
        assert!(keys[1].in_ease.iter().all(|e| (e.speed - 1000.0).abs() < 1e-6), "{:?}", keys[1].in_ease);
        let p = evaluate(&keys, s(0.5), true).unwrap().as_vec2();
        let f = ease_progress(0.0, 0.6, 2.0, 0.2, 0.5);
        // (within the arc-length table's precision)
        assert!((p[0] - 300.0 * f).abs() < 0.05 && (p[1] - 400.0 * f).abs() < 0.05, "{p:?} {f}");
    }

    #[test]
    fn linear_and_auto_bezier_sides_measure_as_they_play() {
        let keys = vec![Keyframe::new(s(0.0), Value::Scalar(0.0)), Keyframe::new(s(1.0), Value::Scalar(10.0))];
        let m = EaseCurve::measure(&keys, 0, false).unwrap();
        assert!((m.out_speed - 1.0).abs() < 1e-9 && (m.in_influence - 100.0 / 3.0).abs() < 1e-9, "{m:?}");
        // Easing the second segment keeps the auto-Bezier middle key's incoming side.
        let mut keys = vec![
            Keyframe::new(s(0.0), Value::Scalar(0.0)),
            Keyframe::new(s(1.0), Value::Scalar(10.0)).eased(),
            Keyframe::new(s(2.0), Value::Scalar(30.0)).eased(),
        ];
        keys[1].auto_bezier = true;
        let before = evaluate(&keys, s(0.5), false);
        assert!(CURVE.apply(&mut keys, 1, false));
        assert!(!keys[1].auto_bezier);
        assert_eq!(evaluate(&keys, s(0.5), false), before);
        let held = vec![Keyframe::new(s(0.0), Value::Scalar(0.0)).hold(), Keyframe::new(s(1.0), Value::Scalar(10.0))];
        assert!(EaseCurve::measure(&held, 0, false).is_none());
        let flat = vec![Keyframe::new(s(0.0), Value::Scalar(5.0)), Keyframe::new(s(1.0), Value::Scalar(5.0))];
        assert!(EaseCurve::measure(&flat, 0, false).is_none());
    }
}
