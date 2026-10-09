//! Render Settings and Output Module features end to end (through a [`Sink`], so nothing touches
//! the disk): field rendering and 3:2 pulldown, crop / region of interest / resize, alpha-only
//! and premultiplied output, the solo and guide-layer overrides, the render log, storage
//! overflow and PCM audio formats.

use std::sync::{Arc, Mutex};

use effectcraft_color::Label;
use effectcraft_export::render_queue::*;
use effectcraft_export::{Job, JobOptions, Report, Sink, StorageQuota, export};
use effectcraft_keyframe::{Keyframe, Value};
use effectcraft_project::{Comp, ItemId, ItemKind, LayerSource, Project, Solid, build};
use effectcraft_render::NoFootage;
use effectcraft_time::{FrameRate, Tick};

const W: u32 = 64;
const H: u32 = 48;

/// A comp at `rate`, black, with a white bar 8 px wide sweeping left to right (64 px/s) and a
/// static red block at the bottom left; `extra` adds layers.
fn project(rate: FrameRate, extra: impl FnOnce(&mut Project, &mut Comp)) -> (Project, ItemId) {
    let mut p = Project::default();
    let mut comp = Comp::new(W, H, rate, Tick::from_seconds_f64(2.0));
    comp.background = [0.0, 0.0, 0.0];
    let bar = p.add_item("Bar", Label::Red, None, ItemKind::Solid(Solid { color: [1.0, 1.0, 1.0], width: 8, height: 32, pixel_aspect: 1.0 }));
    let mut l = build::layer(&mut p, &comp, "Bar", LayerSource::Solid { item: bar }, (8, 32), None);
    l.props.prop_mut("transform/position").unwrap().keys =
        vec![Keyframe::new(Tick::ZERO, Value::Vec3([0.0, 16.0, 0.0])), Keyframe::new(Tick::from_seconds_f64(1.0), Value::Vec3([64.0, 16.0, 0.0]))];
    comp.layers.push(l);
    let red = p.add_item("Block", Label::Red, None, ItemKind::Solid(Solid { color: [1.0, 0.0, 0.0], width: 16, height: 8, pixel_aspect: 1.0 }));
    let mut b = build::layer(&mut p, &comp, "Block", LayerSource::Solid { item: red }, (16, 8), None);
    b.props.prop_mut("transform/position").unwrap().value = Value::Vec3([8.0, 44.0, 0.0]);
    comp.layers.push(b);
    extra(&mut p, &mut comp);
    let cid = p.add_item("Comp", Label::Sandstone, None, ItemKind::Comp(comp.into()));
    (p, cid)
}

type Files = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

fn custom(start: f64, end: f64) -> RenderSettings {
    RenderSettings { time_span: TimeSpan::Custom { start: Tick::from_seconds_f64(start), end: Tick::from_seconds_f64(end) }, ..Default::default() }
}

fn run_with(p: &Project, cid: ItemId, s: &RenderSettings, om: &OutputModule, path: &str, options: JobOptions, cancel: bool) -> (Result<Report, String>, Files) {
    let files: Files = Default::default();
    let f = files.clone();
    let sink: Box<Sink> = Box::new(move |path: &str, data: Vec<u8>| f.lock().unwrap().push((path.to_string(), data)));
    let job = Job {
        project: p,
        footage: &NoFootage,
        expr: None,
        accel: None,
        comp: cid,
        settings: s,
        output: om,
        path,
        sink: Some(&*sink),
        options,
        nested_switches: true,
    };
    let r = export(&job, &mut |pr| !(cancel && pr.done > 0)).map_err(|e| e.to_string());
    (r, files)
}

fn run(p: &Project, cid: ItemId, s: &RenderSettings, om: &OutputModule, path: &str) -> Files {
    let (r, files) = run_with(p, cid, s, om, path, JobOptions::default(), false);
    r.expect("export");
    files
}

