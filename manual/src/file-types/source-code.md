# Source code

Source files render as syntax-highlighted text via [syntect](https://github.com/trishume/syntect)
with [two-face](https://github.com/Enselic/two-face) / bat extended grammars — 100+ languages
including Rust, Python, JavaScript, TypeScript, C, C++, Java, Go, Ruby, Shell, TOML, Dockerfile.

If detection misses, force a language with `-L`:

```sh
cat script | peek -L bash
peek -L rust unknown_file
```

## Line numbers

Off by default. Enable at startup with `-n` / `--line-numbers`, or toggle with `l` in the viewer.

## Soft wrap

On by default. Toggle with `w`. When off, `Left` / `Right` pan the viewport horizontally
(`less -S` feel).

## SQL

`.sql` / `.ddl` / `.dml` / `.psql` / `.pgsql` files render as highlighted source. The Info view
adds an SQL section: dialect guess (PostgreSQL / MySQL / SQLite / T-SQL / generic), statement
count broken down by category (DDL / DML / DQL / TCL), inventories of created objects (tables,
views, indexes, functions, triggers), comment-line count, and a flag when an inline `$$ … $$`
PL/pgSQL block is present.

## CSS

`.css` files render as highlighted source. The Info view adds a CSS section: style-rule count
(CSS nesting included), selector count with a per-kind histogram (class / id / element / pseudo /
attribute / universal), custom-property count, `@media` and `@keyframes` counts, and an `@import`
list — external URLs flagged. A Colors section shows the stylesheet's colour palette as block
swatches, most-frequent first. Colour words inside selectors, strings, or comments are not
mistaken for colours.
