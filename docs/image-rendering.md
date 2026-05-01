# Image Rendering

peek renders images via two-color glyph matching. This doc covers the algorithm, the rendering
modes, and the glyph-atlas regeneration flow.

## Algorithm

For each terminal cell:

1. **Pixel extraction.** Source resized so each cell maps to an 8×16 pixel block (matches typical ~
   2:1 height-to-width terminal cell ratio).
2. **Color clustering.** The 128 pixels are clustered into two dominant colors via fast k-means
   (k=2, 2 iterations). Produces a binary bitmap assigning each pixel to one of the two colors.
3. **Glyph matching.** Binary bitmap compared against a precomputed atlas using Hamming distance
   (XOR + popcount). Inverted match (swap fg/bg) checked for free, doubling the pattern count
   without extra storage.
4. **ANSI rendering.** Selected glyph emitted with 24-bit truecolor fg + bg.

Glyph selection is driven by *spatial color distribution*, not just brightness. A diagonal color
boundary picks `/` or `\`; a cell mostly one color at the top picks `▄`.

## Bitmap representation

Each glyph is a `u128` — 8 cols × 16 rows = 128 bits, row-major:

- Bit 0 = (row 0, col 0)
- Bit 7 = (row 0, col 7)
- Bit 8 = (row 1, col 0)
- Bit 127 = (row 15, col 7)

Set bit = ink (foreground); clear bit = empty (background).

### Hamming-distance match

```
distance_normal   = popcount(cell_bitmap XOR glyph_bitmap)
distance_inverted = 128 - distance_normal
```

Glyph with the minimum `min(distance_normal, distance_inverted)` wins. Inverted distance smaller →
swap fg and bg.

On x86-64, `popcount(a XOR b)` on `u128` compiles to 2 XOR + 2 POPCNT — extremely fast match loop.

## Color clustering

Optimized k-means variant:

1. Mean color of all 128 pixels.
2. Pixel farthest from the mean → initial centroid B.
3. Pixel farthest from centroid B → initial centroid A.
4. 2 iterations of standard k-means (assign + recompute).

**Uniform-cell fast path:** if max pixel distance from mean is below threshold (sum of squared
channel diffs < 300), cell is rendered as a space with the mean as background — skips clustering and
glyph matching.

Distance metric: squared Euclidean in RGB (`Δr² + Δg² + Δb²`). Perceptual spaces (LAB) would be more
accurate but slower; visual difference is marginal here.

## Rendering modes (`--image-mode`)

### `full` (default)

Entire glyph atlas:

- All printable ASCII (32–126)
- Latin-1 Supplement (160–255)
- Block / quadrant elements (U+2580–U+259F)
- Box-drawing (U+2500–U+257F)
- Geometric shapes (U+25A0–U+25FF)

Maximum spatial detail. Can look "noisy" since letters and symbols appear in the output.

### `block`

- Block / quadrant elements (▀▄▌▐▖▗▘▝▙▛▜▟█░▒▓)
- Curated ASCII subset with distinct spatial patterns (`/\|-_()[]{}` etc.)

Cleaner, less text-like. Quadrants alone give 2×2 sub-cell resolution.

### `geo`

Block / quadrant elements + line-segment ASCII (`/\|-_`) only.

### `ascii`

Legacy density-ramp renderer. Per-pixel character based on ITU-R BT.601 luminance, foreground only —
no background colors. Fastest mode; works on terminals without bg colors or Unicode.

## Glyph atlas

Stored in `src/viewer/image/glyph_atlas.rs`, two parts.

### Block elements (hardcoded)

Mathematically exact geometry → bitmaps computed at compile time via `const fn` helpers:

- `full_rows(start, end)` — fills complete rows
- `full_cols(start, end)` — fills complete columns
- `quadrant(tl, tr, bl, br)` — fills quadrants of the 2×2 grid

### Font-rasterized glyphs (generated)

Other glyphs come from the `gen_glyphs` example tool:

```sh
cargo run --example gen_glyphs > src/viewer/image/glyph_atlas_data.rs
```

Pipeline:

1. Loads a monospace font (auto-detected from system fonts; override with
   `PEEK_FONT=/path/to/font.ttf`)
2. Rasterizes each character at ~48px via `fontdue`
3. Composes onto a cell-sized canvas using font metrics (baseline, bearing)
4. Downsamples to 8×16 via area sampling
5. Thresholds at 128 → binary bitmap
6. Outputs Rust source with `GlyphBitmap` entries tagged `Curated` or `Extended`

Glyph categories:

- `Block` — programmatic block elements
- `Curated` — ASCII with distinct spatial patterns (used in `block` mode)
- `Extended` — everything else (only `full` mode)

### Regenerating

```sh
cargo run --example gen_glyphs > src/viewer/image/glyph_atlas_data.rs                      # system default font
PEEK_FONT=/path/to/MyFont.ttf cargo run --example gen_glyphs > src/viewer/image/glyph_atlas_data.rs   # specific font
```

The generator uses `fontdue` (in `[dev-dependencies]`); not a runtime dep.

## ANSI escape format

```
\x1b[38;2;{fg_r};{fg_g};{fg_b}m\x1b[48;2;{bg_r};{bg_g};{bg_b}m{glyph}
```

- `38;2;r;g;b` — 24-bit foreground
- `48;2;r;g;b` — 24-bit background
- Each line ends with `\x1b[0m` to reset

Requires a 24-bit-truecolor terminal (most modern terminals support it).
