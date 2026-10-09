//! Ease Presets panel (Window ▸ Ease Presets): easing curves kept by name and applied to pairs of
//! keyframes (our own design and presets). The value graph at the top edits a working curve by
//! dragging its two handles, and Apply eases the selected keyframes with it; From Keys reads the
//! curve between the first selected pair. Clicking a preset's thumbnail loads it and applies it.
//! Save Current keeps the working curve under the name in the field; Rename and Delete act on the
//! selected user preset. Every action is a `keys.easePreset.*` command.
//!
//! Automation ids: `easePresets.graph`, `easePresets.handle.out` / `.in`, `easePresets.apply`,
//! `easePresets.fromKeys`, `easePresets.name`, `easePresets.save`, `easePresets.rename`,
//! `easePresets.delete`, `easePresets.preset.<n>` (1-based, labelled with the preset's name).

use effectcraft_engine::ease_presets;
use effectcraft_engine::keyframe::EaseCurve;
use egui::epaint::CubicBezierShape;
use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::panel_kit as kit;
use crate::theme::Tokens;
use crate::{EffectcraftApp, widgets};

/// Panel state (serde in the UI state).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EasePanelState {
    /// The curve the graph shows and Apply uses.
    pub curve: EaseCurve,
    /// The preset clicked last (Rename and Delete act on it).
    pub selected: Option<String>,
    /// Name for Save Current and Rename.
    pub name: String,
}

impl Default for EasePanelState {
    fn default() -> Self {
        EasePanelState { curve: EaseCurve::LINEAR, selected: None, name: String::new() }
    }
}

/// Value range of the graph: room for handles above and below the keys.
const V_MIN: f64 = -0.35;
const V_MAX: f64 = 1.35;
const TILE: egui::Vec2 = vec2(84.0, 64.0);
const GAP: f32 = 6.0;
const ROW: f32 = 30.0;
/// Height of the controls under the graph (numbers, three button rows, status, "Presets").
const CONTROLS: f32 = 154.0;
/// Panels at least this wide put the presets beside the graph and controls.
const SIDE_BY_SIDE: f32 = 560.0;

/// Screen point of normalised time `x` and value `v` in `r`, values `lo..hi` bottom to top.
fn to_screen(r: Rect, lo: f64, hi: f64, x: f64, v: f64) -> Pos2 {
    pos2(r.min.x + r.width() * x as f32, r.max.y - r.height() * ((v - lo) / (hi - lo)) as f32)
}

/// The curve from (0, 0) to (1, 1) through its handles.
fn curve_shape(c: &EaseCurve, at: impl Fn(f64, f64) -> Pos2, stroke: Stroke) -> CubicBezierShape {
    let [x1, y1, x2, y2] = c.handles();
    CubicBezierShape::from_points_stroke([at(0.0, 0.0), at(x1, y1), at(x2, y2), at(1.0, 1.0)], false, Color32::TRANSPARENT, stroke)
}

