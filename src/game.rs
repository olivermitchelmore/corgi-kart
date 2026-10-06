//! App states, settings, command line, and the glue between Bevy and `Session`.

use bevy::prelude::*;

use crate::input::{read_kart_inputs, read_nav, stick_nav, StickNav};
use crate::net::{parse_addr, DEFAULT_PORT};
use crate::session::{Role, Session, SessionEvent, MAX_LOCAL_PLAYERS};
use crate::sim::{SpeedClass, Track, TrackId};

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Menu,
    Lobby,
    Race,
    Editor,
}

/// Simulation runs before presentation each frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameSet {
    Simulate,
    Present,
}

/// Marker for everything that belongs to the current race and is rebuilt per race.
#[derive(Component)]
pub struct RaceEntity;

/// The track whose scenery is currently built. Follows the race / lobby / menu selection.
#[derive(Resource)]
pub struct TrackRes(pub Track);

#[derive(Resource)]
pub struct SessionRes(pub Session);

#[derive(Resource, Default)]
pub struct MenuStatus(pub String);

/// Seconds left to confirm leaving the race with a second Esc.
#[derive(Resource, Default)]
pub struct QuitArm(pub f32);

// ----- Settings ----------------------------------------------------------------------------

#[derive(Resource, Clone)]
pub struct Settings {
    pub name: String,
    pub join_addr: String,
    /// Split-screen players for a local race.
    pub players: u8,
    pub bots: u8,
    pub track: TrackId,
    pub class: SpeedClass,
    /// Hardware raytraced lighting (takes effect on restart).
    pub raytracing: bool,
    /// Show the frame rate on screen.
    pub fps: bool,
    /// DLSS render scale for the raytraced renderer (takes effect on restart).
    pub dlss_quality: crate::render::gfx::DlssQuality,
    /// The resolution the world is rendered at (takes effect on restart).
    pub render_res: crate::render::gfx::RenderRes,
}

/// Where settings and custom tracks live: `~/.config/corgikart`.
pub fn config_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))?;
    Some(base.join("corgikart"))
}

impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        Some(config_dir()?.join("settings.txt"))
    }

    pub fn load() -> Self {
        let mut s = Settings { name: "Corgi".into(), join_addr: "192.168.1.".into(), players: 1, bots: 5, track: TrackId::default(), class: SpeedClass::default(), raytracing: false, fps: false, dlss_quality: default(), render_res: default() };
        if let Some(text) = Self::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            for line in text.lines() {
                match line.split_once('=') {
                    Some(("name", v)) => s.name = v.to_string(),
                    Some(("join", v)) => s.join_addr = v.to_string(),
                    Some(("players", v)) => s.players = v.parse::<u8>().unwrap_or(s.players).clamp(1, MAX_LOCAL_PLAYERS as u8),
                    Some(("bots", v)) => s.bots = v.parse().unwrap_or(s.bots),
                    Some(("track", v)) => s.track = TrackId::from_key(v).unwrap_or(s.track),
                    Some(("raytracing", v)) => s.raytracing = v == "on",
                    Some(("fps", v)) => s.fps = v == "on",
                    Some(("dlss", v)) => s.dlss_quality = crate::render::gfx::DlssQuality::from_key(v).unwrap_or(s.dlss_quality),
                    Some(("resolution", v)) => s.render_res = crate::render::gfx::RenderRes::from_key(v).unwrap_or(s.render_res),
                    Some(("class", v)) => s.class = SpeedClass::from_key(v).unwrap_or(s.class),
                    _ => {}
                }
            }
        }
        s
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let on = |b: bool| if b { "on" } else { "off" };
            let (rt, fps) = (on(self.raytracing), on(self.fps));
            let text = format!(
                "name={}\njoin={}\nplayers={}\nbots={}\ntrack={}\nclass={}\nraytracing={rt}\nfps={fps}\ndlss={}\nresolution={}\n",
                self.name,
                self.join_addr,
                self.players,
                self.bots,
                self.track.key(),
                self.class.name(),
                self.dlss_quality.key(),
                self.render_res.key()
            );
            let _ = std::fs::write(p, text);
        }
    }
}

