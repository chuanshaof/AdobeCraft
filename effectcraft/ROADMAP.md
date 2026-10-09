# Roadmap

- 2026-10-08: `effectcraft-cli render --out` resolves a relative path against the working directory, like `--project` (it used the project's folder, which doubled a path that already named it). Render Queue outputs set in a project are unchanged.
- 2026-10-08: Long-press a grouped toolbar button to choose its tools, including Horizontal/Vertical Type; releasing the hold keeps the menu open. Existing point/paragraph text creation is unchanged.

- 2026-10-06: Settings ▸ General ▸ Language persists English/Japanese menu labels (`general.language`); native UI reuses installed Japanese font fallback, with no bundled CJK font. Dialog and panel contents remain English.

EffectCraft aims to do what After Effects does, with the same panels, menus and behaviour, written
from scratch in Rust. This is where it stands. The milestones overlap; several are worked on at
once.

| | Milestone | State |
|---|---|---|
| M0 | Skeleton: crates, compositor, the After Effects style shell, control channel, `cargo xtask` | Done (web build: [docs/web.md](docs/web.md)) |
| M1 | Keyframes: temporal and spatial interpolation, Easy Ease, roving, velocity | Done |
| M2 | Compositing: 38 blend modes, track mattes, parenting, adjustment layers | Done |
| M3 | Project operations: settings dialogs, layer commands, `.ecproj`, undo, After Effects menu bar | Done (unsaved-changes prompts, Solid / Layer Settings, lock-aware layer commands, safe project files; View ▸ New Viewer still opens only the one locked viewer) |
| M4 | Preview: precomps, motion blur, cached playback | Done (Preserve frame rate / resolution when nested, one motion blur gate, RAM preview fixes, Cache Frames When Idle, motion blur of collapsed precomps and of animated shape / text content, content-keyed RAM preview frames) |
| M5 | Timeline depth: graph editor, keyframe clipboard and dialogs, time remapping, pick-whips, expression editor | Done |
| M6 | Shapes, masks and footage: shape operators, masks and the pen tool, video and image import | Done |
| M7 | 3D: 3D layers, cameras, lights, shadows, depth of field, 3D views and camera tools | In review |
| M8 | Expressions with the After Effects object model | Done |
| M9 | Text and effects: text animators, layer styles, 306 effects (all 298 of After Effects') incl. time and audio effects | Done (range, wiggly and expression selectors, per-character 3D, text on a path) |
| M10 | Export: render queue, H.264, ProRes, image sequences, GIF, audio | Done |
| M11 | Animation tools: audio playback, meters and waveforms, Lottie import and export, presets, Motion Sketch, Wiggler, Smoother | Done |
| M12 | Performance: layer cache, parallel and GPU compositing, disk cache, motion tracking (done); GPU versions of the remaining CPU-only effects | Mostly done |
| M13 | Puppet tools, paint, Roto Brush, motion tracking, a plug-in API, Timeline depth | Done (trained models, open source, downloaded on demand, in a swappable model module: MobileSAM for Roto Brush 2.0 / 3.0, MediaPipe Face Landmarker for face tracking) |
| M15 | The web app (WebAssembly, WebGPU) | Done: browser storage, Web Audio, renders and analyses in Web Workers, viewer frames and GPU effects in frame workers with their own WebGPU devices, the disk cache in the Origin Private File System, a storage manager, offline install ([docs/web.md](docs/web.md)) |
| M14 | Built for agents: MCP server, command-line tool, control channel; Settings, keyboard shortcut editor, auto-save and crash recovery ([docs/preferences.md](docs/preferences.md)) | Done |

## How far from full parity

There are two honest answers, and they are far apart. The full assessment, with the evidence, is
in [docs/gaps.md](docs/gaps.md).

- **Breadth: ≈ 99%.** Almost every After Effects feature exists: 89 of 92 catalogued features
  done and 3 partial, all 306 effects implemented, 280 of them on the GPU
  ([docs/parity.md](docs/parity.md)). This is our own checklist, graded by the agents that built
  the features.
- **Real use: ≈ 30–50% (estimated, not yet measured).** Whether someone who uses After Effects for
  a living can do client work in EffectCraft. What holds it back:
  - **Fidelity is unmeasured.** No test compares our output with After Effects itself, and users
    are still finding behaviour bugs in features marked done.
  - **After Effects projects can't be opened.** EffectCraft can't read `.aep` / `.aepx` files,
    and third-party After Effects plug-ins can't run.
  - **Reliability is uneven across platforms.** Early Linux users hit basic problems: viewer
    panning, panel docking, drag-and-drop import. A Windows user found the font menus offered only
    the bundled fonts (fixed 6 October: they list every installed font). Reports of 6 October
    covered drag and drop between panels, a Render Queue menu that ignored clicks, angle
    revolutions, and preview with audio and with little video memory (all fixed 7 October).
  - **AI tools are new.** Roto Brush's trained model (MobileSAM, M13.35) and face tracking's
    (MediaPipe Face Landmarker, M13.36) are optional downloads, not yet compared with After
    Effects.

## Where we're going

In priority order (details and "done" criteria in [docs/gaps.md](docs/gaps.md)):

| | Workstream | Why |
|---|---|---|
| G1 | Measure fidelity against After Effects: a corpus of test projects rendered and sampled in both apps, scored per feature | Turns every estimate into a measurement; a feature that behaves differently is not done |
| G2 | Real-user reliability on macOS, Windows and Linux; fix every open user issue | Basic workflows must work everywhere before more features matter |
| G3 | Stability: a green gate on the current toolchain, fuzzing of every input | A crash loses people's work |
| G4 | Open After Effects projects (`.aep` / `.aepx`, pending an owner decision on clean-room scope); relinking moved footage | The biggest barrier to switching |
| G5 | Performance benchmarks at 1080p and 4K on real-world-sized projects | Unknown today |
| G6 | Media depth: encoder efficiency, camera and phone formats | Files are larger than from mature encoders; camera formats are unverified |
| G7 | Learned models: Roto Brush (M13.35: MobileSAM) and face tracking (M13.36: MediaPipe Face Landmarker), open source, optional downloads, swappable model module; next, measure them | Quality against After Effects is unmeasured |
| G8 | Plug-in ecosystem on our WebAssembly plug-in API | After Effects plug-ins can't run here |
| G9 | Localisation, accessibility, user documentation | Reach beyond English-speaking power users |

Come tell us what matters most to you on [Discord](https://discord.gg/artcraft).
