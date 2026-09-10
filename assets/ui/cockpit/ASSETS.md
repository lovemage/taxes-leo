# Cockpit assets · 2026-09-09

## Graphite material

`graphite-brushed-v1.png` is an original image generated with the built-in imagegen tool. It has been copied into this repository; it is not wired into the application yet.

Use on instrument housings and panel frames at low opacity, beneath a dark overlay. Keep text, tables and controls in real HTML. Generated output was visually inspected; perfect seamless tiling is not certified. Initially use one cropped surface per panel, then verify seams before enabling repeat. Actual dimensions should be read from the file, not inferred from the requested size.

Prompt:

```text
Use case: stylized-concept. Asset type: reusable background material texture for a desktop poker simulation spaceship cockpit dashboard. Generate a single square 1024x1024 flat orthographic surface swatch, edge-to-edge dark graphite anodized aluminum, extremely fine horizontal brushed grain and subtle matte ceramic microtexture, neutral cool charcoal, very low contrast, evenly lit with no directional hotspot, intended underneath crisp real HTML controls and numbers. Seamless repeat intent, uniform edges, no perspective, no panel seams, no bevel border, no objects, no screws, no labels, no letters, no numbers, no stars, no neon, no logo, no watermark. Refined functional spacecraft instrument panel material, restrained and readable.
```

## Sound drafts

`../../audio/cockpit/` contains three original synthesized WAV drafts and their deterministic Python source. They use no downloaded recordings. Generated with 48 kHz, mono, 16-bit PCM, with edge fades and low asset peaks. These files have not yet been auditioned in the application or mixed against its animation timing.

- `deal-soft-v1.wav`: 0.19 s, paper-like filtered noise and soft landing, peak −20 dBFS.
- `chips-collect-soft-v1.wav`: 0.46 s, six short overlapping chip taps, peak −18 dBFS.
- `win-subtle-v1.wav`: 0.72 s, short three-note confirmation, peak −23 dBFS.

These are candidate game SFX, not real-world recordings. In-app listening on speakers and headphones is still required. Use a separate gain per sound and one master gain; do not normalize all three to equal peaks. Play the win cue once per hero winning hand, not once per payout transfer. Keep refunds silent apart from optional chip movement.
