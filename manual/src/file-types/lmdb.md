# LMDB databases

| Format                      | Files                                            |
|-----------------------------|--------------------------------------------------|
| LMDB 0.9 and 1.0 (64-bit)   | `data.mdb`, or any name for a single-file env    |

[LMDB](https://www.symas.com/lmdb) is the key-value store behind OpenLDAP, Postfix `lmdb:`
tables, Monero, Meilisearch and many ML datasets. peek opens it **read-only** and never
touches the lock file.

An LMDB environment is usually a directory holding `data.mdb` and `lock.mdb`. Open the data
file: `peek db/data.mdb`. If you open the directory instead, select `data.mdb` in the listing
and press Enter. An environment created with `MDB_NOSUBDIR` is a single file with any name,
for example Postfix's `virtual.lmdb`. peek recognises LMDB by its header, not by the file
name.

## Landing view

If the environment has named sub-databases, peek lands on a list of them. The size column
shows the space each database's pages take on disk. The Info view (Tab) has the entry
counts. `@main` appears first when the main database also holds
plain keys of its own. Press Enter on a database to open its rows.

If there are no named sub-databases, peek opens the main database's rows directly.

## Rows

Each database opens as a table with three columns: **Key**, **Value** and **Size** (the
value's full length in bytes).

- A key or value that reads as text shows as text. Anything else shows as hex
  (`de ad be ef …`).
- In an `integerkey` database the keys show as numbers. In an `integerdup` database the
  duplicate values show as numbers.
- In a `dupsort` database each duplicate value gets its own row, so one key can span
  several rows. This matches `mdb_dump` and the database's entry count.
- In the main database, the records of named sub-databases show as
  `<database: N entries>`.

Large values preview only their first 256 bytes. Rows load in a sliding window, so a
database with millions of entries opens as fast as a small one. `G` jumps to the true last
row.

### Opening a value

The row table has a cursor. Move it with the arrow keys and press **Enter** to open the
selected value as its own file. peek detects the value's type from its bytes, so a JSON
value opens pretty-printed and a PNG opens as an image. `Esc` returns to the table with the
cursor where you left it.

Press **`e`** to save the selected value to disk. On the command line, pass the row's key
path to `--extract`:

```sh
peek catalog.lmdb --extract users/user:00042 -o user.json
```

The key path is `<database>/<key>`, with `@main` for the main database. A `dupsort` row
adds the duplicate's position: `tags/beta/0`. A key that isn't plain text is written as
`x:` plus hex, for example `blobs/x:00000003`.

## Info section

The Info view (Tab) shows the format version, page size, map size, the pages in use, the
last transaction ID, the active meta page, and the free-list size. Each database gets its
own block with its entry count, tree depth, page counts and flags.

## Limitations

- Only 64-bit little-endian files are read. If a file comes from a 32-bit or big-endian
  build, Info says so and no table opens.
- Encrypted LMDB 1.0 environments are not supported.
- peek doesn't register as a reader in the lock file. If another process is writing to the
  database while peek reads it, peek can see a partly updated tree.
- `--list` lists the databases, not the keys inside them. An environment with no named
  databases has nothing to list. `--extract` needs a full key path such as
  `users/user:00042`; a bare database name like `users` is rejected.
