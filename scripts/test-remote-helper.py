#!/usr/bin/env python3
"""Real Nginx/helper lifecycle on isolated local paths; no SSH or VM claims."""
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import time
from uuid import uuid4
from unittest.mock import patch
from urllib.request import Request, urlopen
from http.client import HTTPConnection
import socket
sys.path.insert(0,str(Path(__file__).parent/'remote'))
from helper import Host
ROOT=Path(__file__).resolve().parents[1]

def port():
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));return sock.getsockname()[1]

def bundle(release):
 files={'release.json':json.dumps(release).encode(),'compatibility.json':json.dumps({'module_abi':1,'release_id':release['release_id']}).encode(),'nginx.conf.example':b'# retained configuration','README.txt':b'test release'}
 files['SHA256SUMS']=''.join(hashlib.sha256(v).hexdigest()+'  '+k+'\n' for k,v in files.items()).encode()
 output=io.BytesIO()
 with tarfile.open(fileobj=output,mode='w:gz') as archive:
  for name,value in files.items():info=tarfile.TarInfo(name);info.size=len(value);archive.addfile(info,io.BytesIO(value))
 return base64.b64encode(output.getvalue()).decode()

with tempfile.TemporaryDirectory(prefix='pc-native-') as temporary:
 work=Path(temporary);nginx=ROOT/'build/nginx/sbin/nginx';root=work/'managed';conf=work/'nginx.conf';proxy,upstream=port(),port()
 backend=subprocess.Popen([sys.executable,str(ROOT/'examples/upstream.py')],env={**os.environ,'PORT':str(upstream)},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 conf.write_text(f'worker_processes 2; pid {work}/nginx.pid; error_log {work}/error.log info; events {{ worker_connections 128; }} http {{ access_log off; server {{ listen 127.0.0.1:{proxy}; server_name api.example.test; location / {{ proxy_pass http://127.0.0.1:{upstream}; }} }} }}')
 conf.chmod(0o600)
 private=work/'signing-key';public=work/'public-key'
 subprocess.run(['openssl','genpkey','-algorithm','ED25519','-out',str(private)],check=True,capture_output=True)
 subprocess.run(['openssl','pkey','-in',str(private),'-pubout','-out',str(public)],check=True,capture_output=True)
 build=subprocess.run([str(nginx),'-V'],capture_output=True,check=True);(work/'build.txt').write_bytes(build.stdout+build.stderr)
 subprocess.run([sys.executable,str(ROOT/'scripts/remote/package-module.py'),'--module',str(ROOT/'build/nginx/modules/ngx_http_pragmachange_module.so'),'--nginx-build-output',str(work/'build.txt'),'--architecture',os.uname().machine,'--signing-key',str(private),'--output',str(work/'module.tar.gz')],check=True)
 host=Host({'root':str(root),'nginx':str(nginx),'config':str(conf),'prefix':str(work),'config_roots':[str(work)],'validator':str(ROOT/'target/debug/pragma-cli'),'module_public_key':str(public),'status_socket':str(root/'status.sock'),'health_probes':[{'url':f'http://127.0.0.1:{proxy}/health','status':200}]})
 def operation(name,**parameters):
  result=host.dispatch({'operation':name,'operation_id':str(uuid4()),**parameters})
  assert result['status']=='succeeded',result
  return result
 try:
  subprocess.run(host.command,check=True,capture_output=True);time.sleep(.3)
  discovered=operation('inspect');assert discovered['observed'] is None
  package=base64.b64encode((work/'module.tar.gz').read_bytes()).decode()
  staged=operation('stage_module',package=package)
  preview=operation('preview',fingerprint=discovered['fingerprint'],module_id=staged['module_id'],locations=[discovered['locations'][0]['id']])
  original=json.loads((ROOT/'build/native-test/release.json').read_text());original['release_id']=str(uuid4())
  deployed=operation('deploy',fingerprint=discovered['fingerprint'],preview_id=preview['preview_id'],archive=bundle(original))
  assert deployed['observed']['release_id']==original['release_id']
  assert deployed['observed']['module_id']==staged['module_id']
  # Disk changes do not change the identity of the running worker generation.
  release_file=root/'current/release.json';saved=release_file.read_bytes();modified=json.loads(saved);modified['release_id']=str(uuid4());release_file.write_text(json.dumps(modified))
  assert host.status()['release_id']==original['release_id'];release_file.write_bytes(saved)
  updated={**original,'release_id':str(uuid4())}
  deployed=operation('deploy',fingerprint=deployed['fingerprint'],archive=bundle(updated));assert deployed['observed']['release_id']==updated['release_id']
  rolled=operation('rollback',fingerprint=deployed['fingerprint']);assert rolled['observed']['release_id']==original['release_id']
  # A failed reload must restore and verify the old release, never escalate to restart.
  real_transition=host.transition; transitions=[]
  def fail_once(restart=False):
   transitions.append(restart)
   if len(transitions)==1: raise ValueError('injected reload failure')
   return real_transition(restart)
  with patch.object(host,'transition',side_effect=fail_once):
   failed=host.dispatch({'operation':'deploy','operation_id':str(uuid4()),'fingerprint':rolled['fingerprint'],'archive':bundle(updated)})
  assert failed['status']=='failed' and transitions==[False,False],failed
  assert host.status()['release_id']==original['release_id']
  # Failure of recovery leaves an uncertain journal; reconnect reconciliation restores the snapshot.
  interrupted=str(uuid4())
  with patch.object(host,'transition',side_effect=ValueError('injected connection loss')):
   uncertain=host.dispatch({'operation':'deploy','operation_id':interrupted,'fingerprint':rolled['fingerprint'],'archive':bundle(updated)})
  assert uncertain['status']=='unknown',uncertain
  recovered=host.dispatch({'operation':'reconcile','operation_id':str(uuid4()),'target_id':interrupted})
  assert recovered['status']=='failed' and host.status()['release_id']==original['release_id'],recovered
  # Public TCP listener must not expose runtime identity, even if configured accidentally.
  text=conf.read_text();conf.write_text(text.replace('location / {','location = /private-status { pragmachange_status; } location / {'))
  subprocess.run(host.command+['-t'],check=True,capture_output=True);subprocess.run(host.command+['-s','reload'],check=True,capture_output=True);time.sleep(.3)
  connection=HTTPConnection('127.0.0.1',proxy);connection.request('GET','/private-status');assert connection.getresponse().status==404;connection.close()
  print('PASS: signed module staging, preserved configuration, preview, initial integration, live release identity, model reload, rollback, failed activation recovery, reconnect reconciliation, and public status isolation')
 finally:
  subprocess.run(host.command+['-s','quit'],capture_output=True)
  backend.terminate();backend.wait()
  for _ in range(20):
   if not (work/'nginx.pid').exists():break
   time.sleep(.1)
