//! Ease presets (Window ▸ Ease Presets): easing curves kept by name and applied to pairs of
//! keyframes. A few original built-in curves ship with EffectCraft; user presets live in the config
//! store (`ease_presets.json`), read leniently: a corrupt file or entry is skipped, never fatal.
//!
//! Commands: `keys.easePreset.apply {preset | curve}` eases every pair of neighbouring selected
//! keyframes of each property (one undo step), `keys.easePreset.capture` reads the curve between
//! the first selected pair, `keys.easePreset.save {name, curve?}`, `keys.easePreset.list`,
//! `keys.easePreset.rename {name, newName}` and `keys.easePreset.delete {name}`.

use std::collections::BTreeMap;

use effectcraft_keyframe::{EaseCurve, key_at};
use effectcraft_project::{LayerId, Uid};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, bad, has_keys, str_p};
use crate::{EngineError, Result, Session, cmd, query};

/// User presets in the config store.
pub const EASE_PRESETS_FILE: &str = "ease_presets.json";
/// Longest preset name, in characters.
const MAX_NAME: usize = 64;
const NEED_PAIR: &str = "select two or more neighbouring keyframes of a property";

/// A user preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EasePreset {
    pub name: String,
    pub curve: EaseCurve,
}

const fn curve(out_influence: f64, out_speed: f64, in_influence: f64, in_speed: f64) -> EaseCurve {
    EaseCurve { out_influence, out_speed, in_influence, in_speed }
}

/// Built-in presets (our own values). "Accelerate" starts slowly and "Decelerate" settles slowly.
const BUILT_IN: [(&str, EaseCurve); 12] = [
    ("Linear", EaseCurve::LINEAR),
    ("Smooth", curve(40.0, 0.25, 40.0, 0.25)),
    ("Ease In-Out Soft", curve(25.0, 0.0, 25.0, 0.0)),
    ("Ease In-Out", curve(50.0, 0.0, 50.0, 0.0)),
    ("Ease In-Out Strong", curve(75.0, 0.0, 75.0, 0.0)),
    ("Accelerate", curve(50.0, 0.0, 20.0, 1.5)),
    ("Accelerate Strong", curve(80.0, 0.0, 15.0, 2.5)),
    ("Decelerate", curve(20.0, 1.5, 50.0, 0.0)),
    ("Decelerate Strong", curve(15.0, 2.5, 80.0, 0.0)),
    ("Expo In-Out", curve(88.0, 0.0, 88.0, 0.0)),
    ("Expo Accelerate", curve(75.0, 0.0, 12.0, 7.0)),
    ("Expo Decelerate", curve(12.0, 7.0, 75.0, 0.0)),
];

/// Every preset: (name, curve, built in), the built-in ones first.
pub fn presets(s: &Session) -> impl Iterator<Item = (&str, EaseCurve, bool)> {
    BUILT_IN.iter().map(|(n, c)| (*n, *c, true)).chain(s.ease_presets.iter().map(|p| (p.name.as_str(), p.curve, false)))
}

/// A preset's curve by name (any case).
pub fn find(s: &Session, name: &str) -> Option<EaseCurve> {
    presets(s).find(|(n, ..)| n.eq_ignore_ascii_case(name)).map(|(_, c, _)| c)
}

fn is_built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

/// Pairs of neighbouring selected keyframes of the active comp: (layer, property, index of the
/// earlier key), by property and time.
pub fn selected_pairs(s: &Session) -> Vec<(LayerId, Uid, usize)> {
    let Some(comp) = s.active_comp() else { return vec![] };
    let mut groups: BTreeMap<(LayerId, Uid), Vec<usize>> = BTreeMap::new();
    for k in &s.state.selected_keys {
        let Some(pr) = comp.layer(k.layer).and_then(|l| l.props.find(k.prop)).filter(|pr| pr.value.interpolates()) else { continue };
        if let Some(i) = key_at(&pr.keys, k.time) {
            groups.entry((k.layer, k.prop)).or_default().push(i);
        }
    }
    let mut out = vec![];
    for ((lid, uid), mut idx) in groups {
        idx.sort_unstable();
        idx.dedup();
        out.extend(idx.windows(2).filter_map(|w| match *w {
            [a, b] if b == a + 1 => Some((lid, uid, a)),
            _ => None,
        }));
    }
    out
}

