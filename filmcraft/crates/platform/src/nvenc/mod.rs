//! NVIDIA NVENC H.264 hardware encoding (Windows).
//!
//! The encoder runs on the GPU's NVENC engine through the driver's `nvEncodeAPI64.dll` (API 12.1,
//! [`ffi`]; no CUDA, no SDK to install): pictures go in as NV12 input buffers, the Annex B output
//! comes back as length-prefixed H.264 samples with the parameter sets split out for the `avcC`.
//! [`export`] plugs it into Export as an alternative to the software encoder.
//!
//! ```text
//! RGBA (the export pipeline) ──► BT.709 limited 4:2:0 (the software encoder's conversion)
//!    ──► NV12 input buffer ──NVENC──► Annex B ──► length-prefixed samples + avcC
//! ```
//!
//! `unsafe` is confined to `ffi` (data) and `session` (every driver call); this module is safe
//! code. Hardware encoding never replaces an export that works in software: the factory declines
//! what NVENC cannot do (no NVIDIA GPU or driver, sizes, HDR, two-pass, MXF...) and the software
//! encoder takes over.

pub mod export;
#[allow(unsafe_code)]
mod ffi;
#[allow(unsafe_code)]
mod session;

#[cfg(test)]
mod abi_tests;

use std::collections::VecDeque;

pub use self::session::{Caps, Params};
use self::session::{Locked, Session, Submitted};

/// H.264 profile (`profile_idc` 66 / 77 / 100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Baseline,
    Main,
    High,
}

/// What to encode.
#[derive(Clone, Debug)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    /// Frame rate as numerator / denominator.
    pub fps: (u32, u32),
    pub bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub cbr: bool,
    /// Frames between IDR pictures.
    pub keyint: u32,
    pub profile: Profile,
    /// Level × 10 (41 = 4.1); `None` lets the encoder pick.
    pub level: Option<u8>,
    /// Sample aspect ratio.
    pub sar: Option<(u32, u32)>,
    /// Allow one B-frame between references when the encoder and the profile support it.
    pub bframes: bool,
}

/// One encoded picture in decoding order.
pub struct Packet {
    /// Length-prefixed (4 bytes) NAL units, without parameter sets or access unit delimiters.
    pub data: Vec<u8>,
    pub key: bool,
    /// Presentation / decoding time in frames.
    pub pts: i64,
    pub dts: i64,
}

/// Pictures in flight: input / output buffer pairs.
const RING: usize = 8;

/// An NVENC H.264 encoder.
pub struct NvencH264 {
    session: Session,
    sps: Vec<u8>,
    pps: Vec<u8>,
    /// Frames the output is delayed by reordering (0 or 1).
    delay: u32,
    free: Vec<usize>,
    /// Slots submitted and not read yet, in submission order, and how many of them are ready.
    pending: VecDeque<usize>,
    ready: usize,
    emitted: i64,
    size: (u32, u32),
}

/// Whether this system has an NVIDIA GPU with a driver that has NVENC.
pub fn available() -> bool {
    Session::open().is_ok()
}