// ----- Command line ------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum CliAction {
    Solo,
    Host,
    Join(String),
    Editor,
}

#[derive(Resource, Default, Clone, Debug)]
pub struct Cli {
    pub action: Option<CliAction>,
    pub name: Option<String>,
    pub players: Option<u8>,
    pub bots: Option<u8>,
    pub port: Option<u16>,
    pub track: Option<TrackId>,
    pub class: Option<SpeedClass>,
    pub raytracing: Option<bool>,
    /// DLSS render scale for the raytraced renderer.
    pub dlss_quality: Option<crate::render::gfx::DlssQuality>,
    /// The resolution the world is rendered at.
    pub render_res: Option<crate::render::gfx::RenderRes>,
    /// Show the frame rate, and log it to the console.
    pub fps: bool,
    pub autodrive: bool,
    /// Keep handing this item to your kart, and this one to the CPU racers.
    pub test_items: [Option<(crate::sim::Item, u8)>; 2],
    /// Host: start the race automatically once someone has joined (for testing).
    pub autostart: bool,
    /// Save a screenshot to this path after `shot_delay` seconds, then quit.
    pub screenshot: Option<String>,
    pub shot_delay: f32,
    /// Debug: park the menu camera at `eye` looking at `target`, with the menu hidden.
    pub view: Option<(Vec3, Vec3)>,
    /// Render offscreen with no window and no sound (for screenshots and benchmarks).
    pub headless: bool,
    /// Resolution of the headless render.
    pub size: (u32, u32),
    /// Measure frame times for this many seconds (after a warm-up), print a summary and quit.
    pub bench: Option<f32>,
    /// Record a video to this file (implies `headless`): see `record`.
    pub record: Option<String>,
    /// Start recording this many seconds in, and record for this long.
    pub record_from: f32,
    pub record_secs: f32,
    /// A film camera in place of the chase camera.
    pub cam: Option<crate::record::FilmCam>,
    pub no_hud: bool,
    /// The editor builds a track by itself, for the trailer.
    pub demo: bool,
}

/// The built-in tracks' `--track` names.
fn track_keys() -> String {
    TrackId::ALL.map(|t| t.key()).join(", ")
}

/// What `--help` prints.
pub fn usage() -> String {
    USAGE.replace("{tracks}", &track_keys())
}

const USAGE: &str = "\
corgiKart - a kart racer starring corgis

USAGE:
    corgikart                      open the main menu
    corgikart solo                 race against bots (split screen with --players)
    corgikart host                 host a LAN game (port 7777)
    corgikart join <ip[:port]>     join a LAN game
    corgikart editor               open the track editor (on --track custom:<file> to edit it)

OPTIONS:
    --name <name>       your racer name
    --players <n>       split-screen players in a local race (1-4)
    --bots <n>          number of CPU racers (0-7)
    --port <port>       port to host on (default 7777)
    --track <track>     {tracks}, or custom:<file> for a track you made
    --cc <class>        50, 100 or 150
    --raytracing        hardware raytraced lighting (needs an RTX-class GPU)
    --no-raytracing     the standard renderer
    --dlss <mode>       DLSS render scale: quality (default), balanced or performance
    --res <res>         render the world at native (default), 1080p or 720p and stretch it
                        over the window (much faster on a big window, a little softer)
    --fps               show the frame rate (F3 toggles it) and log it

TESTING:
    --autodrive         let the AI drive your kart
    --item <name>       keep handing your kart this item (zoomies, mudpie, bone3 for three...)
    --cpu-item <name>   the same for the CPU racers
    --screenshot <png>  save a screenshot after --shot-delay seconds (default 8), then quit
    --view x,y,z,tx,ty,tz   park the menu camera here, looking there, with the menu hidden
    --headless          render offscreen: no window, no sound
    --size <WxH>        headless resolution (default 1920x1080)
    --bench <secs>      measure frame times for <secs> after a warm-up, print them and quit

