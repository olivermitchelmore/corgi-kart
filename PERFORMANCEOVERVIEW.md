# corgiKart performance overview

A full pass over where corgiKart's frame time goes, what the research says about games like
this (Bevy, deferred rendering, real-time ray tracing with Solari and DLSS), and what was
changed. Written 2026-09-28. The ground rule: **nothing here may change how the game looks or
feels.** Ideas that would have changed the look are listed separately and weren't done.

`OPTIMIZEPERFORMANCE.md` has the measuring method and the earlier sky-dome fix. This file
builds on it.

## Summary

- **The game is GPU-bound wherever you play it.** With ray tracing at the user's window size
  (2542x1525, DLSS quality), the GPU runs at 99% for 29-33ms a frame. DLSS Ray Reconstruction
  is ~12.5ms of that, Solari's lighting passes ~11ms, and everything else is small. All of it
  is per-pixel work set by quality settings. Game-side waste in it is negligible.
- **The biggest possible win is outside the game: the GPU's power cap.** Even on AC, the RTX
  4050 is held to its **55W** default limit (its maximum is 140W). During a benchmark it draws
  54W at 99% load, so it's power-limited. Switching the platform profile to `performance`
  did **not** lift the cap. NVIDIA's Dynamic Boost service (`nvidia-powerd`) is installed but
  disabled, and it's the likely fix (untested). See "Power" below.
- **The CPU has a floor of 6-13ms a frame** (worse with ray tracing). It only shows at low
  internal resolutions or on a faster GPU. Adding extra raytracing instances costs about 1µs
  of CPU each per frame. Removing ~2,000 of the existing ones made no measurable difference,
  because the render thread isn't the bottleneck at today's instance counts.
- **Changes made, all invisible.** None of them moves the frame rate at your settings; the
  GPU decides that:
  1. Static scenery is merged into one raytracing mesh per material. Instances drop 53% on
     Circuit and 72% on Meadow. That's headroom, not a measured speed-up.
  2. The sponsor-board atlas camera renders once, then switches off.
  3. The item-icon camera only renders while there's an item in the slot.
  4. The benchmark now reports meshes, materials, raytracing instances, lights and cameras.
- **Found, not fixed (it would change the look):** hidden boost flames still glow in the
  ray-traced scene. See "Bugs found".

## The machine and how it was measured

Lenovo 83DG laptop: RTX 4050 Laptop GPU (6GB), i7-14650HX, Linux + Hyprland. Screen 2560x1600
at 165Hz. Measured headless with the DLSS build, `--autodrive --bench 8`, on all four tracks.
The process for A/B comparisons:

- Keep a copy of the baseline binary **with the `libnvidia-ngx-dlss*.so` files next to it**.
  Without them DLSS silently turns off ("DLSS is not supported on this system" in the log) and
  the numbers are meaningless.
- Build the change in a separate git worktree (another agent was editing the main tree).
- Run baseline and change interleaved, and check `nvidia-smi --query-compute-apps` first so
  no other corgiKart is running. Log the power source with every run: it changed halfway
  through one session and invalidated a whole sweep.

CPU-bound numbers (small window sizes) are sensitive to anything else using the CPU, such as a
`cargo build` in another terminal. GPU-bound numbers are much steadier.

### Baselines on AC power (balanced profile, 55W GPU cap), before these changes

| fps | Prism | Circuit | Lagoon | Meadow |
|---|---|---|---|---|
| RT, DLSS quality, 1920x1080 | 52.7 | 56.6 | 56.3 | 60.2 |
| RT, DLSS quality, 2542x1525 | 30.6 | 34.5 | 34.1 | 36.6 |
| Standard renderer, 1920x1080 | 83.2 | 68.7 | 91.1 | 77.3 |

For comparison, `OPTIMIZEPERFORMANCE.md` has the battery numbers (e.g. Prism RT 1080p 42 fps,
2542x1525 24 fps).

## Power

`/sys/firmware/acpi/platform_profile` reads `balanced` (choices: low-power, balanced,
performance, max-power, custom). `nvidia-smi -q -d POWER` shows the current, requested and
default power limit all at 55W, with a max of 140W. During a ray-traced bench the GPU sits at
99% utilisation and 54-55W, at about 2.1GHz.

