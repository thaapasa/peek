# Fonts

TrueType and OpenType font files open into a **specimen view** — a sample sentence rasterised
through the font and routed through peek's ASCII image pipeline — paired with a rich **Info**
section that decodes every standard OpenType metadata table.

```sh
peek Lobster-Regular.ttf
peek /System/Library/Fonts/Menlo.ttc
```

## Supported formats

- `.ttf` — TrueType outline fonts
- `.otf` — OpenType / CFF outline fonts
- `.ttc` / `.otc` — TrueType / OpenType collections (multiple faces in one file)

WOFF and WOFF2 wrappers are not yet wired — they need separate decompressors. Tracked in
[planned features](https://github.com/thaapasa/peek/blob/main/docs/planned.md).

## Detection

- **By extension** — the four extensions above.
- **By content** — 4-byte magic at offset 0: `00 01 00 00` (TrueType), `true` (Apple TrueType
  variant), `OTTO` (OpenType / CFF), `ttcf` (collection). Unnamed sources (stdin, archive
  entries) still classify.

## Specimen view

The default open lands on a rasterised sample sentence. The current sampler is a hard-coded
ASCII pangram plus digits, common punctuation, and a mixed-case alphabet line — enough to
show baseline / x-height / cap height / descender shapes.

Every image-mode key works on the specimen:

| Key | Action                                                              |
|-----|---------------------------------------------------------------------|
| `m` | Cycle image mode (full-color / block / geo / ascii / contour)       |
| `b` | Cycle background (auto / black / white / checkerboard)              |
| `f` | Cycle fit mode (contain / fit width / fit height)                   |

Zoom / pan keys ([Zoom & pan](../viewer/zoom-pan.md)) work on the specimen as well.

A font that the rasteriser can't parse silently falls through to the Info + Hex tail.

## Font collections

For `.ttc` / `.otc` files, `n` / `p` step the active face through the specimen in place. The
status line shows `Face N/M` and updates as you cycle. The Info screen lists every face's
metadata block separately — so a four-face Apple system font like `Menlo.ttc` shows the
Regular, Bold, Italic, and Bold Italic variants in one screen.

## Info

Per face, the **Font** section surfaces:

- **Identity** — family, subfamily (`Regular` / `Bold` / `Italic` / …), full name, Postscript
  name, version.
- **OS/2 classification** — weight (`100`..`900` with the canonical name shown alongside,
  e.g. `400 (Regular)`, `700 (Bold)`), width class (`Normal`, `Condensed`, `Expanded`, …),
  italic flag, monospaced pitch flag.
- **Geometry** — glyph count, units per em (design grid resolution).
- **Coverage** — Unicode codepoint count (summed across every cmap subtable), plus a script
  list bucketed from cmap ranges: `Latin`, `Latin Extended`, `Greek`, `Cyrillic`, `Hebrew`,
  `Arabic`, `Devanagari`, `Thai`, `Hangul`, `CJK`, `Emoji`, `Symbols`. The list reflects
  what glyphs the font carries, not what scripts it claims to support.
- **Rendering hints** — hinting-present flag (set when `head.flags` bit 0 is set).
- **Attribution** — designer, vendor URL, copyright, license URL (each row is skipped when
  the name table doesn't carry the field).

Apple system fonts (Menlo, San Francisco, …) still ship their canonical name records on the
Macintosh platform in Mac Roman; peek bundles the full Mac Roman upper-half mapping so `©`
/ `™` / accented Latin characters round-trip cleanly.

## What you don't see

- The source view is omitted — fonts are binary containers, so the universal hex aux mode
  (`x`) handles raw byte inspection.
- WOFF / WOFF2 wrappers aren't yet decoded.
- The specimen sampler is hard-coded ASCII; multi-script samplers keyed on cmap coverage are
  planned.
- Faces in a `.ttc` cycle through the specimen in place; true recursive peek into a single
  face (its own viewer frame) would need to synthesise a standalone SFNT from the TTC table
  directory, also tracked as a follow-up.
