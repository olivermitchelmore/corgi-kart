mod audio;
mod editor;
mod game;
mod input;
mod menu;
mod net;
mod record;
mod render;
mod session;
mod sim;

use bevy::prelude::*;

fn main() {
    // Saved custom tracks, before `--track` and the settings name one.
    editor::files::load_all();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match game::Cli::parse(&args) {
        Ok(cli) => cli,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprint!("{}", game::usage());
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };

    // Ray tracing has to be decided before the GPU device is created.
    let mut raytracing = cli.raytracing.unwrap_or_else(|| game::Settings::load().raytracing);
    if raytracing {
        match render::gfx::probe_raytracing() {
            Ok(gpu) => eprintln!("corgiKart: hardware ray tracing on ({gpu})"),
            Err(e) => {
                eprintln!("corgiKart: ray tracing unavailable, using the standard renderer: {e}");
                raytracing = false;
            }
        }
    }
    // These two are read when the cameras are built, so changing them needs a restart.
    let saved = game::Settings::load();
    let gfx = render::gfx::Graphics {
        raytracing,
        dlss: false,
        dlss_quality: cli.dlss_quality.unwrap_or(saved.dlss_quality),
        render_res: cli.render_res.unwrap_or(saved.render_res),
    };

    let window = if cli.headless {
        // No window at all: the main camera renders into an image (see `scene::setup_world`).
        WindowPlugin { primary_window: None, exit_condition: bevy::window::ExitCondition::DontExit, ..default() }
    } else {
        WindowPlugin {
            primary_window: Some(Window { title: "corgiKart".into(), resolution: (1280, 720).into(), ..default() }),
            ..default()
        }
    };
    let mut app = App::new();
    // DLSS needs an app id before the renderer starts.
    #[cfg(feature = "dlss")]
    app.insert_resource(bevy::anti_alias::dlss::DlssProjectId(bevy::asset::uuid::uuid!("d861de15-37e4-4a1f-b986-0d6f4b1f2dc5")));
    let mut plugins = DefaultPlugins.set(window);
    if raytracing {
        use bevy::render::settings::{RenderCreation, WgpuSettings};
        plugins = plugins.set(bevy::render::RenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                features: bevy::solari::prelude::SolariPlugins::required_wgpu_features(),
                ..default()
            })),
            ..default()
        });
    }
    if cli.headless {
        // Run frames back to back, as fast as the GPU allows.
        app.add_plugins(plugins.disable::<bevy::winit::WinitPlugin>().disable::<bevy::audio::AudioPlugin>())
            .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO));
    } else {
        app.add_plugins(plugins);
    }
    if raytracing {
        app.add_plugins(bevy::solari::prelude::SolariPlugins);
    }
    if cli.bench.is_some() {
        // Per-pass GPU timings for the benchmark summary.
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    // For the on-screen counter.
    app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default());
    if cli.fps {
        app.add_plugins(bevy::diagnostic::LogDiagnosticsPlugin::default());
    }
    app.insert_resource(bevy::pbr::DefaultOpaqueRendererMethod::deferred())
        .insert_resource(gfx)
        .insert_resource(cli.clone())
        .add_plugins((game::GamePlugin, menu::MenuPlugin, render::RenderPlugin, editor::EditorPlugin));
    if !cli.headless {
        app.add_plugins(audio::AudioPlugin);
    }
    if cli.record.is_some() {
        app.add_plugins(record::RecordPlugin);
    }
    app.run();
}
