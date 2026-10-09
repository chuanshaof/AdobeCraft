//! Headless UI tests of the Sequence / Markers menu additions: the Add Tracks and Delete Tracks dialogs, the
//! through-edit marks on the timeline, the Markers panel colour filter and the Shift+; gap key.
//! The real `FilmcraftApp` under `egui_kittest`, driven over the control channel by automation id.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
}

impl Driver {
    fn demo() -> Self {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let app = FilmcraftApp::new(session).with_control(rx);
        let harness = Harness::builder().with_size(egui::vec2(1600.0, 980.0)).with_max_steps(10_000).build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx };
        d.frames(4);
        d
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                return v;
            }
        }
        panic!("no reply to {method} {params}");
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params.clone());
        assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
        v["result"].clone()
    }

    fn exec(&mut self, command: &str, params: Value) -> Value {
        self.ok("engine.execute", json!({"command": command, "params": params}))
    }

    fn click(&mut self, id: &str) {
        self.ok("ui.click", json!({"id": id}));
        self.frames(2);
    }

    fn app(&mut self) -> &mut FilmcraftApp {
        self.harness.state_mut()
    }

    /// (video, audio, submix) track names of the active sequence, lowest first.
    fn tracks(&mut self) -> [Vec<String>; 3] {
        let q = self.app().session.active_sequence().unwrap();
        [&q.video_tracks, &q.audio_tracks, &q.submix_tracks].map(|t| t.iter().map(|t| t.name.clone()).collect())
    }

    fn label(&mut self, id: &str) -> String {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        let e = v.as_array().unwrap().iter().find(|e| e["id"] == json!(id)).unwrap_or_else(|| panic!("no element {id}")).clone();
        e["label"].as_str().unwrap_or_default().to_string()
    }

    fn ids(&mut self, prefix: &str) -> Vec<String> {
        let v = self.ok("ui.elements", json!({"prefix": prefix}));
        v.as_array().unwrap().iter().filter_map(|e| e["id"].as_str().map(str::to_string)).collect()
    }
}

/// Sequence ▸ Add Tracks… opens Premiere Pro's dialog instead of adding a video track at once:
/// amounts, a placement and (for audio and submix tracks) a track type for each kind, with
/// Premiere's defaults (one video and one audio track after the last ones, no submix track).
#[test]
fn add_tracks_dialog_offers_amount_placement_and_type() {
    let mut d = Driver::demo();
    let before = d.tracks();
    assert_eq!(
        before,
        [vec!["Video 1", "Video 2", "Video 3"], vec!["Audio 1", "Audio 2", "Audio 3"], vec![]]
            .map(|v: Vec<&str>| v.into_iter().map(String::from).collect::<Vec<_>>())
    );
    let r = d.ok("ui.menu.invoke", json!({"id": "sequence.addTracks"}));
    assert_eq!(r["dialog"], "addTracks", "{r}");
    d.frames(3);
    assert_eq!(d.tracks(), before, "opening the dialog adds nothing");
    let ids = d.ids("addTracks.");
    for id in [
        "addTracks.video.amount",
        "addTracks.video.placement",
        "addTracks.audio.amount",
        "addTracks.audio.placement",
        "addTracks.audio.type",
        "addTracks.submix.amount",
        "addTracks.submix.placement",
        "addTracks.submix.type",
        "addTracks.ok",
        "addTracks.cancel",
    ] {
        assert!(ids.iter().any(|i| i == id), "{id} missing: {ids:?}");
    }
    assert!(!ids.iter().any(|i| i == "addTracks.video.type"), "video tracks have no type");
    // the defaults, as shown
    for (id, shown) in [
        ("addTracks.video.amount", "1"),
        ("addTracks.video.placement", "After Video 3"),
        ("addTracks.audio.amount", "1"),
        ("addTracks.audio.placement", "After Audio 3"),
        ("addTracks.audio.type", "Standard"),
        ("addTracks.submix.amount", "0"),
        ("addTracks.submix.placement", "Before First Track"),
        ("addTracks.submix.type", "Stereo"),
    ] {
        assert_eq!(d.label(id), shown, "{id}");
    }
    // OK with the defaults: one video and one audio track after the last ones
    d.click("addTracks.ok");
    assert!(d.ids("addTracks.").is_empty(), "dialog closed");
    let [v, a, s] = d.tracks();
    assert_eq!((v.len(), a.len(), s.len()), (4, 4, 0));
    assert_eq!((v[3].as_str(), a[3].as_str()), ("Video 4", "Audio 4"));
    assert_eq!(d.app().session.active_sequence().unwrap().audio_tracks[3].channels, filmcraft_engine::project::AudioChannels::Stereo);
    // one undo step for the whole dialog
    d.exec("edit.undo", json!({}));
    assert_eq!(d.tracks(), before);
}

