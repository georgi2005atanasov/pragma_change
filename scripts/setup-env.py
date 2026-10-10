#!/usr/bin/env python3
"""Generate new private settings without replacing existing credentials."""

import argparse
import os
from pathlib import Path
import secrets
import shlex

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    "--expansion",
    action="store_true",
    help="create .env.expansion and a separate credential-vault key",
)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
path = root / (".env.expansion" if args.expansion else ".env")
if path.exists():
    raise SystemExit(
        f"{path.name} already exists; refusing to overwrite local settings"
    )
if args.expansion:
    key_dir = root / ".secrets"
    key_dir.mkdir(mode=0o700, exist_ok=True)
    key_path = key_dir / "credential-key"
    # Never reuse an unknown existing key or rotate a working vault automatically.
    fd = os.open(key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as file:
        file.write(secrets.token_hex(32) + "\n")
    values = {
        "CREDENTIAL_KEY_FILE": str(key_path),
        "BEHAVIOR_URL": "http://127.0.0.1:8090",
        "BEHAVIOR_ADMIN_TOKEN": secrets.token_hex(32),
        "BEHAVIOR_PARTITION_SECRET": secrets.token_hex(32),
        "BEHAVIOR_DB_PASSWORD": secrets.token_hex(32),
    }
else:
    values = {
        "POSTGRES_PASSWORD": secrets.token_urlsafe(32),
        "ADMIN_USERNAME": "admin",
        "ADMIN_PASSWORD": secrets.token_urlsafe(24),
        "TRAINER_TOKEN": secrets.token_hex(32),
        "PARTITION_SECRET": secrets.token_hex(32),
        "PUBLIC_ORIGIN": "http://localhost:3000",
        "COOKIE_SECURE": "false",
        "ARTIFACT_DIR": str(root / "artifacts"),
    }
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "w") as file:
    file.write(
        "".join(f"{key}={shlex.quote(value)}\n" for key, value in values.items())
    )
print(
    f"Created private {path.name}. See docs/setup-commands.md for process commands. Keep these credentials and the vault key with your backups."
)
