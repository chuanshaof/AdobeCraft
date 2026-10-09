//! `perf.stats` through the control channel (headless app under `egui_kittest`): the control
//! method and the command id (what MCP `command_run` sends) both return the engine's decode
//! counters plus playback, frame-worker and UI timings.

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

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                assert_eq!(v["ok"], json!(true), "{method} {params}: {v}");
                return v["result"].clone();
            }
        }
        panic!("no reply to {method}");
    }
}

#[test]
fn perf_stats_reports_playback_frames_and_decode() {
    let mut d = Driver::demo();
    // let the Program monitor render a few frames on the workers
    for _ in 0..50 {
        d.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let v = d.ok("perf.stats", json!({}));
    for (section, keys) in [
        ("decode", &["cacheHitRate", "samplesDecoded", "decodeMs"][..]),
        ("playback", &["shown", "dropped", "dropRate"][..]),
        ("frames", &["jobs", "cancelled", "requestHitRate", "workers", "queued"][..]),
        ("ui", &["fps", "frameMs"][..]),
    ] {
        for k in keys {
            assert!(!v[section][k].is_null(), "{section}.{k} missing in {v}");
        }
    }
    assert!(v["frames"]["decodeMs"]["p95"].is_number() && v["frames"]["renderMs"]["mean"].is_number(), "{v}");
    assert!(v["frames"]["jobs"].as_u64().unwrap() > 0, "the monitor rendered frames: {v}");
    assert_eq!(v["playback"]["playing"], json!(false));
    // the same through the command id (what MCP `command_run` sends in bridge mode)
    let c = d.ok("engine.execute", json!({"command": "perf.stats"}));
    assert!(c["frames"]["workers"].as_u64().unwrap() >= 1 && c["decode"].is_object(), "{c}");
}
