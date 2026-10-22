#!/usr/bin/env python3
"""Load an API-exported trained model in Nginx; exercise real install and rollback."""
import importlib.util
import json
import os
import re
from pathlib import Path
import socket
import subprocess
import sys
import tarfile
import time
from urllib.request import Request, urlopen
from urllib.parse import urlencode
from urllib.error import HTTPError

ROOT=Path(__file__).resolve().parent.parent
WORK=ROOT/'build/release-test'
NGINX=ROOT/'build/nginx/sbin/nginx'
CLI=ROOT/'target/debug/pragma-cli'

def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1',0));return s.getsockname()[1]

def main():
    archive=ROOT/'build/e2e/release.tar.gz'
    browser=ROOT/'build/e2e/browser-release.tar.gz'
    if not archive.exists() or not browser.exists():raise SystemExit('Run API and browser workflow tests first')
    (WORK/'logs').mkdir(parents=True,exist_ok=True)
    deployment=WORK/'deployment';deployment.mkdir(exist_ok=True)
    (deployment/'partition_secret').write_text('cd'*32)
    initial=deployment/'releases/bootstrap';initial.mkdir(parents=True,exist_ok=True)
    with tarfile.open(archive) as tar:
        (initial/'release.json').write_bytes(tar.extractfile('release.json').read())
    current=deployment/'current';current.unlink(missing_ok=True);current.symlink_to('releases/bootstrap')
    (deployment/'previous').unlink(missing_ok=True)
    releases=json.loads((initial/'release.json').read_text())
    p=releases['deployments'][0]['pipeline']
    proxy_port,upstream_port=free_port(),free_port()
    config=WORK/'nginx.conf'
    config.write_text(f'''load_module {ROOT}/build/nginx/modules/ngx_http_pragmachange_module.so;
worker_processes 2;
pid {WORK}/nginx.pid;
error_log {WORK}/error.log;
events {{ worker_connections 128; }}
http {{ access_log off; pragmachange_zone 8m;
 pragmachange_release {current}/release.json;
 pragmachange_secret_file {deployment}/partition_secret;
 server {{ listen 127.0.0.1:{proxy_port}; server_name _;
 location / {{ pragmachange on; proxy_pass http://127.0.0.1:{upstream_port}; }} }} }}
''')
    log=(WORK/'upstream.log').open('w')
    upstream=subprocess.Popen([sys.executable,str(ROOT/'examples/upstream.py')],env={**os.environ,'HOST':'127.0.0.1','PORT':str(upstream_port)},stdout=log,stderr=log)
    nginx=subprocess.Popen([str(NGINX),'-p',str(WORK),'-c',str(config),'-g','daemon off;'])
    command=[sys.executable,str(ROOT/'scripts/deploy.py')]
    flags=['--root',str(deployment),'--nginx',str(NGINX),'--nginx-prefix',str(WORK),'--nginx-config',str(config),'--validator',str(CLI)]
    def inspect(pipeline):
        # Exported browser pipelines can require nested bodies, query fields,
        # and path parameters. Supply their selected fields before testing warm-up.
        path = re.sub(r"\{[^}]+\}", "42", pipeline['path_template'])
        headers = {'Host': pipeline['api_host'], 'Content-Type': 'application/json'}
        query, body = {}, {}
        for extractor in pipeline['extractors']:
            source = extractor['source']
            if source['kind'] == 'header':
                headers[source['name']] = '42'
            elif source['kind'] == 'query_parameter':
                query[source['name']] = '42'
            elif source['kind'] == 'json_pointer':
                parts = [part.replace('~1', '/').replace('~0', '~') for part in source['pointer'].split('/')[1:]]
                current = body
                for part in parts[:-1]:
                    current = current.setdefault(part, {})
                if parts:
                    current[parts[-1]] = 42
        if query:
            path += '?' + urlencode(query)
        try:
            response=urlopen(Request(f'http://127.0.0.1:{proxy_port}'+path,data=json.dumps(body).encode(),headers=headers,method=pipeline['method']),timeout=5)
        except HTTPError as e:response=e
        raw=response.read()
        return response.status,json.loads(raw) if response.status==200 else None
    try:
        time.sleep(.2)
        status,body=inspect(p);assert status==200 and body['risk']['x-pragmachange-reason']=='warmup'
        time.sleep(1.05)
        status,body=inspect(p)
        assert status in (200,403)
        if body:assert body['risk']['x-pragmachange-score']!=''
        subprocess.run(command+['install',str(archive)]+flags,check=True)
        subprocess.run(command+['install',str(browser)]+flags,check=True)
        time.sleep(.2)
        browser_release=json.loads((current/'release.json').read_text())
        browser_pipeline=browser_release['deployments'][0]['pipeline']
        assert inspect(browser_pipeline)[1]['risk']['x-pragmachange-reason']=='warmup'
        subprocess.run(command+['rollback']+flags,check=True)
        time.sleep(.2)
        assert json.loads((current/'release.json').read_text())['release_id']==releases['release_id']
        status,body=inspect(p);assert status in (200,403)
        if body:assert body['risk']['x-pragmachange-pipeline']==p['id']
        print('PASS: API-trained model and browser-exported model loaded by Nginx; real archive validation, atomic install, reload, and rollback')
    finally:
        nginx.terminate();nginx.wait(timeout=10);upstream.terminate();upstream.wait(timeout=10);log.close()

if __name__=='__main__':main()