The profile was later switched to `performance`, and **the cap stayed at 55W**. RT at
2542x1525 measured 29.9-34.7 fps, no better than on `balanced`. So the platform profile isn't
what holds the GPU back.

NVIDIA laptop GPUs only go above their default limit with **Dynamic Boost**, which on Linux
is the `nvidia-powerd` service. Here it's installed (`/usr/bin/nvidia-powerd`) but
`disabled` and inactive. Enabling it is the most promising free win, but it's untested
because it needs root:

```sh
sudo systemctl enable --now nvidia-powerd
nvidia-smi -q -d POWER | grep -i "power limit"   # should rise above 55W under load
```

Then re-run the baselines. With a bigger GPU budget the CPU floor (below) may start to matter.

## Where the frame goes

### Ray tracing (what the user plays)

Circuit, 2542x1525, DLSS quality, AC: 34.5 fps, 29ms. Passes with GPU timings:

| Pass | GPU ms |
|---|---|
| dlss_ray_reconstruction | 12.6 |
| solari diffuse_indirect_lighting | 3.4 |
| solari direct_lighting | 3.0 |
| solari world_cache | 2.4 |
| solari specular_indirect_lighting | 1.7 |
| early deferred prepass | 0.6 |
| bloom | 0.5 |
| everything else | < 0.6 total |
| not timed | ~4.3 |

The GPU is at 99% load, so the untimed ~4ms is GPU work too (DLSS and Solari internals,
buffer uploads, the TLAS build), not idle time.

### Standard renderer

Circuit 1080p: SSR 3.1ms and SSAO 0.9ms are the biggest timed passes. Most of the frame isn't
timed: Bevy's `deferred_lighting` pass and the shadow passes have no timing spans. The deferred
lighting shades every pixel against the clustered point and spot lights (16-38 per track). The
shadow passes render four 4096² sun cascades out to 260m.

### CPU floor: frame time against resolution (AC)

| | 320x180 | 960x540 | 1920x1080 |
|---|---|---|---|
| Meadow, standard | 8.0ms | 8.3ms | 12.9ms |
| Circuit, standard | 9.2ms | 9.7ms | 14.5ms |
| Circuit, RT | 12.6ms | 12.0ms | 17.6ms |

Below ~1080p, frame time stops falling with resolution: that's the CPU (main thread plus
Bevy's render thread). Ray tracing raises the floor by several ms, and Circuit, with the most
raytracing instances, has the highest. At the user's resolution the GPU needs 29ms, so the
floor is hidden. It would show with a faster GPU budget (see Power), `--dlss performance` at
smaller windows, or the standard renderer on the 165Hz screen.

`perf` shows no hot spot. The time is spread over Bevy's executor, visibility and frustum
culling, mesh collection for GPU building, UI layout (taffy), and Solari's scene building.

## Research: how Bevy and Solari spend time

Read from the Bevy 0.19.1 source in `~/.cargo/registry` (research by a Sonnet subagent,
checked here).

- **Solari rebuilds its whole scene every frame.** `bevy_solari/src/scene/extract.rs`
  re-inserts mesh, material and transforms for every `RaytracingMesh3d` entity each frame.
  `scene/binder.rs` (`prepare_raytracing_scene_bindings`) then walks every instance again: it
  looks up its BLAS, vertex and index slices and material, fills fresh storage buffers,
  creates a new TLAS (`PREFER_FAST_TRACE`, full build, no refit), and builds a new bind group.
  None of this checks for changes, so its cost scales with instance count even when nothing
  moves.
- **Measured:** adding 2,000 and 6,000 off-screen cubes to Meadow at 320x180 with RT took the
  frame from 9.3ms to 11.4ms and 14.9ms, about **1µs per instance**. With the standard
  renderer the same cubes cost nothing measurable. At 1080p and above they cost nothing either
  way, because the GPU is the limit there.
- **BLASes are per mesh asset**, built once with `PREFER_FAST_TRACE | ALLOW_COMPACTION`, so
  instancing one mesh many times doesn't cost BLAS memory. Merging does duplicate geometry.
- **Every emissive instance is a light source.** Solari picks a light *source* uniformly, then
  a triangle in it. Merging emissive meshes would change how samples are spread, and so the
  noise. It's a visual change, so it wasn't done (see also `OPTIMIZEPERFORMANCE.md`).
