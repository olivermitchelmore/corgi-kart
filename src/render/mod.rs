//! Everything visual. The simulation never depends on this module.

pub mod ambient;
pub mod corgi;
pub mod fps;
pub mod fx;
pub mod gfx;
pub mod hud;
pub mod icons;
pub mod jungle;
pub mod kart;
pub mod lagoon;
pub mod lod;
pub mod look;
pub mod mesh;
pub mod models;
pub mod race;
pub mod scene;
pub mod scenery;
pub mod shapes;
pub mod sculpt;
pub mod signs;
pub mod split;
pub mod tbilisi;
pub mod textures;
pub mod trackmesh;

use bevy::prelude::*;

use crate::game::{AppState, GameSet};

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(scene::SKY))
            .init_resource::<race::RaceScene>()
            .init_resource::<fx::ParticleCount>()
            .add_message::<fx::RaceFx>()
            .add_systems(Startup, (scene::setup_world, signs::setup_signs, fx::setup_fx, fps::setup_fps))
            .add_systems(Update, (fps::toggle_fps, fps::update_fps).chain())
            .add_systems(PostStartup, icons::setup_icons)
            .add_systems(Update, (icons::tag_icon_layers, icons::animate_icons, signs::freeze_signs))
            .add_systems(OnExit(AppState::Race), (race::reset_scene, split::end_split))
            .init_resource::<gfx::RtMergedGroups>()
            .add_systems(PostUpdate, (gfx::add_raytracing_meshes, gfx::rebuild_merged_raytracing).chain().after(bevy::transform::TransformSystems::Propagate))
            .add_systems(Update, gfx::scale_emissive_for_raytracing)
            .add_systems(Update, (scene::sync_track_scene, look::apply_look).chain().in_set(GameSet::Present).before(RaceVisuals))
            .add_systems(
                Update,
                race::menu_camera.run_if(not(in_state(AppState::Race))).in_set(GameSet::Present).in_set(RaceVisuals),
            )
            .add_systems(Update, split::layout_views.in_set(GameSet::Present).after(RaceVisuals))
            .add_systems(
                Update,
                (
                    race::sync_race_scene,
                    (
                        race::animate_karts,
                        race::animate_kart_parts,
                        race::animate_effects,
                        race::animate_brake_lights,
                        kart::animate_underglow,
                        kart::animate_bandanas,
                        race::animate_boxes,
                        race::sync_items,
                        race::sync_kart_extras,
                    ),
                    race::race_camera,
                    fx::detect_events,
                    (fx::emit_particles, fx::update_particles).chain(),
                    (hud::update_hud, hud::update_mud, hud::update_standings, hud::update_results, hud::update_minimap, hud::update_name_tags),
                )
                    .chain()
                    .run_if(in_state(AppState::Race))
                    .in_set(GameSet::Present)
                    .in_set(RaceVisuals),
            )
            .add_systems(
                Update,
                (lod::update_lods, ambient::animate_crowd, ambient::animate_statue, ambient::animate_rides, ambient::start_lights, ambient::animate_night, ambient::animate_lagoon, ambient::animate_shuttles, ambient::animate_pads, scene::follow_sky)
                    .in_set(GameSet::Present)
                    .after(RaceVisuals),
            );
    }
}

/// Per-frame camera and race presentation, after the scenery is in place.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RaceVisuals;
