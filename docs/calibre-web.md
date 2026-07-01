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

## Troubleshooting

```bash
docker compose logs calibre-web
```

First start is slow due to DOCKER_MODS installation — wait 2-3 minutes before checking logs for errors.
