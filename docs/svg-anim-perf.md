# SVG animation: performance findings and optimization options

> **Status: planning notes (active).** This is a working doc for the queue of SVG animation
> performance work. It holds measurements and proposed optimizations. It doesn't describe shipped
> behavior. Move it to `docs/archived/` when the queue is empty.

These are phase 1 status notes for `types/image/pipeline/svg_anim/` and
`types/svg/animation_mode.rs`. The numbers come from a release build on a 131×40 terminal. The test
file was `~/Downloads/demo.svg`, a termsvg recording with these properties:

- A 1550×928.4 viewport
- 55 stops in its `@keyframes` rule, over 14.586 seconds
- `steps(1, end)` timing

## Memory profile

The table shows the resident set size (RSS) in each state.

| State                          | RSS     | Notes                                                    |
| ------------------------------ | ------- | -------------------------------------------------------- |
| `peek <bitmap.png>` (baseline) | ~27 MB  | Glyph atlas, syntect, theme, and image decode            |
| `peek <demo.svg>` cold start   | ~40 MB  | Adds `fontdb::load_system_fonts()` (font metadata index) |
| Demo, after ~2 frames rendered | ~100 MB | LRU cache partly filled                                  |
| Demo, after 1 full loop        | ~200 MB | LRU cache full: 55 frames × ~3 MB                        |
| Demo, replay (steady state)    | ~200 MB | All cache hits, no allocations                           |

The frame cache is a least recently used (LRU) cache.

You can't avoid `fontdb::load_system_fonts()`. Without it, `<text>` glyphs render as nothing. The
cost comes once, at the first SVG parse. All rasterizations share the result through an
`OnceLock<Arc<fontdb::Database>>` in `types/image/pipeline/svg.rs`.

### Cache size formula

For a grid of `cols × rows` cells in the `Full`, `Block`, `Geo`, and `Contour` modes, the prepared
image is `cols * 8` × `rows * 16` pixels. Each RGBA pixel takes 4 bytes.

```
bytes_per_frame ≈ cols * rows * 8 * 16 * 4 = cols * rows * 512
```

`PreparedImage` holds both the raw image and a composited intermediate, so the real working set is
about twice that. The marker-substituted SVG strings add to this, but they're freed promptly.

| Term size | Per-frame | LRU=64 total |
| --------- | --------- | ------------ |
| 80×24     | ~1 MB     | ~65 MB       |
| 131×40    | ~3 MB     | ~165 MB      |
| 200×60    | ~6 MB     | ~370 MB      |
| 300×80    | ~12 MB    | ~770 MB      |

The LRU cache is bounded by **frame count**, not by bytes. A big terminal and an animation with many
frames can make the RSS balloon.

## First-loop cost

The user sees the first iteration of the demo run **noticeably slowly**, even in release builds.
Each frame that misses the cache costs about 50–200 ms, depending on how much text is visible.

The cost of each frame breaks down like this, from the top:

1. **Marker substitution** (`render_frame`) is cheap. It runs two `String::replace` passes for each
   target on a string of about 58 KB, and takes less than a millisecond.
2. **`usvg::Tree::from_data`** parses the whole SVG again for every frame. That covers tokenization,
   building the document object model (DOM), resolving CSS, and shaping the font for each `<text>`
   element. The demo has hundreds of `<text>` elements, one for each terminal cell of typed output,
   so text shaping dominates this step.
3. **`resvg::render`** rasterizes with `tiny_skia` at full pixel resolution. Content outside the
   canvas gets clipped, which is cheap, but each visible glyph is rasterized on its own.
4. **`prepare_decoded`** does only the alpha composite against the background for SVG. It skips the
   margin padding and the Lanczos resize, because peek rasterizes SVG directly at the target size.
5. **`render_prepared`** runs the glyph matching pass over the cells. In `Full` mode, it runs
   `best_glyph` for each cell against the prepared atlas. The cost is linear in the number of cells,
   and the step is fast.

Step 2 is the biggest target. resvg has no incremental API, so the same SVG bytes cost the same
parse work on every call.

## Optimization options

The options are ranked roughly by impact against effort. None of them is implemented.

### A. Byte-budget LRU (small, defensive)

Today `FRAME_CACHE = 64` sets a frame count. Replace it with a byte budget such as
`MAX_CACHE_BYTES = 64 MB`, and count `cols * rows * 512 * 2` for each entry. Evict from the front
until the cache is under budget.

