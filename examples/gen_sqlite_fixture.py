#!/usr/bin/env python3
# Regenerate the SQLite test fixture at `test-data/library.sqlite`.
#
# Run once to produce the checked-in sample; not part of the build.
# Requires only Python 3 stdlib + network access.
#
# Data source: Project Gutenberg catalog feed
#   https://www.gutenberg.org/cache/epub/feeds/pg_catalog.csv
# Project Gutenberg's catalogue metadata is released into the public
# domain (no attribution required), and the works it describes are
# themselves public-domain texts. Safe to redistribute as a fixture.
#
# Output: a normalized library DB with these tables —
#   languages          (code PK, name)
#   authors            (id PK, name, birth_year, death_year)
#   subjects           (id PK, name)
#   bookshelves        (id PK, name)
#   books              (id PK, title, type, issued, language_code FK)
#   book_authors       (book_id FK, author_id FK)
#   book_subjects      (book_id FK, subject_id FK)
#   book_bookshelves   (book_id FK, bookshelf_id FK)
# Plus a `popular_authors` view and a few indexes, to give peek's
# upcoming SQLite viewer realistic schema variety to render.
#
# Caps the catalogue at MAX_BOOKS rows so the committed fixture stays
# small (~1-2 MB) while still exercising several thousand rows across
# the join tables.

import csv
import io
import os
import re
import sqlite3
import sys
import urllib.request
from pathlib import Path

CATALOG_URL = "https://www.gutenberg.org/cache/epub/feeds/pg_catalog.csv"
MAX_BOOKS = 2500

# (name, role-or-None). Trailing role brackets like "[Editor]" are
# stripped from the name and stored separately. Birth/death year are
# pulled from the trailing ", YYYY-YYYY" tail when present.
AUTHOR_TAIL_RE = re.compile(r",\s*(\d{3,4}\??)\s*-\s*(\d{3,4}\??)?\s*$")
ROLE_RE = re.compile(r"\s*\[([^\]]+)\]\s*$")


def parse_author(raw: str):
    name = raw.strip()
    role = None
    m = ROLE_RE.search(name)
    if m:
        role = m.group(1).strip()
        name = ROLE_RE.sub("", name).strip()
    birth = death = None
    m = AUTHOR_TAIL_RE.search(name)
    if m:
        b, d = m.group(1), m.group(2)
        birth = int(b.rstrip("?")) if b and b.rstrip("?").isdigit() else None
        death = int(d.rstrip("?")) if d and d.rstrip("?").isdigit() else None
        name = AUTHOR_TAIL_RE.sub("", name).strip()
    return name, role, birth, death


def split_semicolon(field: str):
    if not field:
        return []
    return [s.strip() for s in field.split(";") if s.strip()]


LANG_NAMES = {
    "en": "English", "fr": "French", "de": "German", "es": "Spanish",
    "it": "Italian", "nl": "Dutch", "pt": "Portuguese", "fi": "Finnish",
    "sv": "Swedish", "la": "Latin", "el": "Greek", "ru": "Russian",
    "zh": "Chinese", "ja": "Japanese", "ar": "Arabic", "hu": "Hungarian",
    "pl": "Polish", "cs": "Czech", "da": "Danish", "no": "Norwegian",
    "is": "Icelandic", "ga": "Irish", "cy": "Welsh", "tl": "Tagalog",
    "eo": "Esperanto", "he": "Hebrew", "tr": "Turkish", "ko": "Korean",
}


def fetch_catalog():
    cache = Path("/tmp/pg_catalog.csv")
    if cache.exists() and cache.stat().st_size > 1_000_000:
        return cache.read_bytes()
    print(f"fetching {CATALOG_URL} ...", file=sys.stderr)
    with urllib.request.urlopen(CATALOG_URL, timeout=60) as r:
        data = r.read()
    cache.write_bytes(data)
    return data