RECORDING (see trailer/):
    --record <mp4>      render offscreen at a steady 60 frames a second and save a video, with
                        sound (needs ffmpeg). --size sets the resolution.
    --record-from <secs>  start recording this far in (default 0)
    --record-secs <secs>  and record for this long (default 10), then quit
    --cam <dist,height,angle[,spin[,fov]]>  film your kart from <dist> metres away and <height>
                        up, <angle> degrees round from behind it (180 is head on), turning
                        <spin> degrees a second
    --no-hud            hide the race HUD
    --demo              with `editor`: build a track by itself
";

impl Cli {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut cli = Cli { shot_delay: 8.0, size: (1920, 1080), record_secs: 10.0, ..Default::default() };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut val = |flag: &str| it.next().cloned().ok_or(format!("{flag} needs a value"));
            match a.as_str() {
                "solo" => cli.action = Some(CliAction::Solo),
                "host" => cli.action = Some(CliAction::Host),
                "join" => cli.action = Some(CliAction::Join(val("join")?)),
                "editor" => cli.action = Some(CliAction::Editor),
                "--name" => cli.name = Some(val("--name")?),
                "--players" => cli.players = Some(val("--players")?.parse().map_err(|_| "bad --players")?),
                "--bots" => cli.bots = Some(val("--bots")?.parse().map_err(|_| "bad --bots")?),
                "--port" => cli.port = Some(val("--port")?.parse().map_err(|_| "bad --port")?),
                "--cc" => cli.class = Some(SpeedClass::from_key(&val("--cc")?).ok_or("unknown --cc (50, 100, 150)")?),
                "--track" => cli.track = Some(TrackId::from_key(&val("--track")?).ok_or_else(|| format!("unknown --track ({}, or custom:<file> for a saved track)", track_keys()))?),
                "--autodrive" => cli.autodrive = true,
                "--item" | "--cpu-item" => {
                    let item = crate::sim::Item::from_key(&val(a)?).ok_or("unknown item")?;
                    cli.test_items[(a == "--cpu-item") as usize] = Some(item);
                }
                "--raytracing" => cli.raytracing = Some(true),
                "--no-raytracing" => cli.raytracing = Some(false),
                "--fps" => cli.fps = true,
                "--dlss" => cli.dlss_quality = Some(crate::render::gfx::DlssQuality::from_key(&val("--dlss")?).ok_or("unknown --dlss (quality, balanced, performance)")?),
                "--res" => cli.render_res = Some(crate::render::gfx::RenderRes::from_key(&val("--res")?).ok_or("unknown --res (native, 1080p, 720p)")?),
                "--autostart" => cli.autostart = true,
                "--screenshot" => cli.screenshot = Some(val("--screenshot")?),
                "--shot-delay" => cli.shot_delay = val("--shot-delay")?.parse().map_err(|_| "bad --shot-delay")?,
                "--headless" => cli.headless = true,
                "--size" => {
                    let v = val("--size")?;
                    let (w, h) = v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).ok_or("--size needs WxH")?;
                    cli.size = (w, h);
                }
                "--bench" => cli.bench = Some(val("--bench")?.parse().map_err(|_| "bad --bench")?),
                "--view" => {
                    let v: Vec<f32> = val("--view")?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    let [ex, ey, ez, tx, ty, tz] = v[..] else { return Err("--view needs x,y,z,tx,ty,tz".into()) };
                    cli.view = Some((Vec3::new(ex, ey, ez), Vec3::new(tx, ty, tz)));
                }
                "--record" => {
                    cli.record = Some(val("--record")?);
                    cli.headless = true;
                }
                "--record-from" => cli.record_from = val("--record-from")?.parse().map_err(|_| "bad --record-from")?,
                "--record-secs" => cli.record_secs = val("--record-secs")?.parse().map_err(|_| "bad --record-secs")?,
                "--cam" => cli.cam = Some(crate::record::FilmCam::parse(&val("--cam")?).ok_or("--cam needs dist,height,angle[,spin[,fov]]")?),
                "--no-hud" => cli.no_hud = true,
                "--demo" => cli.demo = true,
                "-h" | "--help" => return Err(String::new()),
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(cli)
    }
}

