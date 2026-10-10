#!/usr/bin/env python3
"""Root-owned, fixed-operation deployment helper. Never executes caller commands."""
import base64
import fcntl
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
from urllib.parse import urlsplit
from uuid import UUID

from nginx_config import fingerprint, locations, prepare

POLICY = '/etc/pragmachange-helper.json'


def atomic(path, data, mode=0o600):
    path = Path(path)
    fd, name = tempfile.mkstemp(prefix='.pragma-', dir=path.parent)
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data); stream.flush(); os.fsync(stream.fileno())
        os.replace(name, path)
        directory = os.open(path.parent, os.O_DIRECTORY)
        try: os.fsync(directory)
        finally: os.close(directory)
    finally:
        if os.path.exists(name): os.unlink(name)


def save(path, value): atomic(path, json.dumps(value, sort_keys=True).encode())


def run(command):
    result = subprocess.run(command, capture_output=True, timeout=30)
    if result.returncode: raise ValueError('host command failed; inspect the protected Nginx error log')
    if len(result.stdout) + len(result.stderr) > 4 * 1024 * 1024: raise ValueError('host output limit exceeded')
    return (result.stdout + result.stderr).decode(errors='replace')


class Host:
    def __init__(self, policy):
        self.policy = policy
        self.root = Path(policy['root']).resolve()
        self.root.mkdir(mode=0o700, parents=True, exist_ok=True)
        for folder in ('operations', 'previews', 'releases', 'modules'):
            (self.root / folder).mkdir(mode=0o700, exist_ok=True)
        self.main = str(Path(policy['config']).resolve())
        self.command = [policy['nginx'], '-p', policy['prefix'], '-c', self.main]

    def files(self):
        output = run(self.command + ['-T'])
        names = re.findall(r'^# configuration file (.+):$', output, re.M)
        allowed = [Path(x).resolve() for x in self.policy['config_roots']] + [self.root]
        files = {}
        for name in names:
            path = Path(name).resolve()
            if not any(path.is_relative_to(root) for root in allowed):
                raise ValueError('included configuration is outside administrator-approved roots')
            if path.stat().st_mode & 0o022: raise ValueError('configuration must not be group/world writable')
            files[str(path)] = path.read_text()
        if self.main not in files: raise ValueError('could not discover the main configuration')
        return files

    def status(self):
        class UnixConnection(http.client.HTTPConnection):
            def connect(connection):
                connection.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                connection.sock.settimeout(2)
                connection.sock.connect(self.policy['status_socket'])
        try:
            connection = UnixConnection('localhost', timeout=2)
            try:
                connection.request('GET', '/status', headers={'Connection': 'close'})
                response = connection.getresponse()
                if response.status != 200: return None
                status = json.loads(response.read(4096))
                # Observe the immutable native library mapped by the worker that answered.
                pid = status.get('worker_pid')
                if not isinstance(pid, int) or pid <= 0: return None
                maps = Path(f'/proc/{pid}/maps').read_text()
                pattern = re.escape(str(self.root / 'modules')) + r'/([a-f0-9]{64})/module\.so(?:\s|$)'
                loaded = set(re.findall(pattern, maps))
                if len(loaded) == 1: status['module_id'] = loaded.pop()
                return status
            finally: connection.close()
        except (OSError, ValueError, http.client.HTTPException): return None

    def inspect(self):
        files = self.files()
        build = run([self.policy['nginx'], '-V'])
        return {'fingerprint': fingerprint(files), 'locations': locations(files),
                'nginx_build': build, 'nginx_build_hash': hashlib.sha256(build.encode()).hexdigest(),
                'architecture': os.uname().machine, 'nginx': self.policy['nginx'],
                'config': self.main, 'prefix': self.policy['prefix'],
                'service': self.policy.get('service', 'nginx'), 'observed': self.status(),
                'rollback_available': (self.root / 'previous.json').exists(),
                'modules': [json.loads(p.read_text()) for p in sorted((self.root / 'modules').glob('*/manifest.json'))]}

    def stage_module(self, request):
        package = base64.b64decode(request['package'], validate=True)
        if len(package) > 32 * 1024 * 1024: raise ValueError('module package too large')
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            directory = Path(directory)
            archive = directory / 'module.tar.gz'; archive.write_bytes(package)
            with tarfile.open(archive, 'r:gz') as tar:
                members = tar.getmembers()
                if len(members) != 3 or {m.name for m in members} != {'module.so', 'manifest.json', 'signature'}:
                    raise ValueError('invalid module package')
                for member in members:
                    if not member.isfile() or member.size > 32 * 1024 * 1024: raise ValueError('invalid module member')
                    (directory / member.name).write_bytes(tar.extractfile(member).read())
            run(['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', self.policy['module_public_key'],
                 '-rawin', '-in', str(directory / 'manifest.json'), '-sigfile', str(directory / 'signature')])
            manifest = json.loads((directory / 'manifest.json').read_text())
            digest = hashlib.sha256((directory / 'module.so').read_bytes()).hexdigest()
            build = run([self.policy['nginx'], '-V'])
            if (manifest.get('sha256') != digest or manifest.get('abi') != 1 or
                manifest.get('architecture') != os.uname().machine or
                manifest.get('nginx_build_hash') != hashlib.sha256(build.encode()).hexdigest()):
                raise ValueError('module does not match the discovered target build')
            target = self.root / 'modules' / digest
            target.mkdir(exist_ok=True)
            for name in ('module.so', 'manifest.json', 'signature'):
                data = (directory / name).read_bytes()
                if (target / name).exists() and (target / name).read_bytes() != data: raise ValueError('immutable module conflict')
                atomic(target / name, data)
            return {'module_id': digest, 'activation': 'staged_only',
                    'notice': 'Prepare a module upgrade preview and acknowledge its restart requirement to activate this native build.'}

    def preview(self, request):
        files = self.files()
        if request['fingerprint'] != fingerprint(files): raise ValueError('configuration changed; inspect again')
        module_id = request['module_id']
        if not re.fullmatch('[a-f0-9]{64}', module_id): raise ValueError('invalid module identity')
        module = self.root / 'modules' / module_id / 'module.so'
        if not module.is_file(): raise ValueError('stage a verified module first')
        candidate, diff = prepare(files, self.main, request['locations'], str(module), str(self.root), self.policy['status_socket'])
        preview = {'fingerprint': fingerprint(files), 'candidate': {p: v for p, v in candidate.items() if files.get(p) != v},
                   'module_id': module_id, 'diff': diff}
        save(self.root / 'previews' / (request['operation_id'] + '.json'), preview)
        return {'preview_id': request['operation_id'], 'fingerprint': preview['fingerprint'], 'diff': diff,
                'notice': 'Candidate validation occurs on the host during activation; nothing has been changed yet.'}

    def preview_module(self, request):
        files = self.files()
        if request['fingerprint'] != fingerprint(files): raise ValueError('configuration drift; inspect again')
        module_id = request['module_id']
        if not re.fullmatch('[a-f0-9]{64}', module_id): raise ValueError('invalid module identity')
        module = self.root / 'modules' / module_id / 'module.so'
        name = str(self.root / 'managed-module.conf')
        if not module.is_file() or name not in files or self.status() is None:
            raise ValueError('module upgrade requires a staged signed module and an existing managed integration')
        if not self.policy.get('restart_command'): raise ValueError('administrator must configure a fixed restart_command in the host policy')
        candidate = {name: f'load_module {module};\n'}
        import difflib
        diff = ''.join(difflib.unified_diff(files[name].splitlines(True), candidate[name].splitlines(True), fromfile=name,tofile=name,n=0))
        preview = {'fingerprint':fingerprint(files),'candidate':candidate,'module_id':module_id,'diff':diff,'requires_restart':True}
        save(self.root / 'previews' / (request['operation_id']+'.json'),preview)
        return {'preview_id':request['operation_id'],'fingerprint':preview['fingerprint'],'diff':diff,'requires_restart':True,
                'notice':'This native code upgrade restarts Nginx and resets its in-memory history. Schedule a maintenance window or drain this server first.'}

    def transition(self, restart=False):
        if restart: run(self.policy['restart_command'])
        else: run(self.command + ['-s', 'reload'])

    def backup(self, paths):
        result = {'files': {}, 'current': os.readlink(self.root / 'current') if (self.root / 'current').is_symlink() else None,
                  'observed': self.status()}
        for name in paths:
            path = Path(name)
            result['files'][name] = {'content': path.read_text(), 'mode': path.stat().st_mode & 0o777} if path.exists() else None
        return result

    def restore(self, snapshot):
        for name, previous in snapshot['files'].items():
            if previous is None: Path(name).unlink(missing_ok=True)
            else: atomic(name, previous['content'].encode(), previous['mode'])
        self.set_current(snapshot['current'])

    def set_current(self, target):
        current = self.root / 'current'
        if target is None: current.unlink(missing_ok=True); return
        temporary = self.root / 'current.new'
        temporary.unlink(missing_ok=True); temporary.symlink_to(target); temporary.replace(current)
        directory = os.open(self.root, os.O_DIRECTORY)
        try: os.fsync(directory)
        finally: os.close(directory)

    def verify(self, release, module_id=None):
        for _ in range(20):
            status = self.status()
            if status and status.get('release_id') == release and (module_id is None or status.get('module_id') == module_id):
                for probe in self.policy.get('health_probes', []):
                    url = urlsplit(probe['url'])
                    connection = (http.client.HTTPSConnection if url.scheme == 'https' else http.client.HTTPConnection)(url.hostname, url.port, timeout=3)
                    try:
                        connection.request('GET', url.path or '/', headers={'Host': probe.get('host', url.netloc)})
                        response = connection.getresponse()
                        if response.status != probe.get('status', 200): raise ValueError('representative traffic probe failed')
                    finally: connection.close()
                return status
            time.sleep(.25)
        raise ValueError('running release could not be verified')

    def activate(self, request, journal):
        files = self.files()
        if request['fingerprint'] != fingerprint(files): raise ValueError('configuration drift; prepare again')
        candidate = {}
        restart = False
        expected_module = (self.status() or {}).get('module_id')
        if request.get('preview_id'):
            preview_id = str(UUID(request['preview_id']))
            preview = json.loads((self.root / 'previews' / (preview_id + '.json')).read_text())
            if preview['fingerprint'] != request['fingerprint']: raise ValueError('stale preview')
            candidate = preview['candidate']
            expected_module = preview['module_id']
            restart = preview.get('requires_restart', False)
            if restart and request.get('allow_restart') is not True: raise ValueError('explicit restart acknowledgement required for this module upgrade')
        if not candidate and self.status() is None: raise ValueError('integrate the module before deploying a release')
        archive_bytes = base64.b64decode(request['archive'], validate=True)
        if len(archive_bytes) > 20 * 1024 * 1024: raise ValueError('release too large')
        deploy_source = Path(__file__).parent / 'deploy.py'
        if not deploy_source.is_file(): deploy_source = Path(__file__).parent.parent / 'deploy.py'
        spec = importlib.util.spec_from_file_location('local_deploy', str(deploy_source))
        deploy = importlib.util.module_from_spec(spec); spec.loader.exec_module(deploy)
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            directory = Path(directory); archive = directory / 'archive.tar.gz'; archive.write_bytes(archive_bytes)
            unpacked = directory / 'release'; unpacked.mkdir()
            release = deploy.unpack(archive, unpacked)
            run([self.policy['validator'], 'validate-release', str(unpacked / 'release.json')])
            target = self.root / 'releases' / release
            if target.exists():
                if (target / 'release.json').read_bytes() != (unpacked / 'release.json').read_bytes(): raise ValueError('immutable release conflict')
            else: unpacked.rename(target)
        secret = self.root / 'partition_secret'
        if not secret.exists(): atomic(secret, secrets.token_hex(32).encode())
        snapshot = self.backup(candidate)
        snapshot["restart_required"] = restart
        journal.update(phase='activating', backup=snapshot, desired_release=release)
        save(self.root / 'operations' / (request['operation_id'] + '.json'), journal)
        try:
            for path, text in candidate.items():
                mode = snapshot['files'][path]['mode'] if snapshot['files'][path] else 0o600
                atomic(path, text.encode(), mode)
            self.set_current('releases/' + release)
            run(self.command + ['-t'])
            self.transition(restart)
            observed = self.verify(release, expected_module)
        except Exception:
            self.restore(snapshot)
            run(self.command + ['-t']); self.transition(restart)
            previous = snapshot.get('observed')
            if previous: self.verify(previous['release_id'], previous.get('module_id'))
            journal['recovery_complete'] = True
            raise ValueError('activation failed; previous configuration restored and reload requested')
        save(self.root / 'previous.json', snapshot)
        return {'observed': observed, 'fingerprint': fingerprint(self.files()), 'rollback_available': True}

    def rollback(self, request, journal):
        if fingerprint(self.files()) != request['fingerprint']: raise ValueError('configuration drift; inspect before rollback')
        previous = json.loads((self.root / 'previous.json').read_text())
        restart = previous.get('restart_required',False)
        if restart and request.get('allow_restart') is not True: raise ValueError('this rollback requires explicit restart acknowledgement')
        current = self.backup(previous['files'])
        current['restart_required'] = restart
        journal.update(phase='activating', backup=current)
        save(self.root / 'operations' / (request['operation_id'] + '.json'), journal)
        try:
            self.restore(previous); run(self.command + ['-t']); self.transition(restart)
            if previous['observed']: self.verify(previous['observed']['release_id'], previous['observed'].get('module_id'))
        except Exception:
            self.restore(current); run(self.command + ['-t']); self.transition(restart)
            if current['observed']: self.verify(current['observed']['release_id'], current['observed'].get('module_id'))
            journal['recovery_complete'] = True
            raise ValueError('rollback failed; restored the starting configuration')
        save(self.root / 'previous.json', current)
        return {'observed': self.status(), 'fingerprint': fingerprint(self.files()), 'rollback_available': True}

    def dispatch(self, request):
        op = request.get('operation')
        identifier = str(UUID(request['operation_id']))
        path = self.root / 'operations' / (identifier + '.json')
        with (self.root / '.deployment.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            if op == 'reconcile':
                target = self.root / 'operations' / (str(UUID(request['target_id'])) + '.json')
                if not target.exists(): return {'status': 'unknown', 'error': 'No remote journal; inspect before creating another deployment'}
                old = json.loads(target.read_text())
                if 'result' in old:
                    result = old['result']
                    observed = result.get('observed')
                    if result.get('status') == 'succeeded' and observed:
                        try:
                            self.verify(observed['release_id'], observed.get('module_id'))
                            if result.get('fingerprint') != fingerprint(self.files()):
                                raise ValueError('configuration changed')
                        except Exception:
                            return {'status':'unknown','error':'Historical operation completed but the current installation no longer matches; inspect before proceeding'}
                    return result
                # Never replay an interrupted mutation. Restore its durable pre-change snapshot.
                if old.get('phase') == 'activating' and old.get('backup'):
                    self.restore(old['backup']); run(self.command + ['-t']); self.transition(old['backup'].get('restart_required',False))
                    if old['backup'].get('observed'): self.verify(old['backup']['observed']['release_id'], old['backup']['observed'].get('module_id'))
                    old['result'] = {'status': 'failed', 'error': 'Interrupted operation recovered by restoring its previous configuration'}
                    save(target, old); return old['result']
                return {'status': 'unknown', 'error': 'Operation interrupted before activation; inspect before retrying'}
            digest = fingerprint(request)
            if path.exists():
                old = json.loads(path.read_text())
                if old['request_hash'] != digest: raise ValueError('operation identity reused with different input')
                return old.get('result', {'status': 'unknown', 'error': 'Interrupted operation requires reconciliation'})
            # No subsequent mutation may overwrite an unresolved activation backup.
            for pending in (self.root / 'operations').glob('*.json'):
                old = json.loads(pending.read_text())
                if old.get('phase') == 'activating' and 'result' not in old:
                    raise ValueError('reconcile the interrupted activation first')
            journal = {'request_hash': digest, 'phase': 'started'}; save(path, journal)
            try:
                if op == 'inspect': result = self.inspect()
                elif op == 'preview': result = self.preview(request)
                elif op == 'module_preview': result = self.preview_module(request)
                elif op == 'stage_module': result = self.stage_module(request)
                elif op == 'deploy': result = self.activate(request, journal)
                elif op == 'rollback': result = self.rollback(request, journal)
                else: raise ValueError('unsupported helper operation')
                result = {'status': 'succeeded', **result}
            except Exception as error:
                # Unexpected OS errors can contain private paths; expose only controlled ValueErrors.
                result = {'status': 'failed', 'error': str(error) if isinstance(error, ValueError) else 'Host operation failed; inspect protected host logs'}
                # If recovery failed, retain the journal without a terminal result for reconciliation.
                if journal.get('phase') == 'activating' and not journal.get('recovery_complete'):
                    return {'status': 'unknown', 'error': 'Recovery needs reconciliation'}
            journal['result'] = result; save(path, journal)
            return result


def main():
    os.umask(0o077)
    policy_path = Path(POLICY)
    if policy_path.stat().st_uid != 0 or policy_path.stat().st_mode & 0o022:
        raise ValueError('policy must be root-owned and not group/world writable')
    raw = sys.stdin.buffer.read(40 * 1024 * 1024 + 1)
    if len(raw) > 40 * 1024 * 1024: raise ValueError('request too large')
    print(json.dumps(Host(json.loads(policy_path.read_text())).dispatch(json.loads(raw))))


if __name__ == '__main__':
    try: main()
    except Exception: print(json.dumps({'status': 'failed', 'error': 'Invalid helper request or host policy'}))
