# corgiKart notes for Claude

A kart racer starring corgis, in Rust + Bevy 0.19. See README.md for the layout of `src/`.

## Building

- Normal: `cargo build --release`. Tests: `cargo test --release`.
- With DLSS (what the user plays; RTX 4050 laptop GPU):
  `DLSS_SDK=~/sdk/DLSS VULKAN_SDK=/usr cargo build --release --features dlss`
- Ray tracing without DLSS is covered in speckles, so judge RT visuals and speed on the DLSS build.

## Running without bothering the user

Never pop up a game window to check something. Use `--headless` (offscreen, no window, no
sound):

```sh
# benchmark: frame-time summary plus the most expensive GPU passes
./target/release/corgikart solo --autodrive --headless --track lagoon --raytracing --bench 6
# screenshot (quits once saved)
./target/release/corgikart solo --autodrive --headless --track lagoon --raytracing \
    --size 1280x720 --screenshot shot.png --shot-delay 14
```

- The Bash tool's shell isn't bash: wrap `for` loops in `bash -c '...'`.
- Autodrive races are deterministic, so screenshots at the same `--shot-delay` are close to
  comparable (a frame or two apart). `--view` gives a fixed camera for exact comparisons.
- The first run after a rebuild can capture before shaders compile; the bench has a warm-up.
- `--players 2` (up to 4) gives split screen, for screenshots and benchmarks alike.
- `--item <name>` keeps handing your kart an item and `--cpu-item <name>` the CPU racers
  (`zoomies`, `waterballoon`, `bone3` for three), to screenshot or benchmark one. With
  `--autodrive` your kart uses it as a CPU racer would. To be on the receiving end of something
  that only hits karts ahead (mud pie, big woof), add `--item squirrel` to get to the front.
- The track editor: `./target/release/corgikart editor --headless --size 1600x900 --screenshot
  shot.png --shot-delay 3` (add `--track custom:<file>` to open a saved track). It can't be
  clicked headless; `editor::tests` drives its mouse tools directly.
- Test custom tracks with `XDG_CONFIG_HOME=<scratch dir>` so they don't land in the user's
  `~/.config/corgikart/tracks` (copy `settings.txt` over first). `--track custom:<file>` races
  one.
- `--record out.mp4 --record-from <secs> --record-secs <secs>` saves a 60fps video with sound
  (headless, fixed timestep, so it's repeatable): use it when a screenshot can't show the
  motion. `--cam dist,height,angle` and `--no-hud` film it. `trailer/make.py` builds the
  trailer from such clips (`--draft` to check the cut quickly); `trailer/clips` is its cache.
- Menu input saves `~/.config/corgikart/settings.txt`; back it up if a test could touch it.

## Performance

Read OPTIMIZEPERFORMANCE.md before any performance work: measurement method, what was fixed,
what was ruled out, and leads not yet followed. Keep it up to date.

Benchmark before and after any change that could cost frames, on all six tracks with
`--raytracing`. The user's screen is 2560x1600 at 165Hz, so a tiled window is about
`--size 2542x1525`; that's twice the pixels of 1080p and ray tracing scales with it.

Check `cat /sys/class/power_supply/A*/online`: on battery (0) the laptop GPU is power-capped to
~35W and half its clock, so only compare numbers taken on the same power source. Baselines on
battery, DLSS quality, after the sky fix:

| | 1080p | 2542x1525 |
|---|---|---|
| Prism | 42 fps | 24 fps |
| Circuit | 45 fps | |
| Lagoon | 46 fps | 27 fps |
| Meadow | 50 fps | |

At 2542x1525 DLSS Ray Reconstruction alone is ~16ms and Solari's passes ~20ms. `--dlss
performance` gets ~40 fps there. The CPU side is ~8-10ms a frame, spread thinly over Bevy's
own systems (3.5-6.5k entities), so it only matters above ~100 fps.

Things learned about Solari (Bevy's ray tracer):

- Never let rays start far away. The sky light is a dome 80km out; if it can reflect, every
  reflection that reaches it fires a shadow ray back across the world, and rays from that far
  are imprecise enough to hit most bounding boxes. That made the specular pass 10-40x slower.
  The dome is a black perfect mirror (`gfx::sky_light_material`) so paths end on it.
- Solari's cost is mostly per-pixel: it scales with resolution far more than with scene size.
- An emissive mesh can have at most 65,535 triangles (Solari panics over that), and each
  emissive mesh is one light source for its sampling: merge a track's lamps into a few meshes.
- The camera's exposure is fixed at daylight. Lights that have to light a scene by themselves
  (Tbilisi's lamps, the jungle hall's braziers) need real brightness: an emissive over 100 is
  taken as nits and left alone, anything lower is multiplied by `EMISSIVE_TO_NITS`. `Look::
  sky_light` scales how much the sky lights the scene without changing how it looks.
- Far scenery (the jungle's mesas, over a kilometre out) shimmers if rays light it. Paint it
  unlit and blended with `NoRaytrace`. The sky dome is blended too and centred on the camera,
  so its material has a large negative `depth_bias` to be drawn before the rest (clouds too).
- A surface seen from behind its normal is black (Solari's BRDF returns nothing), so leaves
  are double-sided and their undersides only ever get bounce light.

## Adding a track

Add its `TrackId` and `Theme` (the compiler lists every match to extend), its pieces and
constructor in `sim/track.rs`, a `Look` in `look.rs`, and a scenery module that takes a
`scene::Build` like `jungle.rs` and `tbilisi.rs` do, dispatched from `scene::sync_track_scene`.
`trackmesh.rs` has the walls, embankments, bridge decks, pads, ramps and fords to call, and
`shapes.rs` the shapes. If the scenery can cope with any layout (optional `find_mark`s, set
pieces placed with `find_spot`), add the theme to `Theme::CUSTOM` and the editor offers it.
