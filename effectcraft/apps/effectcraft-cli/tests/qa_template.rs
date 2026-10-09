//! M13.15 scenario 5 — Essential Graphics through the CLI: a MOGRT-style template built by an
//! After Effects-style script (`effectcraft-cli script`), its controls changed with commands on
//! an instance, rendered, exported as a template and imported into a fresh project.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn cli(dir: &PathBuf, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_effectcraft-cli")).current_dir(dir).args(args).arg("--json").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let v = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {stdout} {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code().unwrap_or(-1), v)
}

fn ok(dir: &PathBuf, args: &[&str]) -> Value {
    let (code, v) = cli(dir, args);
    assert_eq!(code, 0, "{args:?}: {v}");
    v
}

const BUILD: &str = r#"
app.beginUndoGroup("Build Template");
var comp = app.project.items.addComp("Lower Third", 320, 90, 1, 2, 12);
var bar = comp.layers.addSolid([1, 1, 1], "Bar", 320, 40, 1, 2);
bar.property("ADBE Transform Group").property("ADBE Position").setValue([160, 65]);
var t = comp.layers.addText("Jane Doe");
t.name = "Name";
t.property("ADBE Transform Group").property("ADBE Position").setValue([160, 30]);
var fill = bar.property("ADBE Effect Parade").addProperty("ADBE Fill");
t.property("ADBE Text Properties").property("ADBE Text Document").addToMotionGraphicsTemplateAs(comp, "Title");
fill.property("ADBE Fill-0002").addToMotionGraphicsTemplateAs(comp, "Bar Color");
bar.property("ADBE Transform Group").property("ADBE Opacity").addToMotionGraphicsTemplateAs(comp, "Bar Opacity");
comp.motionGraphicsTemplateName = "Lower Third";
app.endUndoGroup();
writeLn("controls: " + comp.motionGraphicsTemplateControllerCount);
comp.motionGraphicsTemplateName;
"#;

#[test]
fn essential_graphics_template_via_script_and_commands() {
    let dir = std::env::temp_dir().join(format!("ec-qa-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("build.jsx"), BUILD).unwrap();

    let r = ok(&dir, &["script", "build.jsx", "--save-as", "t.ecproj"]);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["output"], "controls: 3");
    let list = ok(&dir, &["exec", "essential.list", r##"{"comp":"Lower Third"}"##, "t.ecproj"]);
    let controls: Vec<(String, String)> =
        list["controls"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_string(), c["type"].as_str().unwrap().to_string())).collect();
    assert_eq!(
        controls,
        [("Title".into(), "text".into()), ("Bar Color".into(), "color".into()), ("Bar Opacity".into(), "slider".into())],
        "ADBE Fill-0002 is the Fill's Color: {list}"
    );

    // An edit comp with an instance; controls addressed by name.
    let r = ok(
        &dir,
        &[
            "run",
            "t.ecproj",
            "comp.new",
            r##"{"name":"Edit","width":320,"height":90,"frameRate":12,"duration":2}"##,
            "layer.addItem",
            r##"{"item":"Lower Third"}"##,
            "comp.new",
            r##"{"name":"Plain","width":320,"height":90,"frameRate":12,"duration":2}"##,
            "layer.addItem",
            r##"{"item":"Lower Third"}"##,
            "comp.open",
            r##"{"comp":"Edit"}"##,
            "essential.set",
            r##"{"layer":"#1","control":"Title","value":"John Smith"}"##,
            "essential.set",
            r##"{"layer":"#1","control":"Bar Color","value":"#00c080"}"##,
            "essential.set",
            r##"{"layer":"#1","control":"bar opacity","value":50}"##,
            "--save",
        ],
    );
    let inst = ok(&dir, &["exec", "essential.instance", r##"{"comp":"Edit","layer":"#1"}"##, "t.ecproj"]);
    assert!(inst["controls"].as_array().unwrap().iter().all(|c| c["overridden"] == true), "{inst}");
    let (code, e) = cli(&dir, &["exec", "essential.set", r##"{"comp":"Edit","layer":"#1","control":"Subtitle","value":"x"}"##, "t.ecproj"]);
    assert_eq!(code, 1);
    assert!(e["error"].as_str().unwrap().contains("controls: Title, Bar Color, Bar Opacity"), "{e} ({r})");

    // Render both instances and compare.
    ok(&dir, &["render-frame", "t.ecproj", "--comp", "Edit", "--time", "1", "--out", "edit.png"]);
    ok(&dir, &["render-frame", "t.ecproj", "--comp", "Plain", "--time", "1", "--out", "plain.png"]);
    let edit = image::open(dir.join("edit.png")).unwrap().to_rgba8();
    let plain = image::open(dir.join("plain.png")).unwrap().to_rgba8();
    let bar = edit.get_pixel(10, 75).0;
    assert!(bar[0] < 15 && (bar[1] as i32 - 96).abs() < 8 && (bar[2] as i32 - 64).abs() < 8, "50 % of #00c080 over black: {bar:?}");
    let pbar = plain.get_pixel(10, 75).0;
    assert!(pbar[0] > 240 && pbar[1] < 15, "the template's own red: {pbar:?}");
    let diff = |y0: u32, y1: u32| -> u32 {
        (y0..y1).flat_map(|y| (0..320).map(move |x| (x, y))).filter(|(x, y)| edit.get_pixel(*x, *y).0 != plain.get_pixel(*x, *y).0).count() as u32
    };
    assert!(diff(10, 45) > 100, "the title text differs");

    // Export the template and bring it into a fresh project.
    let x = ok(&dir, &["exec", "essential.exportTemplate", r##"{"comp":"Lower Third","path":"lt.ectemplate"}"##, "t.ecproj"]);
    assert_eq!(x["controls"], 3, "{x}");
    let info = ok(&dir, &["exec", "essential.templateInfo", r##"{"path":"lt.ectemplate"}"##, "--empty"]);
    assert_eq!(info["name"], "Lower Third", "{info}");
    let imp = ok(&dir, &["run", "--empty", "essential.importTemplate", r##"{"path":"lt.ectemplate"}"##, "--save-as", "fresh.ecproj"]);
    assert!(imp["saved"].is_string(), "{imp}");
    let list = ok(&dir, &["exec", "essential.list", r##"{"comp":"Lower Third"}"##, "fresh.ecproj"]);
    assert_eq!(list["controls"].as_array().unwrap().len(), 3, "{list}");
    let _ = std::fs::remove_dir_all(&dir);
}
