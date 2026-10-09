//! Effect Controls operations: copy/paste, duplicate, reorder, remove (Delete), reset, and the
//! Edit menu routing to them when effects are selected, all with undo.

use effectcraft_keyframe::Value as KV;
use effectcraft_project::{Layer, LayerId};
use serde_json::json;

use crate::Session;

fn comp_with_two_solids() -> (Session, u64, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 640, "height": 360, "frameRate": 30, "duration": 10})).unwrap();
    let a = s.execute("layer.newSolid", json!({"color": "#ff0000", "width": 200, "height": 100})).unwrap()["layer"].as_u64().unwrap();
    let b = s.execute("layer.newSolid", json!({"color": "#00ff00", "width": 400, "height": 300})).unwrap()["layer"].as_u64().unwrap();
    (s, a, b)
}

fn layer(s: &Session, id: u64) -> Layer {
    s.active_comp().unwrap().layer(LayerId(id)).unwrap().clone()
}

fn fx_names(s: &Session, id: u64) -> Vec<String> {
    layer(s, id).effects().map(|f| f.groups().map(|g| g.name.clone()).collect()).unwrap_or_default()
}

fn apply(s: &mut Session, lid: u64, effect: &str) -> u64 {
    s.execute("effect.apply", json!({"layers": [lid], "effect": effect})).unwrap()["effects"][0].as_u64().unwrap()
}

fn select_effect(s: &mut Session, lid: u64, uid: u64) {
    s.state.selected_layers = vec![LayerId(lid)];
    s.state.selected_props = vec![(LayerId(lid), uid)];
}

#[test]
fn copy_paste_effects_between_layers_with_undo() {
    let (mut s, a, b) = comp_with_two_solids();
    let blur = apply(&mut s, a, "Gaussian Blur");
    let gid = layer(&s, a).effects().unwrap().groups().next().unwrap().get("blurriness").unwrap().uid;
    s.execute("prop.set", json!({"layer": a, "prop": gid, "value": 12.5})).unwrap();
    select_effect(&mut s, a, blur);
    // Edit ▸ Copy with an effect selected copies the effect, not the layer.
    s.execute("edit.copy", json!({})).unwrap();
    assert_eq!(s.state.effect_clipboard.len(), 1);
    assert!(s.state.clipboard.is_empty());
    // Paste onto the other layer.
    s.state.selected_layers = vec![LayerId(b)];
    s.state.selected_props.clear();
    let n_layers = s.active_comp().unwrap().layers.len();
    s.execute("edit.paste", json!({})).unwrap();
    assert_eq!(s.active_comp().unwrap().layers.len(), n_layers, "no layer pasted");
    assert_eq!(fx_names(&s, b), vec!["Gaussian Blur"]);
    let pasted = layer(&s, b).effects().unwrap().groups().next().unwrap().clone();
    assert_ne!(pasted.uid, blur, "fresh uids");
    assert_eq!(pasted.get("blurriness").unwrap().value, KV::Scalar(12.5));
    // Pasting again names the second instance uniquely.
    s.execute("effect.paste", json!({"layers": [b]})).unwrap();
    assert_eq!(fx_names(&s, b), vec!["Gaussian Blur", "Gaussian Blur 2"]);
    s.undo();
    s.undo();
    assert!(fx_names(&s, b).is_empty());
    assert_eq!(fx_names(&s, a), vec!["Gaussian Blur"]);
}

#[test]
fn duplicate_reorder_and_delete_selected_effect() {
    let (mut s, a, _) = comp_with_two_solids();
    let blur = apply(&mut s, a, "Gaussian Blur");
    apply(&mut s, a, "Invert");
    select_effect(&mut s, a, blur);
    // Edit ▸ Duplicate (Cmd+D) duplicates the selected effect right after it and selects the copy.
    s.execute("edit.duplicate", json!({})).unwrap();
    assert_eq!(fx_names(&s, a), vec!["Gaussian Blur", "Gaussian Blur 2", "Invert"]);
    let dup = s.state.selected_props[0].1;
    assert_ne!(dup, blur);
    // Reorder: move Invert to the top.
    s.execute("effect.reorder", json!({"layer": a, "effect": "Invert", "index": 1})).unwrap();
    assert_eq!(fx_names(&s, a), vec!["Invert", "Gaussian Blur", "Gaussian Blur 2"]);
    // Delete with the duplicate selected removes just that effect (not the layer).
    s.execute("edit.clear", json!({})).unwrap();
    assert_eq!(fx_names(&s, a), vec!["Invert", "Gaussian Blur"]);
    assert!(s.active_comp().unwrap().layer(LayerId(a)).is_some());
    s.undo();
    assert_eq!(fx_names(&s, a), vec!["Invert", "Gaussian Blur", "Gaussian Blur 2"]);
    s.undo();
    assert_eq!(fx_names(&s, a), vec!["Gaussian Blur", "Gaussian Blur 2", "Invert"]);
    s.undo();
    assert_eq!(fx_names(&s, a), vec!["Gaussian Blur", "Invert"]);
}

