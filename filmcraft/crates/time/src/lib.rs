//! Exact media time for FilmCraft.
//!
//! All time is an integer number of [`Tick`]s at [`TICKS_PER_SECOND`] = 254 016 000 000/s. That
//! rate divides evenly into every broadcast frame duration (23.976, 24, 25, 29.97, 30, 48, 50,
//! 59.94, 60, 120 …) and every common audio sample duration (8 k … 192 kHz, including the 44.1 k
//! family), so edits, frame math and audio alignment never drift.
//!
//! Timecode (SMPTE drop/non-drop, frames, feet+frames, samples) is only a *display* of ticks.
//!
//! Every conversion is total: a zero or negative rate (from a damaged file or project) gives
//! zero instead of a division-by-zero panic, and an invalid [`FrameRate`] counts as the default.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Ticks per second.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// A point or duration in time, in ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(pub i64);

impl Tick {
    pub const ZERO: Tick = Tick(0);
    pub const MAX: Tick = Tick(i64::MAX / 4);
    pub const MIN: Tick = Tick(i64::MIN / 4);

    pub fn from_seconds_f64(s: f64) -> Tick {
        Tick((s * TICKS_PER_SECOND as f64).round() as i64)
    }
    pub fn seconds(self) -> f64 {
        self.0 as f64 / TICKS_PER_SECOND as f64
    }
    /// Exact conversion from a count of `units` at `per_second` (e.g. samples at 48 000).
    pub fn from_units(units: i64, per_second: i64) -> Tick {
        Tick((units as i128 * TICKS_PER_SECOND as i128).checked_div(per_second as i128).unwrap_or(0) as i64)
    }
    /// Floor conversion to a count of units at `per_second`.
    pub fn to_units_floor(self, per_second: i64) -> i64 {
        (self.0 as i128 * per_second as i128).div_euclid(TICKS_PER_SECOND as i128) as i64
    }
    /// Conversion from a rational timestamp `pts * num / den` seconds (container timebases).
    pub fn from_rational(pts: i64, num: i64, den: i64) -> Tick {
        let t = pts as i128 * num as i128 * TICKS_PER_SECOND as i128;
        Tick(t.checked_div_euclid(den as i128).unwrap_or(0) as i64)
    }
    /// Inverse of [`Tick::from_rational`], floored to the timebase.
    pub fn to_rational_floor(self, num: i64, den: i64) -> i64 {
        (self.0 as i128 * den as i128).checked_div_euclid(num as i128 * TICKS_PER_SECOND as i128).unwrap_or(0) as i64
    }
    /// Inverse of [`Tick::from_rational`], rounded to the nearest timebase unit (halves up).
    ///
    /// For finding a sample by its timestamp: containers store timestamps rounded to their
    /// timebase (Matroska usually to 1 ms), so a stamp can sit up to half a unit after the true
    /// time. Flooring the requested time misses every frame whose stamp was rounded up.
    pub fn to_rational_round(self, num: i64, den: i64) -> i64 {
        let n = self.0 as i128 * den as i128;
        let unit = num as i128 * TICKS_PER_SECOND as i128;
        let (n, unit) = if unit < 0 { (-n, -unit) } else { (n, unit) };
        let (Some(q), Some(r)) = (n.checked_div_euclid(unit), n.checked_rem_euclid(unit)) else { return 0 };
        // 0 <= r < unit: round up from the halfway point
        (if r * 2 >= unit { q + 1 } else { q }) as i64
    }
    pub fn abs(self) -> Tick {
        Tick(self.0.abs())
    }
    pub fn min(self, o: Tick) -> Tick {
        Tick(self.0.min(o.0))
    }
    pub fn max(self, o: Tick) -> Tick {
        Tick(self.0.max(o.0))
    }
    pub fn clamp(self, lo: Tick, hi: Tick) -> Tick {
        // max/min rather than `clamp`, which panics when the bounds cross.
        Tick(self.0.max(lo.0).min(hi.0))
    }
    /// Scale by a rational factor (`num/den`), flooring.
    pub fn mul_ratio(self, num: i64, den: i64) -> Tick {
        Tick((self.0 as i128 * num as i128).checked_div_euclid(den as i128).unwrap_or(0) as i64)
    }
}

