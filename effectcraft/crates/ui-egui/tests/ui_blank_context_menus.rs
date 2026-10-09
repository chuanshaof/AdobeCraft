//! Right-click actions in the blank Project and Timeline areas.
use effectcraft_engine::Session;
use effectcraft_ui_egui::{Dialog, EffectcraftApp};
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable as _};
use serde_json::json;

fn harness() -> Harness<'static, EffectcraftApp> {
    let mut session = Session::default();
    session.execute("comp.new", json!({"name":"Empty", "width":640,"height":360,"duration":4})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(session));
    h.run_steps(3);
    h
}
fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("missing {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}
fn snapshot(h: &mut Harness<'_, EffectcraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("BLANK_CONTEXT_MENU_SNAPSHOTS") {
        h.render().unwrap().save(format!("{dir}/{name}.png")).unwrap();
    }
}
fn right_click(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerMoved(p));
        h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(3);
}

#[test]
fn project_blank_menu_creates_a_folder_and_undo_restores_the_project() {
    let mut h = harness();
    let before = h.state().session.project.items.len();
    let p = rect(&h, "project.empty").center();
    right_click(&mut h, p);
    snapshot(&mut h, "project");
    h.get_by_label("New Folder").click();
    h.run_steps(3);
    assert_eq!(h.state().session.project.items.len(), before + 1);
    assert!(h.state().session.project.items.values().any(|i| i.is_folder()));
    assert!(h.query_by_label("New Folder").is_none());
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.project.items.len(), before);
}

#[test]
fn project_blank_menu_opens_composition_settings_and_offers_import() {
    let mut h = harness();
    let p = rect(&h, "project.empty").center();
    right_click(&mut h, p);
    assert!(h.query_by_label("Import File…").is_some());
    h.get_by_label("New Composition…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(Dialog::NewComp));
}

#[test]
fn timeline_blank_outline_and_time_graph_create_layers() {
    for graph in [false, true] {
        let mut h = harness();
        let area = rect(&h, "timeline.layerMarquee");
        let p = if graph { pos2(area.max.x + 150.0, area.center().y) } else { area.center() };
        right_click(&mut h, p);
        h.get_by_label("New ⏵").click();
        h.run_steps(3);
        snapshot(&mut h, if graph { "timeline-graph" } else { "timeline-outline" });
        h.get_by_label("Null Object").click();
        h.run_steps(3);
        assert_eq!(h.state().session.active_comp().unwrap().layers.len(), 1);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert!(h.state().session.active_comp().unwrap().layers.is_empty());
    }
}

#[test]
fn timeline_blank_menu_opens_solid_settings() {
    let mut h = harness();
    let p = rect(&h, "timeline.layerMarquee").center();
    right_click(&mut h, p);
    h.get_by_label("New ⏵").click();
    h.run_steps(3);
    h.get_by_label("Solid…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(Dialog::SolidSettings));
}

#[test]
fn project_item_and_timeline_layer_keep_their_own_context_menus() {
    let mut h = harness();
    let comp = h.state().session.active_comp_id().unwrap();
    let p = rect(&h, &format!("project.item.{}.name", comp.0)).center();
    right_click(&mut h, p);
    assert!(h.query_by_label("Open Composition").is_some());
    assert!(h.query_by_label("New Folder").is_none());
    egui::Popup::close_all(&h.ctx);
    h.state_mut().session.execute("layer.newNull", json!({})).unwrap();
    h.run_steps(3);
    let id = h.state().session.active_comp().unwrap().layers[0].id;
    let r = rect(&h, &format!("timeline.layer.{}.row", id.0));
    right_click(&mut h, pos2(r.min.x + 210.0, r.center().y));
    assert!(h.query_by_label("Duplicate").is_some());
    assert!(h.query_by_label("Composition Settings…").is_none());
}

#[test]
fn project_blank_import_invokes_the_file_picker() {
    let mut h = harness();
    let called = std::rc::Rc::new(std::cell::Cell::new(false));
    let picked = called.clone();
    h.state_mut().hooks.pick_files = Some(Box::new(move |_| {
        picked.set(true);
        Vec::new()
    }));
    let p = rect(&h, "project.empty").center();
    right_click(&mut h, p);
    h.get_by_label("Import File…").click();
    h.run_steps(3);
    assert!(called.get());
    assert!(h.query_by_label("Import File…").is_none());
}

#[test]
fn timeline_blank_menu_opens_composition_settings() {
    let mut h = harness();
    let p = rect(&h, "timeline.layerMarquee").center();
    right_click(&mut h, p);
    h.get_by_label("Composition Settings…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(Dialog::CompSettings));
}
