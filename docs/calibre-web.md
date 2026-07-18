# Calibre-Web (Calibre-Web-Automated)

Ebook web library/reader with an auto-ingest pipeline. Runs the
[Calibre-Web-Automated](https://github.com/crocodilestick/calibre-web-automated)
(CWA) image — plain calibre-web plus a watched ingest folder that converts,
de-DRMs, and imports dropped files automatically.

- **Port**: 8083 (configurable via `CALIBRE_PORT`)
- **Image**: `crocodilestick/calibre-web-automated`
- **Compose**: [compose.yml](../compose.yml)

## Volumes

| Container Path      | Purpose                                                        |
| ------------------- | ------------------------------------------------------------- |
| `/config`           | CWA config + persisted Calibre plugins (`.config/calibre/plugins/`) |
| `/calibre-library`  | Calibre library (holds `metadata.db`)                          |
| `/cwa-book-ingest`  | Drop folder — files here are auto-converted, de-DRM'd, imported |

## Environment Variables

| Variable                | Default                | Description                          |
| ----------------------- | ---------------------- | ------------------------------------ |
| `TZ`                    | `America/Denver`       | Timezone                             |
| `PUID` / `PGID`         | `1000`                 | Run-as user/group                    |
| `CALIBRE_WEB_IMAGE_TAG` | `latest`               | Image tag                            |
| `CALIBRE_CONFIG_PATH`   | `/opt/appdata/calibre-web` | Config directory                 |
| `CALIBRE_PORT`          | `8083`                 | Host port                            |
| `MEDIA_PATH`            | `/mnt/pool/data/media` | Media base — `books` + `book-ingest` |

## Auto-ingest & de-DRM

Drop any supported file into `/cwa-book-ingest`; CWA converts and imports it,
running installed Calibre plugins on the way in. Install DeDRM/Obok so they
persist across container recreation:

```bash
mkdir -p <CALIBRE_CONFIG_PATH>/.config/calibre/plugins
docker exec calibre-web calibre-customize -a /path/inside/DeDRM_plugin.zip
```

### DRM support reality

- **Works**: older Adobe/Kobo/legacy Kindle formats current DeDRM handles.
- **Does NOT work**: modern Kindle for Mac ("Lassen" 7.x) / current KFX. No
  public tool removes that DRM; it needs an account key hardened Kindle clients
  no longer expose. Those files import but stay encrypted.

## ⚠️ Security: do not deploy `:latest` yet

As of the latest CWA release (**v4.0.6**, Feb 2026) two remotely-exploitable
auth flaws are **unpatched in any released image**:

- **CVE-2026-7713** — Kobo auth-token IDOR → account takeover. Fixed in v4.0.7
  (committed `9f50bb2`, not yet released).
- **CVE-2026-7714** — missing authentication on an admin endpoint
  (`cps/cwa_functions.py`). No fixed release.

Both have public exploits. **Do not point this plugin at `:latest` in
production until ≥ v4.0.7 ships** (or build an image from the patched commit).
Until then, plain `lscr.io/linuxserver/calibre-web` is the safer runtime — it
lacks the affected CWA endpoints. Pin the image via `CALIBRE_WEB_IMAGE_TAG`
once a fixed tag exists.

## Migrating from plain calibre-web

CWA reads the same Calibre library. Point `/calibre-library` at the existing
`books` dir (with its `metadata.db`) — no conversion needed. The old image
mounted the library at `/books`; CWA uses `/calibre-library`, so only the
container-side path changes, not the data.

## Deploy with orca

```bash
orca service.deploy calibre-web --host <docker-host>   # e.g. baldur
orca service.status calibre-web
```

`service.deploy` renders the plugin's `workload_spec` (image, ports, the three
volumes above) and hands it to the deploy target that owns the requested host.

## Troubleshooting

```bash
docker compose logs calibre-web
```
