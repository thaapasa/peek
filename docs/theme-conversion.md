# Converting external themes to peek `.tmTheme`

peek themes are syntect `.tmTheme` (TextMate plist) files in `themes/`. This
guide describes how to convert IntelliJ IDEA `.icls` files and VS Code JSON
themes into `.tmTheme`, plus the syntect-grammar quirks that limit fidelity.

## Where to find source themes

**VS Code (macOS):**

```
/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/theme-defaults/themes/
```

Files: `dark_modern.json`, `dark_plus.json`, `dark_vs.json`, `light_modern.json`, etc.
VS Code themes use an `"include"` chain — read every file in the chain and merge
(child overrides parent). For Dark Modern: `dark_modern → dark_plus → dark_vs`.

**JetBrains IDEs:** export from *Settings → Editor → Color Scheme → ⚙️ → Export*
to `.icls`. The user typically drops it at `~/<Theme>.icls`.

## tmTheme structure

```xml

<plist version="1.0">
    <dict>
        <key>name</key>
        <string>Theme Name</string>
        <key>settings</key>
        <array>
            <!-- 1. Global settings dict (no scope) -->
            <dict>
                <key>settings</key>
                <dict>
                    <key>background</key>
                    <string>#...</string>
                    <key>foreground</key>
                    <string>#...</string>
                    <key>caret</key>
                    <string>#...</string>
                    <key>selection</key>
                    <string>#...</string>
                    <key>lineHighlight</key>
                    <string>#...</string>
                    <key>gutterForeground</key>
                    <string>#...</string>
                    <key>findHighlight</key>
                    <string>#...</string>
                    <key>accent</key>
                    <string>#...</string>
                </dict>
            </dict>
            <!-- 2. Per-scope rules -->
            <dict>
                <key>name</key>
                <string>...</string>
                <key>scope</key>
                <string>scope.a, scope.b</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>#...</string>
                    <key>fontStyle</key>
                    <string>bold italic underline</string>
                </dict>
            </dict>
            ...
        </array>
    </dict>
</plist>
```

`PeekTheme::from_syntect` (in `src/theme/peek_theme.rs`) reads `accent`,
`gutter_foreground`, `find_highlight`, `selection` from the global settings and
derives the rest from scope colors — so always set those eight globals.

## VS Code JSON → tmTheme

VS Code themes have two relevant top-level keys:

- `colors` — UI palette. Map these to global settings:
 
  | VS Code key                                     | tmTheme global     |
  |-------------------------------------------------|--------------------|
  | `editor.background`                             | `background`       |
  | `editor.foreground`                             | `foreground`       |
  | `editorCursor.foreground` (or `foreground`)     | `caret`            |
  | `editor.selectionBackground` (or `#264F78`)     | `selection`        |
  | `editor.lineHighlightBackground` (or `#282828`) | `lineHighlight`    |
  | `editorLineNumber.foreground`                   | `gutterForeground` |
  | `editor.findMatchBackground`                    | `findHighlight`    |
  | `focusBorder` / `textLink.foreground`           | `accent`           |

- `tokenColors` — array of `{ scope, settings: { foreground, fontStyle } }`.
  Drop these into tmTheme almost 1:1. `scope` can be a string or array; tmTheme
  expects a comma-separated string. `fontStyle` strings transfer directly.

**Inheritance:** if `"include": "./parent.json"` is set, merge the parent's
`colors` and `tokenColors` first, then let the child override. Walk the chain.

**Semantic tokens:** VS Code also has `semanticTokenColors` and
`semanticHighlighting: true`. **syntect has no semantic-token support** — it
only sees textmate scopes. Skip the `semanticTokenColors` block; what you ship
will look like VS Code with semantic highlighting *off*. This is why `u8` /
`Self` / `None` may render as keyword-blue instead of type-teal in Rust.

## IntelliJ `.icls` → tmTheme

`.icls` is XML with two relevant sections:

- `<colors>` — UI palette. Map to global settings:
 
  | `.icls` option                                   | tmTheme global              |
  |--------------------------------------------------|-----------------------------|
  | `TEXT` (foreground/background)                   | `foreground` / `background` |
  | `CARET_COLOR`                                    | `caret`                     |
  | `CARET_ROW_COLOR`                                | `lineHighlight`             |
  | `LINE_NUMBERS_COLOR`                             | `gutterForeground`          |
  | (any prominent blue, e.g. `FILESTATUS_MODIFIED`) | `accent`                    |
  | `SEARCH_RESULT_ATTRIBUTES` background            | `findHighlight`             |