impl NvencH264 {
    /// Open an encoder, or say why NVENC does not take this configuration.
    pub fn new(cfg: &Config) -> Result<Self, String> {
        let (w, h) = (cfg.width, cfg.height);
        if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
            return Err(format!("{w}x{h}: NVENC needs even dimensions"));
        }
        if cfg.fps.0 == 0 || cfg.fps.1 == 0 {
            return Err("frame rate".into());
        }
        let mut session = Session::open()?;
        let caps = session.caps()?;
        if w < caps.min_size.0 || h < caps.min_size.1 || w > caps.max_size.0 || h > caps.max_size.1 {
            return Err(format!("{w}x{h} is outside NVENC's {}x{} - {}x{}", caps.min_size.0, caps.min_size.1, caps.max_size.0, caps.max_size.1));
        }
        let bframes = u32::from(cfg.bframes && cfg.profile != Profile::Baseline && caps.max_bframes >= 1);
        let params = Params {
            width: w,
            height: h,
            fps: cfg.fps,
            bitrate: cfg.bitrate_kbps.max(1),
            max_bitrate: cfg.max_bitrate_kbps,
            cbr: cfg.cbr,
            gop: cfg.keyint.max(1),
            profile: match cfg.profile {
                Profile::Baseline => 0,
                Profile::Main => 1,
                Profile::High => 2,
            },
            level: cfg.level,
            sar: cfg.sar,
            bframes,
        };
        session.initialize(&params, RING)?;
        let (sps, pps) = split_parameter_sets(&session.sequence_params()?)?;
        let slots = session.slots();
        Ok(Self { session, sps, pps, delay: bframes, free: (0..slots).rev().collect(), pending: VecDeque::new(), ready: 0, emitted: 0, size: (w, h) })
    }

    /// The sequence and picture parameter sets (NAL units without start codes).
    pub fn parameter_sets(&self) -> (&[u8], &[u8]) {
        (&self.sps, &self.pps)
    }

    /// Frames the decoding time runs behind the presentation time (B-frame reordering).
    pub fn delay(&self) -> u32 {
        self.delay
    }

    /// Encode picture `index` from planar 4:2:0 (`u`, `v` at half size, rows `w` and `w / 2` bytes).
    /// Returns the pictures that came out (usually the oldest; none while the ring fills).
    pub fn encode(&mut self, y: &[u8], u: &[u8], v: &[u8], index: u64) -> Result<Vec<Packet>, String> {
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        if y.len() < w * h || u.len() < w / 2 * (h / 2) || v.len() < w / 2 * (h / 2) {
            return Err("the picture is smaller than the encoder's size".into());
        }
        let mut out = Vec::new();
        let slot = match self.free.pop() {
            Some(s) => s,
            None => {
                // the ring is full: the oldest picture must come out first
                if self.ready == 0 {
                    return Err("the encoder holds more pictures than it can".into());
                }
                out.push(self.read_oldest()?);
                self.free.pop().ok_or("no free encoder buffer")?
            }
        };
        let st = self.session.submit(slot, index, |l| fill_nv12(l, y, u, v, w, h));
        let st = match st {
            Ok(s) => s,
            Err(e) => {
                self.free.push(slot);
                return Err(e);
            }
        };
        self.pending.push_back(slot);
        if st == Submitted::Ready {
            self.ready = self.pending.len();
        }
        Ok(out)
    }

    /// Finish the stream: every picture still inside comes out.
    pub fn flush(&mut self) -> Result<Vec<Packet>, String> {
        if !self.pending.is_empty() {
            self.session.end_of_stream()?;
            self.ready = self.pending.len();
        }
        let mut out = Vec::new();
        while self.ready > 0 {
            out.push(self.read_oldest()?);
        }
        Ok(out)
    }

    fn read_oldest(&mut self) -> Result<Packet, String> {
        let slot = self.pending.pop_front().ok_or("no picture in flight")?;
        self.ready = self.ready.saturating_sub(1);
        let r = self.session.read(slot);
        self.free.push(slot);
        let o = r?;
        let data = annex_b_to_length_prefixed(&o.data)?;
        let k = self.emitted;
        self.emitted = self.emitted.saturating_add(1);
        let pts = i64::try_from(o.pts).unwrap_or(i64::MAX);
        Ok(Packet { data, key: o.pic_type == ffi::NV_ENC_PIC_TYPE_IDR, pts, dts: k.saturating_sub(i64::from(self.delay)) })
    }
}

/// Copy planar 4:2:0 into an NV12 input buffer.
fn fill_nv12(l: Locked<'_>, y: &[u8], u: &[u8], v: &[u8], w: usize, h: usize) {
    let pitch = l.pitch;
    let (luma, chroma) = l.data.split_at_mut((pitch * h).min(l.data.len()));
    if w == 0 || pitch == 0 {
        return;
    }
    for (row, dst) in y.chunks_exact(w).zip(luma.chunks_exact_mut(pitch)).take(h) {
        // `submit` rejects a pitch narrower than a row; a short row is skipped, never overrun
        if let Some(d) = dst.get_mut(..w) {
            d.copy_from_slice(row);
        }
    }
    let cw = w / 2;
    if cw == 0 {
        return;
    }
    for ((ur, vr), dst) in u.chunks_exact(cw).zip(v.chunks_exact(cw)).zip(chroma.chunks_exact_mut(pitch)).take(h / 2) {
        let Some(dst) = dst.get_mut(..w) else { continue };
        for (([du, dv], a), b) in dst.as_chunks_mut::<2>().0.iter_mut().zip(ur).zip(vr) {
            *du = *a;
            *dv = *b;
        }
    }
}

