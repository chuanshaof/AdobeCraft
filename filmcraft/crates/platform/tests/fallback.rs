//! [`HybridDecoder`] fallback on every platform, with a stand-in "hardware" decoder (our software
//! decoder that starts failing after N samples, as a lost hardware session does): the output must
//! be exactly the software decoder's, whatever sample the failure hits. Also: in-band parameter
//! sets that differ from the sample entry's switch to software; identical ones do not.

mod common;

use common::*;
use filmcraft_codecs::hw::NalStreamInfo;
use filmcraft_codecs::{DecodedFrame, Result, VideoDecoder};
use filmcraft_platform::HybridDecoder;

/// Decodes like the software decoder until `fail_after` samples were fed, then fails every call
/// (its pending pictures stay retrievable through `flush`, as with a real session).
struct Failing {
    inner: Box<dyn VideoDecoder>,
    fail_after: usize,
    fed: usize,
}

impl VideoDecoder for Failing {
    fn decode(&mut self, sample: &[u8], pts: i64) -> Result<Vec<DecodedFrame>> {
        self.fed += 1;
        if self.fed > self.fail_after {
            return Err(filmcraft_codecs::CodecError::Decode("session invalidated (test)".into()));
        }
        self.inner.decode(sample, pts)
    }
    fn flush(&mut self) -> Vec<DecodedFrame> {
        self.inner.flush()
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn name(&self) -> &str {
        "stand-in hardware"
    }
}

fn hybrid(s: &Stream, fail_after: usize) -> HybridDecoder {
    let info = NalStreamInfo::from_entry(&s.entry).unwrap().unwrap();
    let inner = filmcraft_codecs::software_video_decoder(&s.entry).unwrap();
    HybridDecoder::new(Box::new(Failing { inner, fail_after, fed: 0 }), s.entry.clone(), info)
}

#[test]
fn failures_anywhere_continue_with_the_software_output() {
    let ff = filmcraft_testkit::require_ffmpeg!();
    for (name, _) in FIXTURES {
        let Some(path) = named(&ff, name) else { continue };
        let s = read_stream(&path);
        let reference = decode_all(filmcraft_codecs::software_video_decoder(&s.entry).unwrap().as_mut(), &s.samples);
        let syncs: Vec<usize> = (0..s.samples.len()).filter(|&i| s.sync[i]).collect();
        let mut points: Vec<usize> = vec![0, 1, 3, 7, s.samples.len() - 1, s.samples.len()];
        for &k in &syncs {
            points.extend([k, k + 1, k + 2, k + 5]);
        }
        for fail_after in points {
            let mut d = hybrid(&s, fail_after);
            let out = decode_all(&mut d, &s.samples);
            assert_eq!(d.is_hardware(), fail_after >= s.samples.len(), "{name}: fell back at {fail_after}");
            assert_same(&format!("{name} failing after {fail_after} samples"), &out, &reference);
        }
        // a failure after a seek (reset) into the second GOP
        if let Some(&k) = syncs.get(1) {
            let tail = &s.samples[k..];
            let expect = {
                let mut sw = filmcraft_codecs::software_video_decoder(&s.entry).unwrap();
                decode_all(sw.as_mut(), tail)
            };
            let mut d = hybrid(&s, k + 4);
            decode_all(&mut d, &s.samples[..k]);
            d.reset();
            assert_same(&format!("{name} failing after a seek"), &decode_all(&mut d, tail), &expect);
        }
    }
}

#[test]
fn changed_in_band_parameter_sets_switch_to_software() {
    let ff = filmcraft_testkit::require_ffmpeg!();
    let Some(path) = named(&ff, "h264_high.mp4") else { return };
    let s = read_stream(&path);
    let info = NalStreamInfo::from_entry(&s.entry).unwrap().unwrap();
    let sps = info.parameter_sets[0].clone();
    let with_sps = |sps: &[u8], sample: &[u8]| {
        let mut v = (sps.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(sps);
        v.extend_from_slice(sample);
        v
    };
    // the same SPS repeated in band at every sync sample: stays in "hardware"
    let same: Vec<_> = s.samples.iter().zip(&s.sync).map(|((x, p), &k)| (if k { with_sps(&sps, x) } else { x.clone() }, *p)).collect();
    let mut d = hybrid(&s, usize::MAX);
    let out = decode_all(&mut d, &same);
    assert!(d.is_hardware());
    assert_same("repeated SPS", &out, &decode_all(filmcraft_codecs::software_video_decoder(&s.entry).unwrap().as_mut(), &same));
    // a different SPS (same values, trailing zero byte) at the second sync sample: software
    let mut changed_sps = sps.clone();
    changed_sps.push(0);
    let k = (1..s.samples.len()).find(|&i| s.sync[i]).unwrap();
    let changed: Vec<_> = s.samples.iter().enumerate().map(|(i, (x, p))| (if i == k { with_sps(&changed_sps, x) } else { x.clone() }, *p)).collect();
    let mut d = hybrid(&s, usize::MAX);
    let out = decode_all(&mut d, &changed);
    assert!(!d.is_hardware(), "changed SPS: software");
    assert_same("changed SPS", &out, &decode_all(filmcraft_codecs::software_video_decoder(&s.entry).unwrap().as_mut(), &changed));
}
