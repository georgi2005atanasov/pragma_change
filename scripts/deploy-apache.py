#!/usr/bin/env python3
"""Validate and activate local Apache model releases. Never edits proxy routing."""
import argparse
import fcntl
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, build_opener
from uuid import UUID

ALLOWED = {"release.json", "compatibility.json", "apache.conf.example", "README.txt", "SHA256SUMS"}
MAX_FILE = 64 * 1024 * 1024


def unpack(archive, destination):
    with tarfile.open(archive, "r:gz") as tar:
        members = tar.getmembers()
        if len(members) != len(ALLOWED) or {m.name for m in members} != ALLOWED:
            raise ValueError("unexpected, missing, or duplicate archive entries")
        for member in members:
            if not member.isfile() or member.size > MAX_FILE:
                raise ValueError("archive contains a link, directory, or oversized file")
            with tar.extractfile(member) as source, (destination / member.name).open("xb") as output:
                shutil.copyfileobj(source, output)
    names = set()
    for line in (destination / "SHA256SUMS").read_text().splitlines():
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
    if release["format_version"] != 1 or compatibility.get("format_version") != 1 or compatibility.get("module_abi") != 1:
        raise ValueError("unsupported release format or module ABI")
    if compatibility.get("proxy") != "apache" or compatibility.get("target") != "x86_64-unknown-linux-gnu":
        raise ValueError("not an Apache Linux x86-64 bundle")
    identifier = str(UUID(release["release_id"]))
    if compatibility.get("release_id") != identifier:
        raise ValueError("release identity mismatch")
    return identifier


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("status redirects are not allowed")


def status(url):
    parsed = urlsplit(url)
    if parsed.scheme != "http" or parsed.username or parsed.password or parsed.fragment:
        raise ValueError("status URL must be plain HTTP on a literal loopback address")
    if not ipaddress.ip_address(parsed.hostname or "").is_loopback:
        raise ValueError("status URL must use a literal loopback address")
    # Ignore proxy environment variables, including a user's HTTP_PROXY.
    with build_opener(ProxyHandler({}), NoRedirect()).open(url, timeout=2) as response:
        raw = response.read(65537)
        if len(raw) > 65536:
            raise ValueError("status response too large")
        value = json.loads(raw)
    value["release_id"] = str(UUID(value["release_id"]))
    if value.get("module_abi") != 1:
        raise ValueError("unexpected running module ABI")
    return value


def wait_for(url, identifier, timeout):
    deadline = time.monotonic() + timeout
    last = "no status response"
    while time.monotonic() < deadline:
        try:
            current = status(url)
            if current["release_id"] == identifier:
                return current
            last = f"worker still serves {current['release_id']}"
        except (OSError, ValueError, KeyError) as error:
            last = str(error)
        time.sleep(.2)
    raise RuntimeError(f"could not observe loaded release {identifier}: {last}")


def apache(args, *extra):
    command = [args.apachectl]
    if args.apache_config:
        command += ["-f", str(Path(args.apache_config).resolve())]
    subprocess.run(command + list(extra), check=True, timeout=30)


def link(root, name, target):
    temporary = root / (name + ".new")
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(target)
    temporary.replace(root / name)


def installed(root, target):
    path = Path(target)
    if path.is_absolute() or len(path.parts) != 2 or path.parts[0] != "releases":
        raise ValueError("deployment link must point to releases/UUID")
    identifier = str(UUID(path.parts[1]))
    release = root / path / "release.json"
    if release.resolve().parent.parent != (root / "releases").resolve():
        raise ValueError("release escapes deployment directory")
    if json.loads(release.read_text())["release_id"] != identifier:
        raise ValueError("installed release identity mismatch")
    return identifier


def activate(args, root, target):
    identifier = installed(root, target)
    current = root / "current"
    if current.exists() and not current.is_symlink():
        raise ValueError("current must be a deployment symlink")
    old = os.readlink(current) if current.is_symlink() else None
    previous_id = installed(root, old) if old else None
    link(root, "current", target)
    activated = False
    try:
        apache(args, "-t")
        apache(args, "-k", "start" if args.start else "graceful")
        activated = True
        observed = wait_for(args.status_url, identifier, args.timeout)
    except (subprocess.SubprocessError, OSError, ValueError, RuntimeError) as activation_error:
        if old:
            link(root, "current", old)
            try:
                apache(args, "-t")
                apache(args, "-k", "graceful")
                wait_for(args.status_url, previous_id, args.timeout)
            except Exception as recovery_error:
                raise RuntimeError(f"activation failed ({activation_error}); old link restored but recovery is unverified ({recovery_error})") from activation_error
        elif not activated:
            current.unlink(missing_ok=True)
        else:
            raise RuntimeError(f"activation is unverified ({activation_error}); no previous release exists, so the current link is retained. Inspect the local Apache instance before retrying") from activation_error
        raise
    if old and old != target:
        link(root, "previous", old)
    return observed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["validate", "install", "rollback", "status"])
    parser.add_argument("archive", nargs="?")
    parser.add_argument("--root", default="/etc/pragmachange/apache")
    parser.add_argument("--apachectl", default="/usr/sbin/apache2ctl")
    parser.add_argument("--apache-config")
    parser.add_argument("--validator", default="/usr/lib/pragmachange/pragma-cli")
    parser.add_argument("--status-url", default="http://127.0.0.1:19001/pragmachange-status")
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--start", action="store_true", help="Start a stopped Apache on first installation")
    args = parser.parse_args()
    if not 0 < args.timeout <= 300:
        parser.error("timeout must be greater than zero and at most 300 seconds")
    if args.command == "status":
        print(json.dumps(status(args.status_url)))
        return
    root = Path(args.root).resolve()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (root / ".deployment.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if args.command == "rollback":
            if args.start:
                parser.error("--start is only available for install")
            previous = root / "previous"
            if not previous.is_symlink():
                raise ValueError("no previous release available")
            target = os.readlink(previous)
            installed(root, target)
            subprocess.run([args.validator, "validate-release", str(root / target / "release.json")], check=True, timeout=30)
            print(json.dumps(activate(args, root, target)))
            return
        if not args.archive:
            parser.error("archive is required")
        with tempfile.TemporaryDirectory(prefix=".release-", dir=root) as tmp:
            staging = Path(tmp)
            identifier = unpack(args.archive, staging)
            subprocess.run([args.validator, "validate-release", str(staging / "release.json")], check=True, timeout=30)
            if args.command == "validate":
                print(f"Validated Apache release {identifier}")
                return
            secret = root / "partition_secret"
            if not secret.is_file() or len(bytes.fromhex(secret.read_text().strip())) != 32:
                raise ValueError("create a local partition_secret containing 64 hex characters first")
            releases = root / "releases"
            releases.mkdir(exist_ok=True)
            target = releases / identifier
            if target.exists():
                if target.is_symlink() or (target / "release.json").read_bytes() != (staging / "release.json").read_bytes():
                    raise ValueError("existing release id has different contents")
            else:
                shutil.copytree(staging, target)
            print(json.dumps(activate(args, root, f"releases/{identifier}")))
            print("Observed the new release in a serving worker. In-flight requests on draining workers can finish on the previous release.")


if __name__ == "__main__":
    main()
