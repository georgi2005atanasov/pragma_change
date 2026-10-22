#!/usr/bin/env python3
"""Build a distro-specific native module package inside its target distribution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="build/apache/packages")
    parser.add_argument("--revision", default="uncommitted")
    args = parser.parse_args()
    system = dict(line.split("=", 1) for line in Path("/etc/os-release").read_text().splitlines() if "=" in line)
    distribution, version = system["ID"].strip('"'), system["VERSION_ID"].strip('"')
    if (distribution, version) not in (("debian", "12"), ("ubuntu", "24.04")) or platform.machine() != "x86_64":
        raise SystemExit("Build inside Debian 12 or Ubuntu 24.04 on x86-64")
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    suffix = distribution + version.replace(".", "")
    package_version = "0.1.0-1+" + suffix
    with tempfile.TemporaryDirectory(prefix="apache-package-") as tmp:
        work = Path(tmp)
        stage = work / "root"
        copies = {
            "build/apache/mod_pragmachange.so": "usr/lib/apache2/modules/mod_pragmachange.so",
            "target/release/pragma-cli": "usr/lib/pragmachange/pragma-cli",
            "scripts/deploy-apache.py": "usr/bin/pragmachange-apache",
            "deploy/apache.conf.example": "usr/share/doc/libapache2-mod-pragmachange/apache.conf.example",
        }
        for source, destination in copies.items():
            target = stage / destination
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / source, target)
            target.chmod(0o755 if destination.startswith("usr/bin/") or destination.endswith((".so", "pragma-cli")) else 0o644)
        for executable in ("usr/lib/apache2/modules/mod_pragmachange.so", "usr/lib/pragmachange/pragma-cli"):
            subprocess.run(["strip", "--strip-unneeded", str(stage / executable)], check=True)
        loader = stage / "etc/apache2/mods-available/pragmachange.load"
        loader.parent.mkdir(parents=True, exist_ok=True)
        loader.write_text("LoadModule pragmachange_module /usr/lib/apache2/modules/mod_pragmachange.so\n")
        metadata = stage / "usr/share/doc/libapache2-mod-pragmachange"
        apache_version = subprocess.check_output(["dpkg-query", "-W", "-f=${Version}", "apache2-bin"], text=True)
        api = subprocess.check_output(["apxs", "-q", "HTTPD_VERSION"], text=True).strip()
        dependencies = subprocess.check_output(["dpkg-query", "-W", "-f=${Package}=${Version}\n", "apache2-bin", "apache2-dev", "libapr1*", "libaprutil1*", "libc6"], text=True)
        manifest = {"format_version": 1, "proxy": "apache", "module_abi": 1, "module_version": "0.1.0", "package_version": package_version, "distribution": distribution, "distribution_version": version, "architecture": "amd64", "apache_version": api, "apache_package": apache_version, "source_revision": args.revision, "build_packages": dependencies.strip().splitlines(), "sha256": {destination: hashlib.sha256((stage / destination).read_bytes()).hexdigest() for destination in copies.values()}}
        (metadata / "compatibility.json").write_text(json.dumps(manifest, indent=2) + "\n")
        (metadata / "README").write_text("Enable with a2enmod pragmachange proxy proxy_http. Create a local partition secret, adapt apache.conf.example and enable PragmaChange only in protected locations. Use pragmachange-apache validate/install/status/rollback. Installing this package does not enable the module or restart Apache. Native binary upgrades require a full Apache restart; model-only updates use graceful reload. Envoy/HAProxy and remote Apache administration are not included.\n")
        control = stage / "DEBIAN"
        control.mkdir()
        (work / "debian").mkdir()
        (work / "debian/control").write_text("Source: pragmachange\nMaintainer: PragmaChange contributors\n\nPackage: libapache2-mod-pragmachange\nArchitecture: amd64\nDescription: Native PragmaChange request detector\n")
        shared = subprocess.check_output(["dpkg-shlibdeps", "-O", "--ignore-missing-info", "-e" + str(stage / "usr/lib/apache2/modules/mod_pragmachange.so"), "-e" + str(stage / "usr/lib/pragmachange/pragma-cli")], cwd=work, text=True).strip().removeprefix("shlibs:Depends=")
        (control / "control").write_text(f"Package: libapache2-mod-pragmachange\nVersion: {package_version}\nArchitecture: amd64\nMaintainer: PragmaChange contributors\nSection: httpd\nPriority: optional\nDepends: apache2, apache2-api-20120211, apache2-bin (>= {apache_version}), python3, {shared}\nDescription: Native PragmaChange request detector for Apache HTTP Server\n Evaluates local models before reverse proxy forwarding with shared history.\n")
        (control / "conffiles").write_text("/etc/apache2/mods-available/pragmachange.load\n")
        # No maintainer scripts: never auto-enable, restart, or alter routing.
        artifact = output / f"libapache2-mod-pragmachange_{package_version}_amd64.deb"
        subprocess.run(["dpkg-deb", "--root-owner-group", "--build", str(stage), str(artifact)], check=True)
        (output / (artifact.name + ".sha256")).write_text(hashlib.sha256(artifact.read_bytes()).hexdigest() + "  " + artifact.name + "\n")
        shutil.copyfile(metadata / "compatibility.json", output / "compatibility.json")
        shutil.copyfile(stage / "usr/lib/apache2/modules/mod_pragmachange.so", output / "mod_pragmachange.so")


if __name__ == "__main__":
    main()