impl Add for Tick {
    type Output = Tick;
    fn add(self, o: Tick) -> Tick {
        Tick(self.0 + o.0)
    }
}
impl Sub for Tick {
    type Output = Tick;
    fn sub(self, o: Tick) -> Tick {
        Tick(self.0 - o.0)
    }
}
impl Neg for Tick {
    type Output = Tick;
    fn neg(self) -> Tick {
        Tick(-self.0)
    }
}
impl AddAssign for Tick {
    fn add_assign(&mut self, o: Tick) {
        self.0 += o.0;
    }
}
impl SubAssign for Tick {
    fn sub_assign(&mut self, o: Tick) {
        self.0 -= o.0;
    }
}

/// A half-open time range `[start, start + duration)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: Tick,
    pub duration: Tick,
}

impl TimeRange {
    pub fn new(start: Tick, duration: Tick) -> Self {
        Self { start, duration }
    }
    pub fn from_bounds(start: Tick, end: Tick) -> Self {
        Self { start, duration: end - start }
    }
    pub fn end(&self) -> Tick {
        self.start + self.duration
    }
    pub fn contains(&self, t: Tick) -> bool {
        t >= self.start && t < self.end()
    }
    pub fn overlaps(&self, o: &TimeRange) -> bool {
        self.start < o.end() && o.start < self.end()
    }
    pub fn intersect(&self, o: &TimeRange) -> Option<TimeRange> {
        let s = self.start.max(o.start);
        let e = self.end().min(o.end());
        (e > s).then(|| TimeRange::from_bounds(s, e))
    }
    pub fn is_empty(&self) -> bool {
        self.duration.0 <= 0
    }
}

/// A frame rate as an exact rational (`num / den` frames per second).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: i64,
    pub den: i64,
}

impl Default for FrameRate {
    fn default() -> Self {
        FrameRate::FPS_23_976
    }
}

impl FrameRate {
    pub const FPS_23_976: FrameRate = FrameRate { num: 24000, den: 1001 };
    pub const FPS_24: FrameRate = FrameRate { num: 24, den: 1 };
    pub const FPS_25: FrameRate = FrameRate { num: 25, den: 1 };
    pub const FPS_29_97: FrameRate = FrameRate { num: 30000, den: 1001 };
    pub const FPS_30: FrameRate = FrameRate { num: 30, den: 1 };
    pub const FPS_48: FrameRate = FrameRate { num: 48, den: 1 };
    pub const FPS_50: FrameRate = FrameRate { num: 50, den: 1 };
    pub const FPS_59_94: FrameRate = FrameRate { num: 60000, den: 1001 };
    pub const FPS_60: FrameRate = FrameRate { num: 60, den: 1 };
    pub const FPS_119_88: FrameRate = FrameRate { num: 120000, den: 1001 };
    pub const FPS_120: FrameRate = FrameRate { num: 120, den: 1 };

    /// Rates offered in sequence settings (Premiere's list).
    pub const COMMON: [FrameRate; 11] = [
        Self::FPS_23_976,
        Self::FPS_24,
        Self::FPS_25,
        Self::FPS_29_97,
        Self::FPS_30,
        Self::FPS_48,
        Self::FPS_50,
        Self::FPS_59_94,
        Self::FPS_60,
        Self::FPS_119_88,
        Self::FPS_120,
    ];

    pub fn new(num: i64, den: i64) -> Self {
        let g = gcd(num.abs(), den.abs()).max(1);
        FrameRate { num: num / g, den: den / g }
    }

    /// Closest standard rate for a float (e.g. from a container's average rate).
    pub fn from_f64(fps: f64) -> Self {
        for r in Self::COMMON {
            if (r.as_f64() - fps).abs() < 0.005 {
                return r;
            }
        }
        FrameRate::new((fps * 1000.0).round() as i64, 1000)
    }