- **Solari ignores `Visibility`.** Its extraction query has no visibility filter, so hidden
  meshes are still traced, and still emit light.
- **`StandardMaterialAssets` clones every `StandardMaterial` into the render world** whenever
  `Assets<StandardMaterial>` changes. The game changes some material every frame (boost pad
  and rainbow ramp UVs, underglow, item-box cores, water UVs), so that happens every frame. With 100-250
  materials it's cheap, but it's a reason not to multiply materials.
- **Solari already skips Bevy's deferred lighting pass** (`SkipDeferredLighting`), so nothing
  is shaded twice.
- **Render-to-texture cameras** are skipped entirely during extraction when
  `Camera::is_active` is false (`bevy_render/src/camera.rs`), and the target keeps its
  contents.
- **General hardware RT guidance** (NVIDIA and Vulkan RT best practices): prefer fewer, larger
  BLASes for static geometry over many small instances. TLAS build and traversal both scale
  with instance count and with how much instance bounding boxes overlap.
- **Build settings** are already where Bevy recommends: thin LTO, `codegen-units = 1`,
  dependencies at opt-level 3. Fat LTO or `target-cpu=native` would only help the CPU, which
  isn't the limit.

## Changes made

All in commits on the `perf` branch, each building on its own.

### 1. Static scenery shares raytracing meshes (`gfx::add_raytracing_meshes`, `gfx::rebuild_merged_raytracing`)

Every opaque mesh used to get its own `RaytracingMesh3d`. A census of the instances showed
most of them are static scenery (trees, stands, fences, props), plus the crowd (animated, so
left alone) and the karts (moving). Now a scenery part is baked in world space into one
raytracing mesh per material if:

- it's under a `Scenery` root,
- nothing in its hierarchy is animated (`Spectator`, `FerrisWheel`, `Drifter`, `Bob`,
  `Floating`, `StartLight` and the rest of `ambient.rs`'s markers, or `NoRtMerge`),
- its material isn't emissive (lights stay separate, see above),
- it isn't a `RaytraceSink` floor, and its mesh has at most 20k vertices.

Why it looks identical:

- The rasterised entities aren't touched. Only the raytracing scene changes.
- Solari transforms an instance's normals and tangents with the plain model matrix, then
  normalises after interpolating. The bake does exactly the same, so the shading matches. The
  remaining difference is float rounding.
- Checked with ray-traced screenshots on all four tracks, including the reflections in
  Lagoon's mirror road and Circuit's wet asphalt. Baseline against merged differs no more
  than two baseline runs differ from each other (DLSS/ReSTIR noise).

Safety net: if a merged part ever moves, or changes mesh or material, it's ejected into its
own instance and its group is rebaked. A despawned part triggers a rebake too. So a new
animation added later can't leave a stale copy in the raytraced scene, only a one-frame lag
the first time it moves. Mark a new animated thing's root `NoRtMerge`, or add its marker to
`gfx::Animated`, to skip that.

| Raytracing instances | Prism | Circuit | Lagoon | Meadow |
|---|---|---|---|---|
| before | 1255 | 3768 | 1975 | 2001 |
| after | 1175 | 1753 | 1339 | 557 |

**Measured: no speed-up.** An A/B on AC (performance profile, ray tracing, DLSS build):
e790a1b (before all the changes here) against 216ea7d (after). The 320x180 runs were ordered
A-B-B-A to cancel drift. Mean frame time:

| | Circuit | Meadow | Lagoon | Prism |
|---|---|---|---|---|
| 320x180, before | 7.70ms | 6.13ms | 6.85ms | 6.77ms |
| 320x180, after | 7.71ms | 6.25ms | 6.58ms | 6.34ms |
| 2542x1525, before | 31.50ms | 28.84ms | 29.39ms | 33.40ms |
| 2542x1525, after | 31.41ms | 28.81ms | 29.41ms | 33.36ms |

The same binary varies by ±0.5ms between runs at 320x180, so every difference here is noise.
At 2542x1525 the results are identical, as expected when GPU-bound. The extra-instance
experiment (+2,000 cubes → +2.1ms) had predicted ~2ms off Circuit. The likely explanation:
extra instances load Bevy's render thread until it becomes the critical path, but at
today's counts it isn't, so removing instances only adds slack. The merge is kept because it
changes nothing visible and keeps the ray-tracing CPU cost from growing with scenery. If it
isn't worth ~200 lines to you, reverting b476e24 is safe.

Cost: merged meshes duplicate geometry once per placed copy (the 20k-vertex cap bounds it).
Their CPU-side copies are dropped after upload (`RenderAssetUsages::RENDER_WORLD`). The
Bevy mesh count rose by ~20. GPU memory wasn't measured.

### 2. The sponsor-board atlas renders once (`signs::freeze_signs`)

The atlas (Bevy UI rendered into a 1024x896 texture) never changes, but its Camera2d rendered
it every frame. It now reads the atlas back every half second. When two captures in a row
match and aren't blank, it switches the camera off (`is_active = false`), and the texture
keeps its contents. Waiting for two identical captures copes with slow shader compiles and
glyph loading, which a fixed delay wouldn't. Measured: about 0.1ms a frame with the standard
renderer (Meadow 1080p, 12.88 → 12.76ms).

### 3. The item icon only renders when needed (`icons::animate_icons`)

The icon studio is a whole extra deferred 3D view: prepass, lighting, FXAA, and with the
standard renderer its own four sun shadow cascades. It rendered every frame, including in the
menu and with an empty slot. It now renders while an item (or the roulette) is showing, plus 3
frames after the slot empties so the image is cleared, then switches off. Measured: ~0.15ms a
frame with the standard renderer (12.88 → 12.71ms), ~0.1ms with RT. (This is also the "FXAA
pass alongside DLSS" in `OPTIMIZEPERFORMANCE.md`: it belonged to this camera.)

### 4. Benchmark counters (`game::BenchCounts`)

`--bench` now also prints mesh entities (and how many are visible), raytracing instances, mesh
and material assets, point and spot lights, and active cameras. That's what showed the
instance counts above.

## Bugs found (not fixed: fixing them changes the look)

- **Hidden boost flames are still in the ray-traced scene.** `race::animate_effects` hides each
  kart's two exhaust flames with `Visibility::Hidden` when it isn't boosting. Solari ignores
  visibility, and the flame material (`models::glow`, emissive 8.0 × 250 nits) is opaque. So
  16 bright orange light sources sit behind the karts all race: they're visible in reflections
  and light the road, and they take light samples from everything else. The fix is to
  add/remove their `RaytracingMesh3d` (or scale them to zero) along with the visibility. It
  would make the image *more* correct, but it changes it, so ask first.
- Any other `Visibility::Hidden` mesh is also still traced. Item boxes shrink to scale 0 while
  hidden, so they're fine.

## Ideas that change the look (not done; ask first)

- `--dlss balanced` / `performance`: +25-60% fps at 2542x1525 (see `OPTIMIZEPERFORMANCE.md`).
- Standard renderer: SSR (24 linear + 6 bisection steps, the biggest timed pass), SSAO, the
  4096² shadow map with 4 cascades, and point and spot light ranges. Changing a light's range
  changes its falloff curve in Bevy, so even that shows.
- Fewer emissive instances (e.g. merging neon) would sample lights better, but changes the
  noise.
- Merging the crowd into one mesh would remove ~600-1,050 instances, but loses each corgi's
  own hop and sway unless it's rebuilt as a vertex-shader animation.
- A frame cap or lower swapchain latency would help feel (input lag) rather than throughput;
  it needs a windowed test with the user.

## Checked and ruled out

- **More entities in the standard renderer:** 6,000 extra off-screen meshes cost nothing
  measurable. Bevy's GPU preprocessing and culling handle this scene size easily.
- **Duplicate materials breaking batching:** there are only 100-250 materials per track,
  and StandardMaterial is bindless in 0.19.
- **Bevy's own deferred lighting running under Solari:** it doesn't.
- **Build profile tweaks:** they only help the CPU, which isn't the limit at the user's
  settings.
- **`trace_chrome` for per-system CPU timing:** the feature needs `profiling-procmacros
  1.0.18`, which isn't in the local crates index, and changing Cargo.lock for a diagnostic
  wasn't worth it. Try it on a machine with network access to the index to split the CPU
  floor per system.