fn png(files: &Files, i: usize) -> image::RgbaImage {
    let f = files.lock().unwrap();
    let pngs: Vec<&(String, Vec<u8>)> = f.iter().filter(|(p, _)| p.ends_with(".png")).collect();
    let mut pngs = pngs;
    pngs.sort_by(|a, b| a.0.cmp(&b.0));
    image::load_from_memory(&pngs[i].1).expect("png").to_rgba8()
}

fn row(img: &image::RgbaImage, y: u32) -> Vec<u8> {
    (0..img.width()).flat_map(|x| img.get_pixel(x, y).0).collect()
}

#[test]
fn field_render_interleaves_two_times() {
    let (p, cid) = project(FrameRate::new(25, 1), |_, _| {});
    let om = OutputModule::for_format(OutputFormat::PngSequence);
    // Frame 10 at 25 fps with fields: 0.40 s and 0.42 s.
    for (fr, upper) in [(FieldRender::UpperFirst, true), (FieldRender::LowerFirst, false)] {
        let s = RenderSettings { field_render: fr, ..custom(0.4, 0.44) };
        let fields = png(&run(&p, cid, &s, &om, "/f/field_[#####].png"), 0);
        // Progressive references at 50 fps: frames 20 (0.40 s) and 21 (0.42 s).
        let r = RenderSettings { frame_rate: Some(FrameRate::new(50, 1)), ..custom(0.4, 0.44) };
        let refs = run(&p, cid, &r, &om, "/f/ref_[#####].png");
        let (a, b) = (png(&refs, 0), png(&refs, 1));
        assert_ne!(row(&a, 10), row(&b, 10), "the bar moved between the fields");
        for y in 0..H {
            let first = (y % 2 == 0) == upper;
            assert_eq!(row(&fields, y), row(if first { &a } else { &b }, y), "{fr:?}: line {y}");
        }
    }
}

#[test]
fn pulldown_follows_the_cadence() {
    let ntsc = FrameRate::new(30000, 1001);
    let (p, cid) = project(ntsc, |_, _| {});
    let om = OutputModule::for_format(OutputFormat::PngSequence);
    let five = 5.0 * 1001.0 / 30000.0;
    let s = RenderSettings { field_render: FieldRender::UpperFirst, pulldown: Pulldown::Wssww, ..custom(0.0, five) };
    let out = run(&p, cid, &s, &om, "/p/pd_[#####].png");
    assert_eq!(out.lock().unwrap().len(), 5);
    // Film frames at 23.976 fps.
    let r = RenderSettings { frame_rate: Some(FrameRate::new(24000, 1001)), ..custom(0.0, 0.2) };
    let film = run(&p, cid, &r, &om, "/p/film_[#####].png");
    let mut pattern = String::new();
    for k in 0..5 {
        let (a, b) = Pulldown::Wssww.fields(k as u64).unwrap();
        pattern.push(if a == b { 'W' } else { 'S' });
        let img = png(&out, k);
        let (fa, fb) = (png(&film, a as usize), png(&film, b as usize));
        for y in 0..H {
            assert_eq!(row(&img, y), row(if y % 2 == 0 { &fa } else { &fb }, y), "frame {k} line {y}");
        }
    }
    assert_eq!(pattern, "WSSWW");
}

