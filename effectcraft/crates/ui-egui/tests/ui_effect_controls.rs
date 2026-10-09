//! Headless Effect Controls checks (egui_kittest): the AE-style widgets register their
//! automation ids, the on-viewer effect point control appears for the selected effect, and the
//! crosshair / eyedropper picks set parameters from a viewer click.

use effectcraft_engine::Session;
use effectcraft_engine::keyframe::Value as KV;
use effectcraft_engine::project::{Layer, LayerId};
use effectcraft_ui_egui::EffectcraftApp;
use effectcraft_ui_egui::dock::PanelKind;
use effectcraft_ui_egui::state::FxPick;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

struct Ids {
    small: u64,
    point: u64,
    color: u64,
    angle: u64,
    layer_ctl: u64,
    curves: u64,
    levels: u64,
    point_fx: u64,
}

fn layer(s: &Session, id: u64) -> Layer {
    s.active_comp().unwrap().layer(LayerId(id)).unwrap().clone()
}

/// A 640×360 comp: a full-frame green solid under a 200×100 solid scaled 200% at the centre,
/// carrying controls, Curves and Levels (Individual Controls).
fn app() -> (EffectcraftApp, Ids) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 640, "height": 360, "frameRate": 30, "duration": 10})).unwrap();
    s.execute("layer.newSolid", json!({"name": "Back", "color": "#20c040"})).unwrap();
    let small = s.execute("layer.newSolid", json!({"name": "Small", "color": "#ff0000", "width": 200, "height": 100})).unwrap()["layer"].as_u64().unwrap();
    s.execute("prop.set", json!({"layer": small, "path": "transform/scale", "value": [200.0, 200.0]})).unwrap();
    let mut fx = |name: &str| s.execute("effect.apply", json!({"layers": [small], "effect": name})).unwrap()["effects"][0].as_u64().unwrap();
    let point_fx = fx("Point Control");
    fx("Color Control");
    fx("Angle Control");
    fx("Layer Control");
    let curves = fx("Curves");
    let levels = fx("Levels (Individual Controls)");
    let l = layer(&s, small);
    let fxg = l.effects().unwrap();
    let param = |effect: u64, id: &str| fxg.groups().find(|g| g.uid == effect).and_then(|g| g.get(id)).map(|p| p.uid).unwrap();
    let find = |m: &str, id: &str| fxg.groups().find(|g| g.match_id == m).and_then(|g| g.get(id)).map(|p| p.uid).unwrap();
    let ids = Ids {
        small,
        point: param(point_fx, "point"),
        color: find("ec.control.color", "color"),
        angle: find("ec.control.angle", "angle"),
        layer_ctl: find("ec.control.layer", "layer"),
        curves,
        levels,
        point_fx,
    };
    s.state.selected_layers = vec![LayerId(small)];
    s.state.selected_props.clear();
    let mut app = EffectcraftApp::new(s);
    app.show_panel(PanelKind::EffectControls);
    (app, ids)
}

