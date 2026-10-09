//! Inline text fields (a name, the current time) open with their whole text selected so typing
//! replaces it, commit on Enter and cancel on Escape (#170).

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> (Harness<'static, EffectcraftApp>, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "duration": 4, "fps": 24})).unwrap();
    let comp = s.active_comp_id().unwrap().0;
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(4);
    (h, comp)
}

fn center(h: &Harness<'_, EffectcraftApp>, id: &str) -> Pos2 {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}"));
    pos2(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)
}

fn click(h: &mut Harness<'_, EffectcraftApp>, p: Pos2, times: usize) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.step();
    // All in one frame (the harness' frames are longer apart than a double-click).
    for _ in 0..times {
        h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
}

fn key(h: &mut Harness<'_, EffectcraftApp>, key: Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn type_text(h: &mut Harness<'_, EffectcraftApp>, text: &str) {
    assert!(h.ctx.egui_wants_keyboard_input(), "the field has the keyboard");
    h.input_mut().events.push(Event::Text(text.into()));
    h.step();
}

#[test]
fn project_rename_replaces_the_name() {
    let (mut h, comp) = harness();
    let name = |h: &Harness<'_, EffectcraftApp>| h.state().session.project.items.values().find(|i| i.id.0 == comp).unwrap().name.clone();
    let p = center(&h, &format!("project.item.{comp}.name"));
    click(&mut h, p, 1);
    key(&mut h, Key::Enter);
    type_text(&mut h, "Hero");
    key(&mut h, Key::Enter);
    assert_eq!(name(&h), "Hero");
    assert!(!h.ctx.egui_wants_keyboard_input(), "Enter releases the keyboard");
}

#[test]
fn timecode_field_replaces_commits_and_cancels() {
    let (mut h, _) = harness();
    let frame = |h: &Harness<'_, EffectcraftApp>| h.state().session.active_comp().unwrap().frame_rate.frame_at(h.state().session.time());
    let p = center(&h, "timeline.timecode");
    click(&mut h, p, 2);
    type_text(&mut h, "0:00:01:00");
    key(&mut h, Key::Enter);
    assert_eq!(frame(&h), 24, "Enter commits the typed time");
    assert!(!h.ctx.egui_wants_keyboard_input(), "Enter releases the keyboard");
    click(&mut h, p, 2);
    type_text(&mut h, "0:00:02:00");
    key(&mut h, Key::Escape);
    assert_eq!(frame(&h), 24, "Escape cancels");
    assert!(!h.ctx.egui_wants_keyboard_input(), "Escape releases the keyboard");
}