#[test]
fn add_tracks_dialog_places_and_types_the_new_tracks() {
    let mut d = Driver::demo();
    let on_v2 = d.app().session.active_sequence().unwrap().video_tracks[1].items[0].id;
    d.ok("ui.menu.invoke", json!({"id": "sequence.addTracks"}));
    d.frames(3);
    // Placement lists "Before First Track" and every track; pick "After Video 1"
    d.click("addTracks.video.placement");
    let options = d.ids("addTracks.video.placement.option.");
    assert_eq!(options.len(), 4, "{options:?}");
    assert_eq!(
        (d.label("addTracks.video.placement.option.0"), d.label("addTracks.video.placement.option.1")),
        ("Before First Track".into(), "After Video 1".into())
    );
    d.click("addTracks.video.placement.option.1");
    assert_eq!(d.label("addTracks.video.placement"), "After Video 1");
    // audio: a mono track before the first one
    d.click("addTracks.audio.placement");
    d.click("addTracks.audio.placement.option.0");
    d.click("addTracks.audio.type");
    let types = d.ids("addTracks.audio.type.option.");
    assert_eq!(types.len(), 4, "Standard, 5.1, Adaptive, Mono: {types:?}");
    d.click("addTracks.audio.type.option.mono");
    // submix: 5.1 (the types are Stereo, 5.1, Adaptive, Mono)
    d.click("addTracks.submix.type");
    assert!(
        d.ids("addTracks.submix.type.option.").iter().any(|i| i.ends_with(".stereo"))
            && !d.ids("addTracks.submix.type.option.").iter().any(|i| i.ends_with(".standard"))
    );
    d.click("addTracks.submix.type.option.5.1");
    // amounts (number fields): two video tracks, one audio, one submix
    d.app().ui.add_tracks.video = 2;
    d.app().ui.add_tracks.submix = 1;
    d.frames(2);
    assert_eq!((d.label("addTracks.video.amount"), d.label("addTracks.submix.amount")), ("2".into(), "1".into()));
    d.click("addTracks.ok");
    assert!(d.ids("addTracks.").is_empty(), "dialog closed");
    let [v, a, s] = d.tracks();
    assert_eq!(v, ["Video 1", "Video 2", "Video 3", "Video 4", "Video 5"]);
    assert_eq!(a, ["Audio 1", "Audio 2", "Audio 3", "Audio 4"]);
    assert_eq!(s, ["Submix 1"]);
    let q = d.app().session.active_sequence().unwrap().clone();
    assert!(q.video_tracks[1].items.is_empty() && q.video_tracks[2].items.is_empty(), "the new tracks are after Video 1");
    assert_eq!(q.video_tracks[3].items[0].id, on_v2, "what was on Video 2 moved up to Video 4");
    use filmcraft_engine::project::AudioChannels;
    assert_eq!((q.audio_tracks[0].channels, q.audio_tracks[0].items.len()), (AudioChannels::Mono, 0));
    assert_eq!(q.submix_tracks[0].channels, AudioChannels::Surround51);

    // with a submix track in the sequence its placement can be chosen too
    d.ok("ui.menu.invoke", json!({"id": "sequence.addTracks"}));
    d.frames(3);
    assert_eq!(d.label("addTracks.submix.placement"), "After Submix 1");
    assert_eq!(
        (d.label("addTracks.video.amount"), d.label("addTracks.audio.type")),
        ("1".into(), "Standard".into()),
        "the dialog starts from the defaults again"
    );
    // Cancel and Escape add nothing; neither does OK with every amount at 0
    d.click("addTracks.cancel");
    assert!(d.ids("addTracks.").is_empty());
    d.ok("ui.menu.invoke", json!({"id": "sequence.addTracks"}));
    d.frames(3);
    d.ok("ui.key", json!({"key": "Escape"}));
    d.frames(3);
    assert!(d.ids("addTracks.").is_empty());
    d.ok("ui.menu.invoke", json!({"id": "sequence.addTracks"}));
    d.frames(3);
    d.app().ui.add_tracks.video = 0;
    d.app().ui.add_tracks.audio = 0;
    d.frames(2);
    d.click("addTracks.ok");
    assert!(d.ids("addTracks.").is_empty());
    assert_eq!(d.tracks(), [v, a, s]);
    assert!(d.app().ui.status.is_empty(), "{}", d.app().ui.status);
}

