#!/usr/bin/env python3
"""Disposable two-VM lab. Requires a SHA256-verified Ubuntu noble cloud image.
No host deployment directories are mounted into guests. Ports bind only to loopback.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import threading

parser=argparse.ArgumentParser()
parser.add_argument('action',choices=['start','stop'])
parser.add_argument('--root',default='build/remote-lab')
args=parser.parse_args();root=Path(args.root).resolve();root.mkdir(mode=0o700,parents=True,exist_ok=True);root.chmod(0o700)
if args.action=='stop':
 for name in ('management','proxy'):
  pid=root/f'{name}.pid'
  if pid.exists():
   import os,signal
   try:os.kill(int(pid.read_text()),signal.SIGTERM)
   except ProcessLookupError:pass
   pid.unlink(missing_ok=True)
 sys.exit()
if not (root/'verified').exists():raise SystemExit('Provide base.img and a verified marker after checking the publisher SHA-256. See docs/remote-lab.md.')
if not (root/'client').exists():subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(root/'client')],check=True)
public=(root/'client.pub').read_text().strip()
known=[]
for name,port in [('management',22331),('proxy',22332)]:
 host=root/f'{name}-host'
 if not host.exists():subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(host)],check=True)
 host_public=Path(str(host)+'.pub').read_text().strip()
 known.append(f'[127.0.0.1]:{port} {host_public}\n')
 seed=root/name;seed.mkdir(exist_ok=True)
 (seed/'meta-data').write_text(json.dumps({'instance-id':'pragma-'+name,'local-hostname':'pragma-'+name}))
 (seed/'user-data').write_text('#cloud-config\n'+json.dumps({'users':[{'name':'lab','sudo':'ALL=(ALL) NOPASSWD:ALL','groups':['sudo'],'shell':'/bin/bash','lock_passwd':True,'ssh_authorized_keys':[public]}], 'ssh_pwauth':False,'ssh_keys':{'ed25519_private':host.read_text(),'ed25519_public':host_public}}))
 disk=root/f'{name}.qcow2'
 if not disk.exists():
  subprocess.run(['qemu-img','create','-f','qcow2','-F','qcow2','-b',str(root/'base.img'),str(disk),'5G'],check=True)
 (seed/'user-data').chmod(0o600)
(root/'known_hosts').write_text(''.join(known));(root/'known_hosts').chmod(0o600)
class Quiet(SimpleHTTPRequestHandler):
 def __init__(self,*a,**kw):super().__init__(*a,directory=str(root),**kw)
 def do_GET(self):
  if self.path not in ('/management/meta-data','/management/user-data','/proxy/meta-data','/proxy/user-data'):
   self.send_error(404);return
  super().do_GET()
 def log_message(self,*a):pass
server=ThreadingHTTPServer(('127.0.0.1',28765),Quiet)
threading.Thread(target=server.serve_forever,daemon=True).start()
try:
 for name,port in [('management',22331),('proxy',22332)]:
  if (root/f'{name}.pid').exists():raise RuntimeError('Lab already started; stop before creating another')
  subprocess.run(['qemu-system-x86_64','-enable-kvm','-m','1536','-smp','2','-cpu','host','-drive',f'file={root/name}.qcow2,format=qcow2,if=virtio','-netdev',f'user,id=net0,hostfwd=tcp:127.0.0.1:{port}-:22'+(',hostfwd=tcp:127.0.0.1:28081-:8081' if name=='proxy' else ''),'-device','virtio-net-pci,netdev=net0','-smbios',f'type=1,serial=ds=nocloud-net;s=http://10.0.2.2:28765/{name}/','-display','none','-serial',f'file:{root/name}.console','-daemonize','-pidfile',str(root/f'{name}.pid')],check=True)
 for name,port in [('management',22331),('proxy',22332)]:
  ready=False
  for _ in range(90):
   result=subprocess.run(['ssh','-F','/dev/null','-i',str(root/'client'),'-p',str(port),'-o','StrictHostKeyChecking=yes','-o',f'UserKnownHostsFile={root}/known_hosts','-o','BatchMode=yes','-o','ConnectTimeout=2','lab@127.0.0.1','cloud-init status --wait'],capture_output=True,timeout=15)
   if result.returncode in (0,2):ready=True;break
   time.sleep(2)
  if not ready:raise RuntimeError(f'{name} did not become ready; inspect console')
  print(name,'ready',flush=True)
finally:server.shutdown()
