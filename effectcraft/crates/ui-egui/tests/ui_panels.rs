//! Headless checks for the M13.6 panels: each opens from the Window menu as a real panel and
//! registers its automation ids; the Footage panel's buttons edit into the comp; the Progress
//! panel lists and cancels a job; Lumetri Scopes switch scope through their dropdown.

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use effectcraft_ui_egui::dock::PanelKind;
use egui::{Event, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, EffectcraftApp> {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Panels", "width": 320, "height": 180, "duration": 4})).unwrap();
    s.execute("layer.newSolid", json!({"name": "Plate", "color": "#406080"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.run_steps(3);
    h
}

fn open(h: &mut Harness<'_, EffectcraftApp>, panel: &str) {
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.panel", json!({"panel": panel})).unwrap();
    h.run_steps(3);
}

fn click(h: &mut Harness<'_, EffectcraftApp>, id: &str) {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}")).clone();
    let p: Pos2 = pos2(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

#[test]
fn window_menu_opens_the_new_panels() {
    let mut h = harness();
    for (name, kind, auto) in [
        ("lumetriScopes", PanelKind::LumetriScopes, "scopes.kind"),
        ("footage", PanelKind::Footage, "footage.empty"),
        ("mediaBrowser", PanelKind::MediaBrowser, "mediaBrowser.path"),
        ("metadata", PanelKind::Metadata, "metadata.projectComment"),
        ("progress", PanelKind::Progress, ""),
        ("contentAwareFill", PanelKind::ContentAwareFill, "contentFill.method"),
    ] {
        open(&mut h, name);
        assert!(h.state().ui.dock.contains(kind) || h.state().ui.floating.iter().any(|f| f.panels.contains(&kind)), "{name} shown");
        if !auto.is_empty() {
            assert!(h.state().auto.find(auto).is_some(), "{name}: {auto}");
        }
    }
}

#[test]
fn scopes_switch_kind() {
    let mut h = harness();
    open(&mut h, "lumetriScopes");
    assert!(h.state().auto.find("scopes.plot").is_some());
    h.state_mut().ui.scopes.scope = "vectorscopeYuv".into();
    h.run_steps(2);
    assert_eq!(h.state().auto.find("scopes.plot").unwrap().label, "Vectorscope YUV");
    h.state_mut().ui.scopes.scope = "histogram".into();
    h.run_steps(2);
    assert_eq!(h.state().auto.find("scopes.kind").unwrap().label, "Histogram");
}

#[test]
fn footage_panel_buttons_edit_into_the_comp() {
    let mut h = harness();
    let item = h.state().session.project.items.values().find(|i| matches!(i.kind, effectcraft_engine::project::ItemKind::Solid(_))).unwrap().id.0;
    h.state_mut().session.execute("footage.open", json!({"item": item})).unwrap();
    h.run_steps(4);
    assert!(h.state().ui.dock.contains(PanelKind::Footage) || h.state().ui.floating.iter().any(|f| f.panels.contains(&PanelKind::Footage)));
    assert!(h.state().auto.find("footage.overlayEdit").is_some());
    let before = h.state().session.active_comp().unwrap().layers.len();
    click(&mut h, "footage.overlayEdit");
    assert_eq!(h.state().session.active_comp().unwrap().layers.len(), before + 1);
    click(&mut h, "footage.rippleInsertEdit");
    assert!(h.state().session.active_comp().unwrap().layers.len() >= before + 2);
}

#[test]
fn progress_panel_cancels_a_job() {
    let mut h = harness();
    h.state_mut()
        .session
        .spawn_task("test", "Long analysis", false, |ctl| {
            while ctl.progress(1, 2) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err("cancelled".into())
        })
        .unwrap();
    open(&mut h, "progress");
    let id = h.state().session.jobs()[0].id.clone();
    assert!(h.state().auto.find(&format!("progress.job.{id}")).is_some());
    click(&mut h, &format!("progress.cancel.{id}"));
    h.state_mut().session.wait_jobs();
    h.run_steps(2);
    assert!(h.state().session.jobs().is_empty());
    assert_eq!(h.state().session.job_log.last().unwrap().status, "cancelled");
}

fn click_at(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

fn hover(h: &mut Harness<'_, EffectcraftApp>, p: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.run_steps(3);
}

/// Issue #191: a workspace saved with Save as New Workspace is listed in Window ▸ Workspace
/// (below the built-ins, where the submenu used to be cut off) and choosing it there brings its
/// layout back.
#[test]
fn saved_workspace_is_listed_in_the_workspace_menu() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.workspace", json!({"name": "Minimal"})).unwrap();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.saveWorkspaceAs", json!({"name": "My Layout"})).unwrap();
    h.run_steps(3);
    let saved = h.state().ui.dock.clone();
    assert_eq!(h.state().saved_workspace_names(), ["My Layout"]);
    let cx = effectcraft_engine::menus::DynCtx { workspace: Some("My Layout"), saved_workspaces: &["My Layout".to_string()] };
    let (entries, _) = effectcraft_engine::menus::dynamic(&h.state().session, "savedWorkspaces", &cx);
    assert_eq!(entries.len(), 1);
    assert_eq!((entries[0].label.as_str(), entries[0].command.as_str(), &entries[0].params), ("My Layout", "window.workspace", &json!({"name": "My Layout"})));
    // Leave it, then pick it from the in-window menu bar.
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.workspace", json!({"name": "Default"})).unwrap();
    h.run_steps(3);
    assert_ne!(h.state().ui.dock, saved);
    click(&mut h, "menu.Window");
    let ws = h.query_by_label(" Workspace ⏵").expect("Window ▸ Workspace").rect();
    hover(&mut h, ws.center());
    let entry = h.state().auto.find("menu.savedWorkspaces.0").expect("the saved workspace is listed").clone();
    assert_eq!(entry.label, "My Layout");
    let at = h.query_by_label(" My Layout").expect("the saved workspace entry").rect().center();
    // Into the submenu, then down it (as a pointer moves).
    hover(&mut h, pos2(at.x, ws.center().y));
    for k in 1..=10 {
        hover(&mut h, pos2(at.x, ws.center().y + (at.y - ws.center().y) * k as f32 / 10.0));
    }
    click_at(&mut h, at);
    assert_eq!(h.state().ui.workspace, "My Layout");
    assert_eq!(h.state().ui.dock, saved, "its layout came back");
}

/// Learn opens the Home screen's tutorials; clicking another workspace tab closes them again,
/// however quickly the tabs are clicked (they stayed over every workspace, #272).
#[test]
fn leaving_the_learn_workspace_closes_its_tutorials() {
    let mut h = harness();
    for name in ["Learn", "Default", "Learn", "Review", "Learn", "Learn", "Small Screen", "Standard", "Learn", "Default"] {
        click(&mut h, &format!("header.workspace.{name}"));
        assert_eq!(h.state().ui.workspace, name);
        let learn = name == "Learn";
        assert_eq!(h.state().ui.start_screen, learn, "{name}: the Home screen shows only for Learn");
        assert_eq!(!h.state().auto.query("home.learn.").is_empty(), learn, "{name}: the tutorials show only for Learn");
    }
}

/// Headless look at the panels (wgpu offscreen; needs a GPU adapter). Run with
/// `PANELS_SNAPSHOT=/abs/dir cargo test -p effectcraft-ui-egui --test ui_panels -- --ignored`.
#[test]
#[ignore]
fn panels_snapshot() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let solid = s.project.items.values().find(|i| matches!(i.kind, effectcraft_engine::project::ItemKind::Solid(_))).map(|i| i.id.0);
    let mut app = Some(EffectcraftApp::new(s));
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_eframe(|_| app.take().expect("app"));
    h.run_steps(3);
    let ctx = h.ctx.clone();
    effectcraft_ui_egui::menus::invoke(h.state_mut(), &ctx, "window.workspace", json!({"name": "All Panels"})).unwrap();
    h.run_steps(3);
    let dir = std::env::var("PANELS_SNAPSHOT").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-out").into());
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(id) = solid {
        h.state_mut().session.execute("footage.open", json!({"item": id})).unwrap();
    }
    for (panel, scope) in [
        ("lumetriScopes", "waveformRgb"),
        ("lumetriScopes", "vectorscopeYuv"),
        ("lumetriScopes", "histogram"),
        ("lumetriScopes", "paradeRgb"),
        ("contentAwareFill", ""),
        ("metadata", ""),
        ("progress", ""),
        ("mediaBrowser", ""),
        ("footage", ""),
    ] {
        if !scope.is_empty() {
            h.state_mut().ui.scopes.scope = scope.into();
        }
        open(&mut h, panel);
        h.run_steps(3);
        let img = h.render().expect("render");
        img.save(format!("{dir}/{panel}{scope}.png")).unwrap();
    }
}
