# Themes

Selectable via `--theme <name>` or `PEEK_THEME`. Default `idea-dark`.

| Theme                | Description                           |
|----------------------|---------------------------------------|
| `idea-dark`          | JetBrains IDEA default Dark (default) |
| `idea-light`         | JetBrains IntelliJ Light              |
| `solarized-light`    | Solarized Light                       |
| `github-light`       | GitHub Light                          |
| `vscode-dark-modern` | VS Code Dark Modern                   |
| `vscode-dark-2026`   | VS Code Dark 2026                     |
| `vscode-monokai`     | VS Code Monokai                       |
| `graveyard`          | Gothic moonlit night                  |
| `candy-floss`        | Pastel candy on dark plum             |
| `victorian`          | Parlour parchment with oxblood        |

With no `--theme` / `PEEK_THEME` set, the default adapts to your terminal: peek probes the
terminal background (OSC 11) and picks `idea-light` on a light background, `idea-dark` on a dark
one. Only when output is a terminal — piped output keeps the dark default. An explicit theme
always wins.

Press `t` (or `T` for reverse) in the interactive viewer to cycle live. Cycling repaints the
whole UI in the new theme — syntax-highlighted code, info screens, help, gradient logo,
everything.

The About screen (`a`) doubles as a theme showcase: cycling themes while on About previews how
each one paints the full palette. The animated logo can be paused / resumed with `Space`.

## How it works

Each theme is a TextMate `.tmTheme` embedded at compile time. Syntect uses it for syntax
scopes; peek derives a set of semantic UI roles (`heading`, `label`, `value`, `accent`,
`muted`, `warning`, `gutter`, `search_match`, `selection`) from the theme's settings + scopes,
so non-code UI stays in keeping with the syntax colors.

`--help --theme <name>` works as a theme preview — the help screen itself is themed.
