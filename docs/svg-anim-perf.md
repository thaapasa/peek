# SVG animation: performance findings + optimization options

Phase-1 status notes for `viewer/image/svg_anim.rs` +
`viewer/modes/svg_animation.rs`. Numbers measured against
`~/Downloads/demo.svg` (termsvg recording, 1550×928.4 viewport, 55
keyframe stops over 14.586s, `steps(1, end)` timing) on a 131×40
terminal. Release build.

## Memory profile

| State                                | RSS    | Notes                                                    |
|--------------------------------------|--------|----------------------------------------------------------|
| `peek <bitmap.png>` (baseline)       | ~27 MB | Glyph atlas + syntect + theme + image decode             |
| `peek <demo.svg>` cold start         | ~40 MB | + `fontdb::load_system_fonts()` (font metadata index)    |
| Demo, after ~2 frames rendered       | ~100 MB| LRU partially populated                                  |
| Demo, after 1 full loop              | ~200 MB| LRU full: 55 frames × ~3 MB                              |
| Demo, replay (steady state)          | ~200 MB| All hits, no allocations                                 |

`fontdb::load_system_fonts()` is unavoidable — without it `<text>`
glyphs drop to nothing. Cost is one-time at first SVG parse and shared
across all rasterizations via `OnceLock<Arc<fontdb::Database>>` in
`viewer/image/svg.rs`.

### Cache size formula

For a `cols × rows` cell grid with `Full` / `Block` / `Geo` / `Contour`
modes, prepared image pixels = `cols * 8` × `rows * 16`. RGBA = 4 bytes.

```
bytes_per_frame ≈ cols * rows * 8 * 16 * 4 = cols * rows * 512
```

Times two (raw + composited intermediate held by `PreparedImage`):
real working set is ~2× that. Plus marker-substituted SVG strings, which
are GC'd promptly.

| Term size | Per-frame | LRU=64 total |
|-----------|-----------|--------------|
| 80×24     | ~1 MB     | ~65 MB       |
| 131×40    | ~3 MB     | ~165 MB      |
| 200×60    | ~6 MB     | ~370 MB      |
| 300×80    | ~12 MB    | ~770 MB      |

LRU is currently bound by **frame count**, not bytes. Big terminal +
many-frame animation can balloon RSS.

## First-loop cost

User-visible: first iteration of demo is **noticeably slow**, even on
release builds. Each cache-miss frame costs ~50-200 ms depending on
visible text density.

Per-frame cost breakdown (top-down):

1. **Marker substitution** (`render_frame`): cheap. Two `String::replace`
   passes per target on a ~58 KB string. Sub-millisecond.
2. **`usvg::Tree::from_data`**: reparses the entire SVG every frame.
   Tokenization + DOM build + CSS resolution + per-`<text>`-element font
   shaping. Demo has hundreds of `<text>` elements (one per terminal
   cell of typed output) → text shaping dominates here.
3. **`resvg::render`** → `tiny_skia` rasterization at full pixel
   resolution. Off-pixmap content is naturally clipped (cheap), but
   visible glyphs are individually rasterized.
4. **`prepare_decoded`**: margin pad + Lanczos resize is skipped for
   SVG (we rasterize at the target size directly), so this is just the
   alpha composite against the background.
5. **`render_prepared`**: glyph-cell matching pass (`Full` mode runs
   `best_glyph` per cell against the prepared atlas). Linear in cells,
   fast.

Step 2 is the biggest target. resvg gives no incremental API: same SVG
bytes → same parse work each call.

## Optimization options

Ranked roughly by impact/effort. None are implemented.

### A. Byte-budget LRU (small, defensive)

Currently `FRAME_CACHE = 64` (count). Replace with `MAX_CACHE_BYTES = 64
MB` (or similar), summing `cols * rows * 512 * 2` per entry. Evict from
the front until under budget.

