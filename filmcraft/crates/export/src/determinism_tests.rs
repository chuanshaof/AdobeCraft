//! An export depends only on the project and the settings: the same file on every machine, however
//! many cores render it and however the work is cut into steps.

use super::*;
use crate::tests::project;

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("fc-export-det-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name).to_string_lossy().to_string()
}

/// Export on a machine with `threads` cores: `export` renders one frame per core of the pool it
/// runs in.
fn export_with_cores(threads: usize, settings: &ExportSettings) -> Vec<u8> {
    let (p, seq, m) = project();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
    pool.install(|| export(&p, seq, settings, &m, &Progress::default())).unwrap();
    std::fs::read(&settings.path).unwrap()
}

/// Export a step at a time with `batch` frames per step (the web app uses 1).
fn export_stepped(batch: i64, settings: &ExportSettings) -> Vec<u8> {
    let (p, seq, m) = project();
    let prog = Progress::default();
    let mut ex = Exporter::new(p, seq, settings, &prog).unwrap();
    ex.set_batch(batch);
    while !matches!(ex.step(&m, &prog).unwrap(), Step::Done(_)) {}
    std::fs::read(&settings.path).unwrap()
}

#[test]
fn same_file_whatever_the_core_count() {
    // 24 frames: one full interleave group and a partial one
    for (format, ext) in [(Format::ProRes, "mov"), (Format::H264, "mp4"), (Format::Mjpeg, "mov")] {
        let settings = |name: &str| ExportSettings { format, path: tmp(&format!("{name}.{ext}")), ..Default::default() };
        let reference = export_with_cores(2, &settings(&format!("{}-2", format.id())));
        for threads in [3, 5, 16] {
            let other = export_with_cores(threads, &settings(&format!("{}-{threads}", format.id())));
            assert!(reference == other, "{}: {threads} cores give a different file than 2 cores", format.label());
        }
        for batch in [1, 7] {
            let other = export_stepped(batch, &settings(&format!("{}-b{batch}", format.id())));
            assert!(reference == other, "{}: {batch} frame(s) per step give a different file", format.label());
        }
    }
}

/// Slice NAL units (types 1 and 5) in a length-prefixed (4-byte) H.264 sample.
fn slices_in(sample: &[u8]) -> usize {
    let (mut n, mut i) = (0, 0);
    while i + 4 <= sample.len() {
        let len = u32::from_be_bytes([sample[i], sample[i + 1], sample[i + 2], sample[i + 3]]) as usize;
        if let Some(&h) = sample.get(i + 4)
            && matches!(h & 0x1f, 1 | 5)
        {
            n += 1;
        }
        i += 4 + len;
    }
    n
}

#[test]
fn h264_slices_follow_the_frame_size_not_the_cores() {
    // 4096 lines = 256 macroblock rows: one slice per four rows on every machine, also on machines
    // with fewer than 64 cores (the encoder's default caps the slices at the core count)
    let path = tmp("tall.mp4");
    let s = ExportSettings { format: Format::H264, path: path.clone(), frame_size: Some((64, 4096)), include_audio: false, ..Default::default() };
    let (p, seq, m) = project();
    export(&p, seq, &s, &m, &Progress::default()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let file = filmcraft_isobmff::open(bytes.as_slice()).unwrap();
    let vt = file.track_of_kind(filmcraft_isobmff::TrackKind::Video).unwrap();
    assert_eq!(file.tracks[vt].samples.len(), 24);
    for i in 0..file.tracks[vt].samples.len() {
        let sample = file.read_sample(bytes.as_slice(), vt, i).unwrap();
        assert_eq!(slices_in(&sample), 64, "picture {i}");
    }
}
