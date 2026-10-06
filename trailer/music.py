#!/usr/bin/env python3
"""The trailer's music, synthesised from scratch like the game's own sound: music.py <out.wav>

150 beats a minute, so a beat is 24 frames of video and a bar is 1.6 seconds: the trailer's
cuts (see make.py) fall on bars and beats. Twenty-four bars:

     1-2   a filtered arpeggio and a snare roll, under the start lights
     3-16  the tune, nearly three times through, over drums and bass
    17-20  a breakdown for the track editor, building back up in bar 20
    21-22  the tune again, up an octave
    23-24  the last chord
"""

import sys
import wave

import numpy as np

RATE = 44100
BPM = 150
BEAT = 60 / BPM
BAR = 4 * BEAT
S16 = BEAT / 4  # a sixteenth note
BARS = 24
N = int(RATE * (BARS * BAR + 1.0))

rng = np.random.default_rng(7)

NAMES = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def freq(note):
    """'A4' -> 440."""
    semis = NAMES[note[0]] + (1 if "#" in note else 0) + 12 * (int(note[-1]) + 1)
    return 440 * 2 ** ((semis - 69) / 12)


def shape(x, cutoff, high=False, order=2):
    """A low-pass (or high-pass) filter; `cutoff` may be one frequency or one per sample."""
    if np.ndim(cutoff):
        # A sweep: blend between a few fixed filters.
        steps = np.geomspace(cutoff.min(), cutoff.max(), 12)
        bank = np.stack([shape(x, c, high, order) for c in steps])
        pos = np.interp(np.log(cutoff), np.log(steps), np.arange(len(steps)))
        lo = np.floor(pos).astype(int).clip(0, len(steps) - 2)
        idx = np.arange(len(x))
        return bank[lo, idx] * (1 - (pos - lo)) + bank[lo + 1, idx] * (pos - lo)
    spec = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / RATE)
    ratio = (f / cutoff) ** (2 * order)
    spec *= np.sqrt(ratio / (1 + ratio)) if high else 1 / np.sqrt(1 + ratio)
    return np.fft.irfft(spec, len(x))


def osc(f, n, kind, width=0.5):
    """`n` samples at frequency `f` (a number or an array): 'sq'uare, 'saw', 'tri' or 'sin'."""
    phase = np.cumsum(np.broadcast_to(f, n)) / RATE
    frac = phase % 1.0
    if kind == "sq":
        return np.where(frac < width, 1.0, -1.0)
    if kind == "saw":
        return frac * 2 - 1
    if kind == "tri":
        return 1 - 4 * np.abs(frac - 0.5)
    return np.sin(2 * np.pi * phase)


def env(n, attack, release, hold=None):
    """Rise over `attack` seconds, hold, and die away over the last `release`."""
    t = np.arange(n) / RATE
    length = n / RATE if hold is None else hold
    return np.clip(t / attack, 0, 1) * np.clip((length - t) / release, 0, 1)


class Track:
    def __init__(self):
        self.buf = np.zeros((N, 2))

    def add(self, when, mono, gain=1.0, pan=0.0):
        i = int(when * RATE)
        mono = mono[: N - i]
        left, right = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
        self.buf[i : i + len(mono), 0] += mono * gain * left
        self.buf[i : i + len(mono), 1] += mono * gain * right

    def echo(self, delay, feedback, mix):
        """A delay that bounces from side to side."""
        d = int(delay * RATE)
        wet = np.zeros_like(self.buf)
        tap = self.buf.copy()
        for k in range(5):
            tap = np.roll(tap, d, axis=0)[:, ::-1] * feedback
            tap[:d] = 0
            wet += tap
        self.buf += wet * mix


def at(bar, sixteenth=0.0):
    """Seconds to this sixteenth of this bar (bars count from 1)."""
    return (bar - 1) * BAR + sixteenth * S16


# ----- Drums ------------------------------------------------------------------------------------

def kick():
    n = int(0.32 * RATE)
    t = np.arange(n) / RATE
    f = 46 + 130 * np.exp(-t * 38)
    body = np.sin(2 * np.pi * np.cumsum(f) / RATE) * np.exp(-t * 11)
    click = rng.standard_normal(n) * np.exp(-t * 400) * 0.25
    return np.tanh((body + click) * 1.8)


