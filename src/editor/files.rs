//! Custom tracks on disk: one small text file per map in `~/.config/corgikart/tracks/`, in the
//! same `key=value` style as the settings file:
//!
//! ```text
//! name=Bone Valley
//! theme=circuit
//! start=0
//! point=12.5,-40,0
//! item=3.25
//! pad=5.5,2
//! ford=7.1,7.6
//! ```
//!
//! Positions along the lap (`start`, `item`, the first number of `pad` and both of `ford`) are
//! spline parameters: see `sim::custom`.

use std::path::PathBuf;

use crate::sim::custom::{self, CustomMap, Ford, MapPoint, Pad, RampSpot};
use crate::sim::{Theme, TrackId};

const EXT: &str = "track";

pub fn tracks_dir() -> Option<PathBuf> {
    Some(crate::game::config_dir()?.join("tracks"))
}

/// Register every saved map, so they can be picked in the menu.
pub fn load_all() {
    let Some(dir) = tracks_dir() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != EXT) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        match std::fs::read_to_string(&path).ok().and_then(|t| from_text(&t)) {
            Some(map) => {
                custom::register(map, Some(stem));
            }
            None => eprintln!("corgiKart: couldn't read the track {}", path.display()),
        }
    }
}

/// Save `map` to `file` (a name without the extension), or to a new file named after the map.
/// Returns the file name and the map's id.
pub fn save(map: &CustomMap, file: Option<&str>) -> Result<(String, TrackId), String> {
    if !map.sane() {
        return Err("This track is too big to save".into());
    }
    let dir = tracks_dir().ok_or("There's nowhere to save tracks (no home directory)")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    let file = match file {
        Some(f) => f.to_string(),
        None => {
            let base = slug(&map.name);
            (1..).map(|k| if k == 1 { base.clone() } else { format!("{base}-{k}") }).find(|f| !dir.join(format!("{f}.{EXT}")).exists()).unwrap()
        }
    };
    let path = dir.join(format!("{file}.{EXT}"));
    std::fs::write(&path, to_text(map)).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    let id = custom::register(map.clone(), Some(file.clone()));
    Ok((file, id))
}

/// A file name from a map's name: lower case letters, digits and dashes.
fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "track".into() } else { s }
}

pub fn to_text(map: &CustomMap) -> String {
    // `{}` prints the shortest text that reads back as the same f32, so a saved map keeps its hash.
    let mut out = format!("name={}\ntheme={}\nstart={}\n", map.name.replace('\n', " "), map.theme.key(), map.start);
    for p in &map.points {
        out += &format!("point={},{},{}\n", p.x, p.z, p.h);
    }
    for u in &map.items {
        out += &format!("item={u}\n");
    }
    for p in &map.pads {
        out += &format!("pad={},{}\n", p.u, p.lateral);
    }
    for r in &map.ramps {
        out += &format!("ramp={},{},{}\n", r.u, r.lateral, if r.boosted { "boosted" } else { "plain" });
    }
    for f in &map.fords {
        out += &format!("ford={},{}\n", f.from, f.to);
    }
    out
}

pub fn from_text(text: &str) -> Option<CustomMap> {
    let mut map = CustomMap { name: "Custom track".into(), theme: Theme::Circuit, ..Default::default() };
    let nums = |v: &str| -> Option<Vec<f32>> { v.split(',').map(|x| x.trim().parse::<f32>().ok().filter(|f| f.is_finite())).collect() };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "name" => map.name = v.to_string(),
            "theme" => map.theme = Theme::from_key(v.trim())?,
            "start" => map.start = nums(v)?[0],
            "point" => {
                let [x, z, h] = nums(v)?[..] else { return None };
                map.points.push(MapPoint { x, z, h: h.clamp(0.0, custom::MAX_HEIGHT) });
            }
            "item" => map.items.push(nums(v)?[0]),
            "pad" => {
                let [u, lateral] = nums(v)?[..] else { return None };
                map.pads.push(Pad { u, lateral: lateral.clamp(-custom::PAD_MAX_LATERAL, custom::PAD_MAX_LATERAL) });
            }
            "ramp" => {
                let (spot, kind) = v.rsplit_once(',')?;
                let [u, lateral] = nums(spot)?[..] else { return None };
                let lateral = lateral.clamp(-custom::RAMP_MAX_LATERAL, custom::RAMP_MAX_LATERAL);
                map.ramps.push(RampSpot { u, lateral, boosted: kind.trim() == "boosted" });
            }
            "ford" => {
                let [from, to] = nums(v)?[..] else { return None };
                map.fords.push(Ford { from, to });
            }
            _ => {}
        }
    }
    (!map.is_empty() && map.sane()).then_some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip_keeps_the_hash() {
        let map = CustomMap {
            name: "Bone Valley".into(),
            theme: Theme::Prism,
            points: vec![
                MapPoint { x: 0.1, z: 0.0, h: 0.0 },
                MapPoint { x: 200.3, z: 10.0, h: 2.5 },
                MapPoint { x: 250.0, z: 180.7, h: 0.0 },
                MapPoint { x: -30.0, z: 150.0, h: 1.0 / 3.0 },
            ],
            start: 0.37,
            items: vec![1.2, 2.9],
            pads: vec![Pad { u: 3.3, lateral: -2.0 }],
            ramps: vec![RampSpot { u: 0.8, lateral: 1.5, boosted: false }, RampSpot { u: 2.2, lateral: -6.5, boosted: true }],
            fords: vec![Ford { from: 1.5, to: 1.8 }],
        };
        let back = from_text(&to_text(&map)).unwrap();
        assert_eq!(back, map);
        assert_eq!(back.hash(), map.hash());
        assert_eq!(slug("  Bone Valley GP! "), "bone-valley-gp");
        assert_eq!(slug("???"), "track");
    }

    /// Files that would build into something enormous, or into nothing, aren't loaded.
    #[test]
    fn wrecked_files_are_refused() {
        let file = |pts: &[(f32, f32)]| pts.iter().fold("name=Wreck\ntheme=circuit\nstart=0\n".to_string(), |t, (x, z)| t + &format!("point={x},{z},0\n"));
        assert!(from_text(&file(&[(0.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0)])).is_some());
        for far in [1e30, 3e8, 1e5] {
            assert!(from_text(&file(&[(0.0, 0.0), (far, 0.0), (far, far), (0.0, far)])).is_none(), "loaded a {far}m square");
        }
        assert!(from_text(&file(&[(5.0, 5.0); 4])).is_none(), "loaded four points in one place");
        assert!(from_text(&file(&[(1.0, 2.0); 5000])).is_none(), "loaded 5000 points");
    }
}
