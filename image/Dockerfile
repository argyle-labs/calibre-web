# Our own, patch-controlled Calibre-Web-Automated image.
#
# Why this exists: upstream CWA ships auth-bypass CVEs (CVE-2026-7713 /
# CVE-2026-7714) that are fixed in source but NOT in any released image. Rather
# than run a vulnerable `:latest`, we pin a known base and apply the security
# fixes ourselves at build time, on our own schedule. When upstream ships a
# fixed release, bump CWA_VERSION and drop the corresponding patch.
#
# Build:  docker build --build-arg CWA_VERSION=V4.0.6 -t ghcr.io/argyle-labs/calibre-web:4.0.6-sec1 image/
# The tag convention is <upstream-version>-sec<N> so a security-only rebuild is
# visible and pinnable (never `latest` in production).

ARG CWA_VERSION=V4.0.6
FROM crocodilestick/calibre-web-automated:${CWA_VERSION}

# Apply our vetted security patches over the pinned upstream source. The script
# is idempotent and FAILS THE BUILD if a target it expects is missing — so a
# base bump that moves the code can't silently ship an unpatched image.
COPY apply-security-patches.py /usr/local/bin/apply-security-patches.py
RUN python3 /usr/local/bin/apply-security-patches.py \
    && echo "security patches applied for ${CWA_VERSION}"

LABEL org.opencontainers.image.title="argyle-labs calibre-web-automated (patched)" \
      org.opencontainers.image.source="https://github.com/argyle-labs/calibre-web" \
      org.opencontainers.image.description="CWA pinned + security patches (CVE-2026-7713/-7714) applied on our schedule"
