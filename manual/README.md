# peek manual sources

The user manual is an [mdbook](https://rust-lang.github.io/mdBook/) under `src/`. The same sources
are built into the peek binary, and `peek --manual` shows them in peek's own Markdown viewer.
`mise run manual` serves the web version on port 12620.

## Writing for both copies

The `peek-manual` crate rewrites each chapter at build time for the copy inside peek. The web manual
is unchanged.

- **Images are dropped** from the in-app copy, together with their alt text.
- **Relative links lose their target.** `[Themes](./viewer/themes.md)` shows as underlined `Themes`.
  Links with a scheme (`https://`, `mailto:`) keep their URL.
- **`<!-- peek: … -->` adds text that only peek shows.** The comment body is Markdown. Browsers
  don't show HTML comments, so the web manual is unaffected. Use it next to an image when the in-app
  copy needs a sentence in its place:

  ```markdown
  ![The Info view for a JPEG](../img/file-info.svg)

  <!-- peek:
  Press `i` on any file to open this view.
  -->
  ```

- **`<!-- peek-skip -->` … `<!-- /peek-skip -->` hides content from peek**, for text that only makes
  sense on the web. Both markers must be on their own lines.

Limits:

- Markers must start a block (a line of their own, with a blank line before). A comment inside a
  paragraph is left as it is.
- A comment body can't contain `-->`.
- Prettier doesn't reflow text inside comments, so wrap the body by hand.

## Build checks

These fail the peek build:

- a chapter that `SUMMARY.md` doesn't list, or a `SUMMARY.md` entry with no file;
- an unclosed, unopened or nested `peek-skip`;
- an empty `peek:` comment, or a comment starting with `<!-- peek` that isn't one of the markers
  above.
