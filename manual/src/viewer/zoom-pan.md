# Zoom and pan

Shared across every image-rendered view: raster images, animated GIF / WebP, animated SVG, PDF
**Read**, CBZ **Read**, and font specimens.

## Keys

| Key       | Action                                                      |
|-----------|-------------------------------------------------------------|
| `+` / `-` | Zoom in / out (1.25× per step, capped at 16×)               |
| `0`       | Reset zoom to 1× and pan to origin                          |
| `1`..`9`  | Jump to whole-number zoom (1× .. 9×)                        |
| Arrows    | Pan the zoomed viewport (or fit-mode scroll at 1×)          |

## Anchoring

Zoom anchors on the viewport centre — the pixel under the centre stays put across `+` / `-`,
so the content you're looking at stays under your eyes instead of drifting toward a corner.

## Rendering

The visible viewport's pixel ROI is cropped from the native-resolution source and rescaled per
draw, so memory stays viewport-sized at any zoom level — a 200 MP photo at 9× costs the same
working set as the same photo at 1×.

For SVG, the source is re-rasterised as zoom grows so glyphs stay crisp rather than turning
into upscaled pixels.

## Pan vs scroll

Arrow keys do different things depending on whether the view is zoomed:

- **Zoomed in (> 1×)** — arrows pan the zoomed viewport.
- **At 1×** — arrows fall back to fit-mode scroll: `FitWidth` lets vertical arrows scroll the
  overflow, `FitHeight` lets horizontal arrows scroll, `Contain` shows the whole frame so
  arrows are inert.

## Print mode

Pipe output and `--print` always render at 1× — zoom is interactive-only.
