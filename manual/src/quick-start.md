# Quick start

The following examples show common ways to use peek:

```sh
# Source code: syntax highlighted, interactive viewer
peek src/main.rs

# Markdown: rendered headings, tables, code, and task lists. Press Tab for the source.
peek README.md

# Structured data: pretty-printed
peek config.json
peek data.yaml

# Image: glyph-matched ASCII art
peek photo.jpg

# PDF: paged ASCII render. Press n or p to step through pages.
peek report.pdf

# Certificate or key: subject, validity, SANs, and fingerprints
peek server.pem

# Pipe: auto-detects JSON, YAML, or XML
echo '{"a":1}' | peek
curl -s https://example.com/data.json | peek

# Force a syntax when piping plain text
cat src/main.rs | peek -L rust

# Write straight to stdout without the viewer
peek --print file.txt
peek -p file.txt

# Show file metadata only
peek --info photo.jpg

# Browse an archive
peek release.tar.gz

# Extract one file from an archive
peek release.tar.gz --extract README.md -o README.md
```

To learn how peek chooses between the viewer and print mode, see
[Operating modes](./operating-modes.md). For the supported formats, see
[File types](./file-types/index.md).
