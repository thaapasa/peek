# Help and about

Every file type has a Help screen and an About screen. You can open these overlays at any time. The
full manual is also built into peek.

## Help

To open the help screen, press `h` or `?`. The help screen is the authoritative keyboard reference
inside peek. All key bindings come from one source, so the screen always matches what the keys do.

The shortcut list has sections. A **Global** section comes first. After it, each loaded mode has its
own section for the keys it adds, with the mode's label as the heading. For example, an EPUB file
shows a **Read** section with keys for moving between chapters, and a **TOC** section with the key
that pins the parent path. These appear under separate headings instead of in one flat list. If a
mode's key duplicates a global key, peek leaves it out of the mode's section. The screen lists every
mode the file has at once. It doesn't filter the list to the active mode.

The help screen also shows the name of the active theme.

## About

To open the About screen, press `a`. It shows the following:

- The peek logo in a gradient
- The version and tagline
- The active theme's full palette as colored swatches
- A short list of pointers: the homepage, the license, and common keys

The About screen also works as a theme showcase. To preview how each theme paints the full palette,
press `t` on the About screen to cycle through the themes.

The logo animates while the About screen is open. The gradient slides back and forth across the
wordmark. Every few seconds, two bright runners trace the wordmark's outline in opposite directions
and meet at the far edge. To pause or resume the animation, press `Space`.

In plain mode, the animation is off. You turn on plain mode with `--color plain`, or by pressing `c`
until you reach it. The About screen then paints the static logo and stops updating until color
comes back.

## Manual

To open this manual inside peek, run `peek --manual`. The manual matches the installed version and
works offline. It opens on the table of contents.

- To read a page, select it and press `Enter`. The page opens in the Markdown viewer.
- To return to the table of contents, press `Esc`.

The terminal copy of the manual leaves out the images.

The manual works like any other container, so the usual options apply:

```sh
peek --manual --list                     # chapter paths and titles
peek --manual -x viewer/themes.md -p     # print one chapter, rendered
peek --manual -x viewer/themes.md -o -   # the chapter's Markdown to stdout
```
