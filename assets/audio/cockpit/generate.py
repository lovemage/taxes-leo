"""Generate original, restrained poker SFX drafts using only Python's standard library.

Run: python3 assets/audio/cockpit/generate.py
PCM WAV, 48 kHz mono. No downloaded samples; fixed seeds reproduce the drafts.
"""
from pathlib import Path
import math
import random
import struct
import wave

RATE = 48000
OUT = Path(__file__).resolve().parent


def noise(length, seed):
    rng = random.Random(seed)
    smoothed = 0.0
    values = []
    for _ in range(length):
        smoothed += 0.22 * (rng.uniform(-1, 1) - smoothed)
        values.append(smoothed)
    return values


def save(name, values, peak_db):
    # Gentle endpoint fades avoid playback clicks. Asset peaks remain intentionally low.
    fade = min(240, len(values) // 2)
    for i in range(fade):
        values[i] *= i / fade
        values[-1-i] *= i / fade
    peak = max(abs(x) for x in values)
    gain = 10 ** (peak_db / 20) / max(peak, 1e-12)
    pcm = [round(max(-1, min(1, x * gain)) * 32767) for x in values]
    with wave.open(str(OUT / name), 'wb') as output:
        output.setparams((1, 2, RATE, 0, 'NONE', 'not compressed'))
        output.writeframes(struct.pack('<' + 'h' * len(pcm), *pcm))
    rms = math.sqrt(sum((x / 32767) ** 2 for x in pcm) / len(pcm))
    print(f'{name}: {len(pcm)/RATE:.3f}s, peak {peak_db} dBFS, RMS {20*math.log10(max(rms,1e-12)):.1f} dBFS')


def generate():
    duration = 0.19
    scratch = noise(int(duration * RATE), 907)
    deal = []
    for i, sample in enumerate(scratch):
        t = i / RATE
        envelope = math.sin(math.pi * t / duration) ** 1.7
        contact = math.sin(2 * math.pi * 430 * t) * math.exp(-max(0, t-.13)*110) if t >= .13 else 0
        deal.append(sample * envelope + .025 * contact)
    save('deal-soft-v1.wav', deal, -20)

    collect = [0.0] * int(.46 * RATE)
    for j, onset in enumerate([0, .032, .073, .118, .175, .238]):
        grain = noise(int(.13 * RATE), 100+j)
        start = round(onset * RATE)
        for i, sample in enumerate(grain):
            t = i / RATE
            attack = min(1, t/.002)
            tap = (sample * .45 + math.sin(2*math.pi*(1250+j*95)*t)*.14 + math.sin(2*math.pi*2600*t)*.04)
            collect[start+i] += tap * attack * math.exp(-t*65) * (1-j*.07)
    save('chips-collect-soft-v1.wav', collect, -18)

    win = [0.0] * int(.72 * RATE)
    for onset, hz, amplitude in [(0, 523.25, .7), (.11, 659.25, .55), (.23, 783.99, .4)]:
        start = round(onset * RATE)
        for i in range(start, len(win)):
            t = (i-start)/RATE
            win[i] += amplitude * min(1,t/.012) * math.exp(-t*10) * (math.sin(2*math.pi*hz*t)+.1*math.sin(2*math.pi*hz*2*t))
    save('win-subtle-v1.wav', win, -23)


if __name__ == '__main__':
    generate()
