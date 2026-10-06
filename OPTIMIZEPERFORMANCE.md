# Optimising corgiKart's performance

Notes for whoever works on frame rate next: how to measure, what was found and fixed, what
was tried and ruled out, and leads that haven't been followed up yet. Written 2026-09-28, updated 2026-09-29.
`PERFORMANCEOVERVIEW.md` has the later full pass: AC numbers, the GPU power cap, the CPU
floor, how Solari's cost scales with instances, and the changes that came out of it.

The user's rule: **don't make the game look or feel worse without asking first.** Every idea
below that trades quality for speed is marked as such.

## The machine

- Laptop: RTX 4050 Laptop GPU (6GB), i7-14650HX (24 threads), 16GB RAM, Linux + Hyprland.
- Screen: 2560x1600 at 165Hz. A tiled game window is about 2542x1525, twice the pixels of
  1080p. Ray tracing cost scales with pixels, so benchmark at that size as well as 1080p.
- **Battery matters.** On battery the GPU is power-capped (~35W of 140W, ~1665 of 3105 MHz,
  throttle reason "SW power cap"). Check `cat /sys/class/power_supply/A*/online` (0 = battery)
  and only compare numbers taken on the same power source. All numbers below are on battery.
- The user plays with ray tracing on, on the DLSS build. Optimise that path first.

## How to measure

Build the DLSS binary (a plain `cargo build --release` or `cargo test --release` can replace it):

```sh
DLSS_SDK=$HOME/sdk/DLSS VULKAN_SDK=/usr cargo build --release --features dlss
```

Benchmark headless. There's no window (never pop one up on the user), no sound, and frames
run back to back, uncapped:

```sh
./target/release/corgikart solo --autodrive --headless --track prism --raytracing \
    --size 2542x1525 --bench 6
```

`--bench N` waits 8s (shader compilation, countdown), records N seconds of frame times, then
prints:

- fps, mean, p50/p95/p99/max frame time, and frames over twice the median (hitches)
- the entity count
- the 30 most expensive GPU passes, from Bevy's `RenderDiagnosticsPlugin`, which the bench
  enables automatically

To compare against an older build, keep its binary in `target/release` (or copy
`target/release/libnvidia-ngx-dlss*.so*` next to it). Anywhere else it can't find the DLSS
library, logs "DLSS is not supported on this system" and quietly runs ray tracing without
DLSS, at full resolution and about 30% slower, which looks like a big win for the new build.

Other useful flags: `--no-raytracing`, `--dlss quality|balanced|performance`,
`--size WxH`, `--screenshot out.png --shot-delay N` (quits once saved), and
`--view ex,ey,ez,tx,ty,tz` for a fixed camera. The Bash tool's shell isn't bash, so wrap loops
in `bash -c '...'`.

Check visual changes with before/after screenshots. Autodrive races are deterministic, so the
same `--shot-delay` gives nearly the same frame. `Read` the PNGs to compare them.