def snare(length=0.2):
    n = int(length * RATE)
    t = np.arange(n) / RATE
    noise = shape(rng.standard_normal(n), 1800, high=True) * np.exp(-t * 24)
    tone = np.sin(2 * np.pi * (185 + 60 * np.exp(-t * 50)) * t) * np.exp(-t * 32)
    return np.tanh(noise * 0.5 + tone * 0.7)


def hat(open_=False):
    n = int((0.22 if open_ else 0.05) * RATE)
    t = np.arange(n) / RATE
    return shape(rng.standard_normal(n), 7000, high=True) * np.exp(-t * (16 if open_ else 90)) * 0.5


def clap():
    n = int(0.18 * RATE)
    t = np.arange(n) / RATE
    bursts = sum(np.exp(-np.clip(t - d, 0, None) * 160) * (t >= d) for d in (0.0, 0.011, 0.023))
    return shape(shape(rng.standard_normal(n), 900, high=True), 4500) * (bursts * 0.5 + np.exp(-t * 28)) * 0.6


def riser(seconds):
    """Noise that opens up, for the build before a drop."""
    n = int(seconds * RATE)
    ramp = np.linspace(0, 1, n)
    return shape(rng.standard_normal(n), 300 * 30 ** ramp, high=True) * ramp ** 2 * 0.5


def crash():
    n = int(1.8 * RATE)
    t = np.arange(n) / RATE
    return shape(rng.standard_normal(n), 3500, high=True) * np.exp(-t * 2.6) * 0.5


# ----- The score --------------------------------------------------------------------------------

#                C      G      Am     F
CHORDS = {
    "C": ("C2", ["C4", "E4", "G4"]),
    "G": ("G1", ["B3", "D4", "G4"]),
    "Am": ("A1", ["C4", "E4", "A4"]),
    "F": ("F1", ["C4", "F4", "A4"]),
}

# One chord a bar (bars 10, 14 and 22 turn round through G in their second half).
HARMONY = ["C", "C"] + ["C", "G", "Am", "F"] * 4 + ["C", "G", "C", "G", "C", "C"]

# The tune: (note, length in sixteenths), a bar a line.
TUNE = [
    [("G5", 2), ("E5", 2), ("G5", 2), ("C6", 4), ("B5", 2), ("C6", 2), ("D6", 2)],
    [("D6", 4), ("B5", 2), ("G5", 2), ("B5", 2), ("D6", 2), ("G6", 4)],
    [("E6", 2), ("D6", 2), ("C6", 2), ("A5", 4), ("C6", 2), ("E6", 2), ("D6", 2)],
    [("C6", 4), ("A5", 2), ("F5", 2), ("A5", 2), ("C6", 2), ("D6", 2), ("E6", 2)],
    [("E6", 2), ("G6", 2), ("E6", 2), ("C6", 4), ("D6", 2), ("E6", 2), ("G6", 2)],
    [("D6", 4), ("G6", 2), ("D6", 2), ("B5", 2), ("D6", 2), ("G6", 4)],
    [("A6", 2), ("G6", 2), ("E6", 2), ("C6", 4), ("E6", 2), ("G6", 2), ("A6", 2)],
    [("F6", 4), ("A6", 4), ("G6", 4), ("B6", 4)],
]
# Which line of the tune each bar plays (bars 3-16 and 21-22).
TUNE_BARS = {3: 0, 4: 1, 5: 2, 6: 3, 7: 4, 8: 5, 9: 6, 10: 7, 11: 0, 12: 1, 13: 2, 14: 3, 15: 4, 16: 5, 21: 4}
LAST_RUN = [("G6", 2), ("A6", 2), ("B6", 2), ("D7", 2), ("G6", 1), ("A6", 1), ("B6", 1), ("D7", 1), ("G7", 4)]


def lead_note(note, sixteenths):
    n = int((sixteenths * S16 + 0.06) * RATE)
    t = np.arange(n) / RATE
    f = freq(note) * (1 + 0.006 * np.sin(2 * np.pi * 6 * t) * np.clip(t / 0.15, 0, 1))
    tone = osc(f, n, "sq", 0.5) * 0.6 + osc(f * 1.006, n, "sq", 0.25) * 0.4 + osc(f / 2, n, "tri") * 0.3
    return shape(tone, 6500) * env(n, 0.004, 0.05) * (0.75 + 0.25 * np.exp(-t * 9))