- **Impact.** This caps memory, whatever the size of the terminal or the animation.
- **Effort.** About 20 lines in `SvgAnimationMode::prepare_current`.
- **Caveat.** Tiny terminals get more cached frames, which is good. Huge terminals get fewer. Replay
  can stop being free for big animations.

### B. Background pre-rasterization (smooth first loop)

Start a worker that walks the frame timeline ahead of the playhead and fills the LRU cache. The
render thread keeps reading live input. When the play cursor reaches a frame, the frame is already
prepared.

- **Impact.** This hides the 50–200 ms cost of each frame behind idle time. The first loop looks as
  smooth as steady-state replay.
- **Effort.** Medium. It needs these parts:
  - A worker thread (`std::thread`).
  - A way to push prepared frames back into the cache without holding the mode's `&mut self`, such
    as a cache wrapped in a mutex or a `crossbeam-channel`.
  - Cancellation when the user toggles fit, mode, or background, which wipes the cache.
- **Caveat.** This doesn't reduce the _total_ CPU work. It only moves when the work happens. Users
  on battery power might dislike this.
- **Caveat.** The terminal is interactive, so the worker must not starve the input loop. A worker
  thread helps here, compared to a blocking poll.

### C. Reuse the parsed `usvg::Tree` and change the transform for each frame

This is the big structural fix. Today peek builds a fresh SVG string for each frame and calls
`Tree::from_data`. The fix works like this instead:

1. Parse the **unmarked** SVG once into a `usvg::Tree`.
2. After the parse, find the animated subtree in the tree. That's the `<g transform="...">` that
   peek would inject.
3. For each frame, change that node's transform and call `resvg::render`.

usvg 0.47 exposes `Tree::root_mut()`, and node transforms are mutable. At parse time, peek can wrap
the children of the animated element in its own transformed `<g>` and keep a handle to that `<g>`.
It can then update the transform without parsing again.

- **Impact.** The largest. This removes step 2 completely after the first frame, so the cost of text
  shaping is spread across all frames. The estimate is a 5–20× speedup per frame for animations with
  a lot of text.
- **Effort.** The largest. It needs these changes:
  - Convert the byte-marker injection to a change of the `usvg::Tree` after the parse. The other
    option is a minimal hand-written change that inserts the wrapper before the parse, because the
    tree mutation API of usvg is small.
  - Handle animations with many targets, which need many wrapper nodes and many handles.
  - Fit this with the `prepare_svg` pipeline. That pipeline takes bytes today, so it would need a
    `prepare_svg_tree(&Tree, ...)` variant.

### D. Rasterize at half resolution and scale up to the grid

The ASCII-art output is heavily downsampled anyway. A 1048×624 pixel image becomes 131×39 cells,
which cuts the area about 25 times in the glyph matching step. Rasterize at half resolution
(524×312), then scale up to the prepared size with nearest-neighbor or bilinear sampling before the
glyph match.

- **Impact.** About 4× faster in the rasterize step (step 3). It doesn't help the expensive step 2,
  because text shaping in usvg doesn't depend on resolution. The net gain is about 1.5–2× per frame.
- **Effort.** Small. Add a render quality setting in `ImageConfig`, rasterize at half size, and
  scale with `image::imageops::resize`.
- **Caveat.** The quality drops visibly in the `Full` and `Geo` modes, which show less glyph detail.
  The `Contour` and `Ascii` modes are mostly unaffected.

### E. Fill the whole cache when the file opens

For animations with `frames < N`, compute every frame into the LRU cache when the file opens, before
the first frame shows.

- **Impact.** The slow first loop turns into a known pause at startup. Some users prefer to wait 5
  seconds and then get smooth playback, rather than watch 5 seconds of stutter.
- **Effort.** Tiny.
- **Caveat.** `frames` can be large with `linear` timing. This needs a guard that skips the
  precompute unless `frames * grid_bytes < budget`.

## Recommended order

If someone returns to this performance work, take the options in this order:

1. **(A)** The byte-budget LRU cache. It's defensive and prevents pathological cases with big
   animations or big terminals.
2. **(C)** Tree reuse. It's the biggest single win for first-loop latency.
3. **(B)** Background pre-rasterization. It's polish on top of (C).
4. **(D)** or **(E)**, if (C) and (B) together aren't enough.

(D) is a good win on its own for the `Contour` and `Ascii` modes in particular. In those modes, the
rasterize cost goes into output that gets quantized hard right away.
