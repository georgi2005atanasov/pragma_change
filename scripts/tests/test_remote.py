import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'remote'))
from nginx_config import parse, prepare, locations, fingerprint
from helper import Host, atomic
from transport import call


class ConfigTests(unittest.TestCase):
    def test_lossless_patch_preserves_routes_tls_and_quoted_braces(self):
        source = '# keep me\nevents {}\nhttp { server { listen 443 ssl; ssl_certificate /cert; location /api { proxy_pass http://backend; set $v "a{b}"; } } }\n'
        files = {'/etc/nginx/nginx.conf': source}
        choices = locations(files)
        result, diff = prepare(files, next(iter(files)), [choices[0]['id']], '/m/module.so', '/etc/nginx/pragmachange', '/run/pc.sock')
        updated = result[next(iter(files))]
        self.assertIn('proxy_pass http://backend;', updated)
        self.assertIn('ssl_certificate /cert;', updated)
        self.assertIn('set $v "a{b}";', updated)
        self.assertEqual(updated.count('pragmachange on;'), 1)
        parse(updated)
        self.assertIn('+', diff)
        self.assertEqual(files[next(iter(files))], source)

    def test_include_location_and_variable_braces(self):
        files = {'/main': 'events {} http { include /sites/*; }', '/sites/a': 'server { location / { proxy_pass http://${upstream}; } }'}
        result, _ = prepare(files, '/main', [locations(files)[0]['id']], '/module', '/root', '/socket')
        self.assertIn('pragmachange on;', result['/sites/a'])

    def test_unsafe_or_ambiguous_configs_refused(self):
        for source in ['http { server { location / { location /nested {} } } }', 'stream { server { location / {} } }', 'http { pragmachange_zone 64m; location / {} }']:
            files = {'/main': source}
            with self.assertRaises(ValueError): prepare(files, '/main', [locations(files)[-1]['id']], '/module', '/root', '/socket')

    def test_invalid_syntax(self):
        for source in ['http {', 'a b', 'a "unterminated', 'a ${noend;', '}']:
            with self.assertRaises(ValueError): parse(source)

    def test_fingerprint_covers_every_file(self):
        self.assertNotEqual(fingerprint({'a':'x','b':'y'}), fingerprint({'a':'x','b':'z'}))
        self.assertEqual(fingerprint({'a':'x','b':'y'}), fingerprint({'b':'y','a':'x'}))


class JournalTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(); self.addCleanup(self.directory.cleanup)
        self.host = Host({'root': self.directory.name, 'config': '/main', 'nginx': '/nginx', 'prefix':'/'})

    def test_disk_exhaustion_does_not_replace_existing_configuration(self):
        path = self.host.root / 'config'
        path.write_text('original')
        with patch('helper.os.replace', side_effect=OSError(28, 'No space left on device')):
            with self.assertRaises(OSError): atomic(path, b'candidate')
        self.assertEqual(path.read_text(), 'original')
        self.assertEqual(list(self.host.root.glob('.pragma-*')), [])

    def test_completed_operation_not_repeated(self):
        request = {'operation':'inspect','operation_id':str(uuid4())}
        with patch.object(self.host, 'inspect', return_value={'fingerprint':'x'}) as inspect:
            first = self.host.dispatch(request)
            self.assertEqual(first, self.host.dispatch(request)); inspect.assert_called_once()
        with self.assertRaises(ValueError): self.host.dispatch({**request,'operation':'rollback'})

    def test_interrupted_activation_blocks_new_jobs(self):
        op = self.host.root / 'operations' / (str(uuid4()) + '.json')
        op.write_text(json.dumps({'phase':'activating','backup':{}}))
        with self.assertRaises(ValueError): self.host.dispatch({'operation':'inspect','operation_id':str(uuid4())})

    def test_missing_journal_does_not_repeat_mutation(self):
        result = self.host.dispatch({'operation':'reconcile','operation_id':str(uuid4()),'target_id':str(uuid4())})
        self.assertEqual(result['status'],'unknown')

    def test_restore_after_interruption(self):
        target = str(uuid4()); snapshot = {'files':{},'current':None,'observed':None}
        (self.host.root / 'operations' / (target + '.json')).write_text(json.dumps({'phase':'activating','backup':snapshot}))
        with patch.object(self.host,'restore') as restore, patch('helper.run'):
            result = self.host.dispatch({'operation':'reconcile','operation_id':str(uuid4()),'target_id':target})
            restore.assert_called_once_with(snapshot)
        self.assertEqual(result['status'],'failed')

    def test_reconcile_never_reports_historical_identity_as_current(self):
        target = str(uuid4())
        result = {'status':'succeeded','observed':{'release_id':str(uuid4())},'fingerprint':'old'}
        path = self.host.root / 'operations' / (target + '.json')
        path.write_text(json.dumps({'result':result}))
        with patch.object(self.host,'verify',side_effect=ValueError('different running release')):
            reconciled = self.host.dispatch({'operation':'reconcile','operation_id':str(uuid4()),'target_id':target})
        self.assertEqual(reconciled['status'],'unknown')
        self.assertEqual(json.loads(path.read_text())['result'],result)

    def test_stale_preview_cannot_change_files(self):
        with patch.object(self.host, 'files', return_value={'/main':'http {}'}):
            with self.assertRaises(ValueError): self.host.preview({'fingerprint':'old'})

    def test_transport_rejects_option_injection(self):
        for address in ['-oProxyCommand=bad', 'host;id', 'host\nother']:
            with self.assertRaises(ValueError): call({'server':{'address':address}})

if __name__ == '__main__': unittest.main()