#[test]
fn delete_tracks_dialog_deletes_empty_tracks() {
    let mut d = Driver::demo();
    let before = d.exec("sequence.inspect", json!({}));
    let nv = before["video"].as_array().unwrap().len();
    let na = before["audio"].as_array().unwrap().len();
    let r = d.ok("ui.menu.invoke", json!({"id": "sequence.deleteTracks"}));
    assert_eq!(r["dialog"], "deleteTracks", "{r}");
    d.frames(3);
    let ids = d.ids("deleteTracks.");
    for id in ["deleteTracks.video", "deleteTracks.audio", "deleteTracks.video.target", "deleteTracks.audio.target", "deleteTracks.ok", "deleteTracks.cancel"] {
        assert!(ids.iter().any(|i| i == id), "{id} missing: {ids:?}");
    }
    d.click("deleteTracks.video");
    d.click("deleteTracks.audio");
    d.click("deleteTracks.ok");
    assert!(d.ids("deleteTracks.").is_empty(), "dialog closed");
    let after = d.exec("sequence.inspect", json!({}));
    let (nv2, na2) = (after["video"].as_array().unwrap().len(), after["audio"].as_array().unwrap().len());
    assert!(nv2 < nv && na2 < na, "empty tracks deleted: {nv}->{nv2}, {na}->{na2}");
    // Cancel leaves the sequence alone
    d.ok("ui.menu.invoke", json!({"id": "sequence.deleteTracks"}));
    d.frames(3);
    d.click("deleteTracks.video");
    d.click("deleteTracks.cancel");
    assert_eq!(d.exec("sequence.inspect", json!({}))["video"].as_array().unwrap().len(), nv2);
}

#[test]
fn through_edits_are_marked_on_the_timeline() {
    let mut d = Driver::demo();
    d.exec("playhead.set", json!({"frame": 30}));
    d.exec("sequence.addEditAllTracks", json!({}));
    d.frames(3);
    assert!(d.ids("timeline.throughEdit.").is_empty(), "hidden until Show Through Edits");
    d.ok("ui.menu.invoke", json!({"id": "sequence.showThroughEdits"}));
    d.frames(3);
    let marks = d.ids("timeline.throughEdit.");
    assert!(marks.len() >= 2, "V1 + A1 marks: {marks:?}");
    d.exec("sequence.joinThroughEdits", json!({"all": true}));
    d.frames(3);
    assert!(d.ids("timeline.throughEdit.").is_empty(), "joined");
}

