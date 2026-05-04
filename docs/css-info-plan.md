# Rich CSS info view (planned)

Plan for upgrading `peek style.css` from the current generic
text-stats display to a CSS-aware info panel, plus the related
animation-parser swap. Not implemented; this is the queue.

## Scope (priority order)

1. **Selector specificity inline-annotated alongside rules in source
   view.** Each rule's selector list shows its specificity tuple
   (`a,b,c`) in the gutter or as a faint trailing comment, so the user
   can scan a real stylesheet and compare rule weights at a glance. The
   killer feature here: it's the thing you actually want when debugging
   "why isn't my style winning". Highest impact, biggest effort.
2. **`@import` URL list.** Extract every `@import url(...)` (and the
   `@import "..."` shorthand). External (`http://`, `https://`, `//`)
   URLs flagged in the warning style — same pattern as the SVG section's
   `External ref` line. Surfaces supply-chain shape at a glance.
3. **Color palette.** Walk every rule's declaration values, extract
   color literals (hex, `rgb(...)`, `hsl(...)`, named colors), dedupe,
   render as a swatch grid in the info panel. Bonus: most-frequent first
   so the brand colors fall out at the top.
4. **Stylesheet stats.** Rule count, selector count by kind
   (class/id/element/pseudo histogram), custom-property count, `@media`
   query count, `@keyframes` count.

The first three are the user-named priorities. The fourth is cheap
once a real CSS parser is in place.

## Library choice

Recommend **`lightningcss`** as the dependency.

|                               | `cssparser` (low-level)                                     | `lightningcss` (high-level)                                               |
|-------------------------------|-------------------------------------------------------------|---------------------------------------------------------------------------|
| AST                           | None — tokens only. Implement `Parse` traits per rule type. | Full typed AST: `StyleSheet`, `Rule`, `Selector`, `Property`, `CssColor`. |
| Selectors                     | Separate `selectors` crate.                                 | Built in.                                                                 |
| Colors                        | Manual extraction.                                          | `CssColor` enum with conversion helpers.                                  |
| @import / @media / @keyframes | Manual at-rule handlers.                                    | First-class rule variants.                                                |
| Compiled size                 | ~150 KB                                                     | ~500 KB + transitive deps (browserslist, etc.)                            |
| Compile time hit              | Small.                                                      | Noticeable.                                                               |
| Custom code we write          | Lots.                                                       | Little — mostly glue.                                                     |

cssparser would let us replace the hand-rolled `@keyframes` parser
modestly cleaner, but doesn't carry its weight for the info-view work.
lightningcss is the direct path: every feature above is a few lines of
visitor / pattern-match.

Trade-off accepted: **bigger binary + slower clean builds, in exchange
for substantially less custom CSS parsing code**.

## Bonus: replace the hand-rolled keyframe parser at the same time

When this work picks up, swap `viewer/image/svg_anim.rs::parse_keyframes`

+ `parse_keyframe_stops` + `parse_anim_spec` for lightningcss-based
  extraction:

- Pull every `<style>` block's text (already done via quick-xml in
  `scan_svg`). Pass each through `lightningcss::stylesheet::StyleSheet::parse`.
- Walk the resulting `Rule` list. Match `Rule::Keyframes(KeyframesRule)`
  → harvest stop percentages + declarations. Use `Property::Transform`
  (typed `Transform` enum: `TranslateX`, `TranslateY`, `Translate`,
  matrix, etc.) instead of regex-matching `translate*(...)`.
- For `animation` shorthand on inline `style="..."`: parse with
  lightningcss's `Property::Animation` instead of our hand split.

Win: drop ~250 lines of hand-rolled CSS lexing
(`parse_keyframes`/`parse_keyframe_stops`/`parse_transform_value`/
`parse_animation_shorthand`/`parse_time`/`parse_length`/the helpers).
Gain: support for `matrix()`, `rotate()`, `scale()`, full timing
function set, multiple animations per element, `@keyframes` with
declarations beyond `transform`.

This is the right time to also extend phase-1 scope to **class/id
selector matching** (currently only inline-style targets):
lightningcss's `selectors` types let us match `.foo` / `#bar` against
real elements without writing a selector engine.

## Integration points

- New gather module: `src/info/gather/css.rs`. Mirrors the SVG one:
  parses bytes, emits a `FileExtras::Css { ... }` payload.
- New render module: `src/info/render/css.rs`. Renders selector kind
  histogram, `@import` list (warning-styled if external), color palette
  swatch grid, rule stats.
- Extend `FileExtras` with a `Css` variant. Drop the current `Text`
  fallback for `text/css` files. (Source code view still uses
  `ContentMode` — that's a separate code path, untouched.)
- `viewer/modes/content.rs`: add specificity gutter annotation when the
  active language token is `css`. Hook through `RenderCtx` so it can read
  the parsed selector list. Keep cheap: parse once, cache the
  `Vec<(line_range, specificity)>` mapping next to the line cache.
- `viewer/image/svg_anim.rs`: rewrite `parse_keyframes` +
  `parse_anim_spec` against lightningcss types. `scan_svg` (XML) is
  unchanged — still quick-xml.

## Risks / unknowns

- **Specificity rendering inside `ContentMode`**: the gutter is fixed-width
  today. Specificity tuples vary in length (`0,0,1` vs `1,2,4`); we'd
  reserve a constant 7 chars (`123,123,123` worst case is unrealistic;
  `9,9,9` covers anything you'll see in practice). Or render as a
  trailing comment via a dim color overlay.
- **Multiline selectors** (`a,\n b {\n  ...}`): one specificity per
  comma-separated part. Decision: show each on its own line, or only
  the highest. Lean toward "each on its own line" for fidelity.
- **`@supports` / `@layer` / `@scope` nesting**: lightningcss parses
  these but the specificity view has to decide whether to surface layer
  membership. Defer to phase 2.
- **Color extraction false positives** in CSS strings (`url("…red…")`)
  and custom properties (`--brand: red`): lightningcss's typed values
  avoid string-level regex traps automatically.
- **Build time / binary size** regression. Measure before/after; drop
  the dep if compile time bloats unbearably.

## Recommended order (when this starts)

1. Add `lightningcss` dep + `FileExtras::Css` + basic gather
   (rule count, selector kind histogram, `@import` URLs).
2. Color palette extraction + swatch render.
3. Rewrite svg_anim CSS parsing on top of lightningcss; delete ~250
   lines of hand-rolled code; extend to class/id selector matching.
4. Specificity inline annotation in `ContentMode`. Biggest UX win,
   biggest scope; needs the cache plumbing decided in step 1.
