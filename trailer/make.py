#!/usr/bin/env python3
"""Make the trailer: trailer/make.py [--draft] [--only <clip>...]

Records each clip with the game's `--record` (autodrive races are repeatable, so a clip is the
same every time), writes the music (music.py) and cuts the clips to it with ffmpeg. Clips are
kept in trailer/clips and only recorded again if their command changes, so re-cutting is quick.
The result is trailer/corgikart-trailer.mp4.

Needs the DLSS build (see CLAUDE.md), ffmpeg and numpy. Nothing touches ~/.config/corgikart: the
game runs with its own config directory, so the editor's demo track is saved in trailer/clips.

--draft records at 640x360 without ray tracing, to check the cut in a couple of minutes.
"""

import hashlib
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
GAME = os.path.join(ROOT, "target", "release", "corgikart")
DRAFT = "--draft" in sys.argv
CLIPS = os.path.join(HERE, "clips-draft" if DRAFT else "clips")
CONFIG = os.path.join(CLIPS, "config")
OUT = os.path.join(HERE, "corgikart-trailer-draft.mp4" if DRAFT else "corgikart-trailer.mp4")
FONT = "/usr/share/fonts/noto/NotoSans-BlackItalic.ttf"
SIZE = (640, 360) if DRAFT else (1920, 1080)

BAR = 1.6  # seconds: four beats at 150 a minute (music.py)
BEAT = BAR / 4

RACE = ["solo", "--autodrive", "--bots", "7", "--cc", "150"]
# Film cameras (--cam): distance, height, degrees round from behind, degrees a second.
HEAD_ON = ["--no-hud", "--cam", "6.5,1.7,165"]
ORBIT = ["--no-hud", "--cam", "8,2.2,120,35"]
LOW = ["--no-hud", "--cam", "7,1.3,200"]

# name: (game arguments, seconds in to start recording, seconds to record)
CLIPS_TO_RECORD = {
    "lights": (RACE + ["--track", "circuit"], 0.8, 3.2),
    "circuit": (RACE + ["--track", "circuit"], 26.8, 5.4),
    "circuit-pack": (RACE + ["--track", "circuit"] + HEAD_ON, 7.2, 1.6),
    "prism-jump": (RACE + ["--track", "prism"], 20.8, 1.6),
    "prism-trick": (RACE + ["--track", "prism"], 39.8, 0.6),
    "prism-orbit": (RACE + ["--track", "prism"] + ORBIT, 9.6, 1.6),
    "lagoon": (RACE + ["--track", "lagoon"], 9.8, 5.2),
    "lagoon-hit": (RACE + ["--track", "lagoon"], 26.0, 1.0),
    "lagoon-low": (RACE + ["--track", "lagoon"] + LOW, 18.8, 1.6),
    "jungle": (RACE + ["--track", "jungle"], 10.6, 1.8),
    "jungle-hall": (RACE + ["--track", "jungle"], 17.4, 1.8),
    "tbilisi-bridge": (RACE + ["--track", "tbilisi"], 13.4, 1.8),
    "tbilisi-town": (RACE + ["--track", "tbilisi"] + LOW, 41.0, 1.8),
    "meadow": (RACE + ["--track", "meadow"], 13.0, 8.4),
    "zoomies": (RACE + ["--track", "meadow", "--item", "zoomies"], 9.2, 1.0),
    "sausage": (RACE + ["--track", "circuit", "--item", "sausage"], 12.4, 1.0),
    "balloons": (RACE + ["--track", "prism", "--cpu-item", "waterballoon"], 12.3, 1.0),
    "split": (["solo", "--autodrive", "--players", "4", "--bots", "4", "--cc", "150", "--track", "circuit"], 8.0, 1.6),
    # The editor is laid out for a window, so it's recorded at 720p whatever the trailer's size.
    "editor": (["editor", "--demo", "--size", "1280x720"], 0.3, 9.4),
    # (The track the editor demo saved.)
    "custom": (RACE + ["--track", "custom:biscuit-bends"], 16.0, 1.6),
    "title": (RACE + ["--track", "circuit", "--no-hud", "--cam", "5,1.5,180,12"], 29.6, 4.0),
}

