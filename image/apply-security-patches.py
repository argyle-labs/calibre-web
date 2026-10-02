#!/usr/bin/env python3
"""Apply argyle-labs security patches to a CWA image at build time.

Idempotent and self-verifying: each patch locates its target by content (not
line number, so it survives minor base drift), skips cleanly if already
applied, and RAISES if the target function has vanished — which fails the
Docker build rather than silently shipping an unpatched image.

Covered:
  - CVE-2026-7713  Kobo auth-token IDOR (generate/delete_auth_token) -> 403 guard
  - CVE-2026-7714  slot reserved (see note); not fabricated until the exact
                   upstream fix is vetted.
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

# The IDOR guard injected into both Kobo auth-token endpoints. Matches the
# upstream fix (crocodilestick/Calibre-Web-Automated commit 9f50bb2 / #1303):
# only the user themselves or an admin may mint/revoke a user's Kobo token.
GUARD = (
    "    # [argyle-labs sec] CVE-2026-7713 IDOR guard: only the user or an admin\n"
    "    # may operate on this user_id's Kobo auth token.\n"
    "    if current_user.id != user_id and not current_user.role_admin():\n"
    "        abort(403)\n"
)
GUARD_MARK = "CVE-2026-7713 IDOR guard"

# Functions in cps/kobo_auth.py that must carry the guard.
TARGET_FUNCS = ("generate_auth_token", "delete_auth_token")


def find_source_file(name: str) -> Path:
    """Locate cps/<name> inside the image, wherever CWA installs its app."""
    hits = subprocess.run(
        ["find", "/", "-type", "f", "-path", f"*/cps/{name}"],
        capture_output=True, text=True,
    ).stdout.split()
    # Prefer the real app tree; ignore any test/venv copies.
    real = [h for h in hits if "site-packages" not in h and "/test" not in h]
    chosen = real or hits
    if not chosen:
        raise SystemExit(f"FATAL: cps/{name} not found in image — base layout changed")
    return Path(chosen[0])


def inject_guard(path: Path) -> bool:
    src = path.read_text()
    changed = False
    for fn in TARGET_FUNCS:
        # Grab the function's def line + its signature so we can insert right
        # after it (handles multi-line signatures ending in ':').
        m = re.search(rf"^(def {re.escape(fn)}\(.*?\):[ \t]*\n)", src, re.MULTILINE | re.DOTALL)
        if not m:
            raise SystemExit(f"FATAL: {path.name}:{fn}() not found — cannot apply CVE-2026-7713 guard")
        # Already patched anywhere in the function region? skip.
        after = src[m.end():m.end() + 400]
        if GUARD_MARK in after:
            print(f"  skip {fn}: already guarded")
            continue
        src = src[:m.end()] + GUARD + src[m.end():]
        changed = True
        print(f"  patched {fn}: injected CVE-2026-7713 guard")
    if changed:
        path.write_text(src)
    return changed


def main() -> int:
    print("== argyle-labs CWA security patches ==")
    kobo = find_source_file("kobo_auth.py")
    print(f"target: {kobo}")
    inject_guard(kobo)
    # CVE-2026-7714 (missing auth on cps/cwa_functions.py admin endpoint):
    # upstream fix not yet vetted; add its guard here once confirmed. Do NOT
    # fabricate — leaving it unpatched is visible in docs/SECURITY, not hidden.
    print("== done ==")
    return 0


if __name__ == "__main__":
    sys.exit(main())
