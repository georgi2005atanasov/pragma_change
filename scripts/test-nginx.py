#!/usr/bin/env python3
"""Native multi-worker tests. Requires scripts/build-nginx.sh and pragma-cli."""
import concurrent.futures
import copy
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from uuid import uuid4

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "build/native-test"
NGINX = ROOT / "build/nginx/sbin/nginx"
CLI = ROOT / "target/debug/pragma-cli"


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def schema(p):
    path = WORK / "pipeline.json"
    path.write_text(json.dumps(p))
    return subprocess.check_output([str(CLI), "schema", str(path)], text=True).strip()


def deployment(path, action="allow", window=False, capacity=False):
    p = json.loads((ROOT / "examples/pipeline.json").read_text())
    p.update(id=str(uuid4()), endpoint_id=str(uuid4()), path_template=path)
    if window:
        p["features"] = p["features"][:1]
        if capacity:
            p["limits"]["max_partitions"] = 1
            p["limits"]["max_events_per_partition"] = 2
    else:
        p["partitions"] = []
        p["windows"] = []
        p["features"] = [{"name": "amount", "operation": "current_value", "source": "amount", "partition": None, "window": None}]
    leaf = {"left": -1, "right": -1, "feature": -2, "threshold": -2.0, "samples": 100}
    tree = [leaf]
    if window:
        tree = [{"left": 1, "right": 2, "feature": 0, "threshold": 5.5, "samples": 100}, dict(leaf, samples=99), dict(leaf, samples=1)]
    model = {"format_version": 1, "id": str(uuid4()), "pipeline_version_id": p["id"], "schema_hash": schema(p), "feature_names": [f["name"] for f in p["features"]], "max_samples": 100, "trees": [tree]}
    thresholds = {"allow": {"check": .8, "block": .9}, "check": {"check": .1, "block": .9}, "block": {"check": .1, "block": .2}}[action]
    return {"pipeline": p, "model": model, "thresholds": thresholds}