- **Impact**: caps memory regardless of terminal/anim size.
- **Effort**: ~20 lines in `SvgAnimationMode::prepare_current`.
- **Caveat**: tiny terminals get more cached frames (good); huge
  terminals fewer. Replay can become non-free for big anims.

### B. Background pre-rasterization (smooth first loop)

Spawn a worker that walks the frame timeline ahead of the playhead and
fills the LRU. The render thread keeps consuming live input; when the
play cursor reaches a frame, it's already prepared.

- **Impact**: hides the 50-200 ms per-frame cost behind idle time.
  First loop appears as smooth as steady-state replay.
- **Effort**: medium. Need a worker thread (`std::thread`), a way to
  push prepared frames back into the cache without holding the mode's
  `&mut self` (mutex-wrapped cache or `crossbeam-channel`), and
  cancellation when the user toggles fit/mode/background (cache wipe).
- **Caveat**: doesn't reduce *total* CPU, just shifts when it lands.
  Battery-powered users may dislike this.
- **Caveat**: terminal is interactive — need to avoid starving the input
  loop. Worker thread (not blocking-poll) helps.

### C. Reuse parsed `usvg::Tree`, mutate transform per frame

The big structural fix. Today we generate a fresh SVG string per frame
and call `Tree::from_data`. Instead:

1. Parse the **un-marked** SVG once into a `usvg::Tree`.
2. Locate the animated subtree (the `<g transform="...">` we'd inject)
   in the tree post-parse.
3. Per frame, mutate that node's transform; call `resvg::render`.

usvg 0.47 exposes `Tree::root_mut()` and node transforms are mutable.
Wrapping the animated element's children in a transformed `<g>` ourselves
(at parse time) and stashing a handle to that `<g>` lets us update its
transform without reparse.

- **Impact**: largest. Eliminates step 2 entirely after the first frame
  — text shaping is amortized across all frames. Estimate 5-20× speedup
  per frame for text-heavy anims.
- **Effort**: largest. Need to:
  - Convert byte-marker injection to `usvg::Tree` post-parse mutation
    (or hand-craft a minimal SVG mutation by inserting the wrap pre-parse,
    since usvg's tree mutation surface is small).
  - Handle multi-target anims (multiple wrap nodes, multiple handles).
  - Reconcile with the prepare_svg pipeline (which currently takes
    bytes; would need a `prepare_svg_tree(&Tree, ...)` variant).

### D. Half-resolution rasterize, upscale to grid

ASCII-art output is heavily downsampled anyway (1048×624 px → 131×39
cells, ~25× area reduction in the glyph-match step). Rasterize at half
resolution (524×312), then nearest/bilinear upscale to the prepared
size before glyph match.

- **Impact**: ~4× speedup on rasterize step (step 3), partial on the
  expensive step 2 (text shaping is resolution-independent in usvg, so
  no help there). Net ~1.5-2× per frame.
- **Effort**: small. Add a render-quality knob in `ImageConfig`,
  rasterize at half size, `image::imageops::resize`.
- **Caveat**: visible quality drop on `Full` / `Geo` modes (less glyph
  detail). `Contour` and `Ascii` mostly unaffected.

### E. Eager full-cache precompute on open

For animations with `frames < N`, precompute every frame at open time
into the LRU before showing the first frame.

- **Impact**: shifts the slow first loop into a known startup pause.
  Some users prefer "wait 5s, then smooth forever" over "stutter for
  5s while watching".
- **Effort**: tiny.
- **Caveat**: `frames` can be large for `linear` timing. Requires a
  guard (`frames * grid_bytes < budget` else skip).

## Recommended order

If we revisit perf:

1. **(A)** byte-budget LRU — defensive, prevents pathological cases on
   big anims/terminals.
2. **(C)** Tree reuse — biggest single win on first-loop latency.
3. **(B)** background pre-rasterize — polish on top of (C).
4. **(D)** / **(E)** if (C) + (B) prove insufficient.

(D) is a good standalone win for `Contour`/`Ascii` modes specifically,
where the rasterize cost is paid for output that immediately gets
quantized hard.
