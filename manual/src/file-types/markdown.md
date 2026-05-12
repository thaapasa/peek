# Markdown

`.md` / `.markdown` / `.mdown` / `.mkd` files render as syntax-highlighted source. A rendered
"read mode" (styled headings, bold, lists, per-language dispatch inside fenced code) is not yet
implemented.

The Info view adds a Markdown section:

- Heading counts by level (H1..H6)
- Fenced code-block count + declared languages
- Inline-code / link / image / table / list-item counts
- Task-list progress (`done / total + percent`)
- Blockquote lines, footnote definitions
- Frontmatter detection (YAML / TOML)
- Prose word count (excludes fenced code)
- Reading-time estimate at 230 wpm