def pluck(note, length=0.16, width=0.25):
    n = int(length * RATE)
    t = np.arange(n) / RATE
    return shape(osc(freq(note), n, "sq", width), 5000) * np.exp(-t * 16) * env(n, 0.002, 0.02)


def bass_note(note, sixteenths):
    n = int(sixteenths * S16 * RATE * 0.92)
    t = np.arange(n) / RATE
    f = freq(note)
    tone = osc(f, n, "saw") * 0.6 + osc(f, n, "sq", 0.4) * 0.4
    return (shape(tone, 900) + osc(f, n, "sin") * 0.8) * env(n, 0.004, 0.03) * (0.7 + 0.3 * np.exp(-t * 14))


def pad(notes, seconds):
    n = int(seconds * RATE)
    tone = sum(osc(freq(x) * d, n, "saw") for x in notes for d in (0.996, 1.004))
    return shape(tone, 2200) * env(n, 0.03, 0.12) / len(notes)


def up(note, octaves=1):
    return note[:-1] + str(int(note[-1]) + octaves)


def compose():
    drums, bass, chords, arp, lead = Track(), Track(), Track(), Track(), Track()
    kicks = []

    def beat(bar, sixteenth, sound, gain, pan=0.0):
        drums.add(at(bar, sixteenth), sound, gain, pan)

    def groove(bar, fill=False):
        for b in range(4):
            beat(bar, b * 4, kick(), 1.0)
            kicks.append(at(bar, b * 4))
            beat(bar, b * 4 + 2, hat(open_=True), 0.5, 0.25)
        for b in (1, 3):
            beat(bar, b * 4, snare(), 0.8)
            beat(bar, b * 4, clap(), 0.55, -0.15)
        for s in range(16):
            beat(bar, s, hat(), 0.32 if s % 2 else 0.22, 0.25)
        if fill:
            for s in (12, 13, 14, 14.5, 15, 15.5):
                beat(bar, s, snare(0.12), 0.55 + 0.03 * s)

    def harmony(bar):
        """The chords in this bar: (name, first sixteenth, length)."""
        if bar in (10, 14, 22):
            return [(HARMONY[bar - 1], 0, 8), ("G", 8, 8)]
        return [(HARMONY[bar - 1], 0, 16)]

    def backing(bar, bass_gain=1.0, arp_gain=1.0, octave=0):
        for name, start, length in harmony(bar):
            root, triad = CHORDS[name]
            for s in range(start, start + length, 2):
                # Root on the beat, up an octave off it.
                bass.add(at(bar, s), bass_note(up(root) if s % 4 else root, 2), 0.8 * bass_gain)
            chords.add(at(bar, start), pad(triad, length * S16), 0.5)
            tones = triad + [up(triad[0]), triad[2], triad[1]]
            for s in range(start, start + length):
                arp.add(at(bar, s), pluck(up(tones[s % len(tones)], 1 + octave)), 0.45 * arp_gain, (-0.6, 0.6)[s % 2])

    def tune(bar, line, octave=0, gain=0.8):
        s = 0
        for note, length in line:
            lead.add(at(bar, s), lead_note(up(note, octave) if octave else note, length), gain)
            s += length

    # 1-2: the start lights. A muffled arpeggio opens up, a snare roll, a riser.
    for bar in (1, 2):
        _, triad = CHORDS["C"]
        tones = triad + [up(triad[0])]
        for s in range(16):
            arp.add(at(bar, s), pluck(up(tones[s % 4])), 0.5, (-0.6, 0.6)[s % 2])
        chords.add(at(bar), pad(triad, BAR), 0.4)
        beat(bar, 0, kick(), 0.9)
        kicks.append(at(bar))
    opening = int(at(3) * RATE)
    sweep = 500 * 24 ** np.linspace(0, 1, opening)
    for t in (arp, chords):
        for ch in range(2):
            t.buf[:opening, ch] = shape(t.buf[:opening, ch], sweep)
    for s in range(16):
        beat(2, s, snare(0.12), 0.25 + 0.035 * s)
    for s in (12.5, 13.5, 14.5, 15.5):
        beat(2, s, snare(0.1), 0.7)
    drums.add(at(1, 8), riser(at(3) - at(1, 8)), 0.8)

    # 3-16: the tune.
    for bar in range(3, 17):
        groove(bar, fill=bar in (6, 10, 14, 16))
        backing(bar)
        tune(bar, TUNE[TUNE_BARS[bar]])
    for bar in (3, 7, 11, 15):
        beat(bar, 0, crash(), 0.6, -0.2)

    # 17-20: the editor. Plucks and claps, the kick comes back in 19, and 20 builds.
    beat(17, 0, crash(), 0.5)
    bells = [("E6", 0), ("C6", 3), ("A5", 6), ("E6", 8), ("D6", 11), ("C6", 14)]
    for bar in range(17, 21):
        backing(bar, bass_gain=0.0 if bar < 19 else 0.9, arp_gain=1.3)
        for b in (1, 3):
            beat(bar, b * 4, clap(), 0.7, -0.15)
        for s in range(0, 16, 2):
            beat(bar, s, hat(), 0.3, 0.25)
        for note, s in bells:
            lead.add(at(bar, s), pluck(note, 0.5, 0.5) , 0.5)
        if bar >= 19:
            for b in range(4):
                beat(bar, b * 4, kick(), 1.0)
                kicks.append(at(bar, b * 4))
    for s in range(16):
        beat(20, s, snare(0.12), 0.3 + 0.035 * s)
    for s in (8.5, 9.5, 10.5, 11.5, 12.5, 13.5, 14.5, 15.5):
        beat(20, s, snare(0.1), 0.4 + 0.03 * s)
    drums.add(at(19, 8), riser(at(21) - at(19, 8)), 0.9)

    # 21-22: the tune again, higher, and a run up to the last chord.
    for bar in (21, 22):
        groove(bar, fill=bar == 22)
        backing(bar, octave=1)
    beat(21, 0, crash(), 0.7, -0.2)
    tune(21, TUNE[TUNE_BARS[21]], gain=0.85)
    tune(21, TUNE[TUNE_BARS[21]], octave=-1, gain=0.4)
    tune(22, LAST_RUN, gain=0.85)

    # 23-24: the last chord, with a sparkle falling away over it.
    root, triad = CHORDS["C"]
    end = at(23)
    drums.add(end, kick(), 1.1)
    kicks.append(end)
    drums.add(end, crash(), 0.9)
    drums.add(end, snare(), 0.8)
    n = int(2.6 * RATE)
    fade = np.exp(-np.arange(n) / RATE * 1.3)
    chords.add(end, pad(triad + [up(triad[0])], 2.6) * fade, 0.9)
    bass.add(end, bass_note(root, 26) * fade[: int(26 * S16 * RATE * 0.92)], 0.9)
    lead.add(end, lead_note("C7", 12) * fade[: int((12 * S16 + 0.06) * RATE)], 0.8)
    for k, note in enumerate(["C7", "G6", "E6", "C6", "G5", "E5", "C5"]):
        arp.add(end + (k + 2) * S16 * 1.5, pluck(note, 0.4), 0.5 * 0.85 ** k, (-0.6, 0.6)[k % 2])

    # The kick ducks everything tuned, so it punches through.
    duck = np.ones(N)
    t = np.arange(int(0.3 * RATE)) / RATE
    dip = 1 - 0.55 * np.exp(-t * 14)
    for k in kicks:
        i = int(k * RATE)
        seg = dip[: N - i]
        duck[i : i + len(seg)] = np.minimum(duck[i : i + len(seg)], seg)

    arp.echo(3 * S16, 0.45, 0.35)
    lead.echo(3 * S16, 0.4, 0.28)
    mix = drums.buf * 0.9 + (bass.buf * 0.75 + chords.buf * 0.3 + arp.buf * 0.45 + lead.buf * 0.5) * duck[:, None]
    mix = np.tanh(mix * 1.15)
    return mix / np.abs(mix).max() * 0.89


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "music.wav"
    pcm = (compose() * 32767).astype("<i2")
    with wave.open(out, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    print(f"{out}: {len(pcm) / RATE:.1f}s")
