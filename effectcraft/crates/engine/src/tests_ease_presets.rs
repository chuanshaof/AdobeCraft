//! Ease presets (#254): a saved curve applied to every pair of neighbouring selected keyframes
//! (one undo step), save / rename / delete persisted through the config store, and lenient loading
//! of a corrupt presets file.

use std::sync::Arc;

use effectcraft_keyframe::{Ease, Interp, ease_progress};
use effectcraft_time::Tick;
use serde_json::{Value, json};

use crate::Session;
use crate::config::{ConfigStore, MemoryConfig};
use crate::ease_presets::EASE_PRESETS_FILE;
use crate::tests_timeline::{animate, prop, setup};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

fn user_names(s: &mut Session) -> Vec<String> {
    let list = s.execute("keys.easePreset.list", json!({})).unwrap();
    list.as_array().unwrap().iter().filter(|p| p["builtIn"] == false).map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_preset_eases_every_selected_pair_in_one_undo_step() {
    let (mut s, l) = setup();
    animate(&mut s, l, "transform/opacity", &[(0.0, json!(0)), (1.0, json!(100)), (3.0, json!(20))]);
    animate(&mut s, l, "transform/rotation", &[(0.0, json!(0)), (2.0, json!(90))]);
    s.execute("prop.select", json!({"layer": l, "path": "transform/opacity"})).unwrap();
    s.execute("prop.select", json!({"layer": l, "path": "transform/rotation", "add": true})).unwrap();
    let steps = s.history.undo.len();
    let r = s.execute("keys.easePreset.apply", json!({"preset": "Ease In-Out"})).unwrap();
    assert_eq!(r["pairs"], 3, "two opacity segments and one rotation segment");
    assert_eq!(s.history.undo.len(), steps + 1, "one undo step");
    let eased = Ease { speed: 0.0, influence: 0.5 };
    let op = prop(&s, l, "transform/opacity");
    assert_eq!((op.keys[0].out_interp, op.keys[0].out_ease.clone()), (Interp::Bezier, vec![eased]));
    assert_eq!((op.keys[1].in_ease.clone(), op.keys[1].out_ease.clone()), (vec![eased], vec![eased]));
    assert_eq!((op.keys[2].in_interp, op.keys[2].in_ease.clone()), (Interp::Bezier, vec![eased]));
    // The first key's in side and the last key's out side are not part of a pair.
    assert_eq!((op.keys[0].in_interp, op.keys[2].out_interp), (Interp::Linear, Interp::Linear));
    // A symmetric curve passes the middle value at mid-time, slow near the keys.
    assert!(close(op.value_at(Tick::from_seconds_f64(2.0)).as_f64(), 60.0, 1e-6));
    let quarter = op.value_at(Tick::from_seconds_f64(1.5)).as_f64();
    assert!(close(quarter, 100.0 - 80.0 * ease_progress(0.0, 0.5, 0.0, 0.5, 0.25), 1e-6), "{quarter}");
    assert!(prop(&s, l, "transform/rotation").keys[1].in_interp == Interp::Bezier);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.history.redo.last().map(|(label, _)| label.as_str()), Some("Apply Ease Preset"));
    let op = prop(&s, l, "transform/opacity");
    assert!(op.keys.iter().all(|k| k.in_interp == Interp::Linear && k.out_interp == Interp::Linear));
    assert!(close(op.value_at(Tick::from_seconds_f64(1.5)).as_f64(), 80.0, 1e-6));
}

