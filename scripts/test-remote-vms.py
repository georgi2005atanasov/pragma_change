#!/usr/bin/env python3
"""Two-machine acceptance: management API/worker in one VM, Nginx in another.
Requires scripts/remote/lab.py start and the isolated lab_database example.
"""
import base64
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tarfile
import time
from uuid import uuid4
from urllib.request import Request,urlopen

ROOT=Path(__file__).resolve().parents[1];LAB=ROOT/'build/remote-lab'
SSH=['ssh','-F','/dev/null','-i',str(LAB/'client'),'-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o',f'UserKnownHostsFile={LAB}/known_hosts']
SCP=['scp','-F','/dev/null','-i',str(LAB/'client'),'-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o',f'UserKnownHostsFile={LAB}/known_hosts']
def remote(port,command,data=None):
 result=subprocess.run(SSH+['-p',str(port),'lab@127.0.0.1',command],input=data,capture_output=True,timeout=90)
 if result.returncode:raise RuntimeError(f'Lab command failed on {port}: '+result.stderr.decode(errors='replace')[:500])
 return result.stdout

def copy_to(port,source,destination):subprocess.run(SCP+['-P',str(port),str(source),f'lab@127.0.0.1:{destination}'],check=True,capture_output=True,timeout=90)
def archive_files(target,files):
 with tarfile.open(target,'w:gz') as archive:
  for source,name in files:archive.add(source,arcname=name)

# Bootstrap material is disposable and scoped to these two guests.
if not (LAB/'deploy-key').exists():subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(LAB/'deploy-key')],check=True)
if not (LAB/'signing-key').exists():subprocess.run(['openssl','genpkey','-algorithm','ED25519','-out',str(LAB/'signing-key')],check=True,capture_output=True)
subprocess.run(['openssl','pkey','-in',str(LAB/'signing-key'),'-pubout','-out',str(LAB/'signing-public')],check=True,capture_output=True)
for binary in ('pragma-api','pragma-cli'):
 subprocess.run(['strip','--strip-debug','-o',str(LAB/binary),str(ROOT/'target/debug'/binary)],check=True)