#[test]
fn markers_panel_colour_filter() {
    let mut d = Driver::demo();
    d.exec("markers.clearAll", json!({}));
    d.exec("markers.add", json!({"frame": 10, "name": "green"}));
    d.exec("markers.add", json!({"frame": 20, "name": "red", "color": "Rose"}));
    d.ok("ui.panel.show", json!({"panel": "Markers"}));
    d.frames(3);
    assert_eq!(d.ids("markers.row.").len(), 2);
    assert!(d.ids("markers.filter.").len() >= 7);
    d.click("markers.filter.Rose");
    d.frames(2);
    assert_eq!(d.ids("markers.row.").len(), 1, "red hidden");
    let m = d.ok("ui.menu.list", json!({}));
    let show_all = m.as_array().unwrap().iter().find(|i| i["id"] == "markers.showAllMarkerColors").unwrap().clone();
    assert_eq!(show_all["enabled"], json!(true));
    d.ok("ui.menu.invoke", json!({"id": "markers.showAllMarkerColors"}));
    d.frames(3);
    assert_eq!(d.ids("markers.row.").len(), 2);
}

#[test]
fn shift_semicolon_goes_to_the_next_gap() {
    let mut d = Driver::demo();
    d.exec("markers.markIn", json!({"frame": 48}));
    d.exec("markers.markOut", json!({"frame": 71}));
    d.exec("sequence.lift", json!({}));
    d.exec("playhead.set", json!({"frame": 0}));
    // the key a US layout sends for Shift+; is `:`
    d.harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Colon,
        physical_key: Some(egui::Key::Semicolon),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    });
    d.frames(3);
    let ph = d.exec("sequence.inspect", json!({}))["playhead"].clone();
    let want = d.exec("playhead.set", json!({"frame": 48}))["time"].clone();
    assert_eq!(ph, want);
}

fn first_movie(v: &Value) -> Option<u64> {
    match v {
        Value::Object(m) => {
            if let Some(id) = m.get("item").and_then(Value::as_u64)
                && m.get("type").and_then(Value::as_str) == Some("Movie")
            {
                return Some(id);
            }
            m.values().find_map(first_movie)
        }
        Value::Array(a) => a.iter().find_map(first_movie),
        _ => None,
    }
}

/// On-screen x of a clip's in and out edges in the timeline (not clipped to the panel).
fn clip_span(d: &mut Driver, clip: u64) -> (f64, f64) {
    let x = |d: &mut Driver, edge: &str| d.ok("ui.timeline.locate", json!({"clip": clip, "edge": edge}))["x"].as_f64().expect("x");
    (x(d, "in"), x(d, "out"))
}

#[test]
fn new_empty_sequence_fits_its_first_clip() {
    let mut d = Driver::demo();
    let project = d.exec("project.inspect", json!({}));
    let item = first_movie(&project).expect("a movie in the demo project");
    // an empty sequence fits its 1 s minimum; the clip placed into it afterwards is fitted again
    d.exec("file.newSequence", json!({"name": "Empty"}));
    d.frames(30);
    let clip = d.exec("timeline.place", json!({"item": item, "track": "V1", "seconds": 0}))["clips"][0].as_u64().expect("clip");
    d.frames(60);
    let (x_in, x_out) = clip_span(&mut d, clip);
    assert!(x_out < 1600.0 && x_out - x_in > 500.0, "clip spans x {x_in}..{x_out}: not fitted to the 1600 px window");

    // a zoom chosen while the sequence is still empty is kept
    d.exec("file.newSequence", json!({"name": "Zoomed"}));
    d.frames(30);
    d.ok("ui.set", json!({"timeline": {"pps": 300.0}}));
    d.frames(30);
    let clip = d.exec("timeline.place", json!({"item": item, "track": "V1", "seconds": 0}))["clips"][0].as_u64().expect("clip");
    d.frames(60);
    let secs = d.exec("sequence.inspect", json!({}))["video"][0]["items"][0]["duration"].as_f64().expect("duration") / 254_016_000_000.0;
    let (x_in, x_out) = clip_span(&mut d, clip);
    assert!(((x_out - x_in) - (secs * 300.0 - 4.0)).abs() < 2.0, "zoom not kept: {} px for {secs} s at 300 px/s", x_out - x_in);
}