def main():
    (WORK / "logs").mkdir(parents=True, exist_ok=True)
    proxy_port, upstream_port = port(), port()
    releases = {"format_version": 1, "release_id": str(uuid4()), "deployments": [deployment("/allow"), deployment("/check", "check"), deployment("/block", "block"), deployment("/window", window=True), deployment("/capacity", window=True, capacity=True)]}
    for prefix, scope in [("group", "shared"), ("separate", "per_endpoint")]:
        grouped = deployment(f"/{prefix}/a", window=True)
        grouped["pipeline"]["additional_endpoints"] = [{"endpoint_id": str(uuid4()), "api_host": "api.example.test", "method": "POST", "path_template": f"/{prefix}/b"}]
        grouped["pipeline"]["history_scope"] = scope
        grouped["model"]["schema_hash"] = schema(grouped["pipeline"])
        releases["deployments"].append(grouped)
    release_path = WORK / "release.json"
    release_path.write_text(json.dumps(releases))
    (WORK / "secret").write_text("ab" * 32)
    conf = WORK / "nginx.conf"
    conf.write_text(f'''load_module {ROOT}/build/nginx/modules/ngx_http_pragmachange_module.so;
worker_processes 4;
pid {WORK}/nginx.pid;
error_log {WORK}/error.log info;
events {{ worker_connections 1024; }}
http {{
 access_log {WORK}/access.log;
 client_body_temp_path {WORK}/body;
 client_body_buffer_size 128k;
 pragmachange_zone 8m;
 pragmachange_release {release_path};
 pragmachange_secret_file {WORK}/secret;
 server {{ listen 127.0.0.1:{proxy_port}; server_name api.example.test;
 location / {{ pragmachange on; proxy_pass http://127.0.0.1:{upstream_port}; }}
 }}
}}
''')
    subprocess.run([str(CLI), "validate-release", str(release_path)], check=True)
    subprocess.run([str(NGINX), "-p", str(WORK), "-c", str(conf), "-t"], check=True)
    log = (WORK / "upstream.log").open("w")
    upstream = subprocess.Popen([sys.executable, str(ROOT / "examples/upstream.py")], env={**os.environ, "PORT": str(upstream_port)}, stdout=log, stderr=log)
    nginx = subprocess.Popen([str(NGINX), "-p", str(WORK), "-c", str(conf), "-g", "daemon off;"])

    def request(path, body=b'{"amount": 42}', client="same-client", extra=None):
        headers = {"Host": "api.example.test", "Content-Type": "application/json", "X-Client-ID": client}
        headers.update(extra or {})
        try:
            response = urlopen(Request(f"http://127.0.0.1:{proxy_port}" + path, data=body, headers=headers), timeout=5)
        except HTTPError as e:
            response = e
        raw = response.read()
        return response.status, json.loads(raw) if response.status == 200 else raw

    try:
        time.sleep(.3)
        for path, action in [("/allow", "allow"), ("/check", "check")]:
            status, body = request(path, extra={"X-PragmaChange-Action": "block", "X-PragmaChange-Score": "0.99", "X-PragmaChange-Fake": "spoofed"})
            assert status == 200 and body["risk"]["x-pragmachange-action"] == action, (status, body)
            assert body["body"] == '{"amount": 42}'
            assert float(body["risk"]["x-pragmachange-score"]) == .5
            assert "x-pragmachange-fake" not in body["risk"]
        try:
            urlopen(Request(f"http://127.0.0.1:{proxy_port}/block", data=b'{"amount": 42}',
                            headers={"Host": "api.example.test", "Content-Type": "application/json",
                                     "X-Client-ID": "same-client"}), timeout=5)
            raise AssertionError("blocked request unexpectedly reached upstream")
        except HTTPError as blocked:
            assert blocked.code == 403
            assert blocked.headers["X-PragmaChange-Decision"] == "model_block"
            assert float(blocked.headers["X-PragmaChange-Score"]) == .5
            assert blocked.headers["X-PragmaChange-Model"] == releases["deployments"][2]["model"]["id"]
        status, body = request("/allow", b'not json')
        assert status == 200 and body["risk"]["x-pragmachange-action"] == "check" and "invalid JSON" in body["risk"]["x-pragmachange-reason"]
        oversized = b'{"amount":42,"padding":"' + b'a' * 70000 + b'"}'
        status, body = request("/allow", oversized)
        assert status == 200 and body["body"].encode() == oversized and body["risk"]["x-pragmachange-action"] == "check"
        status, body = request("/unconfigured", extra={"X-PragmaChange-Action": "allow"})
        assert status == 200 and not body["risk"]
        assert request("/window")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        group_risk = request("/group/a")[1]["risk"]
        assert group_risk["x-pragmachange-reason"] == "warmup"
        assert request("/separate/a")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        time.sleep(1.05)
        # Two routes share one loaded forest and one account history across four workers.
        for i in range(5):
            status, body = request("/group/a" if i % 2 == 0 else "/group/b")
            assert status == 200 and body["risk"]["x-pragmachange-action"] == "allow"
            assert body["risk"]["x-pragmachange-pipeline"] == group_risk["x-pragmachange-pipeline"]
            assert body["risk"]["x-pragmachange-model"] == releases["deployments"][5]["model"]["id"]
        assert request("/group/b")[0] == 403
        # Reusing that model architecture can instead keep endpoint histories independent.
        assert request("/separate/a")[1]["risk"]["x-pragmachange-action"] == "allow"
        assert request("/separate/b")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
            results = list(pool.map(lambda _: request("/window"), range(20)))
        allowed = sum(status == 200 for status, _ in results)
        blocked = sum(status == 403 for status, _ in results)
        assert (allowed, blocked) == (5, 15), (allowed, blocked)
        # Model-only reload keeps shared windows, while changed pipeline IDs isolate history.
        releases["release_id"] = str(uuid4())
        releases["deployments"][3]["model"]["id"] = str(uuid4())
        release_path.write_text(json.dumps(releases))
        subprocess.run([str(NGINX), "-p", str(WORK), "-c", str(conf), "-s", "reload"], check=True)
        time.sleep(.2)
        assert request("/window")[0] == 403
        window = releases["deployments"][3]
        window["pipeline"]["id"] = str(uuid4())
        window["model"]["pipeline_version_id"] = window["pipeline"]["id"]
        window["model"]["schema_hash"] = schema(window["pipeline"])
        release_path.write_text(json.dumps(releases))
        subprocess.run([str(NGINX), "-p", str(WORK), "-c", str(conf), "-s", "reload"], check=True)
        time.sleep(.2)
        assert request("/window")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        for _ in range(4):
            status, body = request("/capacity")
        assert body["risk"]["x-pragmachange-reason"] == "state_capacity_warmup"
        request("/capacity", client="another")
        assert request("/capacity")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        # Corrupt bundles must fail nginx -t and leave already-running workers untouched.
        bad = copy.deepcopy(releases)
        bad["deployments"][0]["model"]["schema_hash"] = "bad"
        release_path.write_text(json.dumps(bad))
        result = subprocess.run([str(NGINX), "-p", str(WORK), "-c", str(conf), "-t"], capture_output=True)
        assert result.returncode != 0
        assert request("/allow")[1]["risk"]["x-pragmachange-action"] == "allow"
        release_path.write_text(json.dumps(releases))
        print("PASS: real Nginx module, four workers, grouped routes sharing one model, combined and separate endpoint histories, shared window counts, allow/check/block, body preservation, spoof stripping, oversized/malformed bodies, reload persistence, version isolation, state limits, corrupt release rejection. No control-plane services are used by this runtime.")
    finally:
        nginx.terminate()
        nginx.wait(timeout=10)
        upstream.terminate()
        upstream.wait(timeout=10)
        log.close()

if __name__ == "__main__":
    main()