// ----- Starting sessions -------------------------------------------------------------------

pub fn start_solo(commands: &mut Commands, settings: &Settings, next: &mut NextState<AppState>, autodrive: bool) {
    let mut s = Session::solo(&settings.name, settings.players, settings.bots, settings.track, settings.class);
    s.autodrive = autodrive;
    s.start_race();
    commands.insert_resource(SessionRes(s));
    next.set(AppState::Race);
}

pub fn start_host(commands: &mut Commands, settings: &Settings, port: u16, next: &mut NextState<AppState>, autodrive: bool) -> Result<(), String> {
    let mut s = Session::host(&settings.name, settings.bots, port, settings.track, settings.class).map_err(|e| format!("Couldn't host on port {port}: {e}"))?;
    s.autodrive = autodrive;
    commands.insert_resource(SessionRes(s));
    next.set(AppState::Lobby);
    Ok(())
}

pub fn start_join(commands: &mut Commands, settings: &Settings, next: &mut NextState<AppState>, autodrive: bool) -> Result<(), String> {
    let addr = parse_addr(&settings.join_addr).ok_or_else(|| format!("Can't understand address '{}'", settings.join_addr))?;
    let mut s = Session::join(&settings.name, addr).map_err(|e| format!("Network error: {e}"))?;
    s.autodrive = autodrive;
    commands.insert_resource(SessionRes(s));
    next.set(AppState::Lobby);
    Ok(())
}

// ----- Plugin ------------------------------------------------------------------------------

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        let settings = Settings::load();
        app.init_state::<AppState>()
            .insert_resource(TrackRes(Track::new(settings.track)))
            .insert_resource(settings)
            .init_resource::<MenuStatus>()
            .init_resource::<QuitArm>()
            .init_resource::<StickNav>()
            .add_systems(PreUpdate, stick_nav.after(bevy::input::InputSystems))
            .configure_sets(Update, (GameSet::Simulate, GameSet::Present).chain())
            .add_systems(PostStartup, apply_cli)
            .add_systems(
                Update,
                (
                    session_update.run_if(resource_exists::<SessionRes>),
                    race_controls.run_if(in_state(AppState::Race)),
                )
                    .chain()
                    .in_set(GameSet::Simulate),
            )
            .add_systems(Update, autostart.run_if(in_state(AppState::Lobby)).run_if(|cli: Res<Cli>| cli.autostart))
            .add_systems(Update, screenshot_and_quit.run_if(|cli: Res<Cli>| cli.screenshot.is_some()))
            .add_systems(Last, bench.run_if(|cli: Res<Cli>| cli.bench.is_some()));
    }
}

fn apply_cli(
    mut commands: Commands,
    cli: Res<Cli>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<AppState>>,
    mut status: ResMut<MenuStatus>,
) {
    if let Some(n) = &cli.name {
        settings.name = n.clone();
    }
    if let Some(t) = cli.track {
        settings.track = t;
    }
    if let Some(c) = cli.class {
        settings.class = c;
    }
    if let Some(p) = cli.players {
        settings.players = p.clamp(1, MAX_LOCAL_PLAYERS as u8);
    }
    if cli.fps {
        settings.fps = true;
    }
    if let Some(q) = cli.dlss_quality {
        settings.dlss_quality = q;
    }
    if let Some(r) = cli.render_res {
        settings.render_res = r;
    }
    if let Some(b) = cli.bots {
        settings.bots = b.min(7);
    }
    let result = match &cli.action {
        None => Ok(()),
        Some(CliAction::Solo) => {
            start_solo(&mut commands, &settings, &mut next, cli.autodrive);
            Ok(())
        }
        Some(CliAction::Host) => start_host(&mut commands, &settings, cli.port.unwrap_or(DEFAULT_PORT), &mut next, cli.autodrive),
        Some(CliAction::Join(addr)) => {
            settings.join_addr = addr.clone();
            start_join(&mut commands, &settings, &mut next, cli.autodrive)
        }
        Some(CliAction::Editor) => {
            next.set(AppState::Editor);
            Ok(())
        }
    };
    if let Err(e) = result {
        status.0 = e;
    }
}

