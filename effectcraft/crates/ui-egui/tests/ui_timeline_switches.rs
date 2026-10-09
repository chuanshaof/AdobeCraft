//! Timeline layer switches with real pointer input (egui_kittest): pressing a switch and
//! dragging over other layers gives them all the state the first one got, in one undo step, as
//! in After Effects (#227).

use effectcraft_engine::Session;
use effectcraft_engine::project::LayerId;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Event, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// Comp "Main" with solids A (top) to D (bottom); returns their ids top to bottom.
fn harness() -> (Harness<'static, EffectcraftApp>, Vec<u64>) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 4})).unwrap();
    let mut ids: Vec<u64> = ["D", "C", "B", "A"]
        .iter()
        .map(|name| s.execute("layer.newSolid", json!({"name": name, "color": "#406080"})).unwrap()["layer"].as_u64().unwrap())
        .collect();
    ids.reverse();
    s.execute("edit.deselectAll", json!({})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    (h, ids)
}

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}")).clone();
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

/// Press on `from`, move to `to` in `steps`, release there.
fn drag(h: &mut Harness<'_, EffectcraftApp>, from: Pos2, to: Pos2, steps: u32) {
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    for k in 1..=steps {
        h.event(Event::PointerMoved(from + (to - from) * (k as f32 / steps as f32)));
        h.step();
    }
    h.event(Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Each layer's switch, top to bottom.
fn states(h: &Harness<'_, EffectcraftApp>, ids: &[u64], f: fn(&effectcraft_engine::project::Layer) -> bool) -> Vec<bool> {
    let comp = h.state().session.active_comp().unwrap();
    ids.iter().map(|id| f(comp.layer(LayerId(*id)).unwrap())).collect()
}

fn video(l: &effectcraft_engine::project::Layer) -> bool {
    l.switches.video
}

fn motion_blur(l: &effectcraft_engine::project::Layer) -> bool {
    l.switches.motion_blur
}

#[test]
fn dragging_over_layer_switches_sets_them_all() {
    let (mut h, ids) = harness();
    let eye = |h: &Harness<'_, EffectcraftApp>, i: usize| rect(h, &format!("timeline.layer.{}.video", ids[i])).center();
    let steps = h.state().session.history.undo.len();

    // From A's eye down to D's: all hidden, one undo step.
    let (a, d) = (eye(&h, 0), eye(&h, 3));
    drag(&mut h, a, d, 10);
    assert_eq!(states(&h, &ids, video), [false; 4]);
    assert_eq!(h.state().session.history.undo.len(), steps + 1, "one undo step");
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(states(&h, &ids, video), [true; 4]);

    // Every layer gets the first one's new state, whatever its own: B hidden, then from C (shown
    // → hidden) up to A, quickly (two moves): B stays hidden, A is hidden, D untouched.
    h.state_mut().session.execute("layer.setSwitch", json!({"layers": [ids[1]], "switch": "video", "value": false})).unwrap();
    h.run_steps(2);
    let (c, a) = (eye(&h, 2), eye(&h, 0));
    drag(&mut h, c, a, 2);
    assert_eq!(states(&h, &ids, video), [false, false, false, true]);
    // From B (hidden → shown) down to D: B, C and D shown.
    let (b, d) = (eye(&h, 1), eye(&h, 3));
    drag(&mut h, b, d + vec2(0.0, 40.0), 6);
    assert_eq!(states(&h, &ids, video), [false, true, true, true]);

    // A click still toggles only that layer.
    let c = eye(&h, 2);
    drag(&mut h, c, c, 1);
    assert_eq!(states(&h, &ids, video), [false, true, false, true]);

    // The Switches column works the same (Motion Blur, from D up to B).
    let mb = |h: &Harness<'_, EffectcraftApp>, i: usize| rect(h, &format!("timeline.layer.{}.switch.motionBlur", ids[i])).center();
    let (d, b) = (mb(&h, 3), mb(&h, 1));
    drag(&mut h, d, b, 8);
    assert_eq!(states(&h, &ids, motion_blur), [false, true, true, true]);
    assert_eq!(states(&h, &ids, video), [false, true, false, true], "other switches untouched");
}