#[test]
fn reset_effect_restores_defaults_in_one_undo_step() {
    let (mut s, _, b) = comp_with_two_solids();
    let ramp = apply(&mut s, b, "Gradient Ramp");
    let g = layer(&s, b).effects().unwrap().groups().next().unwrap().clone();
    let start = g.get("start").unwrap().uid;
    let shape = g.get("shape").unwrap().uid;
    // Point defaults are fractions of the layer (400 × 300).
    assert_eq!(g.get("start").unwrap().value, KV::Vec2([200.0, 0.0]));
    s.execute("prop.set", json!({"layer": b, "prop": start, "value": [10.0, 20.0]})).unwrap();
    s.execute("prop.set", json!({"layer": b, "prop": shape, "value": 1})).unwrap();
    let undo_before = s.history.undo.len();
    s.execute("effect.reset", json!({"layer": b, "effect": ramp})).unwrap();
    assert_eq!(s.history.undo.len(), undo_before + 1);
    let g = layer(&s, b).effects().unwrap().groups().next().unwrap().clone();
    assert_eq!(g.get("start").unwrap().value, KV::Vec2([200.0, 0.0]));
    assert_eq!(g.get("shape").unwrap().value, KV::Enum(0));
    s.undo();
    let g = layer(&s, b).effects().unwrap().groups().next().unwrap().clone();
    assert_eq!(g.get("start").unwrap().value, KV::Vec2([10.0, 20.0]));
}

#[test]
fn reset_keys_animated_params_at_the_cti() {
    let (mut s, a, _) = comp_with_two_solids();
    let blur = apply(&mut s, a, "Gaussian Blur");
    let uid = layer(&s, a).effects().unwrap().groups().next().unwrap().get("blurriness").unwrap().uid;
    s.execute("prop.toggleAnimation", json!({"layer": a, "prop": uid})).unwrap();
    s.execute("prop.set", json!({"layer": a, "prop": uid, "value": 30.0})).unwrap();
    s.execute("time.set", json!({"time": 2.0})).unwrap();
    s.execute("effect.reset", json!({"layer": a, "effect": blur})).unwrap();
    let pr = layer(&s, a).effects().unwrap().groups().next().unwrap().get("blurriness").unwrap().clone();
    assert_eq!(pr.keys.len(), 2, "a key at the CTI with the default value");
    assert_eq!(pr.keys[1].value, KV::Scalar(0.0));
}

#[test]
fn copy_layers_clears_effect_clipboard() {
    let (mut s, a, b) = comp_with_two_solids();
    let blur = apply(&mut s, a, "Gaussian Blur");
    select_effect(&mut s, a, blur);
    s.execute("effect.copy", json!({})).unwrap();
    s.state.selected_props.clear();
    s.state.selected_layers = vec![LayerId(b)];
    s.execute("edit.copy", json!({})).unwrap();
    assert!(s.state.effect_clipboard.is_empty());
    let n = s.active_comp().unwrap().layers.len();
    s.execute("edit.paste", json!({})).unwrap();
    assert_eq!(s.active_comp().unwrap().layers.len(), n + 1, "layer pasted");
}

#[test]
fn layer_params_get_a_source_companion() {
    let (mut s, a, _) = comp_with_two_solids();
    apply(&mut s, a, "ec.channel.blend");
    let g = layer(&s, a).effects().unwrap().groups().next().unwrap().clone();
    let layer_params: Vec<_> = g.props().filter(|p| matches!(p.ui, effectcraft_project::ParamUi::Layer)).map(|p| p.match_id.clone()).collect();
    assert!(!layer_params.is_empty());
    for id in layer_params {
        let src = g.get(&effectcraft_effects::layer_source_id(&id)).expect("companion");
        assert_eq!(src.value, KV::Enum(2), "Effects & Masks by default");
        assert!(matches!(src.ui, effectcraft_project::ParamUi::Hidden));
    }
}