#[test]
fn crop_roi_and_resize() {
    let (p, cid) = project(FrameRate::new(25, 1), |_, _| {});
    let s = custom(0.4, 0.44);
    let full = png(&run(&p, cid, &s, &OutputModule::for_format(OutputFormat::PngSequence), "/c/full_[#####].png"), 0);
    let mut om = OutputModule::for_format(OutputFormat::PngSequence);
    om.crop = Crop { enabled: true, top: 4, left: 10, bottom: 6, right: 2, ..Default::default() };
    let c = png(&run(&p, cid, &s, &om, "/c/crop_[#####].png"), 0);
    assert_eq!((c.width(), c.height()), (52, 38));
    for (x, y) in [(0, 0), (20, 11), (51, 37), (3, 30)] {
        assert_eq!(c.get_pixel(x, y), full.get_pixel(x + 10, y + 4), "({x}, {y})");
    }
    // Region of interest at half resolution.
    om.crop = Crop { enabled: true, use_roi: true, roi: Some([16.0, 8.0, 32.0, 32.0]), ..Default::default() };
    let half = RenderSettings { resolution: 0.5, ..s.clone() };
    let r = png(&run(&p, cid, &half, &om, "/c/roi_[#####].png"), 0);
    assert_eq!((r.width(), r.height()), (16, 16));
    // Resize (fixed size, low and high quality) after the crop.
    om.crop = Crop::default();
    for q in [ResizeQuality::Low, ResizeQuality::High] {
        om.resize = Resize { enabled: true, width: 32, height: 20, lock_aspect: false, quality: q };
        let z = png(&run(&p, cid, &s, &om, "/c/resize_[#####].png"), 0);
        assert_eq!((z.width(), z.height()), (32, 20));
        // The red block (bottom left) survives the resize.
        let px = z.get_pixel(4, 18).0;
        assert!(px[0] > 150 && px[1] < 80, "{q:?}: {px:?}");
    }
    om.resize.lock_aspect = true;
    let z = png(&run(&p, cid, &s, &om, "/c/aspect_[#####].png"), 0);
    assert_eq!((z.width(), z.height()), (32, 24));
}

#[test]
fn alpha_only_and_premultiplied() {
    let (p, cid) = project(FrameRate::new(25, 1), |p, comp| {
        // A half-transparent green layer over the right half.
        let g = p.add_item("Green", Label::Green, None, ItemKind::Solid(Solid { color: [0.0, 1.0, 0.0], width: 32, height: 48, pixel_aspect: 1.0 }));
        let mut l = build::layer(p, comp, "Green", LayerSource::Solid { item: g }, (32, 48), None);
        l.props.prop_mut("transform/position").unwrap().value = Value::Vec3([48.0, 24.0, 0.0]);
        l.props.prop_mut("transform/opacity").unwrap().value = Value::Scalar(50.0);
        comp.layers.insert(0, l);
    });
    let s = custom(0.0, 0.04);
    let mut om = OutputModule::for_format(OutputFormat::PngSequence);
    om.channels = Channels::Alpha;
    let a = png(&run(&p, cid, &s, &om, "/a/alpha_[#####].png"), 0);
    assert_eq!(a.get_pixel(60, 2).0[0], 128, "half-opaque green");
    assert_eq!(a.get_pixel(20, 2).0[0], 0, "transparent");
    assert_eq!(a.get_pixel(8, 44).0[0], 255, "opaque block");
    om.channels = Channels::Rgba;
    let straight = png(&run(&p, cid, &s, &om, "/a/s_[#####].png"), 0);
    om.alpha_mode = AlphaMode::Premultiplied;
    let premul = png(&run(&p, cid, &s, &om, "/a/p_[#####].png"), 0);
    assert_eq!(straight.get_pixel(60, 2).0, [0, 255, 0, 128]);
    assert_eq!(premul.get_pixel(60, 2).0, [0, 128, 0, 128]);
}

#[test]
fn solo_and_guide_layer_overrides() {
    let (p, cid) = project(FrameRate::new(25, 1), |p, comp| {
        let g = p.add_item("Guide", Label::Green, None, ItemKind::Solid(Solid { color: [0.0, 0.0, 1.0], width: 8, height: 8, pixel_aspect: 1.0 }));
        let mut l = build::layer(p, comp, "Guide", LayerSource::Solid { item: g }, (8, 8), None);
        l.props.prop_mut("transform/position").unwrap().value = Value::Vec3([56.0, 44.0, 0.0]);
        l.switches.guide = true;
        comp.layers.insert(0, l);
        // Solo the moving bar.
        comp.layers[1].switches.solo = true;
    });
    let om = OutputModule::for_format(OutputFormat::PngSequence);
    let s = custom(0.4, 0.44);
    let cur = png(&run(&p, cid, &s, &om, "/s/cur_[#####].png"), 0);
    assert_eq!(cur.get_pixel(8, 44).0[0], 0, "solo: the red block is hidden");
    assert_eq!(cur.get_pixel(56, 44).0[2], 0, "guide layers off by default");
    let off = RenderSettings { solo: CurrentOrOff::AllOff, guide_layers: CurrentOrOff::Current, ..s.clone() };
    let all = png(&run(&p, cid, &off, &om, "/s/off_[#####].png"), 0);
    assert_eq!(all.get_pixel(8, 44).0[0], 255, "Solo Switches: All Off");
    // (with solo off the guide renders too: Guide Layers: Current Settings)
    assert_eq!(all.get_pixel(56, 44).0[2], 255);
}

