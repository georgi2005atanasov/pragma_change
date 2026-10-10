#!/usr/bin/env python3
"""Run once as root on the Nginx host, from a verified checkout/package."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--account', required=True)
    parser.add_argument('--nginx', default='/usr/sbin/nginx')
    parser.add_argument('--config', default='/etc/nginx/nginx.conf')
    parser.add_argument('--service', default='nginx')
    parser.add_argument('--prefix', default='/usr/share/nginx')
    parser.add_argument('--validator', default='/usr/local/bin/pragma-cli')
    parser.add_argument('--module-public-key', required=True)
    parser.add_argument('--health-url', required=True, help='Representative read-only HTTP(S) health endpoint on this host')
    args = parser.parse_args()
    if os.geteuid() != 0: parser.error('run bootstrap as root')
    if not re.fullmatch('[a-z_][a-z0-9_-]{0,31}', args.account): parser.error('invalid account')
    for value in (args.nginx, args.config, args.prefix, args.validator, args.module_public_key):
        if not re.fullmatch('/[A-Za-z0-9_./-]+', value): parser.error('paths must be absolute without whitespace or shell metacharacters')
    if not re.fullmatch('[a-zA-Z0-9@_.-]+', args.service): parser.error('invalid service name')
    if not args.health_url.startswith(('http://', 'https://')): parser.error('HTTP(S) health URL required')
    destination = Path('/usr/local/lib/pragmachange'); destination.mkdir(parents=True, exist_ok=True)
    here = Path(__file__).resolve().parent
    for name in ('helper.py', 'nginx_config.py'):
        shutil.copyfile(here / name, destination / name); os.chmod(destination / name, 0o644)
    shutil.copyfile(here.parent / 'deploy.py', destination / 'deploy.py')
    root = Path('/etc/nginx/pragmachange'); root.mkdir(mode=0o700, exist_ok=True)
    shutil.copyfile(args.module_public_key, root / 'module-public.pem')
    policy = {'root': str(root), 'nginx': args.nginx, 'config': args.config, 'prefix': args.prefix,
              'validator': args.validator, 'config_roots': [str(Path(args.config).parent)],
              'module_public_key': str(root / 'module-public.pem'), 'service': args.service,
              'restart_command': ['/usr/bin/systemctl','restart',args.service],
              'status_socket': str(root / 'status.sock'), 'health_probes': [{'url': args.health_url, 'status': 200}]}
    policy_path = Path('/etc/pragmachange-helper.json')
    if policy_path.exists(): parser.error('policy already exists; review and update it manually')
    policy_path.write_text(json.dumps(policy, indent=2)); policy_path.chmod(0o600)
    wrapper = Path('/usr/local/libexec/pragmachange-helper'); wrapper.parent.mkdir(parents=True, exist_ok=True)
    wrapper.write_text('#!/bin/sh\nexec /usr/bin/python3 -I /usr/local/lib/pragmachange/helper_entry.py\n'); wrapper.chmod(0o755)
    # Isolated Python ignores caller-controlled PYTHONPATH; import only the root-owned installation.
    (destination / 'helper_entry.py').write_text("import sys\nsys.path.insert(0, '/usr/local/lib/pragmachange')\nfrom helper import main\nmain()\n")
    sudoers = Path('/etc/sudoers.d/pragmachange')
    sudoers.write_text(f'{args.account} ALL=(root) NOPASSWD: /usr/local/libexec/pragmachange-helper ""\n'); sudoers.chmod(0o440)
    subprocess.run(['visudo', '-cf', str(sudoers)], check=True)
    print('Helper installed. Provision a dedicated SSH key for the account and pin the independently verified host public key in the portal.')


if __name__ == '__main__': main()
