#!/usr/bin/env python3
"""Finish the isolated VM acceptance flow after lab_database seed."""
import json
from pathlib import Path
import subprocess
import time
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[1]
LAB = ROOT / 'build/remote-lab'
SSH = ['ssh', '-F', '/dev/null', '-i', str(LAB / 'client'), '-o', 'BatchMode=yes',
       '-o', 'StrictHostKeyChecking=yes', '-o', f'UserKnownHostsFile={LAB}/known_hosts']

def remote(port, code):
    result = subprocess.run(SSH + ['-p', str(port), 'lab@127.0.0.1', 'python3 -'],
                            input=code.encode(), capture_output=True, timeout=180)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors='replace')[:2000])
    print(result.stdout.decode(), end='', flush=True)

ids = json.loads((LAB / 'release-ids.json').read_text())
DRIVER = r'''
import json,time,urllib.request,pathlib
p=pathlib.Path('/home/lab/management'); prepared=json.loads((p/'prepared.json').read_text())
auth=prepared['auth']; server=prepared['server']
def api(path,body=None):
 data=json.dumps(body).encode() if body is not None else None
 req=urllib.request.Request('http://127.0.0.1:8080/api/v1/'+path,data=data,headers={'Content-Type':'application/json',**auth})
 with urllib.request.urlopen(req,timeout=30) as response:return json.load(response)
def job(operation,parameters=None,expected='succeeded',target=None):
 identifier=api(f'servers/{target or server}/jobs',{'operation':operation,'parameters':parameters or {}})['id']
 for _ in range(120):
  result=next(j for j in api('remote-jobs') if j['id']==identifier)
  if result['status'] not in ('queued','running'):
   assert result['status']==expected, result
   return result['result']
  time.sleep(.5)
 raise RuntimeError('remote job timed out')
'''
remote(22331, DRIVER + f'''
ids={ids!r}
first=job('deploy',{{'fingerprint':prepared['inspection']['fingerprint'],'preview_id':prepared['preview']['preview_id'],'release_id':ids[0]}})
assert first['observed']['release_id']==ids[0]
second=job('deploy',{{'fingerprint':first['fingerprint'],'release_id':ids[1]}})
assert second['observed']['release_id']==ids[1]
rolled=job('rollback',{{'fingerprint':second['fingerprint']}})
assert rolled['observed']['release_id']==ids[0]
(p/'verified.json').write_text(json.dumps(rolled))
print('PASS: release activation, graceful update, observed worker identity, and verified rollback across SSH')
''')
# Add a harmless edit through an independent administrator, then attempt a stale deployment.
remote(22332, "import subprocess\nsubprocess.run(['sudo','sh','-c',\"printf '\\n# independent administrator edit\\n' >> /etc/nginx/nginx.conf\"],check=True)\n")
remote(22331, DRIVER + f'''
verified=json.loads((p/'verified.json').read_text())
failed=job('deploy',{{'fingerprint':verified['fingerprint'],'release_id':{ids[1]!r}}},expected='failed')
assert 'drift' in failed['error']
inspection=job('inspect'); assert inspection['observed']['release_id']=={ids[0]!r}
payload=json.loads((p/'test-input.json').read_text())
wrong_key=' '.join({(LAB / 'management-host.pub').read_text().split()[:2]!r})
wrong=api('servers',{{'name':'wrong pinned identity','address':'10.0.2.2','port':22332,'username':'pragma-deploy','host_key':wrong_key,'private_key':payload['private_key']}})['id']
job('inspect',expected='unknown',target=wrong)
print('PASS: configuration drift refused without changing the loaded release; wrong SSH host identity refused')
''')
# Stop the entire management machine, leaving the Nginx guest running.
subprocess.run(SSH + ['-p', '22331', 'lab@127.0.0.1', 'sudo poweroff'], capture_output=True, timeout=15)
time.sleep(3)
body=json.dumps({'amount':42}).encode()
request=Request('http://127.0.0.1:28081/block',data=body,
                headers={'Host':'api.example.test','Content-Type':'application/json','X-Client-ID':'vm-acceptance'})
try:
    response=urlopen(request,timeout=5); status=response.status
except HTTPError as error:
    status=error.code
assert status==403, f'Expected enforcement with management offline; got {status}'
print('PASS: Nginx continues enforcing the activated release with the management VM powered off')
(LAB/'acceptance-result.json').write_text(json.dumps({'passed':True,'checks':['activation','reload','observed identity','rollback','configuration drift','host key mismatch','management VM offline enforcement']},indent=2))