# The cut: (clip, seconds into the clip, beats it's on screen for, seconds of clip a second).
SHOTS = [
    # 1-2: the start lights
    ("lights", 0.0, 8, 1),
    # 3-12: the tracks
    ("circuit", 3.6, 4, 1),
    ("circuit-pack", 0.0, 4, 1),
    ("prism-jump", 0.0, 4, 1),
    ("prism-orbit", 0.0, 4, 1),
    ("lagoon", 3.4, 4, 1),
    ("lagoon-low", 0.0, 4, 1),
    ("jungle", 0.0, 4, 1),
    ("jungle-hall", 0.0, 4, 1),
    ("tbilisi-bridge", 0.0, 4, 1),
    ("tbilisi-town", 0.0, 4, 1),
    # 13-14: items
    ("zoomies", 0.0, 2, 1),
    ("sausage", 0.0, 2, 1),
    ("balloons", 0.0, 2, 1),
    ("lagoon-hit", 0.0, 2, 1),
    # 15-16: drifting, split screen
    ("meadow", 0.2, 4, 1),
    ("split", 0.0, 4, 1),
    # 17-20: the editor, sped up to fit
    ("editor", 0.0, 16, 9.4 / (4 * BAR)),
    # 21-22: racing the new track, then a trick a beat
    ("custom", 0.0, 4, 1),
    ("meadow", 7.8, 1, 1),
    ("prism-trick", 0.1, 1, 1),
    ("circuit", 0.1, 1, 1),
    ("lagoon", 0.1, 1, 1),
    # 23-24: the title
    ("title", 0.0, 10, 1),
]

# Captions: (text, bar it appears on, bars it stays).
CAPTIONS = [
    ("SIX TRACKS", 5, 2),
    ("RAY TRACED", 9, 4),
    ("15 ITEMS", 13, 2),
    ("DRIFT. BOOST. TRICK.", 15, 1),
    ("4-PLAYER SPLIT SCREEN", 16, 1),
    ("BUILD YOUR OWN TRACKS", 17, 4),
    ("THEN RACE THEM", 21, 1),
]
TITLE_AT = 22 * BAR


def run(cmd, **kw):
    subprocess.run(cmd, check=True, **kw)