fn settle(h: &mut Harness<'_, EffectcraftApp>) {
    for _ in 0..600 {
        h.step();
        if h.state().frames.inflight() == 0 && h.state().frames.last_ms.lock().map(|v| *v > 0.0).unwrap_or(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for _ in 0..4 {
        h.step();
    }
}

fn ids(h: &Harness<'_, EffectcraftApp>) -> Vec<String> {
    h.state().auto.previous.iter().chain(h.state().auto.elements.iter()).map(|e| e.id.clone()).collect()
}

fn click(h: &mut Harness<'_, EffectcraftApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.step();
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.step();
    h.step();
}

#[test]
fn effect_controls_widgets_register_automation_ids() {
    let (app, x) = app();
    let mut h = Harness::builder().with_size(egui::vec2(1700.0, 1100.0)).build_eframe(|_| app);
    settle(&mut h);
    let have = ids(&h);
    let want = [
        format!("effectControls.prop.{}.crosshair", x.point),
        format!("effectControls.prop.{}.eyedropper", x.color),
        format!("effectControls.prop.{}.twirl", x.angle),
        format!("effectControls.prop.{}.source", x.layer_ctl),
        format!("effectControls.effect.{}.curves.graph", x.curves),
        format!("effectControls.effect.{}.curves.channel", x.curves),
        format!("effectControls.effect.{}.curves.reset", x.curves),
        format!("effectControls.effect.{}.reset", x.point_fx),
    ];
    let missing: Vec<&String> = want.iter().filter(|w| !have.contains(w)).collect();
    assert!(missing.is_empty(), "missing {missing:?}");
    // Levels sits below: collapse Curves to bring it into view.
    h.state_mut().ui.fx_closed.insert(x.curves);
    h.step();
    h.step();
    let have = ids(&h);
    let want =
        ["histogram", "channel", "inBlack", "gamma", "inWhite", "outBlack", "outWhite"].map(|k| format!("effectControls.effect.{}.levels.{k}", x.levels));
    let missing: Vec<&String> = want.iter().filter(|w| !have.contains(w)).collect();
    assert!(missing.is_empty(), "missing {missing:?}");
    // The angle reads AE-style.
    let angle =
        h.state().auto.previous.iter().chain(h.state().auto.elements.iter()).find(|e| e.id == format!("effectControls.prop.{}.value", x.angle)).cloned();
    assert_eq!(angle.map(|e| e.label), Some("0x+0.0°".to_string()));
    // Twirling the angle open shows its dial.
    h.state_mut().ui.fx_slider_open.insert(x.angle);
    h.step();
    h.step();
    assert!(ids(&h).contains(&format!("effectControls.prop.{}.dial", x.angle)));
    // No effect selected: no on-viewer point control. Selecting the effect shows it.
    let pt_id = format!("viewer.effectPoint.{}", x.point);
    assert!(!ids(&h).contains(&pt_id));
    h.state_mut().session.state.selected_props = vec![(LayerId(x.small), x.point_fx)];
    h.step();
    h.step();
    assert!(ids(&h).contains(&pt_id), "effect point control shown for the selected effect");
}

#[test]
fn crosshair_and_eyedropper_pick_from_the_viewer() {
    let (app, x) = app();
    let mut h = Harness::builder().with_size(egui::vec2(1700.0, 1100.0)).build_eframe(|_| app);
    settle(&mut h);
    // Crosshair: a click at comp (330, 190) is layer (105, 55) on the 200%-scaled layer
    // (anchor (100, 50) at position (320, 180)).
    h.state_mut().ui.fx_pick = Some(FxPick { kind: "point".into(), layer: x.small, prop: x.point, name: "Point".into() });
    h.step();
    assert!(ids(&h).contains(&"viewer.fxPick".to_string()));
    let pos = effectcraft_ui_egui::panels::viewer::comp_to_screen(&h.ctx, [330.0, 190.0]).unwrap();
    click(&mut h, pos);
    assert!(h.state().ui.fx_pick.is_none(), "the pick is used up");
    let v = layer(&h.state().session, x.small).effects().unwrap().find(x.point).unwrap().value.clone();
    let KV::Vec2(p) = v else { panic!("{v:?}") };
    // One viewer point is up to 1/zoom comp pixels.
    let tol = 1.0 / effectcraft_ui_egui::panels::viewer::last_fit(&h.ctx) as f64;
    assert!((p[0] - 105.0).abs() <= tol && (p[1] - 55.0).abs() <= tol, "{p:?}");
    // Eyedropper over the green background.
    h.state_mut().ui.fx_pick = Some(FxPick { kind: "color".into(), layer: x.small, prop: x.color, name: "Color".into() });
    h.step();
    let pos = effectcraft_ui_egui::panels::viewer::comp_to_screen(&h.ctx, [24.0, 24.0]).unwrap();
    click(&mut h, pos);
    let v = layer(&h.state().session, x.small).effects().unwrap().find(x.color).unwrap().value.clone();
    let KV::Color(c) = v else { panic!("{v:?}") };
    let want = [0x20 as f64 / 255.0, 0xc0 as f64 / 255.0, 0x40 as f64 / 255.0];
    for k in 0..3 {
        assert!((c[k] - want[k]).abs() < 0.02, "{c:?} vs {want:?}");
    }
    assert_eq!(c[3], 1.0);
}

/// Key Light's Screen Colour eyedropper picks from the effect's input: the shown frame is
/// already keyed (the default screen colour takes most of a green), so sampling it would miss.
#[test]
fn keyer_eyedropper_picks_the_screen_from_the_effect_input() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Key", "width": 320, "height": 180, "frameRate": 30, "duration": 1})).unwrap();
    let screen = s.execute("layer.newSolid", json!({"name": "Screen", "color": "#30b050"})).unwrap()["layer"].as_u64().unwrap();
    let fx = s.execute("effect.apply", json!({"layers": [screen], "effect": "Key Light"})).unwrap()["effects"][0].as_u64().unwrap();
    let prop = layer(&s, screen).effects().unwrap().groups().find(|g| g.uid == fx).and_then(|g| g.get("screenColour")).map(|p| p.uid).unwrap();
    s.state.selected_layers = vec![LayerId(screen)];
    let mut app = EffectcraftApp::new(s);
    app.show_panel(PanelKind::EffectControls);
    let mut h = Harness::builder().with_size(egui::vec2(1700.0, 1100.0)).build_eframe(|_| app);
    settle(&mut h);
    h.state_mut().ui.fx_pick = Some(FxPick { kind: "color".into(), layer: screen, prop, name: "Screen Colour".into() });
    h.step();
    let pos = effectcraft_ui_egui::panels::viewer::comp_to_screen(&h.ctx, [160.0, 90.0]).unwrap();
    click(&mut h, pos);
    assert!(h.state().ui.fx_pick.is_none(), "the pick is used up");
    let v = layer(&h.state().session, screen).effects().unwrap().find(prop).unwrap().value.clone();
    let KV::Color(c) = v else { panic!("{v:?}") };
    let want = [0x30 as f64 / 255.0, 0xb0 as f64 / 255.0, 0x50 as f64 / 255.0];
    for k in 0..3 {
        assert!((c[k] - want[k]).abs() < 0.01, "{c:?} vs {want:?}");
    }
    // The picked screen keys the whole solid out.
    let s = &h.state().session;
    let img = s.render(s.active_comp_id().unwrap(), s.time(), Default::default());
    assert!(img.get(160, 90)[3] < 0.01, "{:?}", img.get(160, 90));
}

fn rect_of(h: &Harness<'_, EffectcraftApp>, id: &str) -> egui::Rect {
    let e = h.state().auto.previous.iter().chain(h.state().auto.elements.iter()).find(|e| e.id == id).unwrap_or_else(|| panic!("no {id}")).clone();
    egui::Rect::from_min_size(egui::pos2(e.rect[0], e.rect[1]), egui::vec2(e.rect[2], e.rect[3]))
}

/// Drag one pixel per frame, as a slow hand does.
fn slow_drag(h: &mut Harness<'_, EffectcraftApp>, from: egui::Pos2, dx: f32) {
    h.event(egui::Event::PointerMoved(from));
    h.step();
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for k in 1..=dx as i32 {
        h.event(egui::Event::PointerMoved(from + egui::vec2(k as f32, 0.0)));
        h.step();
    }
    let to = from + egui::vec2(dx, 0.0);
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
}

/// Click a hot number and type a value over it.
fn type_into(h: &mut Harness<'_, EffectcraftApp>, id: &str, text: &str) {
    let p = rect_of(h, id).center();
    click(h, p);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text(text.into()));
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

/// An angle's revolutions ("0x") scrub and take typing on their own, in Effect Controls and
/// the Timeline, keeping the degrees (#93).
#[test]
fn angle_revolutions_scrub_and_take_typing() {
    let (app, x) = app();
    let mut h = Harness::builder().with_size(egui::vec2(1700.0, 1100.0)).build_eframe(|_| app);
    settle(&mut h);
    let angle = |h: &Harness<'_, EffectcraftApp>| layer(&h.state().session, x.small).effects().unwrap().find(x.angle).unwrap().value.as_f64();
    h.state_mut().session.execute("prop.set", json!({"layer": x.small, "prop": x.angle, "value": 45.0})).unwrap();
    h.run_steps(3);
    let revs = format!("effectControls.prop.{}.revolutions", x.angle);
    // Ten pixels a turn, however slowly the pointer moves; the degrees stay.
    let from = rect_of(&h, &revs).center();
    slow_drag(&mut h, from, 40.0);
    let v = angle(&h);
    assert!((v - 45.0).rem_euclid(360.0).abs() < 1e-9 && (2.0..=4.0).contains(&((v - 45.0) / 360.0)), "{v}");
    type_into(&mut h, &revs, "3");
    assert_eq!(angle(&h), 3.0 * 360.0 + 45.0);
    let deg = format!("effectControls.prop.{}.value", x.angle);
    assert_eq!(h.state().auto.find(&deg).map(|e| e.label.clone()), Some("3x+45.0°".into()));
    // Negative angles keep their sign: -1x-30° → 2x-30°.
    h.state_mut().session.execute("prop.set", json!({"layer": x.small, "prop": x.angle, "value": -390.0})).unwrap();
    h.run_steps(3);
    type_into(&mut h, &revs, "2");
    assert_eq!(angle(&h), 2.0 * 360.0 - 30.0);

    // The Timeline's Rotation.
    let rot = layer(&h.state().session, x.small).props.prop("transform/rotation").unwrap().uid;
    h.state_mut().show_panel(PanelKind::Timeline);
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "timeline.reveal.rotation", json!({})).unwrap();
    h.run_steps(4);
    type_into(&mut h, &format!("timeline.prop.{rot}.revolutions"), "-2");
    let rotation = |h: &Harness<'_, EffectcraftApp>| layer(&h.state().session, x.small).props.prop("transform/rotation").unwrap().value.as_f64();
    assert_eq!(rotation(&h), -720.0);
    // The Properties panel's Rotation.
    h.state_mut().show_panel(PanelKind::Properties);
    h.run_steps(4);
    type_into(&mut h, &format!("properties.prop.{rot}.revolutions"), "1");
    assert_eq!(rotation(&h), 360.0);
}