/// Read user presets: entries that don't parse, have no usable name or repeat a name are
/// skipped (and logged); a file that isn't JSON gives none.
pub fn parse(text: &str) -> Vec<EasePreset> {
    let doc: Value = serde_json::from_str(text).unwrap_or_else(|e| {
        log::warn!("{EASE_PRESETS_FILE} is not valid JSON ({e}); no user ease presets loaded");
        Value::Null
    });
    let mut out: Vec<EasePreset> = vec![];
    for entry in doc.get("presets").and_then(Value::as_array).into_iter().flatten() {
        let preset = serde_json::from_value::<EasePreset>(entry.clone())
            .ok()
            .and_then(|p| Some(EasePreset { name: clean_name(&p.name)?, curve: p.curve.sanitized()? }))
            .filter(|p| !is_built_in(&p.name) && !out.iter().any(|q| q.name.eq_ignore_ascii_case(&p.name)));
        match preset {
            Some(p) => out.push(p),
            None => log::warn!("{EASE_PRESETS_FILE}: skipped the unusable entry {entry}"),
        }
    }
    out
}

/// The file as written.
#[derive(Serialize)]
struct PresetFile<'a> {
    version: u32,
    presets: &'a [EasePreset],
}

/// Write `list` to the config store, then make it the session's user presets.
fn store(s: &mut Session, list: Vec<EasePreset>) -> Result<()> {
    if let Some(cfg) = &s.config {
        let text = serde_json::to_string_pretty(&PresetFile { version: 1, presets: &list }).map_err(|e| EngineError::Other(format!("ease presets: {e}")))?;
        cfg.write(EASE_PRESETS_FILE, &text).map_err(|e| EngineError::Other(format!("cannot save ease presets: {e}")))?;
    }
    s.ease_presets = list;
    Ok(())
}

fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME).then(|| name.to_string())
}

fn name_p(p: &Value, key: &str, c: &str) -> Result<String> {
    let raw = str_p(p, key).ok_or_else(|| bad(c, format!("missing `{key}`")))?;
    clean_name(raw).ok_or_else(|| bad(c, format!("`{key}` must be 1–{MAX_NAME} characters")))
}

/// `curve`: `{outInfluence, outSpeed, inInfluence, inSpeed}` or handles `[x1, y1, x2, y2]`.
fn curve_v(v: &Value, c: &str) -> Result<EaseCurve> {
    let parsed = match v.as_array() {
        Some(a) => {
            let h: Option<Vec<f64>> = a.iter().map(Value::as_f64).collect();
            let h: [f64; 4] = h.and_then(|h| h.try_into().ok()).ok_or_else(|| bad(c, "`curve` handles are four numbers [x1, y1, x2, y2]"))?;
            EaseCurve::from_handles(h)
        }
        None => serde_json::from_value::<EaseCurve>(v.clone()).map_err(|e| bad(c, format!("`curve`: {e}")))?.sanitized(),
    };
    parsed.ok_or_else(|| bad(c, "`curve` numbers must be finite"))
}