#[test]
fn render_logs() {
    let (p, cid) = project(FrameRate::new(25, 1), |_, _| {});
    let om = OutputModule::for_format(OutputFormat::PngSequence);
    let s = RenderSettings { field_render: FieldRender::UpperFirst, name: "DV-ish".into(), ..custom(0.0, 0.2) };
    let log_of = |files: &Files| {
        files.lock().unwrap().iter().find(|(p, _)| p.ends_with("_RenderLog.txt")).map(|(p, d)| (p.clone(), String::from_utf8(d.clone()).unwrap()))
    };
    // Errors Only + success: no log.
    let (r, files) = run_with(&p, cid, &s, &om, "/l/a_[#####].png", JobOptions::default(), false);
    assert!(r.unwrap().log.is_none());
    assert!(log_of(&files).is_none());
    // Plus Per Frame Info: settings and one line per frame.
    let opts = JobOptions { log: RenderLog::PlusPerFrameInfo, label: "#1 Comp".into(), ..Default::default() };
    let (r, files) = run_with(&p, cid, &s, &om, "/l/b_[#####].png", opts, false);
    let r = r.unwrap();
    let (path, text) = log_of(&files).expect("log");
    assert_eq!(r.log.as_deref(), Some(path.as_str()));
    assert_eq!(std::path::Path::new(&path), std::path::Path::new("/l/b_RenderLog.txt"));
    assert!(text.contains("Item: #1 Comp") && text.contains("Result: Done (5 frames"), "{text}");
    assert!(text.contains("Field Render: Upper Field First") && text.contains("Template: DV-ish") && text.contains("Format: PNG Sequence"), "{text}");
    assert_eq!(text.lines().filter(|l| l.contains("rendered in")).count(), 5, "{text}");
    assert!(text.contains("Frame 0 (0.000 s): rendered in") && text.contains("Frame 4 (0.160 s)"), "{text}");
    // Plus Settings: no per-frame lines.
    let opts = JobOptions { log: RenderLog::PlusSettings, ..Default::default() };
    let (_, files) = run_with(&p, cid, &s, &om, "/l/c.gif", opts, false);
    let (_, text) = log_of(&files).expect("log");
    assert!(text.contains("Render Settings:") && !text.contains("rendered in"), "{text}");
    // Errors Only + failure (the composition is gone): the error is logged.
    let gif = OutputModule::for_format(OutputFormat::Gif);
    let (r, files) = run_with(&p, ItemId(424_242), &s, &gif, "/l/d.gif", JobOptions::default(), false);
    assert!(r.is_err());
    let (path, text) = log_of(&files).expect("error log");
    assert_eq!(std::path::Path::new(&path), std::path::Path::new("/l/d_RenderLog.txt"));
    assert!(text.contains("Result: Failed") && text.contains("Error: composition not found") && !text.contains("Render Settings:"), "{text}");
    // A user stop is not an error: no log.
    let (r, files) = run_with(&p, cid, &s, &gif, "/l/e.gif", JobOptions::default(), true);
    assert!(r.is_err());
    assert!(log_of(&files).is_none());
}

/// The primary folder holds `limit` bytes; overflow folders are unlimited.
struct Quota {
    limit: u64,
    used: Mutex<u64>,
}