- `<attributes>` — named attribute → color mapping. Map IDEA's `DEFAULT_*`
  attributes to textmate scopes:

  | `.icls` attribute                                          | tmTheme scope(s)                                                                                         |
  |------------------------------------------------------------|----------------------------------------------------------------------------------------------------------|
  | `DEFAULT_LINE_COMMENT` / `DEFAULT_BLOCK_COMMENT`           | `comment, comment.line, comment.block`                                                                   |
  | `DEFAULT_DOC_COMMENT`                                      | `comment.block.documentation, comment.line.documentation`                                                |
  | `DEFAULT_KEYWORD`                                          | `keyword, keyword.control, storage, storage.type, storage.modifier`                                      |
  | `DEFAULT_OPERATION_SIGN`                                   | `keyword.operator`                                                                                       |
  | `DEFAULT_STRING`                                           | `string, string.quoted`                                                                                  |
  | `DEFAULT_VALID_STRING_ESCAPE`                              | `constant.character.escape`                                                                              |
  | `DEFAULT_NUMBER`                                           | `constant.numeric`                                                                                       |
  | `DEFAULT_CONSTANT`                                         | `constant.language, constant.other, support.constant`                                                    |
  | `DEFAULT_FUNCTION_DECLARATION` / `DEFAULT_INSTANCE_METHOD` | `entity.name.function, support.function, meta.function-call`                                             |
  | `DEFAULT_CLASS_NAME` (or `org.rust.STRUCT` etc.)           | `entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, support.type, support.class` |
  | `DEFAULT_INSTANCE_FIELD`                                   | `variable.other.member, variable.other.property`                                                         |
  | `DEFAULT_METADATA`                                         | `storage.type.annotation, meta.annotation, meta.attribute`                                               |
  | `TYPE_PARAMETER_NAME_ATTRIBUTES`                           | `entity.name.type.parameter, variable.parameter.type`                                                    |
  | `HTML_TAG_NAME` / `XML_TAG_NAME`                           | `entity.name.tag`                                                                                        |
  | `ERRORS_ATTRIBUTES` `EFFECT_COLOR`                         | `invalid, invalid.illegal`                                                                               |

  Language-specific overrides (`org.rust.*`, `KOTLIN_*`, `JS.*`) reveal what the
  IDE *intends* a token to look like. Use them to inform color choices for the
  generic scope (e.g. `org.rust.STRUCT` is the "real" struct color even if
  `DEFAULT_CLASS_REFERENCE` is also defined). They cannot be mapped 1:1 because
  syntect doesn't know about IDE-specific scopes.

  `FONT_TYPE`: `1` = bold, `2` = italic, `3` = bold+italic. Map to `fontStyle`.

## syntect Rust-grammar limitations

Lessons from converting `idea-dark` and `vscode-dark-modern`:

1. **`storage.type` is overloaded.** `let`, `fn`, `u8`, `i32`, `Self`, `str`
   all share scope `storage.type.rust`. You can't make `let` orange/blue while
   making `u8` green/teal — they're indistinguishable to a tmTheme.
2. **`support.type.rust` covers both containers and enum variants.** `Vec`,
   `String`, `Some`, `None` all get this scope. Can't split them.
3. **Type references vs. declarations.** `entity.name.struct.rust` fires on
   the declaration site (`pub struct Foo`). The reference site (`impl Foo`,
   `let x: Foo`) is usually unscoped → renders in default foreground.
4. **`storage.type.struct/.class/.enum` matches the `struct`/`class`/`enum`
   keywords themselves.** Don't add these to the type rule or the keyword
   itself turns into the type color. (Add specific subscopes like
   `storage.type.numeric/.boolean/.string/.char/.primitive` instead — these
   target primitive types without catching the keyword.)
5. **Rust attributes:** `derive` is `variable.annotation.rust`, macros like
   `vec!`/`format!` are `support.macro.rust`. Add both to the function-color
   rule for an IDE-like look.
6. **`Self`/`self` split:** `Self` (the type) is `storage.type.rust`, while
   `self` (the expression/binding) is `variable.language.rust` or
   `variable.parameter.rust`.

When in doubt about a scope, dump it with a throwaway syntect script:

```rust
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;
let ss = SyntaxSet::load_defaults_newlines();
let syntax = ss.find_syntax_by_extension("rs").unwrap();
let mut state = ParseState::new(syntax);
let mut stack = ScopeStack::new();
for line in LinesWithEndings::from(/* code */) {
let ops = state.parse_line(line, &ss).unwrap();
let mut last = 0;
for (offset, op) in & ops {
if * offset > last {
println ! ("{:?}: {:?}", & line[last..* offset], stack.scopes);
last = * offset;
}
stack.apply(op).unwrap();
}
}
```

## Registering a new theme

After writing `themes/<name>.tmTheme`:

1. **`src/theme/name.rs`:** add a `const THEME_<NAME>: &str = include_str!(...)`,
   add a variant to `PeekThemeName`, then update `cli_name`, `tmtheme_source`,
   `next` (pick a sensible cycle position), `help_text`, and the
   `clap::ValueEnum::value_variants` list. The `#[default]` attribute on the
   enum determines the CLI default — `cli.rs` reads it via
   `PeekThemeName::default()`.
2. **Docs:** update the theme tables in `README.md`, `docs/features.md`, and
   the `themes/` listing in `CLAUDE.md`.
3. `cargo build && cargo test` to verify.
4. Smoke test: `cargo run -- --theme <name> --print src/theme/peek_theme.rs`
   and eyeball the output against the source theme's reference rendering.