#[test]
fn a_curve_scales_to_each_dimension_and_follows_motion_paths() {
    let (mut s, l) = setup();
    animate(&mut s, l, "transform/scale", &[(0.0, json!([100, 100])), (1.0, json!([200, 50]))]);
    animate(&mut s, l, "transform/position", &[(0.0, json!([0, 0])), (2.0, json!([300, 400]))]);
    s.execute("prop.select", json!({"layer": l, "path": "transform/scale"})).unwrap();
    let curve = json!({"outInfluence": 60, "outSpeed": 0, "inInfluence": 20, "inSpeed": 2});
    s.execute("keys.easePreset.apply", json!({"curve": curve})).unwrap();
    let sc = prop(&s, l, "transform/scale");
    // X rises 100/s and Y falls 50/s: the in speed (2× average) is per dimension.
    assert_eq!(sc.keys[1].in_ease[..2], [Ease { speed: 200.0, influence: 0.2 }, Ease { speed: -100.0, influence: 0.2 }]);
    let f = ease_progress(0.0, 0.6, 2.0, 0.2, 0.5);
    let v = sc.value_at(Tick::from_seconds_f64(0.5)).as_vec2();
    assert!(close(v[0], 100.0 + 100.0 * f, 1e-6) && close(v[1], 100.0 - 50.0 * f, 1e-6), "{v:?}");
    // Handles [x1, y1, x2, y2] on Position: the speed is along the 500 px path.
    s.execute("prop.select", json!({"layer": l, "path": "transform/position"})).unwrap();
    s.execute("keys.easePreset.apply", json!({"curve": [0.25, 0.1, 0.25, 1.0]})).unwrap();
    let pos = prop(&s, l, "transform/position");
    assert!(pos.keys[0].out_ease.iter().all(|e| close(e.speed, 0.4 * 250.0, 1e-6) && close(e.influence, 0.25, 1e-9)), "{:?}", pos.keys[0].out_ease);
    let f = ease_progress(0.4, 0.25, 0.0, 0.75, 0.5);
    let p = pos.value_at(Tick::from_seconds_f64(1.0)).as_vec2();
    assert!(close(p[0], 300.0 * f, 0.05) && close(p[1], 400.0 * f, 0.05), "{p:?} {f}");
    // The captured curve is the applied one.
    let c = s.execute("keys.easePreset.capture", json!({})).unwrap();
    assert!(close(c["inInfluence"].as_f64().unwrap(), 75.0, 1e-6) && close(c["outSpeed"].as_f64().unwrap(), 0.4, 1e-6), "{c}");
}

#[test]
fn apply_needs_neighbouring_keys_and_a_known_curve() {
    let (mut s, l) = setup();
    animate(&mut s, l, "transform/opacity", &[(0.0, json!(0)), (1.0, json!(100)), (2.0, json!(0))]);
    let key = |t: f64| json!({"layer": l, "path": "transform/opacity", "time": t});
    // Keys 1 and 3 aren't neighbours.
    s.execute("keys.select", json!({"keys": [key(0.0), key(2.0)]})).unwrap();
    let steps = s.history.undo.len();
    let e = s.execute("keys.easePreset.apply", json!({"preset": "Linear"})).unwrap_err().to_string();
    assert!(e.contains("neighbouring"), "{e}");
    assert_eq!(s.history.undo.len(), steps, "nothing changed");
    s.execute("keys.select", json!({"keys": [key(0.0), key(1.0)]})).unwrap();
    for bad in
        [json!({"preset": "Nope"}), json!({"curve": [0.1, 0.2]}), json!({"curve": [0.1, "a", 0.3, 0.4]}), json!({"curve": {"outInfluence": 50}}), json!({})]
    {
        assert!(s.execute("keys.easePreset.apply", bad.clone()).is_err(), "{bad}");
    }
    // Names are matched in any case.
    assert_eq!(s.execute("keys.easePreset.apply", json!({"preset": "expo in-out"})).unwrap()["pairs"], 1);
}

