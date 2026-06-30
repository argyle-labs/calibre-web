#!/usr/bin/env bash
# Creates and configures a calibre-web LXC on Proxmox VE. Run on the host as root.
set -euo pipefail
VMID="${1:?Usage: $0 <vmid> [options]}"
# TODO: pct create / config / install calibre-web. Mirror jellyfin/lxc/provision.sh.
echo "[provision] calibre-web LXC $VMID — not yet implemented"