/// The NAL units of an Annex B byte stream (start codes `00 00 01` / `00 00 00 01`).
pub fn annex_b_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0usize;
    while let Some(w) = data.get(i..i.saturating_add(3)) {
        if w == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(starts.len());
    for (k, &s) in starts.iter().enumerate() {
        let mut e = starts.get(k + 1).map_or(data.len(), |n| n.saturating_sub(3));
        // trailing zero bytes belong to the next start code (4-byte form) or are padding
        while e > s && data.get(e - 1) == Some(&0) {
            e -= 1;
        }
        if let Some(n) = data.get(s..e).filter(|n| !n.is_empty()) {
            out.push(n);
        }
    }
    out
}

/// Annex B to 4-byte length-prefixed NAL units, dropping parameter sets (they live in the `avcC`)
/// and access unit delimiters. Errors on a stream without NAL units.
pub fn annex_b_to_length_prefixed(data: &[u8]) -> Result<Vec<u8>, String> {
    let nals = annex_b_nals(data);
    if nals.is_empty() {
        return Err("the encoder produced no NAL units".into());
    }
    let mut out = Vec::with_capacity(data.len());
    for n in nals {
        if n.first().is_none_or(|b| matches!(b & 0x1f, 7..=9)) {
            continue;
        }
        out.extend_from_slice(&(n.len() as u32).to_be_bytes());
        out.extend_from_slice(n);
    }
    if out.is_empty() {
        return Err("the encoder produced no picture data".into());
    }
    Ok(out)
}

/// The SPS and PPS NAL units of an Annex B byte string.
pub fn split_parameter_sets(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let nals = annex_b_nals(data);
    let find = |t: u8| nals.iter().find(|n| n.first().is_some_and(|b| b & 0x1f == t)).map(|n| n.to_vec());
    match (find(7), find(8)) {
        (Some(s), Some(p)) => Ok((s, p)),
        _ => Err("the encoder returned no SPS / PPS".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_annex_b_with_both_start_code_forms() {
        let s = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 9, 9, 0];
        let n = annex_b_nals(&s);
        assert_eq!(n, vec![&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 9, 9][..]]);
        assert_eq!(annex_b_to_length_prefixed(&s).unwrap(), [0, 0, 0, 3, 0x65, 9, 9]);
        assert_eq!(split_parameter_sets(&s).unwrap(), (vec![0x67, 1, 2], vec![0x68, 3]));
    }

    #[test]
    fn hostile_streams_are_errors() {
        for s in [&[][..], &[0, 0, 1], &[1, 2, 3], &[0, 0, 1, 0x09, 0xf0], &[0, 0, 0, 1, 0x67, 1]] {
            assert!(annex_b_to_length_prefixed(s).is_err(), "{s:?}");
        }
        assert!(split_parameter_sets(&[0, 0, 1, 0x65, 1]).is_err());
    }

    #[test]
    fn nv12_is_interleaved_chroma_after_luma_rows() {
        let (w, h, pitch) = (4usize, 2usize, 8usize);
        let y: Vec<u8> = (0..8).collect();
        let (u, v) = (vec![100, 101], vec![200, 201]);
        let mut buf = vec![0u8; pitch * h * 3 / 2];
        fill_nv12(Locked { data: &mut buf, pitch }, &y, &u, &v, w, h);
        assert_eq!(&buf[..4], &[0, 1, 2, 3]);
        assert_eq!(&buf[pitch..pitch + 4], &[4, 5, 6, 7]);
        assert_eq!(&buf[pitch * h..pitch * h + 4], &[100, 200, 101, 201]);
    }

    #[test]
    fn nv12_never_writes_past_a_narrow_pitch() {
        // a driver pitch narrower than the row (rejected by `submit`) must not panic here either
        let (w, h, pitch) = (8usize, 4usize, 4usize);
        let y = vec![7u8; w * h];
        let (u, v) = (vec![1u8; w * h / 4], vec![2u8; w * h / 4]);
        let mut buf = vec![0u8; pitch * h * 3 / 2];
        fill_nv12(Locked { data: &mut buf, pitch }, &y, &u, &v, w, h);
        // and empty or zero-sized inputs
        fill_nv12(Locked { data: &mut [], pitch: 0 }, &[], &[], &[], 0, 0);
        fill_nv12(Locked { data: &mut buf, pitch }, &[1], &[], &[], 1, 1);
    }

    #[test]
    fn empty_nal_units_are_skipped() {
        assert!(annex_b_nals(&[0, 0, 1, 0, 0, 1]).is_empty());
        assert!(annex_b_to_length_prefixed(&[0, 0, 1, 0, 0, 0, 1]).is_err());
        assert!(split_parameter_sets(&[0, 0, 1]).is_err());
    }
}
