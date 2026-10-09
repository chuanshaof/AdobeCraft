//! Public Affinity documents (`cargo xtask corpus --affinity`, pinned and sha256-verified) open
//! natively, and their art renders like Affinity's own picture of them: every document embeds a
//! thumbnail Affinity rendered when saving it. Each file's mean difference from that thumbnail
//! (0–255 per channel, both flattened on white, at the thumbnail's size) must stay under its
//! ceiling, which sits just above the difference measured when the importer landed.
//!
//! Skips when `corpus/affinity` is absent.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

/// (file, ceiling). Measured differences when the importer landed are in the comments.
const FILES: &[(&str, f64)] = &[
    ("affinity3-mexican-guy.af", 3.0),          // 1.80: brush strokes as plain strokes
    ("affinity3-mexican-man.af", 3.5),          // 2.24
    ("affinity3-mexican-woman.af", 3.5),        // 2.55
    ("affinity3-playing-cards.af", 5.5),        // 4.32: layer effects
    ("designer-beer.afdesign", 4.0),            // 2.90
    ("designer-cactus.afdesign", 1.5),          // 0.33
    ("designer-car.afdesign", 1.5),             // 0.40
    ("designer-flowers.afdesign", 3.0),         // 2.01
    ("designer-lion-track.afdesign", 5.5),      // 4.34
    ("afdesignload-color.afdesign", 1.0),       // 0.00
    ("afdesignload-layer_mode.afdesign", 1.0),  // 0.00
    ("afdesignload-layer_test.afdesign", 1.0),  // 0.00
    ("afdesignload-margins.afdesign", 1.0),     // 0.00
    ("afdesignload-raster_test.afdesign", 2.0), // 0.72: pixel layer
    ("afdesignload-revision_test.afdesign", 1.0),
    ("afdesignload-shape_test.afdesign", 4.5), // 3.50: cloud and heart shapes as ellipses
    ("afdesignload-slice_test.afdesign", 1.0),
    ("afdesignload-test_path.afdesign", 1.5), // 0.26
    ("jac21-SimpleLogo.afdesign", 1.5),       // 0.19
    ("jac21-SimpleLogoBanner.afdesign", 5.5), // 4.42: placed images in a Display P3 document
    ("eviltwo-AssetStore.aftemplate", 1.0),   // 0.00: artboards
    ("leakcanary-vector_icon.afdesign", 3.5), // 2.33
];

fn corpus() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/affinity");
    dir.join("SOURCES.md").exists().then_some(dir)
}

/// Mean absolute difference of two RGBA8 images of one size, both composited on white.
fn difference(a: &[u8], b: &[u8]) -> f64 {
    let white = |p: &[u8]| -> [f64; 3] {
        let al = f64::from(p[3]) / 255.0;
        [0, 1, 2].map(|i| f64::from(p[i]) * al + 255.0 * (1.0 - al))
    };
    let (mut sum, mut n) = (0.0, 0usize);
    for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let (x, y) = (white(p), white(q));
        sum += (0..3).map(|i| (x[i] - y[i]).abs()).sum::<f64>();
        n += 3;
    }
    sum / n.max(1) as f64
}

#[test]
fn public_affinity_documents_render_like_their_affinity_thumbnails() {
    let Some(dir) = corpus() else {
        eprintln!("corpus/affinity is absent: run `cargo xtask corpus --affinity`");
        return;
    };
    let mut failures = Vec::new();
    for (name, ceiling) in FILES {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        let loaded = vectorcraft_engine::cmd::fileio::load(name, &bytes).unwrap();
        assert!(!loaded.preview_only, "{name} opened as its preview: {:?}", loaded.warnings);
        let d = &loaded.doc;
        let thumb = image::load_from_memory(vectorcraft_affinity::preview(&bytes).unwrap().png).unwrap().to_rgba8();
        // The thumbnail shows the first page, or every artboard: compare the largest artboard
        // with its part of the thumbnail.
        let union = d.artboards.iter().map(|a| a.rect).reduce(|a, b| a.union(b)).unwrap();
        let scale = f64::from(thumb.width()) / union.width();
        let board = d.artboards.iter().max_by(|a, b| a.rect.area().total_cmp(&b.rect.area())).unwrap().rect;
        let crop = |v: f64| (v * scale).round().max(0.0) as u32;
        let (x, y) = (crop(board.x0 - union.x0), crop(board.y0 - union.y0));
        let (w, h) = (crop(board.width()).min(thumb.width() - x), crop(board.height()).min(thumb.height() - y));
        let expected = image::imageops::crop_imm(&thumb, x, y, w, h).to_image();
        let rendered = vectorcraft_render::Renderer::new().render_region(d, board, scale, false);
        let got = image::RgbaImage::from_raw(rendered.width, rendered.height, rendered.to_straight()).unwrap();
        let got = image::imageops::resize(&got, w, h, image::imageops::FilterType::Triangle);
        let diff = difference(expected.as_raw(), got.as_raw());
        eprintln!("{name}: {diff:.2} (ceiling {ceiling})");
        if diff > *ceiling {
            failures.push(format!("{name}: {diff:.2} > {ceiling}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
