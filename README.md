<p align="center">
  <img src="assets/icon-256.png" width="120" alt="calibre-web" />
</p>

# calibre-web

Calibre-Web is a web app for browsing, reading, and downloading ebooks from a Calibre library.

A first-party [orca](https://github.com/argyle-labs/orca) plugin (service-backend).

This repo is **self-contained** — the steps below run calibre-web **by hand, without orca**. orca automates exactly this (same image, ports, and data) through one generic surface.

---

## Run it without orca

### Docker Compose

```yaml
# compose.yml
services:
  calibre-web:
    image: lscr.io/linuxserver/calibre-web:latest
    container_name: calibre-web
    restart: unless-stopped
    ports:
      - "8083:8083/tcp"   # web UI
    volumes:
      - ./config:/config
      - /path/to/books:/books
```

```sh
docker compose up -d
```

### Other runtimes

**Podman** — the compose above works with `podman compose up -d`, or run it directly:

```sh
podman run -d --name calibre-web --restart unless-stopped \
    -p 8083:8083/tcp \
    -v ./config:/config \
    -v /path/to/books:/books \
    lscr.io/linuxserver/calibre-web:latest
```

**LXC** — on a container-capable LXC (e.g. a Proxmox LXC with nesting enabled) run the same image via Docker/Podman as above, or install calibre-web from upstream directly on the guest: <https://github.com/janeczku/calibre-web>.

**VM** — install calibre-web from upstream (<https://github.com/janeczku/calibre-web>) or run the same container image inside the VM; expose port `8083`.

**Unraid** — add via *Community Applications*, or *Docker → Add Container* with image `lscr.io/linuxserver/calibre-web:latest`, port `8083`, and the volume paths above.

### Dependencies

Requires an existing Calibre library (`metadata.db`) on the mounted books volume.

### Ports & data

| | |
|---|---|
| Default port | `8083` |
| Upstream | <https://github.com/janeczku/calibre-web> |
| Operator notes | [calibre-web.md](docs/calibre-web.md) |


### Backup & restore

Back up the config/data volume(s) above — that's the whole service state (stop the container first for a clean copy). Restore by putting them back and starting it.

> With orca this is **`service.backup` / `service.restore`** — location-agnostic (docker / podman / lxc / vm), one command regardless of where calibre-web runs. No per-service backup script.

## With orca

orca drives this plugin through the single generic `service.*` surface — no per-plugin tools:

```sh
orca service.deploy calibre-web      # render + launch on any supported runtime
orca service.status calibre-web      # health + rich diagnostics (typed payload)
orca service.backup calibre-web      # location-agnostic backup (tar; PBS on Proxmox)
orca service.configure calibre-web   # apply config via the upstream API
```

## Layout

- `src/` — the plugin (pure Rust): the `ServiceBackend` descriptor + `configure` / `status`.
- `docs/` — standalone operator notes.
- [CAPABILITIES.md](CAPABILITIES.md) — the service-backend contract checklist.
- `assets/` — plugin icon.
