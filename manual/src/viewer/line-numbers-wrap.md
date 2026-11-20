# Line numbers and wrap

## Line numbers

Line numbers are off by default. To turn them on at startup, use `-n` or `--line-numbers`. To toggle
them in the viewer, press `l`.

The line numbers appear in a right-aligned gutter that's at least 2 digits wide. peek paints the
gutter in the theme's gutter color. In pretty mode, the numbers count the pretty-printed lines that
you see, not the lines in the source file.

## Soft wrap

Soft wrap is on by default for text views. These include source code, structured data in pretty and
raw form, plain text, and SVG XML. peek splits each logical line into visual rows that fit the
terminal width minus the gutter width (`term_cols - gutter_width`). The row count includes the
wrapped rows, so the status line never scrolls out of view.

To toggle soft wrap, press `w`. When wrap is on, the
[vertical scroll keys](../keyboard-shortcuts.md#navigation) move one **visual row** at a time. A
long line doesn't make a single key press skip all of its wrapped rows.

The gutter shows the logical line number on the first row of each line. The rows that continue the
line have a blank gutter of the same width, so the wrapped text lines up under the first row.

The status bar shows `Wrap` whenever soft wrap is on. This reminder is deliberate. Most status bar
items appear only when a setting differs from its default, but `Wrap` shows even though wrap is on
by default.

## Horizontal scrolling

When wrap is off, `Left` and `Right` pan the viewport horizontally by 8 columns per press, like
`less -S`. When wrap is on, `Left` and `Right` do nothing, because all the content is already
visible. The gutter doesn't pan. It stays at the left edge.
