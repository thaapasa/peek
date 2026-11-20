# Converting external themes to peek `.tmTheme`

peek themes are syntect `.tmTheme` files, which are TextMate property lists, in
`crates/peek-theme/themes/`. This guide covers how to port IntelliJ `.icls` themes and VS Code JSON
themes. It also covers the syntect quirks that limit how closely a port can match the original.

## Source themes

**VS Code.** On macOS, the built-in VS Code themes are in this directory:

```
/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/theme-defaults/themes/
```

The files include `dark_modern.json`, `dark_plus.json`, `dark_vs.json`, and `light_modern.json`. VS
Code themes use an `"include"` chain. Read every file in the chain and merge them, so that the child
overrides the parent. For example, Dark Modern uses the chain `dark_modern → dark_plus → dark_vs`.

**JetBrains.** To get a JetBrains theme, go to _Settings → Editor → Color Scheme → ⚙️ → Export_ and
export it to `.icls`. The file usually lands at `~/<Theme>.icls`.

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

`PeekTheme::from_syntect` in `crates/peek-theme/src/peek_theme.rs` reads `accent`,
`gutter_foreground`, `find_highlight`, and `selection` from the global settings. It derives the rest
from the scopes. Always set all eight global settings shown in the template.

## VS Code JSON to tmTheme

Two top-level keys matter: `colors` and `tokenColors`.

### `colors`

The `colors` key holds the UI palette. Map it to the global settings:

| VS Code key                                               | tmTheme global     |
| --------------------------------------------------------- | ------------------ |
| `editor.background`                                       | `background`       |
| `editor.foreground`                                       | `foreground`       |
| `editorCursor.foreground` (or `foreground`)               | `caret`            |
| `statusBar.background` (NOT `editor.selectionBackground`) | `selection`        |
| `editor.lineHighlightBackground` (or `#282828`)           | `lineHighlight`    |
| `editorLineNumber.foreground`                             | `gutterForeground` |
| `editor.findMatchBackground`                              | `findHighlight`    |
| `focusBorder` or `textLink.foreground`                    | `accent`           |

**Don't use `editor.selectionBackground` for `selection`.** peek uses `selection` _only_ as the
background of the status bar. In VS Code, `editor.selectionBackground` usually has an alpha channel,
such as `#878b9180` or `#276782dd`, and VS Code draws it translucently over the editor. If you strip
the alpha, you get a status bar that is too bright and has poor text contrast. VS Code uses
`statusBar.background` for its own status bar, and that color works correctly without alpha.

### `tokenColors`

The `tokenColors` key is an array of `{ scope, settings: { foreground, fontStyle } }` entries. They
map to tmTheme rules almost one to one. In VS Code, `scope` can be a string or an array, but tmTheme
expects a comma-separated string. `fontStyle` carries over unchanged.

### Inheritance

If a theme has `"include": "./parent.json"`, merge the parent's `colors` and `tokenColors` first.
Then let the child override them. Repeat this for every file in the chain.

### Semantic tokens

VS Code also has `semanticTokenColors` and `semanticHighlighting: true`. **syntect has no support
for semantic tokens.** It sees only TextMate scopes. Skip the `semanticTokenColors` block. The
result looks like VS Code with semantic highlighting turned _off_. This is why `u8`, `Self`, and
`None` can render in keyword blue instead of type teal in Rust.

## IntelliJ `.icls` to tmTheme

An `.icls` file is XML. Two sections matter: `<colors>` and `<attributes>`.

### `<colors>`

The `<colors>` section holds the UI palette. Map it to the global settings:

| `.icls` option                                        | tmTheme global                |
| ----------------------------------------------------- | ----------------------------- |
| `TEXT` (foreground and background)                    | `foreground` and `background` |
| `CARET_COLOR`                                         | `caret`                       |
| `CARET_ROW_COLOR`                                     | `lineHighlight`               |
| `LINE_NUMBERS_COLOR`                                  | `gutterForeground`            |
| Any prominent blue, for example `FILESTATUS_MODIFIED` | `accent`                      |
| `SEARCH_RESULT_ATTRIBUTES` background                 | `findHighlight`               |

### `<attributes>`

The `<attributes>` section maps attribute names to colors. Map the IDEA `DEFAULT_*` attributes to
TextMate scopes:

| `.icls` attribute                                         | tmTheme scopes                                                                                           |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `DEFAULT_LINE_COMMENT`, `DEFAULT_BLOCK_COMMENT`           | `comment, comment.line, comment.block`                                                                   |
| `DEFAULT_DOC_COMMENT`                                     | `comment.block.documentation, comment.line.documentation`                                                |
| `DEFAULT_KEYWORD`                                         | `keyword, keyword.control, storage, storage.type, storage.modifier`                                      |
| `DEFAULT_OPERATION_SIGN`                                  | `keyword.operator`                                                                                       |
| `DEFAULT_STRING`                                          | `string, string.quoted`                                                                                  |
| `DEFAULT_VALID_STRING_ESCAPE`                             | `constant.character.escape`                                                                              |
| `DEFAULT_NUMBER`                                          | `constant.numeric`                                                                                       |
| `DEFAULT_CONSTANT`                                        | `constant.language, constant.other, support.constant`                                                    |
| `DEFAULT_FUNCTION_DECLARATION`, `DEFAULT_INSTANCE_METHOD` | `entity.name.function, support.function, meta.function-call`                                             |
| `DEFAULT_CLASS_NAME` (or `org.rust.STRUCT` and similar)   | `entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, support.type, support.class` |
| `DEFAULT_INSTANCE_FIELD`                                  | `variable.other.member, variable.other.property`                                                         |
| `DEFAULT_METADATA`                                        | `storage.type.annotation, meta.annotation, meta.attribute`                                               |
| `TYPE_PARAMETER_NAME_ATTRIBUTES`                          | `entity.name.type.parameter, variable.parameter.type`                                                    |
| `HTML_TAG_NAME`, `XML_TAG_NAME`                           | `entity.name.tag`                                                                                        |
| `ERRORS_ATTRIBUTES` `EFFECT_COLOR`                        | `invalid, invalid.illegal`                                                                               |

Language-specific overrides such as `org.rust.*`, `KOTLIN_*`, and `JS.*` show how the IDE _intends_
a token to look. Use them to choose colors for the generic scope. For example, `org.rust.STRUCT`
holds the real struct color even when `DEFAULT_CLASS_REFERENCE` is also defined. You can't map these
overrides one to one, because syntect doesn't know the IDE-specific scopes.

`FONT_TYPE` maps to `fontStyle`. The value `1` means bold, `2` means italic, and `3` means bold and
italic.

## Limits of the syntect Rust grammar

These lessons come from porting `idea-dark` and `vscode-dark-modern`:

1. **`storage.type` is overloaded.** `let`, `fn`, `u8`, `i32`, `Self`, and `str` all share the scope
   `storage.type.rust`. You can't make `let` blue and `u8` teal, because the theme can't tell them
   apart.
2. **`support.type.rust` covers both containers and enum variants.** `Vec`, `String`, `Some`, and
   `None` all get this scope. You can't split them.
3. **Type references differ from declarations.** `entity.name.struct.rust` applies at the
   declaration site, as in `pub struct Foo`. The reference site, as in `impl Foo` or `let x: Foo`,
   usually has no scope, so it renders in the default foreground color.
4. **`storage.type.struct`, `storage.type.class`, and `storage.type.enum` match the keyword.** If
   you add them to the type rule, the keyword itself turns the type color. Use these more specific
   scopes `storage.type.numeric`, `storage.type.boolean`, `storage.type.string`,
   `storage.type.char`, and `storage.type.primitive` instead. These target primitive types without
   catching the keyword.
5. **Rust attributes and macros have their own scopes.** `derive` is `variable.annotation.rust`, and
   macros such as `vec!` and `format!` are `support.macro.rust`. For a look like the IDE's, add both
   to the rule for the function color.
6. **`Self` and `self` have different scopes.** `Self`, the type, is `storage.type.rust`. `self`,
   the expression or binding, is `variable.language.rust` or `variable.parameter.rust`.

If you're not sure which scope a token gets, dump the scopes with a throwaway syntect script:

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

## Register a new theme

After you write `themes/<name>.tmTheme`, follow these steps:

1. In `crates/peek-theme/src/name.rs`, make these changes:
   - Add `const THEME_<NAME>: &str = include_str!(...)`.
   - Add a `PeekThemeName` variant.
   - Update `cli_name`, `tmtheme_source`, `next`, `help_text`, and the
     `clap::ValueEnum::value_variants` list. For `next`, pick a sensible position in the cycle.

   The `#[default]` attribute on the enum sets the default for the `--theme` option. `cli.rs` reads
   it through `PeekThemeName::default()`.

2. Update the docs: the theme tables in `README.md` and `docs/features.md`, and the `themes/`
   listing in `CLAUDE.md`.
3. Run `cargo build && cargo test`.
4. Run a smoke test with `cargo run -- --theme <name> --print crates/peek-theme/src/peek_theme.rs`.
   Compare the output by eye against the reference rendering of the source theme.