proxy_files=[(ROOT/'build/nginx/sbin/nginx','nginx'),(LAB/'pragma-cli','pragma-cli'),(ROOT/'scripts/remote','scripts/remote'),(ROOT/'scripts/deploy.py','scripts/deploy.py'),(ROOT/'examples/upstream.py','upstream.py'),(LAB/'signing-public','public.pem'),(Path(str(LAB/'deploy-key')+'.pub'),'deploy.pub')]
archive_files(LAB/'proxy-bootstrap.tar.gz',proxy_files)
copy_to(22332,LAB/'proxy-bootstrap.tar.gz','/home/lab/proxy-bootstrap.tar.gz')
remote(22332,"mkdir -p /home/lab/proxy-bootstrap && tar -xzf /home/lab/proxy-bootstrap.tar.gz -C /home/lab/proxy-bootstrap")
bootstrap=r'''set -eu
sudo install -d /opt/pragmalab /etc/nginx
sudo install -m 755 /home/lab/proxy-bootstrap/nginx /opt/pragmalab/nginx
sudo install -m 755 /home/lab/proxy-bootstrap/pragma-cli /usr/local/bin/pragma-cli
sudo install -m 644 /home/lab/proxy-bootstrap/upstream.py /opt/pragmalab/upstream.py
id pragma-deploy >/dev/null 2>&1 || sudo useradd -m -s /bin/bash pragma-deploy
sudo install -d -m 700 -o pragma-deploy -g pragma-deploy /home/pragma-deploy/.ssh
{ printf 'restrict '; cat /home/lab/proxy-bootstrap/deploy.pub; } | sudo tee /home/pragma-deploy/.ssh/authorized_keys >/dev/null
sudo chown pragma-deploy:pragma-deploy /home/pragma-deploy/.ssh/authorized_keys
sudo chmod 600 /home/pragma-deploy/.ssh/authorized_keys
sudo tee /etc/nginx/nginx.conf >/dev/null <<'CONF'
worker_processes 2;
pid /run/pragmalab.pid;
error_log /var/log/pragma-nginx.log info;
events { worker_connections 1024; }
http {
 access_log off;
 server { listen 8081; server_name api.example.test;
  location / { proxy_pass http://127.0.0.1:9000; }
 }
}
CONF
sudo chmod 644 /etc/nginx/nginx.conf
sudo tee /etc/systemd/system/pragma-upstream.service >/dev/null <<'UNIT'
[Unit]
Description=Disposable Pragma test backend
[Service]
ExecStart=/usr/bin/python3 /opt/pragmalab/upstream.py
UNIT
sudo tee /etc/systemd/system/pragma-nginx.service >/dev/null <<'UNIT'
[Unit]
Description=Disposable Pragma test Nginx
After=pragma-upstream.service
[Service]
Type=forking
PIDFile=/run/pragmalab.pid
ExecStart=/opt/pragmalab/nginx -p /etc/nginx -c /etc/nginx/nginx.conf
ExecReload=/opt/pragmalab/nginx -p /etc/nginx -c /etc/nginx/nginx.conf -s reload
ExecStop=/opt/pragmalab/nginx -p /etc/nginx -c /etc/nginx/nginx.conf -s quit
UNIT
sudo systemctl daemon-reload
sudo systemctl start pragma-upstream pragma-nginx
sudo python3 /home/lab/proxy-bootstrap/scripts/remote/bootstrap.py --account pragma-deploy --module-public-key /home/lab/proxy-bootstrap/public.pem --health-url http://127.0.0.1:8081/health --nginx /opt/pragmalab/nginx --config /etc/nginx/nginx.conf --prefix /etc/nginx --service pragma-nginx
'''
remote(22332,'bash -s',bootstrap.encode())
print('Proxy bootstrapped with a restricted deployment account',flush=True)
nginx_build=remote(22332,'/opt/pragmalab/nginx -V 2>&1');(LAB/'nginx-build.txt').write_bytes(nginx_build)
subprocess.run([sys.executable,str(ROOT/'scripts/remote/package-module.py'),'--module',str(ROOT/'build/nginx/modules/ngx_http_pragmachange_module.so'),'--nginx-build-output',str(LAB/'nginx-build.txt'),'--architecture','x86_64','--signing-key',str(LAB/'signing-key'),'--output',str(LAB/'module.tar.gz')],check=True)
archive_files(LAB/'management.tar.gz',[(LAB/'pragma-api','pragma-api'),(ROOT/'scripts/remote','remote')])
copy_to(22331,LAB/'management.tar.gz','/home/lab/management.tar.gz')
remote(22331,'mkdir -p /home/lab/management && tar -xzf /home/lab/management.tar.gz -C /home/lab/management')
config={'DATABASE_URL':json.loads((LAB/'database.json').read_text())['url'],'ADMIN_USERNAME':'lab','ADMIN_PASSWORD':secrets.token_urlsafe(24),'TRAINER_TOKEN':secrets.token_hex(32),'PARTITION_SECRET':secrets.token_hex(32),'ARTIFACT_DIR':'/home/lab/management/artifacts','BIND_ADDR':'127.0.0.1:8080','CREDENTIAL_KEY_FILE':'/home/lab/management/credential-key','REMOTE_TRANSPORT':'/home/lab/management/remote/transport.py'}
remote(22331,"python3 -c 'import pathlib,sys; p=pathlib.Path(\"/home/lab/management/config.json\");p.write_bytes(sys.stdin.buffer.read());p.chmod(0o600)'",json.dumps(config).encode())
remote(22331,"python3 -c 'import pathlib,secrets; p=pathlib.Path(\"/home/lab/management/credential-key\");p.write_text(secrets.token_hex(32));p.chmod(0o600)' ")
launcher="import json,os,subprocess,pathlib\np=pathlib.Path('/home/lab/management')\nenv={**os.environ,**json.loads((p/'config.json').read_text())}\nwith (p/'api.log').open('wb') as log:\n child=subprocess.Popen([str(p/'pragma-api')],env=env,stdout=log,stderr=log,start_new_session=True)\n(p/'api.pid').write_text(str(child.pid))\n"
remote(22331,'python3 -',launcher.encode())
for _ in range(30):
 try:
  remote(22331,"python3 -c 'import urllib.request; urllib.request.urlopen(\"http://127.0.0.1:8080/health\",timeout=2).read()'");break
 except RuntimeError:time.sleep(1)