#[test]
fn later_clips_keep_the_zoom_once_the_sequence_has_content() {
    let mut d = Driver::demo();
    let project = d.exec("project.inspect", json!({}));
    let item = first_movie(&project).expect("a movie in the demo project");
    d.exec("file.newSequence", json!({"name": "Grows"}));
    d.frames(30);
    d.exec("timeline.place", json!({"item": item, "track": "V1", "seconds": 0}));
    d.frames(60);
    // the editor zooms in; a second clip arriving must not re-fit (only the first clip into an
    // empty sequence does)
    d.ok("ui.set", json!({"timeline": {"pps": 300.0}}));
    d.frames(30);
    let end = d.exec("sequence.inspect", json!({}))["duration"].as_f64().expect("duration") / 254_016_000_000.0;
    let clip = d.exec("timeline.place", json!({"item": item, "track": "V2", "seconds": end}))["clips"][0].as_u64().expect("clip");
    d.frames(60);
    let q = d.exec("sequence.inspect", json!({}));
    let secs = q["video"][1]["items"][0]["duration"].as_f64().expect("duration") / 254_016_000_000.0;
    let (x_in, x_out) = clip_span(&mut d, clip);
    assert!(((x_out - x_in) - (secs * 300.0 - 4.0)).abs() < 2.0, "zoom changed: {} px for {secs} s at 300 px/s", x_out - x_in);
}

impl Driver {
    /// A clipboard shortcut as Windows and Linux deliver it: egui-winit sends Ctrl+C/X/V as
    /// `Event::Copy`/`Cut`/`Paste`, not as a key press (#199).
    fn clipboard_shortcut(&mut self, ev: egui::Event, modifiers: egui::Modifiers) {
        self.harness.input_mut().events.extend([egui::Event::ModifiersChanged(modifiers), ev]);
        self.frames(1);
        self.harness.input_mut().events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        self.frames(2);
    }

    /// Names of every clip in the active sequence's video tracks.
    fn video_clip_names(&mut self) -> Vec<String> {
        let q = self.app().session.active_sequence().unwrap();
        q.video_tracks.iter().flat_map(|t| t.items.iter().map(|i| i.name.clone())).collect()
    }
}

#[test]
fn ctrl_c_and_ctrl_v_copy_and_paste_clips() {
    let mut d = Driver::demo();
    d.ok("ui.set", json!({"focused": "Timeline"}));
    let q = d.app().session.active_sequence().unwrap().clone();
    let first = q.video_tracks[0].items[0].clone();
    let end = q.video_tracks.iter().chain(&q.audio_tracks).flat_map(|t| t.items.iter().map(|i| i.end())).max().unwrap();
    let copies = |d: &mut Driver| d.video_clip_names().iter().filter(|n| **n == first.name).count();
    let before = copies(&mut d);
    d.exec("timeline.select", json!({"clips": [first.id.0]}));

    d.clipboard_shortcut(egui::Event::Copy, egui::Modifiers::COMMAND);
    assert!(!d.app().session.state.clipboard.is_empty(), "Ctrl+C copies the selected clip");

    d.exec("playhead.set", json!({"time": end.0}));
    d.clipboard_shortcut(egui::Event::Paste(first.name.clone()), egui::Modifiers::COMMAND);
    assert_eq!(copies(&mut d), before + 1, "Ctrl+V pastes it at the playhead");
}

#[test]
fn ctrl_x_cuts_clips() {
    let mut d = Driver::demo();
    d.ok("ui.set", json!({"focused": "Timeline"}));
    let first = d.app().session.active_sequence().unwrap().video_tracks[0].items[0].clone();
    d.exec("timeline.select", json!({"clips": [first.id.0]}));
    d.clipboard_shortcut(egui::Event::Cut, egui::Modifiers::COMMAND);
    assert!(!d.app().session.state.clipboard.is_empty());
    assert!(d.app().session.active_sequence().unwrap().find_item(first.id).is_none(), "the cut clip leaves the timeline");
}