    /// This rate if it is positive, else the default (a damaged file or project can carry a zero
    /// or negative rate; every frame computation uses this so it can't divide by zero).
    pub fn sane(self) -> FrameRate {
        if self.num > 0 && self.den > 0 { self } else { FrameRate::default() }
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Exact duration of one frame (rounded down only for exotic rates).
    pub fn frame_duration(self) -> Tick {
        let r = self.sane();
        Tick(((TICKS_PER_SECOND as i128 * r.den as i128) / r.num as i128) as i64)
    }

    /// Index of the frame containing `t` (floor).
    pub fn frame_at(self, t: Tick) -> i64 {
        let r = self.sane();
        (t.0 as i128 * r.num as i128).div_euclid(TICKS_PER_SECOND as i128 * r.den as i128) as i64
    }

    /// Start tick of frame `f`.
    pub fn tick_of(self, f: i64) -> Tick {
        let r = self.sane();
        let n = f as i128 * TICKS_PER_SECOND as i128 * r.den as i128;
        Tick(n.div_euclid(r.num as i128) as i64)
    }

    /// Snap `t` down to a frame boundary.
    pub fn snap(self, t: Tick) -> Tick {
        self.tick_of(self.frame_at(t))
    }

    /// Snap `t` to the nearest frame boundary.
    pub fn snap_nearest(self, t: Tick) -> Tick {
        let a = self.snap(t);
        let b = self.tick_of(self.frame_at(t) + 1);
        if (t - a) <= (b - t) { a } else { b }
    }

    /// Timecode base (frames counted per timecode second): 30 for 29.97, 24 for 23.976.
    pub fn timecode_base(self) -> i64 {
        let r = self.sane();
        ((r.num + r.den - 1) / r.den).max(1)
    }

    /// NTSC (x/1001) rates can use drop-frame timecode.
    pub fn is_ntsc(self) -> bool {
        self.den == 1001
    }

    /// Whether drop-frame counting applies (29.97 / 59.94 / 119.88).
    pub fn supports_drop_frame(self) -> bool {
        self.is_ntsc() && self.timecode_base() % 30 == 0
    }

    pub fn label(self) -> String {
        if self.den == 1 {
            format!("{}", self.num)
        } else {
            let v = self.as_f64();
            let s = format!("{v:.3}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            s.to_string()
        }
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} fps", self.label())
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// How time is displayed (Premiere: Timecode, Feet+Frames 16mm/35mm, Frames, Audio Samples).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeDisplay {
    #[default]
    Timecode,
    Frames,
    Feet16,
    Feet35,
    AudioSamples,
    Seconds,
}

impl TimeDisplay {
    pub const ALL: [TimeDisplay; 6] =
        [TimeDisplay::Timecode, TimeDisplay::Feet35, TimeDisplay::Feet16, TimeDisplay::Frames, TimeDisplay::AudioSamples, TimeDisplay::Seconds];
    pub fn label(self) -> &'static str {
        match self {
            TimeDisplay::Timecode => "Timecode",
            TimeDisplay::Frames => "Frames",
            TimeDisplay::Feet16 => "Feet + Frames 16mm",
            TimeDisplay::Feet35 => "Feet + Frames 35mm",
            TimeDisplay::AudioSamples => "Audio Samples",
            TimeDisplay::Seconds => "Seconds",
        }
    }
}

/// Drop-frame parameters: frames dropped per minute and nominal base.
fn df_params(rate: FrameRate) -> (i64, i64) {
    let base = rate.timecode_base();
    (base / 15, base) // 30 → 2, 60 → 4, 120 → 8
}

/// Convert a frame count to SMPTE fields `(negative, h, m, s, f)`.
pub fn frames_to_fields(frame: i64, rate: FrameRate, drop_frame: bool) -> (bool, i64, i64, i64, i64) {
    let neg = frame < 0;
    let mut n = frame.saturating_abs();
    let (drop, base) = df_params(rate);
    if drop_frame && rate.supports_drop_frame() {
        let per_min = base * 60 - drop;
        let per_10 = per_min * 10 + drop;
        let d = n / per_10;
        let m = n % per_10;
        n = n.saturating_add(drop * 9 * d);
        if m > drop {
            n = n.saturating_add(drop * ((m - drop) / per_min));
        }
    }
    let f = n % base;
    let s = (n / base) % 60;
    let mi = (n / (base * 60)) % 60;
    let h = n / (base * 3600);
    (neg, h, mi, s, f)
}

/// Convert SMPTE fields back to a frame count.
pub fn fields_to_frames(h: i64, m: i64, s: i64, f: i64, rate: FrameRate, drop_frame: bool) -> i64 {
    let (drop, base) = df_params(rate);
    let mut n = h.saturating_mul(3600).saturating_add(m.saturating_mul(60)).saturating_add(s).saturating_mul(base).saturating_add(f);
    if drop_frame && rate.supports_drop_frame() {
        let total_min = h.saturating_mul(60).saturating_add(m);
        n = n.saturating_sub(drop.saturating_mul(total_min - total_min / 10));
    }
    n
}

/// Format a frame count as SMPTE timecode (`HH:MM:SS:FF`, or `HH;MM;SS;FF` for drop-frame).
pub fn format_timecode_frames(frame: i64, rate: FrameRate, drop_frame: bool) -> String {
    let df = drop_frame && rate.supports_drop_frame();
    let (neg, h, m, s, f) = frames_to_fields(frame, rate, df);
    let sep = if df { ';' } else { ':' };
    let fw = if rate.timecode_base() >= 100 { 3 } else { 2 };
    format!("{}{h:02}{sep}{m:02}{sep}{s:02}{sep}{f:0fw$}", if neg { "-" } else { "" })
}

/// Format a tick according to a display mode.
pub fn format_time(t: Tick, rate: FrameRate, drop_frame: bool, display: TimeDisplay, sample_rate: i64) -> String {
    let frame = rate.frame_at(t);
    match display {
        TimeDisplay::Timecode => format_timecode_frames(frame, rate, drop_frame),
        TimeDisplay::Frames => format!("{frame}"),
        TimeDisplay::Feet35 | TimeDisplay::Feet16 => {
            let per_ft = if display == TimeDisplay::Feet35 { 16 } else { 40 };
            let neg = frame < 0;
            let a = frame.abs();
            format!("{}{}+{:02}", if neg { "-" } else { "" }, a / per_ft, a % per_ft)
        }
        TimeDisplay::AudioSamples => {
            let secs = t.0.div_euclid(TICKS_PER_SECOND);
            let rem = Tick(t.0.rem_euclid(TICKS_PER_SECOND)).to_units_floor(sample_rate);
            format!("{:02}:{:02}:{:02}:{rem:05}", secs / 3600, (secs / 60) % 60, secs % 60)
        }
        TimeDisplay::Seconds => format!("{:.3}", t.seconds()),
    }
}

/// Error from [`parse_timecode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ParseError {}

/// Parse user-typed timecode into a frame count, the way Premiere's timecode fields do:
/// - `01:02:03:04`, `01;02;03;04`, `1.2.3.4` (any of `:;.,` separators),
/// - bare digits are read right-aligned as `HHMMSSFF` (`1000` = 00:00:10:00),
/// - a leading `+`/`-` makes the value relative to `current` (`+15` = 15 frames later, `-1.00` = 1 s earlier).
pub fn parse_timecode(input: &str, rate: FrameRate, drop_frame: bool, current: i64) -> Result<i64, ParseError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ParseError("empty timecode".into()));
    }
    let (rel, body) = match s.as_bytes()[0] {
        b'+' => (Some(1), &s[1..]),
        b'-' => (Some(-1), &s[1..]),
        _ => (None, s),
    };
    let parts: Vec<&str> = body.split([':', ';', '.', ',']).collect();
    let nums: Vec<i64> = if parts.len() == 1 {
        let digits = parts[0];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseError(format!("not a timecode: `{input}`")));
        }
        // Right-aligned pairs: "12345" -> [1, 23, 45]; relative bare numbers are frames.
        if rel.is_some() {
            vec![digits.parse().map_err(|_| ParseError("number too large".into()))?]
        } else {
            let mut v = Vec::new();
            let b = digits.as_bytes();
            let mut end = b.len();
            while end > 0 {
                let start = end.saturating_sub(2);
                v.push(digits[start..end].parse::<i64>().unwrap_or(0));
                end = start;
            }
            v.reverse();
            v
        }
    } else {
        parts
            .iter()
            .map(|p| if p.is_empty() { Ok(0) } else { p.parse::<i64>().map_err(|_| ParseError(format!("bad field `{p}`"))) })
            .collect::<Result<_, _>>()?
    };
    if nums.len() > 4 {
        return Err(ParseError("too many timecode fields".into()));
    }
    let mut f4 = [0i64; 4];
    let off = 4 - nums.len();
    f4[off..].copy_from_slice(&nums);
    let [h, m, sec, fr] = f4;
    // Overflowing fields (e.g. 90 frames) are allowed, as in Premiere.
    let base = rate.timecode_base();
    let frames = if nums.len() == 1 && rel.is_some() {
        fr
    } else if drop_frame && rate.supports_drop_frame() && m < 60 && sec < 60 && fr < base {
        fields_to_frames(h, m, sec, fr, rate, true)
    } else {
        // Saturating: typed or agent-sent timecodes can have absurdly large fields.
        h.saturating_mul(3600).saturating_add(m.saturating_mul(60)).saturating_add(sec).saturating_mul(base).saturating_add(fr)
    };
    Ok(match rel {
        Some(sign) => current.saturating_add(frames.saturating_mul(sign)),
        None => frames,
    })
}

