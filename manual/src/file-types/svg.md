# SVG

SVG (`.svg`) is vector; rasterized via [resvg](https://github.com/RazrFalcon/resvg) before
ASCII rendering.

Two viewing modes (cycle with Tab):

- **Rendered preview** (default) — rasterized and run through the image pipeline.
- **Source** — XML syntax-highlighted (pretty or raw).

Re-renders on terminal resize.

## Animation

CSS `@keyframes` animation is supported. The parser collects each `@keyframes` rule plus
inline-style `animation-*` references on elements, builds a merged frame timeline (one frame
per stop for `steps()` timing, ~30 fps interpolated for `linear`), and rasterizes each frame on
demand. A bounded LRU cache (64 entries) keeps a full second loop free.

Covers what termsvg / asciinema-svg-style files use: `transform: translateX/Y/translate` under
`steps()` or `linear` timing, inline-style targets only. SMIL (`<animate>`, `<animateMotion>`)
and class/id-selector targets are not supported. `--no-svg-anim` forces the static render.

`Space` plays / pauses, `n` / `p` step frames, `e` extracts the current frame as PNG.

The Info panel reports frame count, total duration, and looping vs one-shot.

## Info view

viewBox, declared dimensions, element counts (paths, groups, rects, circles, text), script /
external-href flags, animation summary, plus source text stats.
