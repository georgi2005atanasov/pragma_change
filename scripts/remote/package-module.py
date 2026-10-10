#!/usr/bin/env python3
"""Package an off-host build. Sign metadata binding it to the target nginx -V output."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('--module', required=True)
parser.add_argument('--nginx-build-output', required=True, help='Exact nginx -V stdout+stderr bytes from target')
parser.add_argument('--architecture', required=True)
parser.add_argument('--signing-key', required=True, help='Ed25519 private key')
parser.add_argument('--output', required=True)
args = parser.parse_args()
with tempfile.TemporaryDirectory() as directory:
    directory = Path(directory)
    data = Path(args.module).read_bytes()
    (directory / 'module.so').write_bytes(data)
    manifest = {'abi': 1, 'sha256': hashlib.sha256(data).hexdigest(), 'architecture': args.architecture,
                'nginx_build_hash': hashlib.sha256(Path(args.nginx_build_output).read_bytes()).hexdigest()}
    (directory / 'manifest.json').write_text(json.dumps(manifest, sort_keys=True))
    subprocess.run(['openssl', 'pkeyutl', '-sign', '-inkey', args.signing_key, '-rawin',
                    '-in', str(directory / 'manifest.json'), '-out', str(directory / 'signature')], check=True)
    with tarfile.open(args.output, 'w:gz') as archive:
        for name in ('module.so', 'manifest.json', 'signature'): archive.add(directory / name, arcname=name)
