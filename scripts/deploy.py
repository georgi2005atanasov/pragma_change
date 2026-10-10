#!/usr/bin/env python3
"""Verify, install, and roll back locally supplied PragmaChange release bundles."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
from uuid import UUID

ALLOWED = {"release.json", "compatibility.json", "nginx.conf.example", "README.txt", "SHA256SUMS"}


def unpack(archive, destination):
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        if len(members) != len(ALLOWED) or {m.name for m in members} != ALLOWED:
            raise ValueError("unexpected, missing, or duplicate archive entries")
        for member in members:
            if not member.isfile() or member.size > 64 * 1024 * 1024:
                raise ValueError("archive contains a link, directory, or oversized file")
            source = tar.extractfile(member)
            with (destination / member.name).open("wb") as output:
                shutil.copyfileobj(source, output)
    lines = (destination / "SHA256SUMS").read_text().splitlines()
    names = set()
    for line in lines:
        digest, name = line.split("  ", 1)
        if name not in ALLOWED - {"SHA256SUMS"} or name in names:
            raise ValueError("invalid checksum manifest")
        names.add(name)
        if hashlib.sha256((destination / name).read_bytes()).hexdigest() != digest:
            raise ValueError(f"checksum mismatch: {name}")
    if names != ALLOWED - {"SHA256SUMS"}:
        raise ValueError("incomplete checksum manifest")
    release = json.loads((destination / "release.json").read_text())
    compatibility = json.loads((destination / "compatibility.json").read_text())
    if release["format_version"] != 1 or compatibility["module_abi"] != 1:
        raise ValueError("unsupported release format")
    identifier = str(UUID(release["release_id"]))
    if compatibility["release_id"] != identifier:
        raise ValueError("release identity mismatch")
    return identifier


def link(root, name, target):
    temporary = root / (name + ".new")
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(target)
    temporary.replace(root / name)


def nginx(args, *extra):
    command = [args.nginx]
    if args.nginx_prefix:
        command += ["-p", str(Path(args.nginx_prefix).resolve())]
    if args.nginx_config:
        command += ["-c", str(Path(args.nginx_config).resolve())]
    subprocess.run(command + list(extra), check=True)


def activate(args, root, target):
    current = root / "current"
    old = os.readlink(current) if current.is_symlink() else None
    link(root, "current", target)
    try:
        nginx(args, "-t")
        if getattr(args, "start", False):
            nginx(args)
        else:
            nginx(args, "-s", "reload")
    except (subprocess.CalledProcessError, OSError):
        if old:
            link(root, "current", old)
            nginx(args, "-t")
            nginx(args, "-s", "reload")
        else:
            current.unlink(missing_ok=True)
        raise
    if old and old != target:
        link(root, "previous", old)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["validate", "install", "rollback"])
    parser.add_argument("archive", nargs="?")
    parser.add_argument("--root", default="/etc/nginx/pragmachange")
    parser.add_argument("--nginx", default="nginx")
    parser.add_argument("--start", action="store_true", help="Start Nginx instead of reloading it on the first install")
    parser.add_argument("--nginx-config")
    parser.add_argument("--nginx-prefix")
    parser.add_argument("--validator", default="pragma-cli", help="Path to the Rust release validator")
    args = parser.parse_args()
    root = Path(args.root).resolve()
    root.mkdir(parents=True, exist_ok=True)
    with (root / ".deployment.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if args.command == "rollback":
            previous = root / "previous"
            if not previous.is_symlink():
                raise ValueError("no previous release available")
            activate(args, root, os.readlink(previous))
            print("Previous release activated")
            return
        if not args.archive:
            parser.error("archive is required")
        with tempfile.TemporaryDirectory(prefix=".release-", dir=root) as tmp:
            staging = Path(tmp)
            identifier = unpack(args.archive, staging)
            subprocess.run([args.validator, "validate-release", str(staging / "release.json")], check=True)
            if args.command == "validate":
                print(f"Validated {identifier}")
                return
            if not (root / "partition_secret").is_file():
                raise ValueError("create a local partition_secret containing 64 hex characters first")
            releases = root / "releases"
            releases.mkdir(exist_ok=True)
            target = releases / identifier
            if target.exists():
                if (target / "release.json").read_bytes() != (staging / "release.json").read_bytes():
                    raise ValueError("existing release id has different contents")
            else:
                shutil.copytree(staging, target)
            activate(args, root, f"releases/{identifier}")
            print(f"Activated {identifier}")


if __name__ == "__main__":
    main()