/// Common audio sample rates offered in sequence settings.
pub const SAMPLE_RATES: [i64; 6] = [32_000, 44_100, 48_000, 88_200, 96_000, 192_000];

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Damaged files and projects can carry zero / negative rates and timebases, and users or
    /// agents can type huge timecodes: these used to panic (division by zero, overflow,
    /// crossed `clamp` bounds).
    #[test]
    fn hostile_rates_and_timecodes_never_panic() {
        let zero = FrameRate { num: 0, den: 0 };
        for r in [
            zero,
            FrameRate { num: 0, den: 1 },
            FrameRate { num: 25, den: 0 },
            FrameRate { num: -30, den: 1 },
            FrameRate::from_f64(0.0),
            FrameRate::from_f64(f64::NAN),
        ] {
            assert_eq!(r.tick_of(10), FrameRate::default().tick_of(10));
            assert_eq!(r.frame_at(Tick(TICKS_PER_SECOND)), FrameRate::default().frame_at(Tick(TICKS_PER_SECOND)));
            assert!(r.frame_duration() > Tick::ZERO);
            assert!(r.timecode_base() >= 1);
            let _ = r.snap_nearest(Tick(12345));
            let _ = format_time(Tick(TICKS_PER_SECOND), r, true, TimeDisplay::Timecode, 48_000);
        }
        assert_eq!(Tick::from_rational(100, 1, 0), Tick::ZERO);
        assert_eq!(Tick::from_units(100, 0), Tick::ZERO);
        assert_eq!(Tick(5).to_rational_floor(0, 1), 0);
        assert_eq!(Tick(5).to_rational_round(0, 1), 0);
        for t in [i64::MIN, -1, 0, 1, i64::MAX] {
            for (n, d) in [(i64::MIN, 1), (-1, 1000), (1, i64::MAX), (i64::MAX, i64::MIN), (1, 0)] {
                let _ = Tick(t).to_rational_round(n, d);
            }
        }
        assert_eq!(Tick(5).mul_ratio(1, 0), Tick::ZERO);
        assert_eq!(Tick(5).clamp(Tick(10), Tick(0)), Tick(0));
        let _ = format_timecode_frames(i64::MIN, FrameRate::FPS_29_97, true);
        for tc in ["99999999999:99:99:99", "9223372036854775807", "+9223372036854775807", "99999999999;59;59;29"] {
            let _ = parse_timecode(tc, FrameRate::FPS_29_97, true, i64::MAX);
            let _ = parse_timecode(tc, FrameRate::FPS_25, false, i64::MIN);
        }
    }

    #[test]
    fn every_common_rate_is_exact() {
        for r in FrameRate::COMMON {
            let d = r.frame_duration();
            assert_eq!(d.0 as i128 * r.num as i128, TICKS_PER_SECOND as i128 * r.den as i128, "{r}");
        }
        for sr in SAMPLE_RATES.iter().chain(&[8000, 11025, 16000, 22050, 176_400]) {
            assert_eq!(TICKS_PER_SECOND % sr, 0, "{sr}");
        }
    }

    #[test]
    fn drop_frame_known_values() {
        let r = FrameRate::FPS_29_97;
        assert_eq!(format_timecode_frames(0, r, true), "00;00;00;00");
        assert_eq!(format_timecode_frames(1799, r, true), "00;00;59;29");
        assert_eq!(format_timecode_frames(1800, r, true), "00;01;00;02");
        assert_eq!(format_timecode_frames(17982, r, true), "00;10;00;00");
        assert_eq!(format_timecode_frames(107892, r, true), "01;00;00;00");
        assert_eq!(format_timecode_frames(1800, r, false), "00:01:00:00");
        let r60 = FrameRate::FPS_59_94;
        assert_eq!(format_timecode_frames(3600, r60, true), "00;01;00;04");
    }

    #[test]
    fn parse_forms() {
        let r = FrameRate::FPS_25;
        assert_eq!(parse_timecode("00:00:10:00", r, false, 0).unwrap(), 250);
        assert_eq!(parse_timecode("1000", r, false, 0).unwrap(), 250);
        assert_eq!(parse_timecode("1.00", r, false, 0).unwrap(), 25);
        assert_eq!(parse_timecode("+15", r, false, 100).unwrap(), 115);
        assert_eq!(parse_timecode("-1.00", r, false, 100).unwrap(), 75);
        assert_eq!(parse_timecode("00;01;00;02", FrameRate::FPS_29_97, true, 0).unwrap(), 1800);
        assert!(parse_timecode("abc", r, false, 0).is_err());
    }

    #[test]
    fn display_modes() {
        let r = FrameRate::FPS_24;
        let t = r.tick_of(40);
        assert_eq!(format_time(t, r, false, TimeDisplay::Feet35, 48000), "2+08");
        assert_eq!(format_time(t, r, false, TimeDisplay::Feet16, 48000), "1+00");
        assert_eq!(format_time(t, r, false, TimeDisplay::Frames, 48000), "40");
        assert_eq!(format_time(Tick::from_units(48_001, 48_000), r, false, TimeDisplay::AudioSamples, 48000), "00:00:01:00001");
    }

    #[test]
    fn rational_conversions() {
        // 90 kHz MPEG timebase
        let t = Tick::from_rational(90_000, 1, 90_000);
        assert_eq!(t.0, TICKS_PER_SECOND);
        assert_eq!(t.to_rational_floor(1, 90_000), 90_000);
        assert_eq!(t.to_rational_round(1, 90_000), 90_000);
        // 1 ms timebase (Matroska): to the nearest millisecond, halves up
        let ms = |t: Tick| t.to_rational_round(1, 1000);
        assert_eq!(ms(FrameRate::FPS_30.tick_of(1)), 33);
        assert_eq!(ms(FrameRate::FPS_30.tick_of(2)), 67);
        assert_eq!(FrameRate::FPS_30.tick_of(2).to_rational_floor(1, 1000), 66);
        assert_eq!(ms(FrameRate::FPS_29_97.tick_of(15)), 501, "500.5 ms");
        assert_eq!(ms(Tick::from_rational(-4, 1, 10_000)), 0, "-0.4 ms");
        assert_eq!(ms(Tick::from_rational(-5, 1, 10_000)), 0, "-0.5 ms");
        assert_eq!(ms(Tick::from_rational(-6, 1, 10_000)), -1, "-0.6 ms");
        // a negative timebase is nonsense, but rounds the same way
        assert_eq!(FrameRate::FPS_30.tick_of(2).to_rational_round(-1, -1000), 67);
        assert_eq!(FrameRate::FPS_29_97.tick_of(15).to_rational_round(-1, -1000), 501);
        assert_eq!(FrameRate::from_f64(29.97), FrameRate::FPS_29_97);
        assert_eq!(FrameRate::FPS_23_976.label(), "23.976");
    }

    proptest! {
        #[test]
        fn frame_tick_roundtrip(f in -1_000_000i64..10_000_000, ri in 0usize..11) {
            let r = FrameRate::COMMON[ri];
            prop_assert_eq!(r.frame_at(r.tick_of(f)), f);
            prop_assert_eq!(r.frame_at(r.tick_of(f) + r.frame_duration() - Tick(1)), f);
        }

        #[test]
        fn drop_frame_bijection(f in 0i64..(24 * 107_892)) {
            let r = FrameRate::FPS_29_97;
            let (_, h, m, s, fr) = frames_to_fields(f, r, true);
            // dropped labels never appear
            prop_assert!(!(s == 0 && fr < 2 && m % 10 != 0));
            prop_assert_eq!(fields_to_frames(h, m, s, fr, r, true), f);
            let txt = format_timecode_frames(f, r, true);
            prop_assert_eq!(parse_timecode(&txt, r, true, 0).unwrap(), f);
        }

        #[test]
        fn ndf_parse_roundtrip(f in 0i64..10_000_000, ri in 0usize..11) {
            let r = FrameRate::COMMON[ri];
            let txt = format_timecode_frames(f, r, false);
            prop_assert_eq!(parse_timecode(&txt, r, false, 0).unwrap(), f);
        }
    }
}
