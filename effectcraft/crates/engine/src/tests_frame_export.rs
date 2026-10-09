//! Composition ▸ Save Frame As ▸ Photoshop Layers… / ProEXR….

use serde_json::json;

use crate::Session;

fn out_dir(name: &str) -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A 32×32 comp: a red full-frame solid under a 16×16 blue Multiply solid at 50%.
fn frame() -> Session {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Frame", "width": 32, "height": 32, "frameRate": 24, "duration": 1})).unwrap();
    s.execute("layer.newSolid", json!({"name": "Red", "color": "#ff0000"})).unwrap();
    let blue = s.execute("layer.newSolid", json!({"name": "Blue.Box", "color": "#0000ff", "width": 16, "height": 16})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.setBlendMode", json!({"layers": [blue], "mode": "Multiply"})).unwrap();
    s.execute("prop.set", json!({"layer": blue, "path": "transform/opacity", "value": 50})).unwrap();
    s
}

#[test]
fn photoshop_layers_writes_one_psd_layer_per_comp_layer() {
    let mut s = frame();
    let dir = out_dir("frame-psd");
    let path = dir.join("frame.psd").to_string_lossy().to_string();
    let r = s.execute("comp.saveFrameAsPsd", json!({"path": path})).unwrap();
    assert_eq!(r["layers"], 2);
    let psd = effectcraft_psd::Psd::parse(std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((psd.width, psd.height), (32, 32));
    let names: Vec<&str> = psd.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Red", "Blue.Box"], "bottom first");
    let blue = &psd.layers[1];
    assert_eq!(blue.blend_key(), "mul ");
    assert_eq!(blue.opacity, 128);
    // Cropped to its pixels (16×16, centred) and rendered at full opacity.
    assert_eq!((blue.rect.left, blue.rect.top, blue.rect.width(), blue.rect.height()), (8, 8, 16, 16));
    assert_eq!(psd.layers[0].blend_key(), "norm");
    assert_eq!(psd.layers[0].opacity, 255);
    // The merged image is the comp frame: red × (50% blue multiply) in the middle.
    let m = psd.composite().unwrap();
    let mid = m.data[16 * 32 + 16];
    assert!(mid[0] > 0.4 && mid[0] < 0.6 && mid[2] < 0.05, "{mid:?}");
    let corner = m.data[0];
    assert!(corner[0] > 0.99 && corner[2] < 0.01, "{corner:?}");
    // The menu has it.
    assert!(crate::commands::find("comp.saveFrameAsPsd").is_some());
}

#[test]
fn proexr_writes_layer_prefixed_channels_and_the_composite() {
    use exr::prelude::*;
    let mut s = frame();
    let dir = out_dir("frame-exr");
    let path = dir.join("frame.exr").to_string_lossy().to_string();
    let r = s.execute("comp.saveFrameAsExr", json!({"path": path})).unwrap();
    let names: Vec<&str> = r["channels"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert_eq!(names.len(), 12);
    assert!(names.contains(&"R") && names.contains(&"A") && names.contains(&"Red.R") && names.contains(&"Blue_Box.B"), "{names:?}");
    let img = read().no_deep_data().largest_resolution_level().all_channels().all_layers().all_attributes().from_file(&path).unwrap();
    let layer = &img.layer_data[0];
    assert_eq!((layer.size.0, layer.size.1), (32, 32));
    let ch = |n: &str| layer.channel_data.list.iter().find(|c| c.name.to_string() == n).unwrap_or_else(|| panic!("no channel {n}"));
    let at = |n: &str, x: usize, y: usize| match &ch(n).sample_data {
        FlatSamples::F32(v) => v[y * 32 + x],
        o => panic!("{o:?}"),
    };
    // The blue layer alone: blue inside its box, transparent outside.
    assert!((at("Blue_Box.B", 16, 16) - 1.0).abs() < 1e-3);
    assert_eq!(at("Blue_Box.A", 0, 0), 0.0);
    assert!((at("Red.R", 0, 0) - 1.0).abs() < 1e-3);
    // Composite: opaque everywhere.
    assert!((at("A", 16, 16) - 1.0).abs() < 1e-3);
    assert!(at("R", 16, 16) < 0.5, "multiplied by 50% blue (linear)");
}
