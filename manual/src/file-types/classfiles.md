# Java classfiles

Compiled Java classes — `.class` files — open in a dedicated viewer rather than the binary hex
fallback. Detection is by magic bytes, so a classfile is recognised even without the `.class`
extension.

## Views

Tab cycles four views:

- **Info** — the class header: name, superclass, implemented interfaces, JDK version, kind
  (class / interface / enum), the source file it was compiled from, and field and method counts.
- **Fields** — a table of every field: modifiers, type, name.
- **Methods** — a table of every method: modifiers, name, signature. Type descriptors are
  decoded to source form — `(String, int) -> int`, not the raw `(Ljava/lang/String;I)I`.
- **Bytecode** — a `javap -c`-style disassembly of every method: byte offset, mnemonic, and
  operand (method / field references resolved, branch targets shown as absolute offsets). `n` and
  `p` jump to the next / previous method; `/` searches the listing.

The Fields and Methods tables behave like the object-file tables: a pinned column header,
`Left` / `Right` column pan, and `/` search.

## Limitations

Fields and methods are views, not extractable files, so there is no `e` extract here.
