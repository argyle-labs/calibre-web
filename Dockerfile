# TODO: base image + build for calibre-web. Mirror jellyfin/Dockerfile conventions.
FROM debian:12-slim
LABEL org.opencontainers.image.source="https://github.com/argyle-labs/calibre-web"
EXPOSE 8083
