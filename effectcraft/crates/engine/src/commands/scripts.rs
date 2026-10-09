//! File ▸ Scripts and ScriptUI.
//!
//! * **Installed scripts** live in the settings store under `Scripts/` (File ▸ Scripts ▸ Install
//!   Script File…) and `Scripts/ScriptUI Panels/` (Install ScriptUI Panel…), next to the
//!   settings on the desktop, so dropping `.jsx` files in those folders works too. File ▸ Scripts
//!   lists them with the bundled sample scripts ([`SAMPLES`]) and runs them by name; ScriptUI
//!   panels appear at the bottom of the Window menu and open as dockable panels
//!   (`window.scriptPanel`), running with `this` = the panel as in After Effects.
//! * **ScriptUI windows** (`scriptui.*`): list the open script windows, read their control
//!   trees, click buttons and set values like a user, close them (see [`crate::scriptui`]).

use serde::Serialize;
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, str_p};
use crate::{EngineError, Event, Result, Session, cmd, scriptui};

/// The settings-store folders of installed scripts.
pub const SCRIPTS_DIR: &str = "Scripts";
pub const PANELS_DIR: &str = "Scripts/ScriptUI Panels";

/// Sample scripts that ship with EffectCraft (original work): (file name, ScriptUI panel, code).
pub const SAMPLES: &[(&str, bool, &str)] = &[
    ("Create Null at Selected Layers.jsx", false, include_str!("../../scripts/Create Null at Selected Layers.jsx")),
    ("Rename Layers.jsx", false, include_str!("../../scripts/Rename Layers.jsx")),
    ("Render Queue Batch.jsx", false, include_str!("../../scripts/Render Queue Batch.jsx")),
    ("Sort Layers by In Point.jsx", false, include_str!("../../scripts/Sort Layers by In Point.jsx")),
    ("Layer Tools.jsx", true, include_str!("../../scripts/ScriptUI Panels/Layer Tools.jsx")),
];

/// A script File ▸ Scripts (or the Window menu, for panels) offers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScriptEntry {
    /// File name (`Rename Layers.jsx`): what the menus show and commands take.
    pub name: String,
    /// In the ScriptUI Panels folder: opens as a dockable panel.
    pub panel: bool,
    /// `installed` or `sample`.
    pub source: &'static str,
}

fn is_script(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".jsx") || l.ends_with(".js")
}

