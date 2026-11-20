# Image rendering

peek renders images by matching each terminal cell to a glyph drawn in two colors. This doc covers
the algorithm, the rendering modes, and how to regenerate the glyph atlas.

## Algorithm

For each terminal cell, the renderer runs these steps:

1. **Pixel extraction.** The renderer resizes the source image so that each cell maps to an 8×16
   pixel block. This matches the typical terminal cell, which is about twice as high as it is wide.
2. **Color clustering.** A fast k-means pass (k=2, 2 iterations) clusters the 128 pixels into two
   dominant colors. The result is a binary bitmap that assigns each pixel to one of the two colors.
3. **Glyph matching.** The renderer compares the bitmap against a precomputed atlas by Hamming
   distance, computed as XOR followed by a population count. The inverted match, with foreground and
   background swapped, comes for free. This doubles the number of patterns without extra storage.
4. **ANSI rendering.** The renderer emits the selected glyph with 24-bit truecolor foreground and
   background colors.

The _spatial distribution of color_ drives glyph selection, not brightness alone. A diagonal color
boundary picks `/` or `\`. A cell that is mostly one color at the top picks `▄`.

## Bitmap representation

Each glyph is a `u128`: 8 columns × 16 rows gives 128 bits, stored in row-major order.

- Bit 0 is row 0, column 0.
- Bit 7 is row 0, column 7.
- Bit 8 is row 1, column 0.
- Bit 127 is row 15, column 7.

A set bit is ink (foreground). A clear bit is empty (background).

### Hamming-distance match

```
distance_normal   = popcount(cell_bitmap XOR glyph_bitmap)
distance_inverted = 128 - distance_normal
```

The glyph with the smallest `min(distance_normal, distance_inverted)` wins. If the inverted distance
is the smaller one, the renderer swaps the foreground and background colors.

On x86-64, `popcount(a XOR b)` on a `u128` compiles to two XOR and two POPCNT instructions. This
makes the match loop very fast.

## Color clustering

The renderer uses an optimized k-means variant:

1. Compute the mean color of all 128 pixels.
2. Take the pixel farthest from the mean as the initial centroid B.
3. Take the pixel farthest from centroid B as the initial centroid A.
4. Run two iterations of standard k-means. Each iteration assigns the pixels and then recomputes the
   centroids.

**Fast path for uniform cells.** If the largest pixel distance from the mean is below a threshold,
the renderer draws the cell as a space with the mean color as its background. The threshold is a sum
of squared channel differences below 300. This skips both clustering and glyph matching.

The distance metric is the squared Euclidean distance in RGB (`Δr² + Δg² + Δb²`). A perceptual color
space such as LAB would be more accurate but slower. The visual difference is marginal for this use.

## Rendering modes (`--image-mode`)

### `full` (default)

This mode uses the whole glyph atlas:

- All printable ASCII (32–126)
- Latin-1 Supplement (160–255)
- Block and quadrant elements (U+2580–U+259F)
- Box-drawing characters (U+2500–U+257F)
- Geometric shapes (U+25A0–U+25FF)

It gives the most spatial detail. The output can look noisy, because letters and symbols appear in
it.

### `block`

This mode uses these glyphs:

- Block and quadrant elements (▀▄▌▐▖▗▘▝▙▛▜▟█░▒▓)
- A curated subset of ASCII with distinct spatial patterns, such as `/\|-_()[]{}`

The output is cleaner and looks less like text. The quadrants alone give a 2×2 resolution inside
each cell.

### `geo`

This mode uses only the block and quadrant elements and the line-segment ASCII characters (`/\|-_`).

### `ascii`

This mode is the legacy density-ramp renderer. It picks a character for each pixel from its ITU-R
BT.601 luminance and draws it in a foreground color only, with no background colors. It is the
fastest mode, and it works on terminals that lack background colors or Unicode.

## Glyph atlas

The atlas lives in `crates/peek-types/src/types/image/pipeline/glyph_atlas.rs` and has two parts.

### Block elements (hardcoded)

Block elements have exact mathematical geometry, so `const fn` helpers compute their bitmaps at
compile time:

- `full_rows(start, end)` fills complete rows.
- `full_cols(start, end)` fills complete columns.
- `quadrant(tl, tr, bl, br)` fills quadrants of the 2×2 grid.

### Font-rasterized glyphs (generated)

The `gen_glyphs` example tool generates the other glyphs:

```sh
cargo run --example gen_glyphs > crates/peek-types/src/types/image/pipeline/glyph_atlas_data.rs
```

The tool runs these steps:

1. It loads a monospace font. By default it finds one among the system fonts. To use another font,
   set `PEEK_FONT=/path/to/font.ttf`.
2. It rasterizes each character at about 48 pixels with `fontdue`.
3. It places the glyph on a canvas of cell size, using the font metrics for baseline and bearing.
4. It scales the canvas down to 8×16 with area sampling.
5. It thresholds the result at 128 to get a binary bitmap.
6. It writes Rust source with `GlyphBitmap` entries tagged `Curated` or `Extended`.

The glyph categories are:

- `Block`: block elements built in code.
- `Curated`: the ASCII characters with distinct spatial patterns, used in `block` mode.
- `Extended`: all other glyphs, used only in `full` mode.

### Regenerate the atlas

To regenerate the atlas with the system default font, run:

```sh
cargo run --example gen_glyphs > crates/peek-types/src/types/image/pipeline/glyph_atlas_data.rs
```

To use a specific font, run:

```sh
PEEK_FONT=/path/to/MyFont.ttf cargo run --example gen_glyphs > crates/peek-types/src/types/image/pipeline/glyph_atlas_data.rs
```

The generator gets `fontdue` from the root crate's `[dev-dependencies]`.

## ANSI escape format

```
\x1b[38;2;{fg_r};{fg_g};{fg_b}m\x1b[48;2;{bg_r};{bg_g};{bg_b}m{glyph}
```

- `38;2;r;g;b` sets a 24-bit foreground color.
- `48;2;r;g;b` sets a 24-bit background color.
- Each line ends with `\x1b[0m` to reset the colors.

This format needs a terminal with 24-bit truecolor support. Most modern terminals have it.