For CPU profiling, `perf` works (`nsys`, RenderDoc and Tracy aren't installed). `-D 9000`
skips startup:

```sh
perf record -D 9000 -F 999 -g --call-graph dwarf,32768 -o perf.data \
    ./target/release/corgikart solo --autodrive --headless --track circuit --no-raytracing --size 960x540 --bench 5
perf report -i perf.data --no-children --sort sym --stdio
```

### Blind spots in the GPU pass list

Not every pass has a timing span, so the listed passes don't add up to the frame time:

- Ray traced: ~4ms unaccounted at 2542x1525 (41.5ms frame, 37.4ms listed). Probably Solari's
  per-frame TLAS build and other setup outside the timed nodes.
- Standard renderer: **~10ms unaccounted** at 2542x1525 (21.8ms frame, 11.4ms listed). It
  grows with resolution, so it's likely the deferred lighting pass, which isn't listed. The
  directional shadow cascades aren't listed either (4096 map, 4 cascades to 260m). See the
  leads below.

## Found and fixed: rays starting on the sky dome (commit 1c6e569)

**Symptom.** With ray tracing, `solari_lighting/specular_indirect_lighting` took 20-40ms, half
the frame or more. Meadow, the simplest track, was the slowest at 18 fps at 1080p.

**Cause.** Solari only lights with the sun and emissive meshes, so `gfx::spawn_sky_light` adds
an emissive dome 80km out (`SKY_LIGHT_RADIUS`) that only exists in the ray-tracing scene. Its
material had the defaults: perceptual roughness 0.5, so not a mirror, and reflectance 0.5.
Solari's specular pass (`bevy_solari/src/realtime/specular_gi.wgsl`, `trace_glossy_path`)
traces a path for every glossy pixel (roughness < 0.4). When a bounce hit the dome, it did
next-event estimation there: a shadow ray from the dome, 80km out, back to a random light in
the scene. Rays starting that far away are so imprecise that they hit a huge number of
bounding boxes as they cross the scene.

**Fix.** `gfx::sky_light_material` makes the dome a *black perfect mirror* (metallic 1.0,
perceptual roughness 0.0). Solari skips next-event estimation on perfect mirrors
(`roughness <= MIRROR_ROUGHNESS_THRESHOLD (0.001) && metallic > 0.9999`). A black mirror's
reflection is zero, so Russian roulette ends the path there. Only the dome's emission was ever
visible, so the image is unchanged. This was checked with screenshots on all four tracks,
including the Lagoon mirror road.

**Result.** The specular pass dropped from 20-40ms to 1-5ms.

| RT, 1080p | Before | After |
|---|---|---|
| Meadow | 18 fps | 50 fps |
| Lagoon | 28 fps | 46 fps |
| Circuit | 27 fps | 45 fps |
| Prism | 32 fps | 42 fps |

**The experiments that pinned it down** (Meadow or Lagoon, RT, 1080p):

| Experiment | Specular pass | Conclusion |
|---|---|---|
| No sky dome at all | < 1.5ms | the dome is the cause |
| Dome radius 80km / 40km / 20km / 10km / 5km | 24.6 / 10.1 / 5.0 / 3.1 / 2.5ms | cost grows with distance |
| 80km dome with 16x fewer triangles | unchanged | not its triangle count or BVH |
| Dome rough (1.0) and non-reflective | unchanged | not bounces off the dome |
| Dome a black perfect mirror | 1.45ms | the NEE shadow rays from the dome |

DI's shadow rays towards the sun also travel 100km, but they *start* on the track and are
cheap. What makes a ray expensive is where it starts, not how long it is.

**Rule for the future:** nothing in the ray-traced scene should make Solari start rays far
from the track. Any huge, far-away emissive or reflective helper geometry should be a black
perfect mirror.

## Found and fixed: full-detail meshes at any distance (level of detail)

**Symptom.** With the standard renderer the frame was 21.7ms (Circuit, 2542x1525) and the
shadow cascades alone cost ~5.8ms (a run with the sun's shadow maps off: 15.8ms). Shrinking the
shadow map to 2048 changed nothing, so it wasn't fill rate: it was geometry.

**Cause.** `--bench` now prints triangle counts. Circuit sent **7.5M triangles** a frame to
the main view, and the shadow cascades draw the casters again. The sculpted meshes are cut with
1-2cm cells, so a front tyre is 47k triangles, a crowd corgi 4.3k, and a kart about 330k in
all (three quarters of that in its wheels). Eight karts and 680 crowd corgis were drawn like that
at any distance, when at 50m a cell is under a pixel.

**Fix (`render/lod.rs`).** Every kart part, the driver, its head, and the crowd corgis are meshed
three times, at 1x, 2x and 4x cell size (about 1x, 1/4 and 1/16 the triangles). `update_lods`
swaps the entity's `Mesh3d` by distance to the nearest player camera, with 6% hysteresis:
karts at 20m and 55m, crowd corgis at 45m and 110m (at the 70-86 degree field of view the game
uses, the coarser cells are about a pixel across at those distances). Only the rasterised
mesh changes; the raytracing proxies are made from the full mesh. Screenshots of Circuit before
and after, zoomed 4x on the stands and the karts ahead, look the same.

Circuit went from 7.5M to 3.1M visible triangles.

| Standard renderer, 2542x1525, AC | Before | After |
|---|---|---|
| Circuit | 46.2 fps (21.7ms) | 54.8 fps (18.2ms) |
| Prism | 54.4 fps (18.4ms) | 63.0 fps (15.9ms) |
| Lagoon | 62.8 fps (15.9ms) | 72.9 fps (13.7ms) |
| Meadow | 51.7 fps (19.4ms) | 58.7 fps (17.0ms) |

The shadow cascades now cost ~3.1ms on Circuit (was 5.8) and ~2.3ms on Prism. With ray
tracing (DLSS quality, 2542x1525) the early prepass halves (1.6 → 0.8ms), which is worth ~1.3ms
on Circuit and ~0.9ms on Prism: 31.4 → 32.7 fps and 29.8 → 30.6 fps. Ray tracing is bound by
per-pixel work, so geometry can't do more for it.

**Not LOD'd:** trees, props and the track itself (720-4.6k triangles each, and static scenery is
merged into the raytracing scene, so swapping a mesh would eject it: see `PERFORMANCEOVERVIEW.md`),
and four ~99k-triangle single meshes that could be looked at next (`--bench` lists nothing per
mesh now, so check triangle counts by hand).

**Rule for the future:** a new sculpted model should be meshed with `LOD_CELL_SCALE` and spawned
with `Lods::lod`, not as a single dense mesh.

## Checked and not done

- **Frustum culling** is Bevy's default and works: Circuit has 3,951 mesh entities and 563 are
  in view (2,034 counting the ones the shadow cascades need).
- **Occlusion culling** (`bevy::render::occlusion_culling::OcclusionCulling`): Bevy's docs say it
  is incompatible with deferred shading (`DeferredPrepass`), which this renderer uses, and the
  tracks are open, so there is little to occlude anyway.
- **Cascade shadow maps** (standard renderer only; Solari traces its own shadows). Cascades
  are 4096², so the cost is the caster geometry and one pass per cascade, not the map size.
  Measured on AC at 2542x1525 after the LOD change (Circuit / Prism fps): 4 cascades to 260m
  55.4 / 63.2 (what it was), **3 cascades to 260m 57.2 / 65.2 (now)**, 3 to 200m 57.2 / 65.1,
  2 cascades to 150m 59.6 / 67.3, 4 to 150m 55.4 / 63.3. Distance alone buys nothing, the
  cascade count is what costs. Three cascades are indistinguishable in screenshots on Lagoon
  and Circuit; two would be a visible drop in shadow sharpness, so they weren't taken.
- **`VisibilityRange`** (Bevy's hierarchical LOD) would double the entities for each LOD'd
  object. Swapping the mesh handle costs no extra entities and no per-frame work when the level
  doesn't change.
- **Animating glowing surfaces** (2026-09-30, the scrolling boost pads and rainbow ramps): scroll
  an emissive texture with `StandardMaterial::uv_transform`, one material change a frame, rather
  than adding meshes, entities or lights. Solari ignores `uv_transform`, but what you see
  directly comes from the rasterised G-buffer, so it animates with ray tracing too; only the
  light it casts stays still. RT + DLSS, 1080p, on AC, old/new fps: Prism 66.2/67.2, Circuit
  67.5/68.2, Lagoon 70.4/71.4, Meadow 74.5/74.8, Lagoon at 2542x1525 42.6/43.4. (One textured
  mesh replaced three chevron meshes and a mirror-glass panel.) The soft glow round each pad
  is an unlit, additive, vertex-coloured mesh, which stays out of the RT scene.

## Ruled out, or don't do this

- **Shrinking the sky dome.** It helps speed, but the hole left open round the sun has to grow
  to cover parallax (`asin(PLAY_RADIUS / R)`), and mirror surfaces reflect that hole as black.
  It would look worse, and the mirror-material fix gets the same speed anyway.
- **Splitting the sky dome into many meshes.** 64 patch entities made Bevy's mesh slab
  allocator log "Use-after-free: attempted to copy element data for an unallocated key" and
  segfault (exit 139). It would also have skewed Solari's light sampling:
  `generate_random_light_sample` picks a light *source* uniformly, then a triangle in it, so
  64 sky lights would starve the neon, headlights and underglow of samples. More noise, same
  average. **Keep emissive meshes merged**, and remember that the number of emissive entities
  changes how Solari spends samples.
- **Changing the DLSS Ray Reconstruction model preset.** `dlss_wgpu` 4.0.0 only exposes the
  quality mode, not the preset (CNN vs transformer), so it can't be changed without patching
  that crate.

## Where the frame goes now

RT, DLSS quality, 2542x1525, Prism (24 fps, 41.5ms):

| Pass | GPU ms |
|---|---|
| dlss_ray_reconstruction | 16.0 |
| solari diffuse_indirect_lighting | 6.5 |
| solari direct_lighting | 6.4 |
| solari specular_indirect_lighting | 4.8 |
| solari world_cache | 1.8 |
| bloom | 0.6 |
| early deferred prepass | 0.55 |
| everything else listed | < 0.5 total |
| unlisted | ~4 |

At 1080p the same frame is 23.6ms, and DLSS RR is 8.4ms of it.

DLSS RR's cost scales with the *output* (window) resolution. Solari's passes scale with the
*internal* resolution. Both are legitimate per-pixel work, with no waste found in them.

Standard renderer, 2542x1525, Prism (46 fps, 21.8ms): SSR 6.2ms, SSAO 2.3ms, prepass 0.7ms,
TAA 0.6ms, bloom 0.6ms, plus the ~10ms unaccounted above.

CPU: 8-10ms a frame, the same with or without ray tracing, with 3.5-6.5k entities (Circuit has
the most, ~6.4k, from its crowd). It's spread thinly over Bevy's own systems: the task
executor, frustum culling (`Frustum::intersects_obb`), visibility, mesh collection for GPU
building, and UI layout (taffy). No hot spot is in the game's code. Bevy renders on a pipelined
thread, so while the GPU needs 25-40ms the CPU isn't the limit. It only caps the frame rate
at ~100-120 fps. (`sculpt::Shape::dist` / `Sculpt::sdf` dominate a whole-run profile, but
that's startup meshing, once.)

## Split screen

`--players 2` (or 3, 4) benchmarks split screen. Each player has a full camera rendering into
an image of its own (`render/split.rs`), so per-pixel work stays about the same as one player
at the same window size, and what grows is each view's fixed cost.

RT, DLSS quality, 2542x1525, AC (balanced profile, 55W cap), 2026-09-28:

| fps | Prism | Circuit | Lagoon | Meadow |
|---|---|---|---|---|
| 1 player | 29.9 | 31.4 | 33.6 | 34.9 |
| 2 players | 24.9 | 25.6 | 27.7 | 28.8 |
| 4 players | | | 24.6 | |

On Lagoon, two players cost 6.3ms more than one. The bench lists each pass once per view:
DLSS RR and Solari's lighting passes halve per view, so their totals barely change, while
Solari's **world cache** (~2ms) and the **early deferred prepass** (~1ms) are paid again for
every view. With four players that's 7.6ms of world cache and 3.4ms of prepass. The world cache
is per camera in Solari (`SolariLightingResources`), so sharing it between the players' views
would mean patching Bevy.

Don't use camera viewports for split screen: in Bevy 0.19, TAA squashes a viewport's image
(it sets the viewport but samples the whole texture), and DLSS RR and Solari render a view
that doesn't start at the texture's corner blank. (From reading Bevy's source, not measured:
the deferred lighting and SSR nodes don't set a viewport either, so they would likely run
over the whole window once per view.)

Custom tracks (the track editor, 2026-09-30). RT, DLSS quality, 2542x1525, AC (performance
profile), master before the editor vs the branch, fps:

| | Circuit | Prism | Lagoon | Meadow | Custom (Circuit theme, 2.3km) | Custom (Prism, 2km figure of eight) |
|---|---|---|---|---|---|---|
| Before | 33.0 | 31.6 | 32.4 | 36.3 | | |
| After | 33.0 | 31.5 | 32.3 | 36.1 | 34.4 | 30.6 |

The built-in tracks build the same scenery as before, so the difference is noise. The editor
itself switches the 3D camera off while it's open.

## Jade Jungle and Tbilisi Twilight (2026-10-05)

RT, 2542x1525, on AC, with the user's settings as they were that day (DLSS performance), one
run each straight after the other:

| fps | Circuit | Prism | Lagoon | Meadow | Jungle | Tbilisi |
|---|---|---|---|---|---|---|
| | 52.3 | 50.6 | 55.2 | 57.1 | 46.2 | 49.7 |

The four older tracks build the same scenery as before. Jungle is the heaviest: 2.4M visible
triangles (its leaves) and more of Solari's diffuse and direct lighting (3.9 + 3.3ms against
2.3 + 2.0 on Lagoon), since most of what's on screen is in shade and lit by bounces.

**Found and fixed: tangents for huge meshes.** Loading Jungle froze for ten seconds with ray
tracing. `gfx::raytracing_proxy` works out tangents (mikktspace) for any mesh without them,
at about a second per 100k triangles, and the forest's leaves are one mesh of several hundred
thousand. Only a normal map uses tangents, so meshes over `MAX_TANGENT_VERTICES` now get a
made-up one; a mesh that big with a normal map must bring its own.

**Rules that came out of it:** keep each emissive mesh under 65,535 triangles (Solari panics),
and merge a track's lights into a few meshes by kind (Tbilisi has about ten: lamps, LEDs, lit
windows, floodlights...) rather than one entity per lamp. Lights are given in nits where they
have to light the scene themselves (see CLAUDE.md).

### The sky drawn first, and the clouds it was hiding (checked: no cost)

The sky dome is a blended mesh centred on the camera, so Bevy's back-to-front sort drew it
after, and over, every blended thing in front of it: the clouds on Circuit, Meadow and the
Jungle had never been visible. A large negative `depth_bias` on the sky's material draws it
first. Measured back to back on AC (ray traced, 2542x1525, DLSS performance, two runs each):

| fps | Circuit | Prism | Lagoon | Meadow | Jungle | Tbilisi |
|---|---|---|---|---|---|---|
| sky sorted as before | 52.2, 51.9 | 50.5, 50.4 | 55.1, 54.9 | 56.8, 56.8 | 46.7, 46.7 | 49.6, 49.5 |
| sky first | 52.7, 52.3 | 50.9, 50.8 | 55.4, 55.4 | 57.2, 57.1 | 47.0, 46.9 | 49.8, 49.8 |

The transparent pass is 0.03-0.06ms either way. The jungle's far mesas are unlit, blended
and out of the raytraced scene, so they cost nothing there either.

## The items (checked: no cost)

The fifteen items were added with these rules, and the frame rate didn't move (DLSS quality,
ray traced, 1080p, on AC at the usual 55W cap, the builds run alternately twice each):

