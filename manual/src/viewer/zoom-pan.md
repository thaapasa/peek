# Zoom and pan

Zoom and pan work the same way in every view that renders an image. These views include raster
images, animated GIF and WebP images, animated SVG images, the PDF **Read** view, the CBZ **Read**
view, and font specimens.

## Keys

| Key      | Action                                                  |
| -------- | ------------------------------------------------------- |
| `+`, `-` | Zoom in or out by 1.25× per step, up to 16×             |
| `0`      | Reset the zoom to 1× and the pan to the origin          |
| `1`..`9` | Jump to a whole-number zoom level, from 1× to 9×        |
| Arrows   | Pan the zoomed viewport. At 1×, scroll in the fit mode. |

## Anchoring

Zoom anchors on the center of the viewport. When you press `+` or `-`, the pixel at the center stays
in place. The content that you're looking at stays in front of you instead of drifting toward a
corner.

## Rendering

On each draw, peek crops the visible region from the source image at its native resolution and
scales it to fit. This keeps memory use at the size of the viewport at any zoom level. A 200
megapixel photo at 9× uses as much working memory as the same photo at 1×.

For SVG images, peek rasterizes the source again as the zoom grows. Glyphs stay sharp instead of
turning into enlarged pixels.

## Panning and scrolling

The arrow keys work differently depending on whether the view is zoomed:

- **Zoomed in past 1×**: the arrow keys pan the zoomed viewport.
- **At 1×**: the arrow keys scroll according to the fit mode:
  - In `FitWidth`, the vertical arrow keys scroll the part that doesn't fit.
  - In `FitHeight`, the horizontal arrow keys scroll.
  - `Contain` shows the whole frame, so the arrow keys do nothing.

## Print mode

Piped output and `--print` always render at 1×. Zoom works only in the interactive viewer.
