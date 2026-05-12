# Line numbers and wrap

## Line numbers

Off by default. Enable at startup with `-n` / `--line-numbers`; toggle with `l` in the viewer.

Right-aligned gutter, minimum width 2 digits, painted in the theme's gutter color. In pretty
mode the numbers count visible pretty-printed lines (the lines actually shown), not source
byte lines.

## Soft wrap

On by default for text views (source, structured pretty/raw, plain text, SVG XML). Each visible
logical line is sliced into visual rows of width `term_cols - gutter_width`, so the row budget
accounts for wrapped continuations and the status line never scrolls out of view.

Toggle with `w`. Vertical scroll (`j` / `k`, PgUp / PgDn, Home / End) moves one **visual row**
at a time when wrap is on — long lines no longer make a single keypress jump over all their
wrapped rows.

The line-number gutter shows the real (logical) line number on the first segment; continuation
rows have a blank gutter of the same width so wrapped content aligns under its first row.

Status bar shows `Wrap` only when wrap is on (default-on convention; absence means "off").

## Horizontal scrolling

Companion to wrap-off mode: `Left` / `Right` pan the viewport horizontally by 8 columns per
press (`less -S` feel). Active only when wrap is off — wrap-on makes Left/Right inert because
content is already fully visible. The gutter does not pan; it stays anchored to the left edge.