pub fn show(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let p = ui.painter().with_clip_rect(rect);
    let mut st = app.ui.ease_presets.clone();
    let pairs = ease_presets::selected_pairs(&app.session).len();
    let x0 = rect.min.x + 10.0;
    // Wide panels: graph and controls on the left, presets on the right; else presets below,
    // with room for at least one row of them.
    let side = rect.width() >= SIDE_BY_SIDE;
    let w = if side { 300.0 } else { (rect.width() - 20.0).max(210.0) };
    let room = rect.height() - 16.0 - CONTROLS - if side { 0.0 } else { TILE.y + GAP };
    let g = Rect::from_min_size(pos2(x0, rect.min.y + 10.0), vec2(w, (w * 0.6).min(room).clamp(90.0, 220.0)));
    graph(app, ui, g, &mut st.curve);
    let c = st.curve;
    let mut y = g.max.y + 4.0;
    let numbers = format!("Out {:.1} % · {:.2}      In {:.1} % · {:.2}", c.out_influence, c.out_speed, c.in_influence, c.in_speed);
    p.text(pos2(x0, y + 8.0), Align2::LEFT_CENTER, numbers, Tokens::mono(11.0), t.text_faint);
    y += 22.0;
    let at = |x: f32, y: f32, width: f32| Rect::from_min_size(pos2(x, y), vec2(width, 24.0));
    if kit::button(app, ui, at(x0, y, 90.0), "easePresets.apply", "Apply", pairs > 0) {
        apply(app, json!({"curve": c}), pairs, "the curve");
    }
    if kit::button(app, ui, at(x0 + 96.0, y, 100.0), "easePresets.fromKeys", "From Keys", false)
        && let Some(v) = kit::exec(app, "keys.easePreset.capture", json!({}))
        && let Ok(curve) = serde_json::from_value(v)
    {
        st.curve = curve;
        st.selected = None;
    }
    y += ROW;
    let field = at(x0, y, (w - 116.0).max(80.0));
    widgets::text_field(ui, field, &mut st.name, "Preset name", &t);
    app.auto.add("easePresets.name", field, "Preset name");
    let name = st.name.trim().to_string();
    if kit::button(app, ui, at(field.max.x + 6.0, y, 110.0), "easePresets.save", "Save Current", false) {
        if name.is_empty() {
            app.ui.status = "Type a name for the preset first".into();
        } else if let Some(v) = kit::exec(app, "keys.easePreset.save", json!({"name": name, "curve": c})) {
            st.selected = v["name"].as_str().map(str::to_string);
            app.ui.status = format!("Saved the ease preset “{}”", st.selected.as_deref().unwrap_or(&name));
        }
    }
    y += ROW;
    // Rename and Delete act on the selected user preset.
    let user = st.selected.clone().filter(|n| app.session.ease_presets.iter().any(|q| &q.name == n));
    let rename = kit::button(app, ui, at(x0, y, 90.0), "easePresets.rename", "Rename", false);
    let delete = kit::button(app, ui, at(x0 + 96.0, y, 90.0), "easePresets.delete", "Delete", false);
    match (&user, rename, delete) {
        (None, true, _) | (None, _, true) => app.ui.status = "Select one of your own presets first".into(),
        (Some(old), true, _) if name.is_empty() => app.ui.status = format!("Type a new name for “{old}” first"),
        (Some(old), true, _) => {
            if let Some(v) = kit::exec(app, "keys.easePreset.rename", json!({"name": old, "newName": name})) {
                st.selected = v["name"].as_str().map(str::to_string);
            }
        }
        (Some(old), _, true) => {
            let deleted = kit::exec(app, "keys.easePreset.delete", json!({"name": old})).is_some();
            if deleted {
                st.selected = None;
            }
        }
        _ => {}
    }
    y += ROW;
    let msg = if pairs > 0 {
        format!("{pairs} keyframe pair{} selected", if pairs == 1 { "" } else { "s" })
    } else {
        "Select two or more neighbouring keyframes".into()
    };
    p.text(pos2(x0, y + 4.0), Align2::LEFT_CENTER, msg, Tokens::ui(11.0), t.text_faint);
    y += 16.0;
    let head = if side { pos2(x0 + w + 16.0, rect.min.y + 6.0) } else { pos2(x0, y) };
    kit::label(ui, head, "Presets", &t);
    let list = Rect::from_min_max(head + vec2(0.0, 22.0), pos2(rect.max.x - 10.0, rect.max.y - 6.0));
    if let Some((name, curve)) = grid(app, ui, list, st.selected.as_deref()) {
        st.curve = curve;
        if pairs > 0 {
            apply(app, json!({"preset": name}), pairs, &format!("“{name}”"));
        }
        st.selected = Some(name);
    }
    if st != app.ui.ease_presets {
        app.ui.ease_presets = st;
    }
}

fn apply(app: &mut EffectcraftApp, params: serde_json::Value, pairs: usize, what: &str) {
    if pairs == 0 {
        app.ui.status = "Select two or more neighbouring keyframes of a property".into();
    } else if let Some(r) = kit::exec(app, "keys.easePreset.apply", params) {
        app.ui.status = format!("Eased {} keyframe pair(s) with {what}", r["pairs"]);
    }
}