#[test]
fn user_presets_save_rename_delete_and_persist() {
    let store = Arc::new(MemoryConfig::default());
    let (mut s, l) = setup();
    s.config = Some(store.clone());
    animate(&mut s, l, "transform/opacity", &[(0.0, json!(0)), (1.0, json!(100))]);
    s.execute("prop.select", json!({"layer": l, "path": "transform/opacity"})).unwrap();
    s.execute("keys.easePreset.apply", json!({"preset": "Decelerate"})).unwrap();
    // Without a curve, save keeps the one between the selected keys.
    let saved = s.execute("keys.easePreset.save", json!({"name": "  Settle "})).unwrap();
    assert_eq!(saved["name"], "Settle");
    assert!(close(saved["curve"]["outSpeed"].as_f64().unwrap(), 1.5, 1e-9) && close(saved["curve"]["inInfluence"].as_f64().unwrap(), 50.0, 1e-9), "{saved}");
    s.execute("keys.easePreset.save", json!({"name": "Snap", "curve": [0.1, 0.5, 0.2, 1.0]})).unwrap();
    assert_eq!(user_names(&mut s), ["Settle", "Snap"]);
    // Built-in names and existing names are refused; built-in presets can't change.
    assert!(s.execute("keys.easePreset.save", json!({"name": "linear", "curve": [0.3, 0.3, 0.7, 0.7]})).is_err());
    assert!(s.execute("keys.easePreset.save", json!({"name": "   "})).is_err());
    assert!(s.execute("keys.easePreset.rename", json!({"name": "Settle", "newName": "snap"})).is_err());
    assert!(s.execute("keys.easePreset.rename", json!({"name": "Linear", "newName": "Straight"})).is_err());
    assert!(s.execute("keys.easePreset.delete", json!({"name": "Ease In-Out"})).is_err());
    s.execute("keys.easePreset.rename", json!({"name": "settle", "newName": "Soft Landing"})).unwrap();
    // Saving under an existing name replaces that preset.
    s.execute("keys.easePreset.save", json!({"name": "SNAP", "curve": [0.2, 0.5, 0.2, 1.0]})).unwrap();
    assert_eq!(user_names(&mut s), ["Soft Landing", "SNAP"]);
    s.execute("keys.easePreset.delete", json!({"name": "snap"})).unwrap();
    assert!(s.execute("keys.easePreset.delete", json!({"name": "snap"})).is_err());
    // A new session reads them back from the store.
    let mut s2 = Session { config: Some(store.clone()), ..Default::default() };
    s2.load_settings();
    assert_eq!(user_names(&mut s2), ["Soft Landing"]);
    let c = crate::ease_presets::find(&s2, "soft landing").unwrap();
    assert!(close(c.out_influence, 20.0, 1e-9) && close(c.out_speed, 1.5, 1e-9) && close(c.in_speed, 0.0, 1e-9), "{c:?}");
}

#[test]
fn a_corrupt_presets_file_loads_what_it_can() {
    let store = Arc::new(MemoryConfig::default());
    store.write(EASE_PRESETS_FILE, "{\"presets\": [ not json").unwrap();
    let mut s = Session { config: Some(store.clone()), ..Default::default() };
    s.load_settings();
    assert!(s.ease_presets.is_empty());
    assert_eq!(s.execute("keys.easePreset.list", json!({})).unwrap().as_array().unwrap().len(), 12, "the built-in presets");
    let c = json!({"outInfluence": 30, "outSpeed": 0, "inInfluence": 30, "inSpeed": 0});
    let doc = json!({"version": 1, "presets": [
        {"name": "Good", "curve": c},
        {"name": "Linear", "curve": c},
        {"name": "", "curve": c},
        {"name": "No curve"},
        {"name": "good", "curve": c},
        42,
        {"name": "Wild", "curve": {"outInfluence": 1e9, "outSpeed": -1e9, "inInfluence": -5, "inSpeed": 0}},
    ]});
    store.write(EASE_PRESETS_FILE, &doc.to_string()).unwrap();
    s.load_settings();
    assert_eq!(s.ease_presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Good", "Wild"]);
    let wild = s.ease_presets[1].curve;
    assert_eq!((wild.out_influence, wild.out_speed, wild.in_influence), (100.0, -100.0, 0.1));
    // Saving rewrites a clean file.
    s.execute("keys.easePreset.delete", json!({"name": "Wild"})).unwrap();
    let text: Value = serde_json::from_str(&store.read(EASE_PRESETS_FILE).unwrap()).unwrap();
    assert_eq!(text["presets"].as_array().unwrap().len(), 1);
}
