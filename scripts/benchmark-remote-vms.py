#!/usr/bin/env python3
"""Small repeatable topology benchmark, not a production capacity estimate.
Run after VM acceptance (management VM powered off, proxy still running).
"""
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import statistics
import subprocess
import threading
import time
from urllib.request import Request,urlopen
ROOT=Path(__file__).resolve().parents[1];LAB=ROOT/'build/remote-lab'
SSH=['ssh','-F','/dev/null','-i',str(LAB/'client'),'-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o',f'UserKnownHostsFile={LAB}/known_hosts']
def remote(port,code):
 result=subprocess.run(SSH+['-p',str(port),'lab@127.0.0.1','python3 -'],input=code.encode(),capture_output=True,timeout=20)
 if result.returncode:raise RuntimeError(result.stderr.decode()[:1000])
 return result.stdout
# Boot the existing management disk again; no shared filesystems or deployment mounts.
subprocess.run(['qemu-system-x86_64','-enable-kvm','-m','1536','-smp','2','-cpu','host','-drive',f'file={LAB}/management.qcow2,format=qcow2,if=virtio','-netdev','user,id=net0,hostfwd=tcp:127.0.0.1:22331-:22','-device','virtio-net-pci,netdev=net0','-display','none','-serial',f'file:{LAB}/management.console','-daemonize','-pidfile',str(LAB/'management.pid')],check=True)
for _ in range(45):
 try:remote(22331,'print("ready")');break
 except RuntimeError:time.sleep(1)
else:raise RuntimeError('management VM unavailable')
subprocess.run(['strip','--strip-debug','-o',str(LAB/'train-load'),str(ROOT/'target/debug/examples/train_load')],check=True)
subprocess.run(['scp','-F','/dev/null','-i',str(LAB/'client'),'-o','StrictHostKeyChecking=yes','-o',f'UserKnownHostsFile={LAB}/known_hosts','-P','22331',str(LAB/'train-load'),'lab@127.0.0.1:/home/lab/train-load'],check=True,capture_output=True)
stats_code=r'''
import pathlib,json,os
clock=os.sysconf('SC_CLK_TCK');cpu=0;rss=0
for p in pathlib.Path('/proc').glob('[0-9]*/stat'):
 try:
  s=p.read_text().split()
  if s[1]!='(nginx)':continue
  cpu+=(int(s[13])+int(s[14]))/clock;rss+=int(s[23])*os.sysconf('SC_PAGE_SIZE')
 except (OSError,ValueError):pass
print(json.dumps({'cpu_seconds':cpu,'rss_bytes':rss}))
'''
def set_mode(enabled):
 code="from pathlib import Path;import subprocess;p=Path('/etc/nginx/nginx.conf');s=p.read_text();p.write_text(s.replace('pragmachange off;', 'pragmachange on;'))" if enabled else "from pathlib import Path;import subprocess;p=Path('/etc/nginx/nginx.conf');s=p.read_text();p.write_text(s.replace('pragmachange on;', 'pragmachange off;'))"
 code+=";subprocess.run(['/opt/pragmalab/nginx','-p','/etc/nginx','-c','/etc/nginx/nginx.conf','-t'],check=True);subprocess.run(['systemctl','reload','pragma-nginx'],check=True)"
 remote(22332,'import subprocess\nsubprocess.run(["sudo","python3","-c",'+repr(code)+'],check=True)')
 time.sleep(1)
def bench(name):
 before=json.loads(remote(22332,stats_code));latencies=[];errors=[];end=time.monotonic()+5
 def worker():
  while time.monotonic()<end:
   start=time.monotonic()
   try:
    req=Request('http://127.0.0.1:28081/allow',data=b'{"amount":42}',headers={'Host':'api.example.test','X-Client-ID':'benchmark','Content-Type':'application/json'})
    with urlopen(req,timeout=3) as r:
     assert r.status==200;r.read()
    latencies.append((time.monotonic()-start)*1000)
   except Exception:errors.append(1)
 start=time.monotonic()
 with ThreadPoolExecutor(max_workers=4) as pool:list(pool.map(lambda _:worker(),range(4)))
 elapsed=time.monotonic()-start;after=json.loads(remote(22332,stats_code));ordered=sorted(latencies)
 return {'case':name,'requests':len(latencies),'errors':len(errors),'seconds':round(elapsed,2),'requests_per_second':round(len(latencies)/elapsed,1),'p50_ms':round(statistics.median(ordered),2),'p95_ms':round(ordered[int(len(ordered)*.95)],2),'p99_ms':round(ordered[int(len(ordered)*.99)],2),'nginx_cpu_seconds':round(after['cpu_seconds']-before['cpu_seconds'],3),'nginx_rss_mib':round(after['rss_bytes']/1024/1024,2)}
results=[]
try:
 set_mode(False);results.append(bench('inspection disabled'))
 set_mode(True);results.append(bench('inspection enabled'))
 remote(22331,"import subprocess\nf=open('/home/lab/training-benchmark.log','wb')\nsubprocess.Popen(['/home/lab/train-load'],stdout=f,stderr=f,start_new_session=True)\nsubprocess.Popen(['/home/lab/train-load'],stdout=f,stderr=f,start_new_session=True)")
 results.append(bench('inspection plus two management-side training workers'))
finally:set_mode(True)
(LAB/'benchmark.json').write_text(json.dumps(results,indent=2));print(json.dumps(results,indent=2))