def main():
    repo_root = Path(__file__).resolve().parent.parent
    out_path = repo_root / "test-data" / "library.sqlite"
    if out_path.exists():
        out_path.unlink()

    raw = fetch_catalog().decode("utf-8", errors="replace")
    reader = csv.DictReader(io.StringIO(raw))

    con = sqlite3.connect(out_path)
    cur = con.cursor()
    cur.executescript("""
        PRAGMA foreign_keys = ON;

        CREATE TABLE languages (
            code TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE authors (
            id          INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            birth_year  INTEGER,
            death_year  INTEGER,
            UNIQUE (name, birth_year, death_year)
        );

        CREATE TABLE subjects (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE bookshelves (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE books (
            id            INTEGER PRIMARY KEY,
            title         TEXT NOT NULL,
            type          TEXT NOT NULL,
            issued        TEXT,
            language_code TEXT REFERENCES languages(code)
        );

        CREATE TABLE book_authors (
            book_id   INTEGER NOT NULL REFERENCES books(id),
            author_id INTEGER NOT NULL REFERENCES authors(id),
            role      TEXT,
            PRIMARY KEY (book_id, author_id)
        );

        CREATE TABLE book_subjects (
            book_id    INTEGER NOT NULL REFERENCES books(id),
            subject_id INTEGER NOT NULL REFERENCES subjects(id),
            PRIMARY KEY (book_id, subject_id)
        );

        CREATE TABLE book_bookshelves (
            book_id      INTEGER NOT NULL REFERENCES books(id),
            bookshelf_id INTEGER NOT NULL REFERENCES bookshelves(id),
            PRIMARY KEY (book_id, bookshelf_id)
        );

        CREATE INDEX idx_books_language    ON books(language_code);
        CREATE INDEX idx_books_issued      ON books(issued);
        CREATE INDEX idx_authors_birth     ON authors(birth_year);
        CREATE INDEX idx_book_authors_aid  ON book_authors(author_id);
        CREATE INDEX idx_book_subjects_sid ON book_subjects(subject_id);

        CREATE VIEW popular_authors AS
            SELECT a.id, a.name, a.birth_year, a.death_year,
                   COUNT(ba.book_id) AS book_count
            FROM authors a
            JOIN book_authors ba ON ba.author_id = a.id
            GROUP BY a.id
            HAVING book_count >= 5
            ORDER BY book_count DESC;
    """)

    author_ids = {}
    subject_ids = {}
    shelf_ids = {}
    seen_langs = set()

    def author_id(raw):
        name, role, birth, death = parse_author(raw)
        if not name:
            return None, None
        key = (name, birth, death)
        if key in author_ids:
            return author_ids[key], role
        cur.execute(
            "INSERT INTO authors (name, birth_year, death_year) VALUES (?, ?, ?)",
            (name, birth, death),
        )
        author_ids[key] = cur.lastrowid
        return author_ids[key], role

    def subject_id(name):
        if name in subject_ids:
            return subject_ids[name]
        cur.execute("INSERT INTO subjects (name) VALUES (?)", (name,))
        subject_ids[name] = cur.lastrowid
        return subject_ids[name]

    def shelf_id(name):
        if name in shelf_ids:
            return shelf_ids[name]
        cur.execute("INSERT INTO bookshelves (name) VALUES (?)", (name,))
        shelf_ids[name] = cur.lastrowid
        return shelf_ids[name]

    def ensure_language(code):
        if not code or code in seen_langs:
            return
        seen_langs.add(code)
        cur.execute(
            "INSERT INTO languages (code, name) VALUES (?, ?)",
            (code, LANG_NAMES.get(code, code)),
        )

    inserted = 0
    for row in reader:
        if row.get("Type") != "Text":
            continue
        try:
            book_id = int(row["Text#"])
        except (TypeError, ValueError):
            continue

        title = (row.get("Title") or "").strip()
        if not title:
            continue
        issued = row.get("Issued") or None
        lang_field = (row.get("Language") or "").strip()
        primary_lang = lang_field.split(",")[0].strip() if lang_field else None
        if primary_lang:
            ensure_language(primary_lang)

        cur.execute(
            "INSERT INTO books (id, title, type, issued, language_code) "
            "VALUES (?, ?, ?, ?, ?)",
            (book_id, title, row["Type"], issued, primary_lang),
        )

        for a in split_semicolon(row.get("Authors") or ""):
            aid, role = author_id(a)
            if aid is None:
                continue
            cur.execute(
                "INSERT OR IGNORE INTO book_authors (book_id, author_id, role) "
                "VALUES (?, ?, ?)",
                (book_id, aid, role),
            )

        for s in split_semicolon(row.get("Subjects") or ""):
            cur.execute(
                "INSERT OR IGNORE INTO book_subjects (book_id, subject_id) "
                "VALUES (?, ?)",
                (book_id, subject_id(s)),
            )

        for sh in split_semicolon(row.get("Bookshelves") or ""):
            cur.execute(
                "INSERT OR IGNORE INTO book_bookshelves (book_id, bookshelf_id) "
                "VALUES (?, ?)",
                (book_id, shelf_id(sh)),
            )

        inserted += 1
        if inserted >= MAX_BOOKS:
            break

    con.commit()
    cur.execute("ANALYZE")
    con.commit()
    con.execute("VACUUM")
    con.close()

    size = out_path.stat().st_size
    print(
        f"wrote {out_path.relative_to(repo_root)} "
        f"({inserted} books, {size / 1024:.1f} KiB)",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