/// Projects saved before parameters were renamed, regrouped, added or had their popups
/// reordered open with their instances brought up to date (`effectcraft_effects::migrate`).
#[test]
fn opening_an_old_project_upgrades_effect_instances() {
    use effectcraft_effects::migrate::PARAM_ID_ALIASES;
    use effectcraft_project::{Node, ParamUi, PropGroup};
    let (mut s, a, _) = comp_with_two_solids();
    apply(&mut s, a, "ec.blur.gaussian");
    // One instance of every effect with renamed parameter ids.
    let mut aliased: Vec<&str> = PARAM_ID_ALIASES.iter().map(|(e, _, _)| *e).collect();
    aliased.dedup();
    for e in &aliased {
        apply(&mut s, a, e);
    }
    // Rewrite the saved instances the way an older version wrote them.
    fn take(g: &mut PropGroup, path: &str) -> Option<effectcraft_project::Property> {
        match path.split_once('/') {
            None => {
                let i = g.children.iter().position(|c| matches!(c, Node::Prop(p) if p.match_id == path))?;
                match g.children.remove(i) {
                    Node::Prop(p) => Some(p),
                    Node::Group(_) => None,
                }
            }
            Some((first, rest)) => take(g.sub_mut(first)?, rest),
        }
    }
    let mut old = (*s.project).clone();
    let cid = s.active_comp_id().unwrap();
    {
        let comp = std::sync::Arc::make_mut(match &mut old.items.get_mut(&cid).unwrap().kind {
            effectcraft_project::ItemKind::Comp(c) => c,
            _ => panic!("not a composition"),
        });
        let l = comp.layers.iter_mut().find(|l| l.id == LayerId(a)).unwrap();
        for n in &mut l.props.sub_mut("effects").unwrap().children {
            let Node::Group(g) = n else { continue };
            if g.match_id == "ec.blur.gaussian" {
                // Repeat Edge Pixels did not exist yet, Blurriness had another name, the popup
                // listed its options in reverse.
                take(g, "repeatEdge").unwrap();
                g.get_mut("blurriness").unwrap().name = "Old Blurriness".into();
                let d = g.get_mut("dimensions").unwrap();
                let ParamUi::Popup { options } = &d.ui else { panic!("dimensions is a popup") };
                d.ui = ParamUi::Popup { options: options.iter().rev().cloned().collect() };
                d.value = KV::Enum(0);
            }
            for (e, old_id, new_id) in PARAM_ID_ALIASES {
                if g.match_id == *e {
                    let mut p = take(g, new_id).unwrap_or_else(|| panic!("{e}: no {new_id}"));
                    // Put it back at its old path (which may be inside a group).
                    let (path, leaf) = old_id.rsplit_once('/').map(|(p, l)| (Some(p), l)).unwrap_or((None, old_id));
                    p.match_id = leaf.to_string();
                    let mut at: &mut PropGroup = g;
                    for seg in path.into_iter().flat_map(|p| p.split('/')) {
                        if at.sub(seg).is_none() {
                            at.children.push(PropGroup::new(1_000_000 + at.children.len() as u64, seg, seg).into());
                        }
                        at = at.sub_mut(seg).unwrap();
                    }
                    at.children.push(p.into());
                }
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("ec-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.ecproj").to_string_lossy().to_string();
    std::fs::write(&path, old.to_json()).unwrap();
    let mut s2 = Session::default();
    s2.execute("file.open", json!({"path": path})).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    let l = s2.project.comp(cid).unwrap().layer(LayerId(a)).unwrap().clone();
    let fx = l.effects().unwrap();
    let spec = effectcraft_effects::find("ec.blur.gaussian").unwrap();
    let sp = |id: &str| spec.params.iter().find(|p| p.id == id).unwrap();
    let blur = fx.groups().find(|g| g.match_id == "ec.blur.gaussian").unwrap();
    assert_eq!(blur.get("blurriness").unwrap().name, sp("blurriness").name);
    assert!(blur.get("repeatEdge").is_some(), "missing parameter added");
    let dims = blur.get("dimensions").unwrap();
    let ParamUi::Popup { options } = &sp("dimensions").ui else { panic!() };
    assert_eq!(dims.value, KV::Enum(options.len() as u32 - 1), "popup value remapped by label");
    assert_eq!(dims.ui, sp("dimensions").ui);
    for (e, old_id, new_id) in PARAM_ID_ALIASES {
        let g = fx.groups().find(|g| g.match_id == *e).unwrap();
        let at = |id: &str| {
            let (path, leaf) = id.rsplit_once('/').map(|(p, l)| (Some(p), l)).unwrap_or((None, id));
            let mut node = Some(g);
            for seg in path.into_iter().flat_map(|p| p.split('/')) {
                node = node.and_then(|n| n.sub(seg));
            }
            node.and_then(|n| n.get(leaf)).is_some()
        };
        assert!(!at(old_id), "{e}: {old_id} still present");
        let (path, leaf) = new_id.rsplit_once('/').map(|(p, l)| (Some(p), l)).unwrap_or((None, new_id));
        let mut node = Some(g);
        for seg in path.into_iter().flat_map(|p| p.split('/')) {
            node = node.and_then(|n| n.sub(seg));
        }
        assert!(node.and_then(|n| n.get(leaf)).is_some(), "{e}: {old_id} not moved to {new_id}");
    }
    // The upgraded project renders.
    let _ = s2.render(cid, s2.time(), Default::default());
}
