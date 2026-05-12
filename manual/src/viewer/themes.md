# Themes

Selectable via `--theme <name>` or `PEEK_THEME`. Default `idea-dark`.

| Theme                | Description                           |
|----------------------|---------------------------------------|
| `idea-dark`          | JetBrains IDEA default Dark (default) |
| `vscode-dark-modern` | VS Code Dark Modern                   |
| `vscode-dark-2026`   | VS Code Dark 2026                     |
| `vscode-monokai`     | VS Code Monokai                       |

Press `t` (or `T` for reverse) in the interactive viewer to cycle live. Cycling repaints the
whole UI in the new theme — syntax-highlighted code, info screens, help, gradient logo,
everything.

The About screen (`a`) doubles as a theme showcase: cycling themes while on About previews how
each one paints the full palette.

## How it works

Each theme is a TextMate `.tmTheme` embedded at compile time. Syntect uses it for syntax
scopes; peek derives a set of semantic UI roles (`heading`, `label`, `value`, `accent`,
`muted`, `warning`, `gutter`, `search_match`, `selection`) from the theme's settings + scopes,
so non-code UI stays in keeping with the syntax colors.

`--help --theme <name>` works as a theme preview — the help screen itself is themed.