| | before | after |
|---|---|---|
| Prism | 67.8 / 67.5 fps | 67.6 / 67.4 fps |
| Circuit | 68.5 / 68.5 | 68.4 / 68.5 |
| Lagoon | 71.5 / 71.5 | 71.4 / 71.6 |
| Meadow | 75.1 / 75.0 | 75.0 / 74.9 |

- **Nothing is spawned until it's wanted.** The rain cloud, the squirrel and the bones or balls
  circling a kart are spawned when a kart has them and despawned after
  (`race::sync_kart_extras`), not kept hidden on every kart. The HUD's item models are built
  the first time a player's slot shows them (`icons::animate_icons`): building all fifteen for
  all four studios up front would have added about 350 entities, and a solo race now starts
  with about 70 fewer than it did with four items.
- **They're made from the kit's shared meshes and materials**, so no new meshes reach the
  raytracer: an item on the track is a handful of extra instances, like a bone always was.
- **Their particles go through the same capped pool** (`MAX_PARTICLES`) and are `NoRaytrace`
  like the rest. A burst is about 95 particles.
- **Mud is eleven UI nodes per player**, hidden unless that player is muddied.
- **The simulation's extra work is a few distance checks** per kart per tick.

Racing with items going off constantly (`--item zoomies --cpu-item waterballoon` and the like)
runs at 65-70 fps on Prism against 67.5 without. The slowest case found was every CPU racer
throwing frisbees non-stop (65 fps, with a doubled frame whenever seven burst at once), which
a real race can't produce: only one frisbee is handed out at a time.

