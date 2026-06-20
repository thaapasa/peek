# Help and about

Two always-available overlay screens, on every file type.

## Help

`h` or `?` opens the help screen — the authoritative in-app keyboard reference. All bindings
derive from one source, so the screen never drifts from what the keys actually do.

The shortcut list is sectioned: a **Global** block first, then one block per loaded mode (its
label as the heading) for that mode's extras. An EPUB file shows a **Read** section (chapter nav)
and a **TOC** section (pin parent path) under separate headings, rather than one flat list. A
mode's entry is dropped from its section when it duplicates a global key. The screen lists every
mode the file has at once — it doesn't filter to the active mode.

The active theme name shows alongside the shortcuts.

## About

`a` shows the gradient peek logo, version, tagline, the active theme's full palette as colored
swatches, and a short list of pointers (homepage, license, common keys). It doubles as a theme
showcase: cycle themes with `t` while on About to preview how each one paints the full palette.

The logo animates while About is open — the gradient slides across the wordmark in a ping-pong,
and every few seconds two bright runners trace the wordmark outline in opposite directions and
meet at the far edge. `Space` pauses / resumes the animation. In plain mode (`--color plain`, or
cycling to it with `c`) the animation is off — About paints the static logo and stops ticking
until color comes back.
