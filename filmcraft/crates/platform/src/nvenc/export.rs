//! NVENC as an Export encoder: [`factory`] is registered in front of the software H.264 encoder
//! (`filmcraft_export::register_encoder`) and takes an export only when Export ▸ Hardware encoding
//! is Auto and NVENC can do what the settings ask; otherwise it returns `None` and the software
//! encoder runs, exactly as before.
//!
//! A hardware encoder that fails in the middle of an export (a lost device) ends the export with an
//! error naming it; unlike a decoder it cannot be replayed into the software encoder, because the
//! two write different streams.

use filmcraft_export::{
    BitrateMode, EncodedPacket, EncoderFrame, ExportError, ExportSettings, FieldOrder, Format, H264Pass, H264Profile, HardwareEncoding, Result, VideoEncoder,
};
use filmcraft_isobmff::{AvcConfig, SampleEntry};
use filmcraft_time::FrameRate;

use super::{Config, NvencH264, Profile};

/// The NVENC export encoder.
struct NvencEncoder {
    enc: NvencH264,
    w: u32,
    h: u32,
    rate: FrameRate,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl NvencEncoder {
    fn packets(&self, ps: Vec<super::Packet>) -> Vec<EncodedPacket> {
        let den = self.rate.den;
        let duration = u32::try_from(den).unwrap_or(u32::MAX);
        ps.into_iter().map(|p| EncodedPacket { data: p.data, key: p.key, duration, composition_offset: composition_offset(p.pts, p.dts, den) }).collect()
    }
}

/// `(pts - dts) × den` as an MP4 composition offset. It is a few frames (the B-frame delay); a
/// hostile value saturates instead of wrapping.
fn composition_offset(pts: i64, dts: i64, den: i64) -> i32 {
    let offset = pts.saturating_sub(dts).saturating_mul(den);
    i32::try_from(offset).unwrap_or(if offset < 0 { i32::MIN } else { i32::MAX })
}

impl VideoEncoder for NvencEncoder {
    fn sample_entry(&self) -> SampleEntry {
        let (sps, pps) = self.enc.parameter_sets();
        // `config` declines sizes above u16::MAX
        let (w, h) = (u16::try_from(self.w).unwrap_or(u16::MAX), u16::try_from(self.h).unwrap_or(u16::MAX));
        SampleEntry::avc(AvcConfig::new(vec![sps.to_vec()], vec![pps.to_vec()], 4), w, h)
    }

    fn timescale(&self) -> u32 {
        u32::try_from(self.rate.num).unwrap_or(u32::MAX)
    }

    fn encode(&mut self, f: &EncoderFrame) -> Result<Vec<EncodedPacket>> {
        if f.width != self.w || f.height != self.h {
            return Err(ExportError::Encode(format!("NVENC: a {}x{} picture for a {}x{} encoder", f.width, f.height, self.w, self.h)));
        }
        filmcraft_export::rgba_to_yuv420_8(f.rgba, self.w as usize, self.h as usize, &mut self.y, &mut self.u, &mut self.v);
        let ps = self
            .enc
            .encode(&self.y, &self.u, &self.v, f.index)
            .map_err(|e| ExportError::Encode(format!("NVENC: {e} (turn Export ▸ Hardware encoding off to use the software encoder)")))?;
        filmcraft_export::note_hw_encode_frame();
        Ok(self.packets(ps))
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        let ps = self.enc.flush().map_err(|e| ExportError::Encode(format!("NVENC: {e}")))?;
        Ok(self.packets(ps))
    }

    fn media_start(&self) -> Option<i64> {
        // with B-frames the first DTS is `delay` frames before the first PTS
        (self.enc.delay() > 0).then_some(i64::from(self.enc.delay()).saturating_mul(self.rate.den))
    }
}

/// Why NVENC does not take this export (the software encoder does), or the encoder's configuration.
fn config(format: Format, w: u32, h: u32, rate: FrameRate, s: &ExportSettings) -> std::result::Result<Config, String> {
    if format != Format::H264 || s.format.is_mxf() {
        return Err("only MP4 / MOV H.264 exports".into());
    }
    if s.signal.is_hdr() {
        return Err("HDR (8-bit H.264 here carries it with software signalling)".into());
    }
    if !matches!(s.h264_pass, H264Pass::Single) {
        return Err("two-pass VBR".into());
    }
    if s.field_order != FieldOrder::Progressive {
        return Err("interlaced output".into());
    }
    if w > u32::from(u16::MAX) || h > u32::from(u16::MAX) {
        return Err(format!("{w}x{h} does not fit an MP4 sample entry"));
    }
    if rate.num <= 0 || rate.den <= 0 || rate.num > i64::from(u32::MAX) || rate.den > i64::from(u32::MAX) {
        return Err("frame rate".into());
    }
    let (Ok(num), Ok(den)) = (u32::try_from(rate.num), u32::try_from(rate.den)) else {
        return Err("frame rate".into());
    };
    let fps = (num, den);
    let kbps = s.bitrate_kbps.max(100);
    Ok(Config {
        width: w,
        height: h,
        fps,
        bitrate_kbps: kbps,
        max_bitrate_kbps: s.max_bitrate_kbps.filter(|m| *m >= kbps).unwrap_or(kbps / 2 * 3),
        cbr: s.bitrate_mode == BitrateMode::Cbr,
        keyint: s.keyframe_distance.filter(|k| *k > 0).unwrap_or_else(|| (f64::from(fps.0) / f64::from(fps.1) * 2.0).round().max(1.0) as u32),
        profile: match s.h264_profile {
            H264Profile::Baseline => Profile::Baseline,
            H264Profile::Main => Profile::Main,
            H264Profile::High => Profile::High,
        },
        level: s.h264_level,
        sar: s.pixel_aspect,
        bframes: true,
    })
}

/// The Export encoder factory (see the module documentation).
pub fn factory(format: Format, w: u32, h: u32, rate: FrameRate, s: &ExportSettings) -> Option<Result<Box<dyn VideoEncoder>>> {
    if s.hardware_encoding != HardwareEncoding::Auto || format != Format::H264 {
        return None;
    }
    let declined = |why: &str| {
        log::info!("hardware encoding declined: {why}");
        filmcraft_export::note_hw_encode_declined();
        None
    };
    let cfg = match config(format, w, h, rate, s) {
        Ok(c) => c,
        Err(why) => return declined(&why),
    };
    match NvencH264::new(&cfg) {
        Ok(enc) => {
            filmcraft_export::note_hw_encode_session();
            Some(Ok(Box::new(NvencEncoder { enc, w, h, rate, y: Vec::new(), u: Vec::new(), v: Vec::new() })))
        }
        Err(why) => declined(&why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_offsets_saturate() {
        assert_eq!(composition_offset(3, 1, 1001), 2002);
        assert_eq!(composition_offset(0, -2, 1001), 2002);
        assert_eq!(composition_offset(i64::MAX, i64::MIN, 1001), i32::MAX);
        assert_eq!(composition_offset(i64::MIN, i64::MAX, 1001), i32::MIN);
        assert_eq!(composition_offset(1 << 40, 0, 1), i32::MAX);
    }

    #[test]
    fn oversized_pictures_are_declined() {
        let s = ExportSettings { format: Format::H264, hardware_encoding: HardwareEncoding::Auto, ..Default::default() };
        assert!(config(Format::H264, 70_000, 64, FrameRate::FPS_24, &s).is_err());
        assert!(config(Format::H264, 64, 70_000, FrameRate::FPS_24, &s).is_err());
    }
}