fn session_update(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    mut session: ResMut<SessionRes>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut status: ResMut<MenuStatus>,
    cli: Res<Cli>,
) {
    session.0.test_items = cli.test_items;
    let players = session.0.race.as_ref().map_or(1, |r| r.locals.len());
    let inputs = if *state.get() == AppState::Race { read_kart_inputs(&keys, &pads, players) } else { vec![] };
    for event in session.0.update(time.delta_secs(), &inputs) {
        match event {
            SessionEvent::RaceStarted => next.set(AppState::Race),
            SessionEvent::BackToLobby => next.set(AppState::Lobby),
            SessionEvent::Disconnected(msg) => {
                status.0 = msg;
                commands.remove_resource::<SessionRes>();
                next.set(AppState::Menu);
                return;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn race_controls(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    stick: Res<StickNav>,
    session: Option<ResMut<SessionRes>>,
    mut quit: ResMut<QuitArm>,
    mut over_for: Local<f32>,
    mut next: ResMut<NextState<AppState>>,
    test_drive: Option<Res<crate::editor::TestDrive>>,
) {
    let Some(mut session) = session else { return };
    quit.0 = (quit.0 - time.delta_secs()).max(0.0);
    let esc = keys.just_pressed(KeyCode::Escape) || pads.iter().any(|p| p.just_pressed(GamepadButton::Select));
    if esc {
        if quit.0 > 0.0 {
            quit.0 = 0.0;
            session.0.leave();
            commands.remove_resource::<SessionRes>();
            // A test drive goes back to the editor.
            next.set(if test_drive.is_some() { AppState::Editor } else { AppState::Menu });
            return;
        }
        quit.0 = 2.0;
    }

    let over = session.0.race.as_ref().is_some_and(|r| r.world.over);
    *over_for = if over { *over_for + time.delta_secs() } else { 0.0 };
    let nav = read_nav(&keys, &pads, &stick);
    // Give the results a moment first: Enter is also player 2's item button.
    if over && nav.confirm && *over_for > 1.0 {
        match session.0.role {
            Role::Solo => session.0.start_race(),
            Role::Host(_) => {
                session.0.back_to_lobby();
                next.set(AppState::Lobby);
            }
            Role::Client(_) => {}
        }
    }
}

fn autostart(
    time: Res<Time>,
    session: Option<ResMut<SessionRes>>,
    mut since: Local<Option<f32>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(mut session) = session else { return };
    let Role::Host(h) = &session.0.role else { return };
    if h.clients.is_empty() {
        *since = None;
        return;
    }
    let t = time.elapsed_secs();
    if t - *since.get_or_insert(t) > 2.0 {
        session.0.start_race();
        next.set(AppState::Race);
    }
}

fn screenshot_and_quit(
    mut commands: Commands,
    time: Res<Time>,
    cli: Res<Cli>,
    headless: Option<Res<crate::render::scene::HeadlessTarget>>,
    mut taken: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
    let t = time.elapsed_secs();
    if !*taken && t > cli.shot_delay {
        *taken = true;
        let shot = match headless {
            Some(target) => Screenshot::image(target.0.clone()),
            None => Screenshot::primary_window(),
        };
        // Quit once it's saved (a benchmark quits by itself when it's done).
        let quit = cli.bench.is_none();
        commands.spawn(shot).observe(save_to_disk(cli.screenshot.clone().unwrap())).observe(
            move |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                if quit {
                    exit.write(AppExit::Success);
                }
            },
        );
    }
    // In case the capture never arrives.
    if cli.bench.is_none() && t > cli.shot_delay + 30.0 {
        error!("screenshot wasn't captured");
        exit.write(AppExit::error());
    }
}

/// What the renderer has to chew through each frame, for the benchmark summary.
#[derive(bevy::ecs::system::SystemParam)]
struct BenchCounts<'w, 's> {
    meshes: Res<'w, Assets<Mesh>>,
    mats: Res<'w, Assets<StandardMaterial>>,
    mesh_entities: Query<'w, 's, (&'static ViewVisibility, &'static Mesh3d)>,
    rt_instances: Query<'w, 's, (), With<bevy::solari::prelude::RaytracingMesh3d>>,
    point_lights: Query<'w, 's, (), With<PointLight>>,
    spot_lights: Query<'w, 's, (), With<SpotLight>>,
    cameras: Query<'w, 's, &'static Camera>,
}

impl BenchCounts<'_, '_> {
    fn print(&self) {
        let visible = self.mesh_entities.iter().filter(|(v, _)| v.get()).count();
        let tris = |m: &Mesh3d| self.meshes.get(&m.0).map_or(0, |m| m.indices().map_or(0, |i| i.len()) / 3);
        let shown: usize = self.mesh_entities.iter().filter(|(v, _)| v.get()).map(|(_, m)| tris(m)).sum();
        let all: usize = self.mesh_entities.iter().map(|(_, m)| tris(m)).sum();
        println!("bench: {}k triangles in visible meshes, {}k in all mesh entities", shown / 1000, all / 1000);
        println!(
            "bench: {} mesh entities ({} visible), {} raytracing instances, {} meshes, {} materials, {} point + {} spot lights, {}/{} cameras active",
            self.mesh_entities.iter().len(),
            visible,
            self.rt_instances.iter().len(),
            self.meshes.len(),
            self.mats.len(),
            self.point_lights.iter().len(),
            self.spot_lights.iter().len(),
            self.cameras.iter().filter(|c| c.is_active).count(),
            self.cameras.iter().len(),
        );
    }
}

/// Seconds to let shaders compile and the race get going before `--bench` starts measuring.
const BENCH_WARMUP: f32 = 8.0;

/// `--bench`: record frame times for a while, print a summary and quit.
fn bench(
    time: Res<Time<Real>>,
    cli: Res<Cli>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    entities: &bevy::ecs::entity::Entities,
    counts: BenchCounts,
    mut frames: Local<Vec<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    let secs = cli.bench.unwrap_or(0.0);
    if t < BENCH_WARMUP {
        return;
    }
    if t < BENCH_WARMUP + secs {
        frames.push(time.delta_secs() * 1000.0);
        return;
    }
    let mut ms = std::mem::take(&mut *frames);
    if ms.is_empty() {
        exit.write(AppExit::Success);
        return;
    }
    let total: f32 = ms.iter().sum();
    ms.sort_by(f32::total_cmp);
    let pct = |p: f32| ms[((ms.len() - 1) as f32 * p).round() as usize];
    let slow = ms.iter().filter(|&&m| m > pct(0.5) * 2.0).count();
    println!(
        "bench: {} frames in {:.1}s: {:.1} fps, mean {:.2}ms, p50 {:.2}ms, p95 {:.2}ms, p99 {:.2}ms, max {:.2}ms, {} frames over 2x median",
        ms.len(),
        total / 1000.0,
        ms.len() as f32 / (total / 1000.0),
        total / ms.len() as f32,
        pct(0.5),
        pct(0.95),
        pct(0.99),
        pct(1.0),
        slow
    );
    println!("bench: {} entities", entities.count_spawned());
    counts.print();
    // The most expensive render passes on the GPU (averaged over the last few frames).
    let mut passes: Vec<(String, f64)> = diagnostics
        .iter()
        .filter(|d| d.path().as_str().ends_with("/elapsed_gpu"))
        .filter_map(|d| Some((d.path().as_str().trim_end_matches("/elapsed_gpu").to_string(), d.average()?)))
        .collect();
    passes.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (name, ms) in passes.iter().take(30) {
        println!("bench: gpu {ms:6.2}ms  {name}");
    }
    exit.write(AppExit::Success);
}
