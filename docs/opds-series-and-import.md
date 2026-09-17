# OPDS series, metadata embedding, and dedup on import (operational KB)

Field notes from a large ebook library cleanup running Calibre-Web-Automated
(CWA). Complements [calibre-web.md](calibre-web.md).

## Embed metadata into files after ANY edit

`calibredb set_metadata` writes series/author only into `metadata.db`, **not** into
the book files. OPDS-downloaded or file-based readers therefore see no series. After
any metadata edit, embed it into the files so it travels with them:

```
docker exec <container> calibredb embed_metadata --with-library=/calibre-library all
```

This embeds series as EPUB3 `belongs-to-collection` + `group-position` into every
file. Run it after **every** `calibredb` metadata edit. Caveat: embedding is not
retroactive to already-synced device copies — those must be re-downloaded.

## OPDS series for reader apps — Calibre-Web's OPDS has no series field

Some reader apps read the series from the OPDS *feed entries* the way native
Calibre tags them (a `SERIES: <name> [n]` line in the entry). **Calibre-Web's
built-in OPDS carries no series field** (only genre categories), so those readers
cannot group by series — and neither a DB series edit nor `embed_metadata` fixes
it, because the reader reads the feed, not the file.

**Fix:** run a native `calibre-server` alongside Calibre-Web — its OPDS *does*
embed `SERIES: <name> [n]` — and point the reader app at it.

**Gotcha:** the `calibre-server` binary is installed during CWA container *init*,
not baked into the raw image, so you cannot run it from a bare sidecar
(`/usr/bin/calibre-server: no such file`). Launch it **inside** the CWA container
via a custom init script (linuxserver `custom-cont-init.d`) that waits for the
binary to exist, then starts it read-only against the library, e.g.:

```
nohup calibre-server --listen-on 0.0.0.0 --port <opds-port> /calibre-library &
```

Serve it anonymous/read-only so it respects CWA's single-writer model, then route
the reader app to that OPDS endpoint (optionally behind your reverse proxy with
basic auth).

## Dedup on metadata, not filename

CWA auto-ingest does **not** dedupe on import, and mangled/derived filenames bypass
filename-based automerge — so bulk imports readily create a full parallel set of
duplicates. Dedup on **real file metadata**, not filename: match on author surname
**AND** a decoration-stripped core title (strip `#NN`, `book/vol/part/no N`,
parens/brackets, leading articles).

Use **strict title equality**, not substring containment — containment
false-matches sequels (e.g. "Foundation" ⊂ "Second Foundation") and would skip a
legitimately-wanted next book. A false skip of a wanted book is worse than a rare
series-prefixed duplicate. Run the dedup pass after any bulk ingest, keeping the
series-tagged / lowest-id copy.

## Library-path trap under CWA

Point tooling at the real library (`--with-library=/calibre-library`). A stale
path (e.g. `/books`) silently auto-creates an empty phantom library and returns 0
books. Drop new files into the CWA auto-ingest directory, not the old ingest path.
