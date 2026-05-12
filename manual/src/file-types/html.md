# HTML

`.html` / `.htm` / `.xhtml` (and stdin streams that start with `<!DOCTYPE html>` or `<html`) get a
dual view:

- **Rendered** (default) — lynx-style flow via the
  [html2text](https://crates.io/crates/html2text) crate: paragraph wrap to terminal width, list
  bullets, table grid, numbered link references, ANSI styling for `<strong>` / `<em>` / `<code>`
  / `<s>` / `<a>` plus author colors from inline `style="..."` and `<style>` rules. Near-grayscale
  author colors are filtered so body / heading defaults don't fight the terminal foreground.
- **Source** — raw HTML with XML syntax highlighting. Tab cycles between the two.

The Info view shows structured XML stats (root element, element counts).
