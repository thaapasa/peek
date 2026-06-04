# Object files

Executables, shared libraries, relocatable objects, and **WebAssembly** modules — **ELF**,
**Mach-O**, **PE / COFF**, and **`.wasm`** — open in a dedicated viewer rather than the binary hex
fallback. Detection is by magic bytes, so an extensionless binary like `/bin/ls` is recognised
without a `.elf` / `.exe` extension. WebAssembly functions surface in the Symbols view.

## Views

Tab cycles three views:

- **Info** — the header summary: format, architecture, kind (executable, relocatable object,
  dynamic library, core dump), 32- or 64-bit, endianness, entry point, section and symbol counts,
  and whether debug info is present.
- **Sections** — a table of every section: index, name, address, size, kind.
- **Symbols** — a table of every symbol: address, size, type, bind, name. When the file is
  stripped, the dynamic symbol table is shown in place of the missing `.symtab`.

## Tables

The Sections and Symbols views share a table layout:

- The column header stays pinned at the top while the body scrolls.
- `Left` / `Right` pan the columns — symbol names are often wider than the terminal.
- `/` searches names; `n` / `p` step through matches, scrolling only as far as needed to bring
  each hit on screen.
- Column widths fit their content.
- `t` cycles the theme; the table recolours in place.

## Universal (fat) Mach-O

A universal Mach-O carries several architecture slices in one file. peek parses the slice matching
your machine and lists every slice in the Info view.

## Limitations

Sections and symbols are views, not extractable files — there is no `e` extract here. Bare COFF
`.obj` files without a magic signature aren't auto-detected.