fn right_click(h: &mut Harness<'_, EffectcraftApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.step();
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
    h.step();
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

fn hover(h: &mut Harness<'_, EffectcraftApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(3);
}

/// #227: right-clicking Effect Controls where no control has a menu of its own shows the
/// Effect menu (After Effects), which applies an effect to the selected layers; an effect's
/// header keeps its own menu.
#[test]
fn right_click_in_effect_controls_shows_the_effect_menu() {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "frameRate": 30, "duration": 4})).unwrap();
    let a = s.execute("layer.newSolid", json!({"name": "A", "color": "#406080"})).unwrap()["layer"].as_u64().unwrap();
    let b = s.execute("layer.newSolid", json!({"name": "B", "color": "#804060"})).unwrap()["layer"].as_u64().unwrap();
    let fill = s.execute("effect.apply", json!({"layers": [b], "effect": "Fill"})).unwrap()["effects"][0].as_u64().unwrap();
    s.state.selected_layers = vec![LayerId(b), LayerId(a)];
    let mut app = EffectcraftApp::new(s);
    app.show_panel(PanelKind::EffectControls);
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_eframe(|_| app);
    h.run_steps(3);
    let category = " Blur & Sharpen ⏵";
    // An effect's header: its own menu.
    let header = rect_of(&h, &format!("effectControls.effect.{fill}")).center();
    right_click(&mut h, header);
    assert!(h.query_by_label("Duplicate").is_some(), "the effect's own menu");
    assert!(h.query_by_label(category).is_none());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    // A control without a menu (the header's Reset): the Effect menu.
    let reset = rect_of(&h, &format!("effectControls.effect.{fill}.reset")).center();
    right_click(&mut h, reset);
    assert!(h.query_by_label(category).is_some());
    assert_eq!(layer(&h.state().session, b).effects().map(|f| f.groups().count()), Some(1), "Reset didn't run");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.query_by_label(category).is_none());
    // The empty area below it: the Effect menu.
    let panel = rect_of(&h, "panel.EffectControls");
    right_click(&mut h, egui::pos2(panel.center().x, panel.max.y - 30.0));
    let cat = h.query_by_label(category).expect("the Effect menu's categories").rect();
    assert!(h.query_by_label_contains("Remove All").is_some(), "the whole Effect menu");
    hover(&mut h, cat.center());
    let at = h.query_by_label(" Gaussian Blur").expect("Blur & Sharpen ▸ Gaussian Blur").rect().center();
    hover(&mut h, egui::pos2(at.x, cat.center().y));
    for k in 1..=10 {
        hover(&mut h, egui::pos2(at.x, cat.center().y + (at.y - cat.center().y) * k as f32 / 10.0));
    }
    click(&mut h, at);
    let names = |h: &Harness<'_, EffectcraftApp>, l: u64| -> Vec<String> {
        layer(&h.state().session, l).effects().map(|f| f.groups().map(|g| g.name.clone()).collect()).unwrap_or_default()
    };
    assert_eq!(names(&h, b), ["Fill", "Gaussian Blur"]);
    assert_eq!(names(&h, a), ["Gaussian Blur"], "every selected layer gets it");
    assert!(h.query_by_label(category).is_none(), "the menu closed");
}
