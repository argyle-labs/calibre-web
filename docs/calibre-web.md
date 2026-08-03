# Calibre-Web operator notes

Standalone operator notes for running calibre-web from this repo by hand.
These reflect the [`compose.yml`](../compose.yml) shipped here.

- **Image**: `lscr.io/linuxserver/calibre-web` (tag via `CALIBRE_WEB_IMAGE_TAG`, default `latest`)
- **Web UI port**: `8083` in the container (host port via `CALIBRE_PORT`, default `8083`)
- **Upstream**: <https://github.com/janeczku/calibre-web>

## Deploy

```bash
docker compose up -d
```

> `DOCKER_MODS=linuxserver/mods:universal-calibre` is set in `compose.yml`. It
> installs the full Calibre toolchain on first start and can take 2-3 minutes,
> so the container may report unhealthy briefly (the healthcheck allows a
> 180s start period).

## Environment variables

These are the variables read by `compose.yml`, with the defaults it sets:

| Variable                | Default                       | Description                                     |
| ----------------------- | ----------------------------- | ----------------------------------------------- |
| `TZ`                    | `America/Denver`              | Container timezone                              |
| `CALIBRE_WEB_IMAGE_TAG` | `latest`                      | Image tag                                       |
| `CALIBRE_PORT`          | `8083`                        | Host port mapped to container port `8083`       |
| `CALIBRE_CONFIG_PATH`   | `/opt/appdata/calibre-web`    | Host path mounted at `/config`                  |
| `MEDIA_PATH`            | `/mnt/pool/data/media`        | Base media path; its `books` subdir mounts at `/books` |

Override any of these in your environment (or an `.env` file next to
`compose.yml`) to suit your host.

## Initial setup

The `universal-calibre` mod installs the Calibre CLI inside the container. To
initialize an empty library at `/books` before first use:

```bash
docker exec calibre-web calibredb add --empty --with-library /books
```

Then in the calibre-web UI, set the library path to `/books`.

On first run calibre-web creates a default administrator account with the
username `admin` and password `admin123`. This is a well-known default and must
be changed immediately after logging in for the first time.

## Troubleshooting

```bash
docker compose logs calibre-web
```

First start is slow because of the `DOCKER_MODS` install — wait 2-3 minutes
before treating startup log noise as an error.
