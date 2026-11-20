# Themes

To select a theme, pass `--theme <name>` or set `PEEK_THEME`. The default is `idea-dark`.

| Theme                | Description                           |
| -------------------- | ------------------------------------- |
| `idea-dark`          | JetBrains IDEA default Dark (default) |
| `idea-light`         | JetBrains IntelliJ Light              |
| `solarized-light`    | Solarized Light                       |
| `github-light`       | GitHub Light                          |
| `vscode-dark-modern` | VS Code Dark Modern                   |
| `vscode-dark-2026`   | VS Code Dark 2026                     |
| `vscode-monokai`     | VS Code Monokai                       |
| `iceberg`            | Iceberg: cold bluish dark             |
| `iceberg-light`      | Iceberg Light: cold bluish light      |
| `graveyard`          | Gothic moonlit night                  |
| `candy-floss`        | Pastel candy on dark plum             |
| `victorian`          | Parlour parchment with oxblood        |

If you don't set `--theme` or `PEEK_THEME`, the default adapts to your terminal. peek queries the
terminal background color with an OSC 11 (Operating System Command) escape sequence. On a light
background, peek picks `idea-light`, and on a dark one, `idea-dark`. This only happens when the
output is a terminal. Piped output keeps the dark default. A theme that you set explicitly always
wins.

To cycle themes live in the interactive viewer, press `t`. To cycle in reverse, press `T`. Cycling
repaints the whole UI in the new theme, including syntax-highlighted code, info screens, help, and
the gradient logo.

The About screen (`a`) doubles as a theme showcase. If you cycle themes while on About, you see how
each theme paints the full palette. To pause or resume the animated logo, press `Space`.

## How themes work

Each theme is a TextMate `.tmTheme` file that peek embeds at compile time. Syntect uses it for
syntax scopes. From the theme's settings and scopes, peek also derives a set of semantic UI roles:
`heading`, `label`, `value`, `accent`, `muted`, `warning`, `gutter`, `search_match`, and
`selection`. This keeps the rest of the UI in line with the syntax colors.

`--help --theme <name>` works as a theme preview, because the help screen itself uses the theme.