def record(name):
    """Record a clip, unless it's there already from the same command."""
    args, start, secs = CLIPS_TO_RECORD[name]
    path = os.path.join(CLIPS, name + ".mp4")
    cmd = [GAME] + args + ["--record", path, "--record-from", str(start), "--record-secs", str(secs)]
    if "--size" not in args:
        cmd += ["--size", f"{SIZE[0]}x{SIZE[1]}"]
    cmd += ["--no-raytracing"] if DRAFT or args[0] == "editor" else ["--raytracing", "--dlss", "quality"]
    stamp = path + ".cmd"
    key = hashlib.sha1(" ".join(cmd).encode()).hexdigest()
    if os.path.exists(path) and os.path.exists(stamp) and open(stamp).read() == key:
        return path
    print(f"recording {name} ...", flush=True)
    env = dict(os.environ, XDG_CONFIG_HOME=CONFIG)
    if name == "editor":
        # So the demo saves its track under the same name each time.
        shutil.rmtree(os.path.join(CONFIG, "corgikart", "tracks"), ignore_errors=True)
    run(cmd, cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    open(stamp, "w").write(key)
    return path


def text(s, size, y, start, end, colour="white"):
    """A drawtext filter: `s` centred at height `y` (a fraction of the frame), fading in and out."""
    fade = f"min(1,(t-{start:.3f})/0.1)*min(1,({end:.3f}-t)/0.1)"
    px = round(size * SIZE[1] / 1080)
    s = s.replace(":", r"\:").replace("'", "")
    return (
        f"drawtext=fontfile={FONT}:text='{s}':fontsize={px}:fontcolor={colour}:alpha='{fade}':"
        f"borderw={max(2, px // 14)}:bordercolor=0x14141e:x=(w-text_w)/2:y=h*{y}-text_h/2:"
        f"enable='between(t,{start:.3f},{end:.3f})'"
    )


def main():
    os.makedirs(os.path.join(CONFIG, "corgikart"), exist_ok=True)
    only = sys.argv[sys.argv.index("--only") + 1 :] if "--only" in sys.argv else None
    # The editor first: "custom" races the track it saves.
    names = ["editor"] + [n for n in CLIPS_TO_RECORD if n != "editor"]
    paths = {n: record(n) for n in names if only is None or n in only}
    if only is not None:
        return

    music = os.path.join(CLIPS, "music.wav")
    run([sys.executable, os.path.join(HERE, "music.py"), music])

    inputs, graph, pads = [], [], []
    for i, (clip, offset, beats, speed) in enumerate(SHOTS):
        secs = beats * BEAT
        inputs += ["-i", paths[clip]]
        graph.append(
            f"[{i}:v]trim=start={offset}:duration={secs * speed:.4f},setpts=(PTS-STARTPTS)/{speed:.4f},"
            f"scale={SIZE[0]}:{SIZE[1]}:flags=lanczos,fps=60,setsar=1,format=yuv420p[v{i}]"
        )
        if speed == 1:
            graph.append(f"[{i}:a]atrim=start={offset}:duration={secs:.4f},asetpts=PTS-STARTPTS,aresample=44100,apad=whole_dur={secs:.4f}[a{i}]")
        else:
            graph.append(f"anullsrc=r=44100:cl=mono,atrim=duration={secs:.4f}[a{i}]")
        pads.append(f"[v{i}][a{i}]")
    total = sum(s[2] for s in SHOTS) * BEAT
    graph.append("".join(pads) + f"concat=n={len(SHOTS)}:v=1:a=1[cut][sfx]")

    # The title darkens the picture behind it, and everything fades out at the end.
    look = [f"eq=brightness='-0.22*clip((t-{TITLE_AT})/0.3,0,1)':eval=frame"]
    look += [text(s, 104, 0.76, (bar - 1) * BAR + 0.03, (bar - 1 + bars) * BAR - 0.03) for s, bar, bars in CAPTIONS]
    look += [
        text("corgiKart", 250, 0.42, TITLE_AT, total, colour="0xff9a2e"),
        text("a very good kart racer", 70, 0.62, TITLE_AT + BEAT, total),
        text("split screen  /  LAN multiplayer  /  track editor", 44, 0.9, TITLE_AT + 2 * BEAT, total),
        f"fade=t=out:st={total - 0.6:.3f}:d=0.6",
    ]
    graph.append("[cut]" + ",".join(look) + "[v]")
    m = len(SHOTS)
    graph.append(
        f"[sfx]volume=0.5,pan=stereo|c0=c0|c1=c0[fx];[{m}:a]atrim=duration={total:.3f}[mu];"
        f"[mu][fx]amix=inputs=2:normalize=0,alimiter=limit=0.95,afade=t=out:st={total - 0.8:.3f}:d=0.8[a]"
    )

    quality = ["-crf", "28", "-preset", "veryfast"] if DRAFT else ["-crf", "20", "-preset", "slow"]
    run(
        ["ffmpeg", "-loglevel", "error", "-y"] + inputs + ["-i", music, "-filter_complex", ";".join(graph)]
        + ["-map", "[v]", "-map", "[a]", "-c:v", "libx264"] + quality
        + ["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart", "-t", f"{total:.3f}", OUT]
    )
    print(f"{OUT}: {total:.1f}s")


if __name__ == "__main__":
    main()