/// The working curve's value graph: keys at (0, 0) and (1, 1), the linear ease for reference, and
/// the two handles, dragged to edit the curve.
fn graph(app: &mut EffectcraftApp, ui: &mut egui::Ui, g: Rect, curve: &mut EaseCurve) {
    let t = app.tokens;
    let p = ui.painter().with_clip_rect(g);
    p.rect_filled(g, 3.0, t.field_bg);
    p.rect_stroke(g, 3.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    app.auto.add("easePresets.graph", g, "Ease curve");
    let plot = g.shrink2(vec2(16.0, 6.0));
    let at = |x: f64, v: f64| to_screen(plot, V_MIN, V_MAX, x, v);
    for v in [0.0, 1.0] {
        p.line_segment([at(0.0, v), at(1.0, v)], Stroke::new(1.0, t.separator));
    }
    p.add(egui::Shape::dashed_line(&[at(0.0, 0.0), at(1.0, 1.0)], Stroke::new(1.0, t.separator), 4.0, 4.0));
    p.add(curve_shape(curve, at, Stroke::new(2.0, t.accent)));
    let [x1, y1, x2, y2] = curve.handles();
    let mut next = None;
    for (out, key, h) in [(true, at(0.0, 0.0), at(x1, y1)), (false, at(1.0, 1.0), at(x2, y2))] {
        p.line_segment([key, h], Stroke::new(1.0, t.text_dim));
        p.rect_filled(Rect::from_center_size(key, vec2(7.0, 7.0)), 1.0, t.keyframe);
        let r = Rect::from_center_size(h, vec2(14.0, 14.0));
        let resp = ui.interact(r, egui::Id::new(("easePresets.handle", out)), Sense::drag());
        app.auto.add(if out { "easePresets.handle.out" } else { "easePresets.handle.in" }, r, if out { "Out handle" } else { "In handle" });
        let hot = resp.hovered() || resp.dragged();
        if hot {
            ui.ctx().set_cursor_icon(if resp.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab });
        }
        p.circle_filled(h, if hot { 5.5 } else { 4.5 }, if hot { t.keyframe_selected } else { t.text });
        if resp.dragged() {
            let to = h + resp.drag_delta();
            let x = f64::from((to.x - plot.min.x) / plot.width()).clamp(0.0, 1.0);
            let v = (V_MIN + f64::from((plot.max.y - to.y) / plot.height()) * (V_MAX - V_MIN)).clamp(V_MIN, V_MAX);
            next = EaseCurve::from_handles(if out { [x, v, x2, y2] } else { [x1, y1, x, v] });
        }
    }
    if let Some(c) = next {
        *curve = c;
    }
}

/// Preset thumbnails in a wrapping grid that scrolls with the wheel; returns the clicked one.
fn grid(app: &mut EffectcraftApp, ui: &mut egui::Ui, list: Rect, selected: Option<&str>) -> Option<(String, EaseCurve)> {
    let t = app.tokens;
    let p = ui.painter().with_clip_rect(list);
    let scroll = widgets::PanelScroll::begin(ui, egui::Id::new("ease-presets-scroll"), list);
    let cols = (((list.width() + GAP) / (TILE.x + GAP)).floor() as usize).max(1);
    let (mut clicked, mut count) = (None, 0);
    for (n, (name, curve, built_in)) in ease_presets::presets(&app.session).enumerate() {
        count = n + 1;
        let r = Rect::from_min_size(list.min + vec2((n % cols) as f32 * (TILE.x + GAP), (n / cols) as f32 * (TILE.y + GAP) - scroll.offset), TILE);
        if !r.intersects(list) {
            continue;
        }
        let resp = ui.interact(r.intersect(list), egui::Id::new(("easePresets.preset", n)), Sense::click());
        app.auto.add(&format!("easePresets.preset.{count}"), r, name);
        let on = selected == Some(name);
        p.rect_filled(
            r,
            4.0,
            if on {
                t.row_selected
            } else if resp.hovered() {
                t.hover
            } else {
                t.field_bg
            },
        );
        let thumb = Rect::from_min_size(r.min, vec2(TILE.x, TILE.y - 16.0)).shrink2(vec2(12.0, 7.0));
        let [_, y1, _, y2] = curve.handles();
        let (lo, hi) = (y1.min(y2).min(0.0), y1.max(y2).max(1.0));
        p.add(curve_shape(&curve, |x, v| to_screen(thumb, lo, hi, x, v), Stroke::new(1.5, if on { t.text } else { t.text_dim })));
        widgets::text_fit(
            &p,
            pos2(r.center().x, r.max.y - 9.0),
            Align2::CENTER_CENTER,
            name,
            Tokens::ui(11.0),
            TILE.x - 6.0,
            if built_in { t.text_dim } else { t.text },
        );
        if resp.on_hover_text(format!("{name}: click to apply it to the selected keyframes")).clicked() {
            clicked = Some((name.to_string(), curve));
        }
    }
    scroll.end(ui, &mut app.auto, "easePresets.scroll", count.div_ceil(cols) as f32 * (TILE.y + GAP), &t);
    clicked
}
