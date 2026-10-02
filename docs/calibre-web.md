# Calibre-Web

E-book library with Calibre integration.

- **Host**: <host> (<ip>)
- **Port**: 8083 (configurable via `CALIBRE_PORT`)
- **Image**: `lscr.io/linuxserver/calibre-web`
- **Compose**: [compose/calibre-web/](../../compose/calibre-web/docker-compose.yml)

## Deploy

```bash
cd compose/calibre-web
docker compose up -d
```

> Note: `DOCKER_MODS=linuxserver/mods:universal-calibre` is set automatically — it installs Calibre and takes 2-3 minutes on first start.

## Environment Variables

| Variable                | Default          | Description                |
| ----------------------- | ---------------- | -------------------------- |
| `TZ`                    | `Etc/UTC` | Timezone                   |
| `CALIBRE_WEB_IMAGE_TAG` | `latest`         | Image tag                  |
| `CALIBRE_CONFIG_PATH`   | `./config`       | Config directory           |
| `CALIBRE_PORT`          | `8083`           | Host port                  |
| `MEDIA_PATH`            | *(required)*     | Base path — books subdir is mounted |

## Initial Setup

The `DOCKER_MODS=linuxserver/mods:universal-calibre` mod installs the full Calibre CLI inside the container. On first deploy, create the library metadata before starting calibre-web:

```bash
# Run once to initialize an empty Calibre library at /books
docker exec calibre-web calibredb add --empty --with-library /books
```

Then in the calibre-web UI, set the library path to `/books` and log in with default credentials (`admin` / `admin123`). Change the password immediately.

## Ebook pipeline (with LazyLibrarian)

Calibre-Web is the **reader/library UI** — it does not acquire books. Pair it with
LazyLibrarian for the full pipeline:

1. **One shared Calibre library.** Calibre-Web reads a Calibre library
   (`metadata.db`) at the books directory (e.g. `/books` → `/data/media/books`).
   Initialize it once (see *Initial Setup*) before anything writes there.
2. **LazyLibrarian imports into that library.** In LazyLibrarian → *Settings →
   Processing*, set the Calibre library path to the same books directory so
   grabbed ebooks are added via `calibredb` with correct metadata, instead of
   dropping loose files. Loose, oddly-named files (e.g. `Some Title 2.epub`) in
   the books dir are a sign LazyLibrarian is **not** writing through Calibre —
   fix the Calibre integration and re-import so the library stays clean.
3. **Metadata.** Calibre-Web shows whatever metadata is in `metadata.db`. Use
   *Edit Metadata* (or Calibre desktop / `calibredb`) to fix covers and series;
   LazyLibrarian's provider (GoogleBooks/OpenLibrary — **not** the defunct
   GoodReads) supplies metadata at import time.
4. **Kavita** can serve the same books directory in parallel for a different
   reading experience; both readers index the same tree read-only.

## Troubleshooting

```bash
docker compose logs calibre-web
```

First start is slow due to DOCKER_MODS installation — wait 2-3 minutes before checking logs for errors.

## Scan-on-import integration (orca)

Unlike the other content servers, Calibre-Web has **no scan API** — it reads the
Calibre library database (`metadata.db`) live. New books appear automatically once
they are added to the Calibre library (e.g. LazyLibrarian → `calibredb add`, or a
Calibre-Web upload). So there is nothing for the
[scan dispatcher](../../sabnzbd/docs/scan-dispatcher.md) to push here.

**The real integration is upstream:** LazyLibrarian must add finished ebooks into
the Calibre library so Calibre-Web (and Kavita) can serve them. Track that as the
ebooks-ingest follow-up. **Endpoint:** `http://10.0.0.6:8083` (baldur); creds in
1Password `calibre-web` (Private vault).

**Future plugin capability:** `configure` wires LazyLibrarian's Calibre target;
`status` reports library size. See [CAPABILITIES.md](../CAPABILITIES.md).
