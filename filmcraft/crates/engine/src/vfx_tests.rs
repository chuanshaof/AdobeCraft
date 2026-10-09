//! M5.11: every video effect is listed with its folder and badges, applies by id or name through
//! `effects.apply` (undoable), takes parameters through `effects.setParam`, and renders.

use filmcraft_project::{EffectKind, effect_defs};
use serde_json::{Value, json};

use crate::Session;

fn demo() -> Session {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s
}

fn first_v1_clip(s: &Session) -> u64 {
    s.active_sequence().unwrap().video_tracks[0].items[0].id.0
}

fn video_effects() -> Vec<&'static filmcraft_project::EffectDef> {
    effect_defs().iter().filter(|d| d.kind == EffectKind::Video && !d.intrinsic && !filmcraft_project::graphic::is_layer_id(d.id)).collect()
}

#[test]
fn effects_list_reports_folders_and_badges() {
    let mut s = demo();
    let list = s.execute("effects.list", json!({})).unwrap();
    let arr = list.as_array().unwrap();
    let find = |id: &str| arr.iter().find(|e| e["id"] == id).cloned().unwrap_or_else(|| panic!("{id} not listed"));
    let wsz = find("warp_stabilizer");
    assert_eq!(wsz["folder"], "Distort");
    assert_eq!(wsz["category"], json!(["Video Effects", "Distort"]));
    assert_eq!(find("vr_rotate_sphere")["folder"], "Immersive Video");
    assert_eq!(find("crop")["category"], json!(["Legacy", "Video Effects"]));
    assert_eq!(find("luma_key")["badges"], json!({"accelerated": true, "float32": true, "yuv": true}));
    assert_eq!(find("lens_flare")["folder"], "Lights & Glows");
    let n =
        arr.iter().filter(|e| e["kind"] == "Video" && e["category"][0] == "Video Effects" && e["folder"] != "Obsolete" && e["folder"] != Value::Null).count();
    assert_eq!(n, 93, "Premiere 26's current Video Effects");
}

#[test]
fn every_video_effect_applies_by_id_and_name_and_renders() {
    let mut s = demo();
    let clip = first_v1_clip(&s);
    for d in video_effects() {
        s.execute("effects.apply", json!({"clips": [clip], "effect": d.id})).unwrap_or_else(|e| panic!("{}: {e}", d.id));
        let it = s.active_sequence().unwrap().find_item(filmcraft_project::ClipId(clip)).unwrap().1.clone();
        let inst = it.effects.iter().find(|e| e.effect == d.id).unwrap_or_else(|| panic!("{} not on the clip", d.id));
        // auto points were resolved against the frame
        for (k, p) in &inst.params {
            if let Some(v) = p.value.as_vec2() {
                assert!(v.x.is_finite() && v.y.is_finite(), "{}.{k} unresolved", d.id);
            }
        }
        if !matches!(d.id, "warp_stabilizer" | "auto_reframe" | "echo") {
            let img = s.render_program(0.125).unwrap();
            assert!(img.px.iter().all(|v| v.is_finite()), "{} rendered non-finite pixels", d.id);
        }
        s.execute("edit.undo", json!({})).unwrap();
        assert!(s.active_sequence().unwrap().find_item(filmcraft_project::ClipId(clip)).unwrap().1.effects.iter().all(|e| e.effect != d.id), "{} undo", d.id);
        // by display name (MCP / CLI agents use either)
        s.execute("effects.apply", json!({"clips": [clip], "effect": d.name})).unwrap_or_else(|e| panic!("{} by name: {e}", d.name));
        s.execute("edit.undo", json!({})).unwrap();
    }
}

#[test]
fn new_effect_params_are_settable_and_keyframable() {
    let mut s = demo();
    let clip = first_v1_clip(&s);
    s.execute("effects.apply", json!({"clips": [clip], "effect": "Turbulent Displace"})).unwrap();
    s.execute("effects.setParam", json!({"clip": clip, "effect": "turbulent_displace", "param": "amount", "value": 120.0})).unwrap();
    s.execute("effects.toggleAnimation", json!({"clip": clip, "effect": "turbulent_displace", "param": "evolution"})).unwrap();
    s.execute("effects.apply", json!({"clips": [clip], "effect": "simple_text"})).unwrap();
    s.execute("effects.setParam", json!({"clip": clip, "effect": "simple_text", "param": "text", "value": "Hello"})).unwrap();
    s.execute("effects.apply", json!({"clips": [clip], "effect": "corner_pin"})).unwrap();
    s.execute("effects.setParam", json!({"clip": clip, "effect": "corner_pin", "param": "upper_left", "value": [100.0, 50.0]})).unwrap();
    let it = s.active_sequence().unwrap().find_item(filmcraft_project::ClipId(clip)).unwrap().1.clone();
    let td = it.effect("turbulent_displace").unwrap();
    assert_eq!(td.param("amount").unwrap().value.as_f64(), Some(120.0));
    assert!(td.param("evolution").unwrap().is_animated() || !td.param("evolution").unwrap().keyframes.is_empty());
    assert!(matches!(&it.effect("simple_text").unwrap().param("text").unwrap().value, filmcraft_project::ParamValue::Text(t) if t == "Hello"));
    let img = s.render_program(0.125).unwrap();
    assert!(img.px.iter().all(|v| v.is_finite()));
}