impl StorageQuota for Quota {
    fn has_room(&self, path: &str, bytes: u64) -> bool {
        if !std::path::Path::new(path).starts_with("/primary") {
            return true;
        }
        let mut u = self.used.lock().unwrap();
        if *u + bytes > self.limit {
            return false;
        }
        *u += bytes;
        true
    }
}

#[test]
fn storage_overflow_moves_files_to_the_overflow_folder() {
    let (p, cid) = project(FrameRate::new(25, 1), |_, _| {});
    let om = OutputModule::for_format(OutputFormat::PngSequence);
    let s = RenderSettings { skip_existing: false, ..custom(0.0, 0.4) };
    let q = Quota { limit: 1, used: Mutex::new(0) };
    let opts = JobOptions { storage: Some(&q), overflow: vec!["/overflow".into()], log: RenderLog::PlusSettings, ..Default::default() };
    let (r, files) = run_with(&p, cid, &s, &om, "/primary/seq_[#####].png", opts, false);
    let r = r.unwrap();
    assert_eq!(r.overflow.len(), 10, "{:?}", r.overflow);
    let f = files.lock().unwrap();
    assert_eq!(
        f.iter()
            .filter(|(p, _)| std::path::Path::new(p).parent() == Some(std::path::Path::new("/overflow"))
                && std::path::Path::new(p).file_name().is_some_and(|n| n.to_string_lossy().starts_with("seq_"))
                && p.ends_with(".png"))
            .count(),
        10
    );
    let log = f.iter().find(|(p, _)| p.ends_with("_RenderLog.txt")).map(|(_, d)| String::from_utf8_lossy(d).to_string()).unwrap();
    assert!(log.contains("Storage overflow: 10 file(s)"), "{log}");
    drop(f);
    // Use Storage Overflow off: everything stays in place.
    let off = RenderSettings { storage_overflow: false, ..s };
    let q = Quota { limit: 1, used: Mutex::new(0) };
    let opts = JobOptions { storage: Some(&q), overflow: vec!["/overflow".into()], ..Default::default() };
    let (r, files) = run_with(&p, cid, &off, &om, "/primary/seq_[#####].png", opts, false);
    assert!(r.unwrap().overflow.is_empty());
    assert!(files.lock().unwrap().iter().all(|(p, _)| std::path::Path::new(p).starts_with("/primary")));
}

#[test]
fn wav_and_aiff_channels_and_formats() {
    let (p, cid) = project(FrameRate::new(25, 1), |_, _| {});
    let s = custom(0.0, 0.5);
    let mut om = OutputModule::for_format(OutputFormat::Wav);
    om.audio_channels = 1;
    om.audio_format = AudioFormat::S24;
    om.audio_sample_rate = 44_100;
    let files = run(&p, cid, &s, &om, "/w/a.wav");
    let d = files.lock().unwrap()[0].1.clone();
    assert_eq!(&d[..4], b"RIFF");
    let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    assert_eq!((u16_at(20), u16_at(22), u32_at(24), u16_at(32), u16_at(34)), (1, 1, 44_100, 3, 24));
    assert_eq!(u32_at(40), 22_050 * 3, "half a second of mono 24-bit");
    om.audio_channels = 2;
    om.audio_format = AudioFormat::F32;
    let files = run(&p, cid, &s, &om, "/w/b.wav");
    let d = files.lock().unwrap()[0].1.clone();
    let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    assert_eq!((u16_at(20), u16_at(22), u16_at(32), u16_at(34)), (3, 2, 8, 32), "IEEE float stereo");
    // AIFF has no float: 24-bit.
    let mut aiff = OutputModule::for_format(OutputFormat::Aiff);
    aiff.audio_format = AudioFormat::F32;
    let files = run(&p, cid, &s, &aiff, "/w/c.aif");
    let d = files.lock().unwrap()[0].1.clone();
    assert_eq!(&d[8..12], b"AIFF");
    assert_eq!(u16::from_be_bytes([d[26], d[27]]), 24, "sample size");
}
