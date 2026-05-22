# CSS info view

Status of the CSS-aware Info panel for `peek style.css`.

## Shipped

A CSS-specific Info section, built as a sidecar to the standard text
stats (same pattern as Markdown / SQL — the source is still rendered as
syntax-highlighted CSS in `ContentMode`). Lives under `types/css/`:
`info.rs` (shapes), `info_gather.rs` (parser), `info_render.rs` (render).

The Info view surfaces:

- **Stylesheet stats** — style-rule count (CSS nesting included),
  selector count with a per-kind occurrence histogram (class / id /
  element / pseudo / attribute / universal), distinct custom-property
  count, `@media` count, `@keyframes` count.
- **`@import` list** — every `@import` URL; absolute / protocol-relative
  URLs (`http(s)://`, `//`, `ftp://`) flagged in the warning style, same
  as the SVG section's external-ref row.
- **Colour palette** — every colour literal in declaration values,
  deduped and rendered as a block-glyph swatch grid in the resolved
  colours, most-frequent first.

### Parser

`cssparser` + `cssparser-color`, not `lightningcss`. The lighter pair
(~120–200 KB vs ~400–700 KB) covers everything the Info view needs;
`lightningcss`'s unique win — typed `Transform` for the svg_anim rewrite
— is deferred (see below), so it didn't earn the binary / build cost.

`CssScanner` implements cssparser's four parser traits
(`DeclarationParser` / `QualifiedRuleParser` / `AtRuleParser` /
`RuleBodyItemParser`) and is driven by `StyleSheetParser` (top level) and
`RuleBodyParser` (rule bodies). Letting cssparser own the parse buys:

- **Declaration vs. nested rule** — CSS nesting makes `& .x { … }` and
  `color: red;` share a context; cssparser does the lookahead.
- **No false colours** — `parse_value` only ever sees declaration
  values, so a colour word in a selector (`.gold`), a string
  (`content: "red"`), or a comment never reaches the colour scan.

Hex / `rgb()` / `hsl()` / `hwb()` / named colours resolve to swatches;
the CIE / Oklab spaces and `currentColor` are counted as colours but not
swatched (no fixed sRGB triple without more context).

## Remaining

Two items from the original plan, both deferred to their own tasks.

### Selector specificity inline-annotation

Annotate each rule's selector list with its specificity tuple (`a,b,c`)
in the highlighted CSS source view. The "killer feature" for debugging
"why isn't my style winning", and the biggest effort: a separate
`ContentMode` code path (gutter or trailing-comment rendering, a
parsed-selector cache plumbed through `RenderCtx`) — independent of the
Info-view work above. Specificity itself is cheap to compute; the
`ContentMode` integration is the cost.

### svg_anim keyframe-parser rewrite

Replace the hand-rolled `viewer/image/svg_anim` CSS parsing
(`parse_keyframes` / `parse_anim_spec` / transform parsing, ~250 LOC)
with a typed parser. Deferred: it's a separate concern with real SVG-
animation regression risk, and it's the one piece that would actually
justify `lightningcss`'s typed `Transform` / `Animation` values. Revisit
as its own task; if picked up, weigh swapping the CSS dep then.