/// The curve between the first selected pair of keyframes.
fn captured(s: &Session, c: &str) -> Result<EaseCurve> {
    let comp = s.active_comp().ok_or(EngineError::NoComp)?;
    let (lid, uid, i) = selected_pairs(s).into_iter().next().ok_or_else(|| bad(c, NEED_PAIR))?;
    let pr = comp.layer(lid).and_then(|l| l.props.find(uid)).ok_or_else(|| bad(c, "no property"))?;
    EaseCurve::measure(&pr.keys, i, pr.spatial).ok_or_else(|| bad(c, "the selected keyframes have no curve to keep (a hold, or no change in value)"))
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "keys.easePreset.apply";
    let curve = match (str_p(p, "preset"), p.get("curve")) {
        (Some(name), _) => find(s, name).ok_or_else(|| bad(C, format!("no ease preset `{name}` (see keys.easePreset.list)")))?,
        (None, Some(v)) => curve_v(v, C)?,
        (None, None) => return Err(bad(C, "pass `preset` (a name) or `curve`")),
    };
    let pairs = selected_pairs(s);
    if pairs.is_empty() {
        return Err(bad(C, NEED_PAIR));
    }
    let cid = s.active_comp_id().ok_or(EngineError::NoComp)?;
    let n = s.edit("Apply Ease Preset", None, |proj, _| {
        let comp = proj.comp_mut(cid).ok_or(EngineError::NoComp)?;
        let mut n = 0;
        for (lid, uid, i) in &pairs {
            if let Some(pr) = comp.layer_mut(*lid).and_then(|l| l.props.find_mut(*uid))
                && curve.apply(&mut pr.keys, *i, pr.spatial)
            {
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"pairs": n, "curve": curve}))
}

fn capture(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!(captured(s, "keys.easePreset.capture")?))
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "keys.easePreset.save";
    let name = name_p(p, "name", C)?;
    if is_built_in(&name) {
        return Err(bad(C, format!("`{name}` is a built-in preset; choose another name")));
    }
    let curve = match p.get("curve") {
        Some(v) => curve_v(v, C)?,
        None => captured(s, C)?,
    };
    let preset = EasePreset { name, curve };
    let mut list = s.ease_presets.clone();
    // Saving under an existing name replaces that preset.
    match list.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&preset.name)) {
        Some(q) => *q = preset.clone(),
        None => list.push(preset.clone()),
    }
    store(s, list)?;
    Ok(json!(preset))
}

/// Index of the user preset `name`; built-in presets can't be changed.
fn user_preset(s: &Session, name: &str, c: &str) -> Result<usize> {
    s.ease_presets.iter().position(|q| q.name.eq_ignore_ascii_case(name)).ok_or_else(|| {
        if is_built_in(name) { bad(c, format!("`{name}` is a built-in preset and can't be changed")) } else { bad(c, format!("no user ease preset `{name}`")) }
    })
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "keys.easePreset.rename";
    let (name, new_name) = (name_p(p, "name", C)?, name_p(p, "newName", C)?);
    let i = user_preset(s, &name, C)?;
    if is_built_in(&new_name) || s.ease_presets.iter().enumerate().any(|(j, q)| j != i && q.name.eq_ignore_ascii_case(&new_name)) {
        return Err(bad(C, format!("an ease preset named `{new_name}` exists")));
    }
    let mut list = s.ease_presets.clone();
    if let Some(q) = list.get_mut(i) {
        q.name = new_name.clone();
    }
    store(s, list)?;
    Ok(json!({"name": new_name}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "keys.easePreset.delete";
    let name = name_p(p, "name", C)?;
    user_preset(s, &name, C)?;
    let mut list = s.ease_presets.clone();
    list.retain(|q| !q.name.eq_ignore_ascii_case(&name));
    store(s, list)?;
    Ok(json!({"deleted": name}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(presets(s).map(|(name, curve, built_in)| json!({"name": name, "builtIn": built_in, "curve": curve})).collect())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "keys.easePreset.apply",
            "Apply Ease Preset",
            [],
            None,
            "{preset (name) | curve: {outInfluence (%), outSpeed, inInfluence (%), inSpeed} | [x1, y1, x2, y2]} — eases every pair of neighbouring selected keyframes; speeds are relative to the segment's average speed",
            has_keys,
            apply
        ),
        cmd!(
            "keys.easePreset.save",
            "Save Ease Preset",
            [],
            None,
            "{name, curve? (as for apply; default: the curve between the first selected pair of keyframes)} — replaces a user preset of that name",
            always,
            save
        ),
        cmd!("keys.easePreset.rename", "Rename Ease Preset", [], None, "{name, newName}", always, rename),
        cmd!("keys.easePreset.delete", "Delete Ease Preset", [], None, "{name}", always, delete),
        query!("keys.easePreset.list", "Ease Presets", "{} — [{name, builtIn, curve}]", list),
        query!("keys.easePreset.capture", "Ease Curve of Selected Keyframes", "{} — the curve between the first selected pair of keyframes", capture),
    ]
}