## Quality trade-offs (ask the user first)

`--dlss` (in `gfx::DlssQuality`) sets DLSS's render scale, and `--res` (`gfx::RenderRes`)
caps the height the world is rendered at. The defaults, `quality` and `native`, are unchanged.
Both are in the menu and saved in settings (they apply on restart). `--res` reuses split
screen's view images: the world renders into a smaller image that the overlay camera stretches
over the window, with the HUD drawn on top at full resolution (and name tags scaled to suit).

Ray traced, 2542x1525, on AC (fps, Circuit / Prism):

| | quality | balanced | performance |
|---|---|---|---|
| native | 32.8 / 30.6 | 41.1 / 38.5 | 52.7 / 50.3 |
| 1080p | 58.0 / 55.6 | 70.8 / 68.6 | 86.9 / 86.2 |
| 720p | 105.6 / 104.1 | | |

DLSS's scale only shrinks what Solari traces; the output (and DLSS Ray Reconstruction's ~12ms)
stays at window size. `--res` shrinks the output too, so it is the bigger lever.

The older battery table follows.

| 2542x1525, battery | Prism | Lagoon |
|---|---|---|
| quality (67%) | 24 fps | 27 fps |
| balanced (58%) | 30 fps | 34 fps |
| performance (50%) | 39 fps | 43 fps |