/// Installed scripts and panels, then the samples not shadowed by an installed one, by name.
pub fn scripts(s: &Session) -> Vec<ScriptEntry> {
    let mut v: Vec<ScriptEntry> = vec![];
    if let Some(cfg) = &s.config {
        for (dir, panel) in [(SCRIPTS_DIR, false), (PANELS_DIR, true)] {
            for n in cfg.list(dir).into_iter().filter(|n| is_script(n)) {
                v.push(ScriptEntry { name: n, panel, source: "installed" });
            }
        }
    }
    for (name, panel, _) in SAMPLES {
        if !v.iter().any(|e| e.name == *name && e.panel == *panel) {
            v.push(ScriptEntry { name: name.to_string(), panel: *panel, source: "sample" });
        }
    }
    v.sort_by(|a, b| a.panel.cmp(&b.panel).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

/// The code of an installed or sample script / panel.
pub fn script_code(s: &Session, name: &str, panel: Option<bool>) -> Option<(bool, String)> {
    if let Some(cfg) = &s.config {
        for (dir, is_panel) in [(SCRIPTS_DIR, false), (PANELS_DIR, true)] {
            if panel.is_some_and(|p| p != is_panel) {
                continue;
            }
            if let Some(code) = cfg.read(&format!("{dir}/{name}")) {
                return Some((is_panel, code));
            }
        }
    }
    SAMPLES.iter().find(|(n, p, _)| *n == name && panel.is_none_or(|w| w == *p)).map(|(_, p, c)| (*p, c.to_string()))
}

fn install(s: &mut Session, p: &Value, panel: bool) -> Result<Value> {
    let c = if panel { "file.installScriptUIPanel" } else { "file.installScript" };
    let path = str_p(p, "path").ok_or_else(|| bad(c, "missing `path` (a .jsx / .js script)"))?;
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if !is_script(&name) {
        return Err(bad(c, "scripts are .jsx or .js files (.jsxbin is not supported)"));
    }
    let bytes = s.services.read_file(path).map_err(|e| EngineError::Other(format!("cannot read {path}: {e}")))?;
    let code = String::from_utf8(bytes).map_err(|_| bad(c, "the script is not UTF-8 text"))?;
    let cfg = s.config.clone().ok_or_else(|| EngineError::Other("this session has no settings folder to install scripts into".into()))?;
    let dir = if panel { PANELS_DIR } else { SCRIPTS_DIR };
    cfg.write(&format!("{dir}/{name}"), &code).map_err(|e| EngineError::Other(format!("cannot install {name}: {e}")))?;
    // After Effects asks for a restart; our menus pick it up at once.
    s.toast(if panel {
        format!("Installed ScriptUI panel {name}: find it at the bottom of the Window menu")
    } else {
        format!("Installed script {name}: find it in File ▸ Scripts")
    });
    Ok(json!({"name": name, "panel": panel}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!(scripts(s)))
}

fn uninstall(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "file.uninstallScript";
    let name = str_p(p, "name").ok_or_else(|| bad(c, "missing `name`"))?;
    let cfg = s.config.clone().ok_or_else(|| bad(c, "no settings folder"))?;
    let mut removed = false;
    for dir in [SCRIPTS_DIR, PANELS_DIR] {
        let key = format!("{dir}/{name}");
        if cfg.read(&key).is_some() {
            cfg.remove(&key).map_err(|e| EngineError::Other(e.to_string()))?;
            removed = true;
        }
    }
    if removed { Ok(json!({"removed": name})) } else { Err(bad(c, format!("`{name}` is not installed"))) }
}

/// Run an installed or sample script by name (File ▸ Scripts ▸ <name>). A ScriptUI panel run
/// this way gets `this` = a floating palette-like panel window.
pub(crate) fn run_named(s: &mut Session, name: &str) -> Result<Value> {
    let (panel, code) = script_code(s, name, None).ok_or_else(|| bad("file.runScript", format!("no installed or sample script `{name}`")))?;
    if panel {
        return open_panel(s, &json!({"name": name}));
    }
    super::file_more::run_js_named(s, &code, name)
}

/// The code a ScriptUI panel runs with: `this` is a dockable Panel titled after the script.
pub fn panel_wrapper(title: &str, code: &str) -> String {
    // On one line, so line numbers in errors stay those of the file.
    format!("(function () {{ {code}\n}}).call(__uiDockPanel({}));", Value::String(title.to_string()))
}

/// Window ▸ <ScriptUI panel>: run the panel script (or bring its panel forward).
fn open_panel(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "window.scriptPanel";
    let name = str_p(p, "name").ok_or_else(|| bad(c, "missing `name` (a script in the ScriptUI Panels folder)"))?.to_string();
    let title = name.trim_end_matches(".jsx").trim_end_matches(".js").to_string();
    if let Some(w) = s.script_ui.windows.iter().find(|w| w.kind == scriptui::WindowKind::Panel && w.script == name) {
        let id = w.id;
        s.events.push(Event::Frontend { command: c.into(), params: json!({"window": id}) });
        return Ok(json!({"window": id, "open": true}));
    }
    let (_, code) = script_code(s, &name, Some(true)).or_else(|| script_code(s, &name, None)).ok_or_else(|| bad(c, format!("no ScriptUI panel `{name}`")))?;
    let out = super::file_more::run_js_named(s, &panel_wrapper(&title, &code), &name)?;
    let id = s.script_ui.windows.iter().rev().find(|w| w.kind == scriptui::WindowKind::Panel && w.script == name).map(|w| w.id);
    if let Some(id) = id {
        s.events.push(Event::Frontend { command: c.into(), params: json!({"window": id}) });
    }
    Ok(json!({"window": id, "result": out}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "file.installScript",
            "Install Script File...",
            ["File", "Scripts"],
            None,
            "{path (.jsx / .js)} → copies it to the Scripts folder; it appears in File ▸ Scripts",
            always,
            |s, p| install(s, p, false)
        ),
        cmd!(
            "file.installScriptUIPanel",
            "Install ScriptUI Panel...",
            ["File", "Scripts"],
            None,
            "{path (.jsx / .js)} → copies it to the ScriptUI Panels folder; it appears in the Window menu",
            always,
            |s, p| install(s, p, true)
        ),
        cmd!("file.uninstallScript", "Uninstall Script", [], None, "{name}", always, uninstall),
        crate::query!("file.scripts.list", "List Scripts", "{} → [{name, panel, source: installed|sample}]", list),
        cmd!(
            "window.scriptPanel",
            "ScriptUI Panel",
            [],
            None,
            "{name (a script in the ScriptUI Panels folder, e.g. `Layer Tools.jsx`)} → opens it as a dockable panel",
            always,
            open_panel
        ),
        crate::query!("scriptui.list", "List Script Windows", "{} → [{window, title, kind: dialog|palette|window|panel, script, modal, size}]", scriptui::list),
        crate::query!(
            "scriptui.get",
            "Script Window Controls",
            "{window?: id | title} → {id, title, kind, root: {id, type, name, text, value, checked, items, selection, bounds, enabled, draw (onDraw paint list), children…}}",
            scriptui::get
        ),
        cmd!(
            "scriptui.click",
            "Click Script Window Control",
            [],
            None,
            "{window?: id | title, widget: id | \"#id\" | properties.name | text} (buttons, checkboxes, radio buttons, tabs)",
            always,
            scriptui::click
        ),
        cmd!(
            "scriptui.set",
            "Set Script Window Control",
            [],
            None,
            "{window?, widget, value: text | number | bool | item index | item text, changing?: bool (a live update: each keystroke / slider step; fires onChanging only)} (edit text, sliders, checkboxes, lists) → fires onChanging / onChange",
            always,
            scriptui::set
        ),
        cmd!(
            "scriptui.close",
            "Close Script Window",
            [],
            None,
            "{window?, result? (a dialog's show() returns it; default 2 = Cancel)}",
            always,
            scriptui::close
        ),
    ]
}
