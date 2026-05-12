# Quick start

```sh
# Source code — syntax highlighted, interactive viewer
peek src/main.rs

# Structured data — pretty-printed
peek config.json
peek data.yaml

# Image — glyph-matched ASCII art
peek photo.jpg

# PDF — paged ASCII render, n/p step pages
peek report.pdf

# Pipe — auto-detects JSON / YAML / XML
echo '{"a":1}' | peek
curl -s https://example.com/data.json | peek

# Force a syntax when piping plain text
cat src/main.rs | peek -l rust

# Direct stdout (no viewer)
peek --print file.txt
peek -p file.txt

# File metadata only
peek --info photo.jpg

# Browse an archive
peek release.tar.gz

# Extract one file from an archive
peek release.tar.gz --extract README.md -o README.md
```

See [Operating modes](./operating-modes.md) for how viewer vs print is selected, and
[File types](./file-types/index.md) for the supported formats.