Balanced and performance make Solari cheaper, and DLSS RR also gets cheaper
(16 → 12.5 → 9ms). Other trade-offs that weren't measured: a lower output resolution, and
fewer or rougher glossy surfaces (the specular pass traces every pixel under roughness 0.4).

## Leads not yet followed up

Roughly in order of likely value. None has been measured unless it says so.

1. **Test on AC power.** Done, see `PERFORMANCEOVERVIEW.md`. On AC the laptop's `balanced`
   platform profile still caps the GPU at 55W (of 140W), so it's power-limited there too.
   Try the `performance` profile before any code change.
2. **Input latency.** At 25-45 fps, Bevy's pipelined rendering plus up to 2 queued frames
   (the wgpu surface's `desired_maximum_frame_latency`, default 2) adds several frames of
   input lag, which can feel "laggy" on its own. Setting it to 1 on the primary window might
   feel better at some throughput cost. It needs a windowed test with the user's consent,
   since headless mode has no swapchain.
3. **The standard renderer's unaccounted ~10ms** (deferred lighting, shadows). Prism and
   Lagoon have many point and spot lights (per-kart underglow and road spotlights, lamps,
   neon). They cast no shadows, but they all go through clustered deferred lighting. Bevy
   also warns that no entity has `ShadowLodOrigin` in headless mode. SSR at 6.2ms is the
   biggest listed pass, with 24 linear steps and 6 bisection steps in `gfx::camera_effects`.
   Tuning either is a quality trade-off.
4. **Render-to-texture cameras run every frame.** `signs::setup_signs` has a Camera2d that
   renders the sponsor-board atlas, which never changes, and `icons.rs` has a Camera3d for the
   item icon. Their GPU cost is tiny (ui pass ~0.07ms), but they add views to CPU visibility
   work. Done: the sign camera switches off once the atlas is drawn, and the icon camera
   only runs while an item is showing.
5. **Solari's world cache** (1.8-2.8ms) depends on how many cells are live. Geometry spread
   over large areas, like big floors and the Lagoon sea, keeps more cells alive.
6. **An FXAA pass** runs alongside DLSS (0.02ms, so harmless). It was the item-icon camera's,
   not the main camera's.
7. **Split screen's world cache.** See [Split screen](#split-screen): each extra player pays
   ~2ms for Solari's world cache, which is the same world each time.
8. **CPU entity count.** If the CPU ever becomes the limit (for example with the standard
   renderer at 165Hz), the obvious targets are Circuit's crowd of ~600 corgis and the per-part
   kart hierarchies: merge static parts into single meshes, and use fewer entities with more
   vertices. For ray tracing this is partly done: static scenery now shares one raytracing
   mesh per material (Solari costs ~1µs of CPU per instance per frame). The crowd is the
   biggest remaining group.