else:raise RuntimeError('Management API failed; inspect its protected guest log')
# The acceptance driver executes inside the management VM, through the real HTTP API and SSH worker.
host_key=' '.join((LAB/'proxy-host.pub').read_text().split()[:2])
payload={'password':config['ADMIN_PASSWORD'],'private_key':(LAB/'deploy-key').read_text(),'host_key':host_key,'package':base64.b64encode((LAB/'module.tar.gz').read_bytes()).decode()}
remote(22331,"python3 -c 'import pathlib,sys; p=pathlib.Path(\"/home/lab/management/test-input.json\");p.write_bytes(sys.stdin.buffer.read());p.chmod(0o600)'",json.dumps(payload).encode())
# Use a real API-created forest and release. The lab trainer is a deterministic data-only fixture,
# isolated from production credentials. Existing training/parity tests cover the Python trainer.
driver=r'''
import json,time,urllib.request,uuid,pathlib
p=pathlib.Path('/home/lab/management');payload=json.loads((p/'test-input.json').read_text());auth={}
def api(path,body=None):
 data=json.dumps(body).encode() if body is not None else None
 req=urllib.request.Request('http://127.0.0.1:8080/api/v1/'+path,data=data,headers={'Content-Type':'application/json',**auth})
 with urllib.request.urlopen(req,timeout=30) as response:return json.load(response)
login=api('login',{'username':'lab','password':payload['password']});auth={'Authorization':'Bearer '+login['token'],'X-CSRF-Token':login['csrf_token']}
server=api('servers',{'name':'isolated proxy VM','address':'10.0.2.2','port':22332,'username':'pragma-deploy','host_key':payload['host_key'],'private_key':payload['private_key']})['id']
def job(operation,parameters=None):
 identifier=api(f'servers/{server}/jobs',{'operation':operation,'parameters':parameters or {}})['id']
 for _ in range(100):
  result=next(j for j in api('remote-jobs') if j['id']==identifier)
  if result['status'] not in ('queued','running'):
   assert result['status']=='succeeded',result
   return result['result']
  time.sleep(1)
 raise RuntimeError('remote job timed out')
inspection=job('inspect');assert inspection['observed'] is None
staged=job('stage_module',{'package':payload['package']})
preview=job('preview',{'fingerprint':inspection['fingerprint'],'module_id':staged['module_id'],'locations':[inspection['locations'][0]['id']]})
(p/'prepared.json').write_text(json.dumps({'server':server,'inspection':inspection,'preview':preview,'auth':auth}));(p/'prepared.json').chmod(0o600)
print('PASS: management VM API registered proxy, pinned SSH host identity, staged a signed module and prepared the configuration diff',flush=True)
'''
remote(22331,'python3 -',driver.encode())
# Release insertion is performed by a separate test-only SQL fixture, never a product upload bypass.
prepared=json.loads(remote(22331,'cat /home/lab/management/prepared.json'))
(LAB/'prepared.json').write_text(json.dumps(prepared));(LAB/'prepared.json').chmod(0o600)
print('VM setup and remote discovery passed. Continue with lab_database seed and test-remote-vms-finish.py.',flush=True)
