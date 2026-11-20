# LMDB databases

| Format                   | Files                                                 |
| ------------------------ | ----------------------------------------------------- |
| LMDB 0.9 and 1.0, 64-bit | `data.mdb`, or any name for a single-file environment |

[LMDB](https://www.symas.com/lmdb) is the key-value store behind OpenLDAP, Postfix `lmdb:` tables,
Monero, Meilisearch, and many machine learning datasets. peek opens it **read-only** and never
touches the lock file.

An LMDB environment is usually a directory that holds `data.mdb` and `lock.mdb`. To view it, open
the data file: `peek db/data.mdb`. If you open the directory instead, select `data.mdb` in the
listing and press Enter. An environment created with `MDB_NOSUBDIR` is a single file with any name,
for example Postfix's `virtual.lmdb`. peek recognizes LMDB by its header, not by the filename.

## Landing view

If the environment has named sub-databases, peek opens on a list of them. The size column shows the
disk space that each database's pages take. The Info view (Tab) shows the entry counts. `@main`
appears first when the main database also holds plain keys of its own. To open a database's rows,
press Enter on it.

If there are no named sub-databases, peek opens the main database's rows directly.

## Rows

Each database opens as a table with three columns:

- **Key**
- **Value**
- **Size**, the full length of the value in bytes

How peek shows keys and values:

- A key or value that reads as text shows as text. Anything else shows as hex (`de ad be ef …`).
- In an `integerkey` database, the keys show as numbers. In an `integerdup` database, the duplicate
  values show as numbers.
- In a `dupsort` database, each duplicate value gets its own row, so one key can span several rows.
  This matches `mdb_dump` and the database's entry count.
- In the main database, the records of named sub-databases show as `<database: N entries>`.

Large values preview only their first 256 bytes. Rows load in a sliding window, so a database with
millions of entries opens as fast as a small one. To jump to the true last row, press `G`.

### Opening a value

The row table has a cursor. To open the selected value as its own file, move the cursor with the
arrow keys and press **Enter**. peek detects the value's type from its bytes, so a JSON value opens
pretty-printed and a PNG opens as an image. To return to the table, press `Esc`. The cursor stays
where you left it.

To save the selected value to disk, press **`e`**. On the command line, pass the row's key path to
`--extract`:

```sh
peek catalog.lmdb --extract users/user:00042 -o user.json
```

The key path is `<database>/<key>`, with `@main` for the main database. A `dupsort` row adds the
position of the duplicate, as in `tags/beta/0`. If a key isn't plain text, the key path writes it as
`x:` plus hex, for example `blobs/x:00000003`.

## Info section

The Info view (Tab) shows these environment details:

- Format version
- Page size
- Map size
- Pages in use
- Last transaction ID
- Active meta page
- Free-list size

Each database gets its own block with its entry count, tree depth, page counts, and flags.

## Limitations

- peek reads only 64-bit little-endian files. If a file comes from a 32-bit or big-endian build, the
  Info view says so and no table opens.
- peek doesn't support encrypted LMDB 1.0 environments.
- peek doesn't register as a reader in the lock file. If another process writes to the database
  while peek reads it, peek can see a partly updated tree.
- `--list` lists the databases, not the keys inside them. An environment with no named databases has
  nothing to list.
- `--extract` needs a full key path such as `users/user:00042`. peek rejects a bare database name
  such as `users`.
