//! Auto-save and crash recovery in OPFS (the desktop recovery journal, mapped to the browser).
//!
//! - While the project has unsaved changes, a snapshot is written every few seconds to
//!   `recovery/snapshot.fcproj` with `recovery/meta.json` saying it is dirty; once the project is
//!   saved (downloaded) the meta is marked clean.
//! - Imported media are copied to `media/<name>` (the browser streams the copy; nothing goes
//!   through wasm memory), so the snapshot's `/files/<name>` references resolve after a reload.
//! - At start-up a dirty snapshot is reopened (`?norecover` skips it, `?fresh` also skips the media).

use filmcraft_engine::Session;
use serde_json::{Value, json};

use crate::opfs;

const SNAPSHOT: &str = "recovery/snapshot.fcproj";
const META: &str = "recovery/meta.json";
/// Seconds between snapshots of a changing project.
const INTERVAL_S: f64 = 5.0;
/// Media larger than this are not copied to OPFS.
const KEEP_MAX: f64 = 4.0 * 1024.0 * 1024.0 * 1024.0;

#[derive(Default)]
pub struct Autosave {
    last_revision: u64,
    last_time: f64,
    last_dirty: Option<bool>,
}

impl Autosave {
    /// Called every frame.
    pub fn tick(&mut self, s: &Session, ctx: &egui::Context) {
        if !opfs::available() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let dirty = s.revision != s.saved_revision;
        if dirty {
            if s.revision != self.last_revision && now - self.last_time >= INTERVAL_S {
                opfs::write(SNAPSHOT.into(), filmcraft_format::encode(&s.project, false));
                let meta = json!({"dirty": true, "name": s.project.name, "revision": s.revision, "time": js_sys::Date::now()});
                opfs::write(META.into(), meta.to_string().into_bytes());
                self.last_revision = s.revision;
                self.last_time = now;
            } else if s.revision != self.last_revision {
                // the UI may go idle: wake up when the snapshot is due
                ctx.request_repaint_after(std::time::Duration::from_secs_f64((INTERVAL_S - (now - self.last_time)).max(0.05)));
            }
        } else if self.last_dirty != Some(false) {
            opfs::write(META.into(), json!({"dirty": false}).to_string().into_bytes());
        }
        self.last_dirty = Some(dirty);
    }
}

/// The unsaved snapshot of a session that ended without saving: (file name, bytes).
pub async fn load_snapshot() -> Option<(String, Vec<u8>)> {
    if !opfs::available() {
        return None;
    }
    let meta: Value = serde_json::from_slice(&opfs::read(META).await?).ok()?;
    if meta.get("dirty").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let bytes = opfs::read(SNAPSHOT).await?;
    let name = meta.get("name").and_then(Value::as_str).unwrap_or("Recovered").to_string();
    Some((format!("{name}.fcproj"), bytes))
}

/// Keep a copy of an imported file in OPFS (background).
pub fn keep_media(path: &str, file: &web_sys::File) {
    if !opfs::available() || file.size() > KEEP_MAX {
        return;
    }
    let Some(name) = path.strip_prefix("/files/").map(str::to_string) else { return };
    let blob: web_sys::Blob = file.clone().into();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = opfs::store_blob(&format!("media/{name}"), &blob).await {
            log::warn!("keeping {name} in OPFS: {e:?}");
        }
    });
}

/// Register the media kept in OPFS under their `/files/` paths; returns how many.
pub async fn restore_media() -> usize {
    let files = opfs::files_in("media").await;
    let n = files.len();
    for (name, f) in files {
        crate::fs::register_blob(&name, f.into());
    }
    n
}
