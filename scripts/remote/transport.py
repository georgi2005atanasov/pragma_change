#!/usr/bin/env python3
"""One bounded SSH invocation. Secrets arrive on stdin, never command arguments."""
import base64
import json
import os
import re
import subprocess
import sys
import tempfile


def call(data):
    server = data['server']
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9.:-]{0,252}', server['address']): raise ValueError('invalid address')
    if not re.fullmatch(r'[a-z_][a-z0-9_-]{0,31}', server['username']): raise ValueError('invalid account')
    if not 1 <= server['port'] <= 65535: raise ValueError('invalid port')
    if not re.fullmatch(r'(ssh-ed25519|ecdsa-sha2-nistp256|ssh-rsa) [A-Za-z0-9+/=]+', server['host_key']):
        raise ValueError('provide the independently verified host public key without its hostname or comment')
    with tempfile.TemporaryDirectory(prefix='pragma-ssh-') as directory:
        key, hosts = directory + '/identity', directory + '/known_hosts'
        for path, content in [(key, data['private_key']), (hosts, 'pragma-target ' + server['host_key'] + '\n')]:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'w') as stream: stream.write(content)
        command = ['ssh', '-F', '/dev/null', '-T', '-p', str(server['port']), '-i', key,
                   '-o', 'BatchMode=yes', '-o', 'IdentitiesOnly=yes', '-o', 'IdentityAgent=none',
                   '-o', 'StrictHostKeyChecking=yes', '-o', 'HostKeyAlias=pragma-target',
                   '-o', 'UserKnownHostsFile=' + hosts, '-o', 'GlobalKnownHostsFile=/dev/null',
                   '-o', 'ConnectTimeout=10', '-o', 'ServerAliveInterval=10', '-o', 'ServerAliveCountMax=2',
                   '-o', 'ClearAllForwardings=yes', '-o', 'PermitLocalCommand=no',
                   server['username'] + '@' + server['address'], 'sudo -n /usr/local/libexec/pragmachange-helper']
        # Bounded output file prevents a remote endpoint exhausting controller memory.
        with tempfile.TemporaryFile() as output:
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=output, stderr=subprocess.DEVNULL)
            try:
                process.communicate(json.dumps(data['request']).encode(), timeout=150)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait()
                return {'status': 'unknown', 'error': 'SSH timed out; reconcile the operation before retrying'}
            if process.returncode != 0:
                return {'status': 'unknown', 'error': 'SSH or remote helper failed; verify credentials, host key and helper installation, then reconcile'}
            output.seek(0)
            raw = output.read(8 * 1024 * 1024 + 1)
            if len(raw) > 8 * 1024 * 1024: raise ValueError('remote response too large')
            return json.loads(raw)


if __name__ == '__main__':
    try:
        data = json.loads(sys.stdin.buffer.read(40 * 1024 * 1024))
        print(json.dumps(call(data)))
    except Exception:
        print(json.dumps({'status': 'unknown', 'error': 'Remote transport failed; reconcile before retrying'}))
