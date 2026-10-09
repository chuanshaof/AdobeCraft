# Monitor view options

The View menu and each monitor's wrench menu set how the Source and Program monitors show their
picture. It is all UI state (`MonitorView` in `crates/ui-egui/src/state.rs`, serde): agents read
it with `ui.inspect` (`ui.program`, `ui.source`) and set it with `ui.set {"program": {…}}`, or run
the `view.*` UI commands. Commands act on `params.monitor` (`"program"` / `"source"`), else on the
focused monitor (Program by default). Code: `crates/ui-egui/src/panels/monitor_view.rs`.

| Command | Menu | Effect |
|---|---|---|
| `view.playbackRes.<full\|half\|quarter\|eighth\|sixteenth>` | View ▸ Playback Resolution | render scale while playing |
| `view.pausedRes.<…>` | View ▸ Paused Resolution | render scale while stopped (default Full) |
| `view.highQualityPlayback` | View ▸ High Quality Playback | play at the paused resolution when it is higher |
| `view.display.<composite\|alpha\|red\|green\|blue>` | View ▸ Display Mode | composite, or one channel as greyscale (colour channels premultiplied) |
| `view.display.multicam` | Display Mode ▸ Multi-Camera | the Program's multi-camera view (`multicam.toggleView`) |
| `view.display.audioWaveform`, `view.display.videoAndWaveform` | Display Mode ▸ Audio Waveform / Video and Audio Waveform Split | Source Monitor: the clip's waveform (click to move the source playhead); audio-only clips always show it |
| `view.display.comparison` | Display Mode ▸ Comparison View | Program: the reference frame (left) beside the current frame; the reference starts at the playhead and is stepped or reset with the buttons under it, or `view.compare.setReference {time\|seconds}` |
| `view.magnification.<fit\|10\|25\|50\|75\|100\|150\|200\|400\|800\|1600>` | View ▸ Magnification and the zoom dropdown | 100% = one frame pixel per screen pixel; scroll or drag with the Hand tool to pan |
| `view.showRulers`, `view.showGuides`, `view.lockGuides`, `view.clearGuides` | View | toggles take `{"enabled": bool}` |
| `view.addGuide` | View ▸ Add Guide… | dialog, or `{"orientation": "vertical\|horizontal", "position": px}` |
| `view.snapInProgramMonitor` | View ▸ Snap in Program Monitor | graphic moves snap to the frame edges / centre and the guides (6 pt) |
| `view.safeMargins` | Guide Templates ▸ Safe Margins | action- and title-safe boxes |
| `view.guideTemplates.save`, `.manage`, `.apply`, `.delete` | Guide Templates ▸ Save Guides as Template… / Manage Guides… | templates are stored in the user preferences (`guides.templates`) |

Guides are in frame pixels. Drag out of the top ruler for a horizontal guide or the left ruler for
a vertical one; drag a guide to move it (unless locked) and drop it outside the picture to remove it.

Automation ids: `<monitor>.zoom`, `<monitor>.zoom.<level>`, `<monitor>.settings.<command suffix>`
(wrench menu items, e.g. `program.settings.display.alpha`), `<monitor>.ruler.top|left`,
`<monitor>.guide.<n>`, `program.compare.reference|prev|next|set`, `source.waveform`, and in the
dialogs `guides.add.vertical|horizontal|position|ok|cancel`, `guides.save.name|ok|cancel`,
`guides.manage.row.<n>|apply|delete|close`.
