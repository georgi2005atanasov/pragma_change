#!/usr/bin/env python3
"""Real native adapter acceptance. Run once per MPM/distribution, no management services.
APACHE_BIN/APACHE_MODULE_DIR/PRAGMA_APACHE_MODULE/PRAGMA_CLI override packaged paths.
"""
import argparse
import concurrent.futures
import copy
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import hashlib
import io
import tarfile
import json
import os
import re
from pathlib import Path
import socket
import subprocess
import threading
import time
from urllib.request import urlopen
from urllib.parse import urlencode
from uuid import uuid4

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("native_fixtures", ROOT / "scripts/test-nginx.py")
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Echo(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            chunks = []
            while True:
                n = int(self.rfile.readline().split(b";")[0], 16)
                if not n:
                    self.rfile.readline()
                    break
                chunks.append(self.rfile.read(n))
                assert self.rfile.read(2) == b"\r\n"
            body = b"".join(chunks)
        else:
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        payload = json.dumps({"body": body.decode(), "risk": {k.lower(): v for k, v in self.headers.items() if k.lower().startswith("x-pragmachange-")}}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mpm", choices=["event", "worker", "prefork"], default="event")
    parser.add_argument("--http2", action="store_true")
    parser.add_argument("--benchmark", action="store_true")
    parser.add_argument("--release-bundle", action="append", default=[], help="Additionally activate an API/browser-exported Apache bundle")
    args = parser.parse_args()
    work = ROOT / "build" / f"apache-test-{args.mpm}"
    work.mkdir(parents=True, exist_ok=True)
    fixtures.WORK = work
    fixtures.CLI = Path(os.environ.get("PRAGMA_CLI", str(ROOT / "target/debug/pragma-cli")))
    apache = os.environ.get("APACHE_BIN", "/usr/sbin/apache2")
    modules = Path(os.environ.get("APACHE_MODULE_DIR", "/usr/lib/apache2/modules"))
    module = Path(os.environ.get("PRAGMA_APACHE_MODULE", str(ROOT / "build/apache/mod_pragmachange.so")))
    api_port, status_port, public_port = port(), port(), port()
    upstream = ThreadingHTTPServer(("127.0.0.1", 0), Echo)
    thread = threading.Thread(target=upstream.serve_forever, daemon=True)
    thread.start()
    releases = {"format_version": 1, "release_id": str(uuid4()), "deployments": [
        fixtures.deployment("/allow"), fixtures.deployment("/check", "check"),
        fixtures.deployment("/block", "block"), fixtures.deployment("/window", window=True),
        fixtures.deployment("/capacity", window=True, capacity=True), fixtures.deployment("/pool"),
    ]}
    # Header/query/path/IP/body-size extractors alongside JSON extraction.
    fields = fixtures.deployment("/fields/{id}")
    p = fields["pipeline"]
    p["extractors"] += [
        {"name": "query", "source": {"kind": "query_parameter", "name": "number"}},
        {"name": "path", "source": {"kind": "path_parameter", "name": "id"}},
        {"name": "ip", "source": {"kind": "client_ip"}},
    ]
    # Validate the schema independently of Apache before generating the model.
    fields["model"]["schema_hash"] = fixtures.schema(p)
    releases["deployments"].append(fields)
    for prefix, scope in [("group", "shared"), ("separate", "per_endpoint")]:
        grouped = fixtures.deployment(f"/{prefix}/a", window=True)
        grouped["pipeline"]["additional_endpoints"] = [{"endpoint_id": str(uuid4()), "api_host": "api.example.test", "method": "POST", "path_template": f"/{prefix}/b"}]
        grouped["pipeline"]["history_scope"] = scope
        grouped["model"]["schema_hash"] = fixtures.schema(grouped["pipeline"])
        releases["deployments"].append(grouped)
    size_only = fixtures.deployment("/size")
    size_only["pipeline"]["extractors"] = size_only["pipeline"]["extractors"][:1]
    size_only["pipeline"]["features"] = [{"name":"bytes","operation":"body_size","source":None,"partition":None,"window":None}]
    size_only["model"]["feature_names"] = ["bytes"]
    size_only["model"]["schema_hash"] = fixtures.schema(size_only["pipeline"])
    leaf = {"left":-1,"right":-1,"feature":-2,"threshold":-2.0,"samples":99}
    size_only["model"]["trees"] = [[{"left":1,"right":2,"feature":0,"threshold":13.5,"samples":100},leaf,dict(leaf,samples=1)]]
    releases["deployments"].append(size_only)
    pressure = fixtures.deployment("/pressure", window=True)
    pressure["pipeline"]["features"].append({"name":"distinct", "operation":"distinct_count", "source":"amount", "partition":"by_client", "window":"last_second"})
    pressure["model"]["feature_names"].append("distinct")
    pressure["model"]["schema_hash"] = fixtures.schema(pressure["pipeline"])
    releases["deployments"].append(pressure)
    release_path = work / "release.json"
    release_path.write_text(json.dumps(releases))
    (work / "secret").write_text("ab" * 32)
    loads = [f"mpm_{args.mpm}", "authz_core", "authz_host", "proxy", "proxy_http", "proxy_balancer", "lbmethod_byrequests", "slotmem_shm"]
    if args.http2:
        loads.append("http2")
    configuration = [f'LoadModule {name}_module {modules}/mod_{name}.so' for name in loads]
    configuration += [f"LoadModule pragmachange_module {module}", f"ServerRoot {work}", f"DefaultRuntimeDir {work}", f"PidFile {work}/apache.pid", f"ErrorLog {work}/error.log", "LogLevel info", "ServerName api.example.test", f"Listen 127.0.0.1:{api_port}", f"Listen 127.0.0.1:{status_port}", f"Listen 0.0.0.0:{public_port}", f"PragmaChangeRelease {release_path}", f"PragmaChangeSecretFile {work}/secret", "PragmaChangeStateMiB 2", "StartServers 3", "ServerLimit 16", "MaxRequestWorkers 16"]
    if args.mpm != "prefork":
        configuration += ["ThreadsPerChild 4", "MinSpareThreads 4", "MaxSpareThreads 12"]
    else:
        configuration += ["MinSpareServers 2", "MaxSpareServers 6"]
    if os.geteuid() == 0:
        configuration += ["User www-data", "Group www-data"]
    else:
        configuration += [f"User #{os.getuid()}", f"Group #{os.getgid()}"]
    configuration += [f'''<VirtualHost 127.0.0.1:{api_port}>
ServerName api.example.test
Protocols h2c http/1.1
<Location />
  PragmaChange On
  Require all granted
</Location>
<Proxy "balancer://test">
  BalancerMember http://127.0.0.1:{upstream.server_port}
</Proxy>
ProxyPass /pool balancer://test/pool
ProxyPass / http://127.0.0.1:{upstream.server_port}/
</VirtualHost>
<VirtualHost 127.0.0.1:{status_port} *:{public_port}>
<Location /pragmachange-status>
  SetHandler pragmachange-status
  Require all granted
</Location>
</VirtualHost>''']
    config = work / "apache.conf"
    config.write_text("\n".join(configuration) + "\n")
    command = [apache, "-f", str(config)]
    subprocess.run(command + ["-t"], check=True)
    output = (work / "console.log").open("w")
    process = subprocess.Popen(command + ["-DFOREGROUND"], stdout=output, stderr=output, start_new_session=True)
    status_url = f"http://127.0.0.1:{status_port}/pragmachange-status"

    def status():
        with urlopen(status_url, timeout=2) as response:
            return json.load(response)

    def request(path, body=b'{"amount": 42}', client="same-client", extra=None, chunked=False):
        headers = {"Host": "api.example.test", "Content-Type": "application/json", "X-Client-ID": client}
        headers.update(extra or {})
        connection = http.client.HTTPConnection("127.0.0.1", api_port, timeout=5)
        connection.request("POST", path, [body[:5], body[5:]] if chunked else body, headers, encode_chunked=chunked)
        response = connection.getresponse()
        raw = response.read()
        result = (response.status, json.loads(raw) if response.status == 200 else raw)
        connection.close()
        return result

    def reload(expected):
        subprocess.run(command + ["-k", "graceful"], check=True)
        for _ in range(80):
            if status()["release_id"] == expected:
                return
            time.sleep(.05)
        raise AssertionError("new generation not observed")

    try:
        for _ in range(100):
            try:
                if status()["release_id"] == releases["release_id"]:
                    break
            except OSError:
                pass
            if process.poll() is not None:
                raise RuntimeError((work / "console.log").read_text() + (work / "error.log").read_text())
            time.sleep(.05)
        else:
            raise RuntimeError("Apache did not start")
        observations = []
        for path, action in [("/allow", "allow"), ("/check", "check"), ("/pool", "allow")]:
            code, body = request(path, extra={"X-PragmaChange-Action": "block", "X-PragmaChange-Fake": "spoof"})
            assert code == 200 and body["risk"]["x-pragmachange-action"] == action, (code, body)
            assert body["body"] == '{"amount": 42}'
            assert float(body["risk"]["x-pragmachange-score"]) == .5
            assert "x-pragmachange-fake" not in body["risk"]
            observations.append({"path": path, "score": .5, "action": action})
        assert request("/block")[0] == 403
        for chunked in (False, True):
            assert request("/size", b"x"*13, chunked=chunked)[1]["risk"]["x-pragmachange-score"] != ""
            assert request("/size", b"x"*14, chunked=chunked)[0] == 403
        connection = http.client.HTTPConnection("127.0.0.1", api_port, timeout=5)
        connection.putrequest("POST", "/allow", skip_host=True)
        for name,value in [("Host","api.example.test"),("X-Client-ID","first"),("x-client-id","second"),("Content-Length","13")]:
            connection.putheader(name,value)
        connection.endheaders(b'{"amount":42}')
        assert json.loads(connection.getresponse().read())["risk"]["x-pragmachange-score"] == ""
        connection.close()
        with socket.create_connection(("127.0.0.1",api_port),timeout=5) as stream:
            stream.sendall(b"POST /allow HTTP/1.1\r\nHost: api.example.test\r\nX-Client-ID: expect-client\r\nContent-Length: 13\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n")
            interim = stream.recv(4096)
            assert b"100 Continue" in interim, interim
            stream.sendall(b'{"amount":42}')
            reply = b""
            while True:
                chunk = stream.recv(8192)
                if not chunk: break
                reply += chunk
            assert b'"x-pragmachange-action": "allow"' in reply

        for chunked in (False, True):
            for body in (b'{"amount": 42}', b'not json', b'{"amount":42,"padding":"' + b'a' * 70000 + b'"}'):
                code, result = request("/allow", body, chunked=chunked)
                assert code == 200 and result["body"].encode() == body, (code, result)
                if body != b'{"amount": 42}':
                    assert result["risk"]["x-pragmachange-action"] == "check" and result["risk"]["x-pragmachange-score"] == ""
        assert request("/unknown", extra={"X-PragmaChange-Action": "allow"})[1]["risk"] == {}
        assert request("/allow", extra={"X-Unused": "one,two"})[1]["risk"]["x-pragmachange-action"] == "allow"
        assert request("/allow", client="one,two")[1]["risk"]["x-pragmachange-score"] == ""
        assert request("/fields/17?number=3")[1]["risk"]["x-pragmachange-action"] == "allow"
        assert request("/fields/17?number=3&number=4")[1]["risk"]["x-pragmachange-score"] == ""
        for path in ("/window", "/group/a", "/separate/a"):
            assert request(path)[1]["risk"]["x-pragmachange-reason"] == "warmup"
        time.sleep(1.05)
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
            responses = list(pool.map(lambda _: request("/window"), range(20)))
        counts = [sum(code == expected for code, _ in responses) for expected in (200, 403)]
        assert counts == [5, 15], counts
        for n in range(5):
            assert request("/group/a" if n % 2 == 0 else "/group/b")[1]["risk"]["x-pragmachange-action"] == "allow"
        assert request("/group/b")[0] == 403
        assert request("/separate/a")[1]["risk"]["x-pragmachange-action"] == "allow"
        assert request("/separate/b")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        releases["release_id"] = str(uuid4())
        releases["deployments"][3]["model"]["id"] = str(uuid4())
        releases["deployments"][3]["thresholds"] = {"check": .99, "block": 1.0}
        release_path.write_text(json.dumps(releases))
        reload(releases["release_id"])
        code, retained = request("/window")
        assert code == 200 and retained["risk"]["x-pragmachange-score"] != "", (code, retained)
        assert retained["risk"]["x-pragmachange-model"] == releases["deployments"][3]["model"]["id"]
        window = releases["deployments"][3]
        window["pipeline"]["id"] = str(uuid4())
        window["model"]["pipeline_version_id"] = window["pipeline"]["id"]
        window["model"]["schema_hash"] = fixtures.schema(window["pipeline"])
        releases["release_id"] = str(uuid4())
        release_path.write_text(json.dumps(releases))
        reload(releases["release_id"])
        assert request("/window")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        for _ in range(4):
            code, body = request("/capacity")
        assert body["risk"]["x-pragmachange-reason"] == "state_capacity_warmup"
        request("/capacity", client="other")
        assert request("/capacity")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        bad = copy.deepcopy(releases)
        bad["deployments"][0]["model"]["schema_hash"] = "bad"
        release_path.write_text(json.dumps(bad))
        assert subprocess.run(command + ["-t"], capture_output=True).returncode != 0
        assert status()["release_id"] == releases["release_id"]
        assert request("/allow")[1]["risk"]["x-pragmachange-action"] == "allow"
        guarded = copy.deepcopy(releases)
        guarded["format_version"] = 2
        guarded["ingress_policies"] = [{"name": "upload_guard", "max_body_bytes": 16777216,
            "max_multipart_parts": 16, "max_headers_per_part": 16,
            "max_header_bytes_per_part": 8192, "max_field_nesting": 8,
            "max_file_parts": 2}]
        release_path.write_text(json.dumps(guarded))
        assert subprocess.run(command + ["-t"], capture_output=True).returncode != 0
        assert status()["release_id"] == releases["release_id"]
        guarded["release_id"] = str(uuid4())
        release_path.write_text(json.dumps(guarded))
        original_config = config.read_text()
        config.write_text(original_config + "\nPragmaChangeGuard upload_guard\n")
        subprocess.run(command + ["-t"], check=True)
        reload(guarded["release_id"])
        boundary = b"lab-boundary"
        part = b"--" + boundary + b"\r\nContent-Disposition: form-data; name=\"x\"\r\n\r\nok\r\n"
        content_type = {"Content-Type": "multipart/form-data; boundary=lab-boundary"}
        code, forwarded = request("/unknown", part + b"--" + boundary + b"--\r\n", extra=content_type)
        assert code == 200 and forwarded["risk"]["x-pragmachange-guard"] == "allow"
        assert request("/unknown", part, extra=content_type)[0] == 403
        assert request("/unknown", part * 17 + b"--" + boundary + b"--\r\n", extra=content_type)[0] == 403
        config.write_text(original_config)
        release_path.write_text(json.dumps(releases))
        reload(releases["release_id"])
        if args.http2:
            payload = work / "body.json"
            for content in (b'{"amount":42}', b'not json', b'{"amount":42,"padding":"'+b'x'*70000+b'"}'):
                payload.write_bytes(content)
                h2 = subprocess.run(["nghttp", "-d", str(payload), "-H", ":authority: api.example.test", "-H", "x-client-id: h2-client", f"http://127.0.0.1:{api_port}/allow"], check=True, capture_output=True, text=True)
                response = json.loads(h2.stdout)
                assert response["body"].encode() == content
                assert response["risk"]["x-pragmachange-action"] == ("allow" if content == b'{"amount":42}' else "check")
        # The handler rejects access through a non-loopback socket even when
        # Apache authorization grants it. Forwarding headers cannot change that.
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
            probe.connect(("192.0.2.1", 9))
            local_ip = probe.getsockname()[0]
        connection = http.client.HTTPConnection(local_ip, public_port, timeout=3)
        connection.request("GET", "/pragmachange-status", headers={"X-Forwarded-For":"127.0.0.1"})
        assert connection.getresponse().status == 403
        connection.close()
        large = json.dumps({"amount": "x" * 4000}).encode()
        request("/pressure", large, client="oldest")
        time.sleep(1.05)
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
            pressure_results = list(pool.map(lambda n: request("/pressure", large, client=f"pressure-{n}"), range(700)))
        assert all(code == 200 for code, _ in pressure_results)
        assert request("/pressure", large, client="oldest")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        # Exercise the actual local deployment helper and immutable archive.
        deployment_root = work / "deployment"
        deployment_root.mkdir(exist_ok=True)
        (deployment_root / "partition_secret").write_text("ab" * 32)
        old = deployment_root / "releases" / releases["release_id"]
        old.mkdir(parents=True, exist_ok=True)
        (old / "release.json").write_text(json.dumps(releases))
        current = deployment_root / "current"
        current.unlink(missing_ok=True)
        current.symlink_to(f"releases/{releases['release_id']}")
        config.write_text(config.read_text().replace(str(release_path), str(current / "release.json")))
        new_release = copy.deepcopy(releases)
        new_release["release_id"] = str(uuid4())
        files = {"release.json": json.dumps(new_release).encode(), "compatibility.json": json.dumps({"format_version":1,"module_abi":1,"proxy":"apache","target":"x86_64-unknown-linux-gnu","release_id":new_release["release_id"]}).encode(), "apache.conf.example":b"# test", "README.txt":b"test release"}
        files["SHA256SUMS"] = "".join(hashlib.sha256(value).hexdigest()+"  "+name+"\n" for name,value in files.items()).encode()
        bundle = work / "release.tar.gz"
        with tarfile.open(bundle, "w:gz") as tar:
            for name,value in files.items():
                member = tarfile.TarInfo(name); member.size = len(value)
                tar.addfile(member, io.BytesIO(value))
        helper = ["python3", str(ROOT / "scripts/deploy-apache.py")]
        flags = ["--root", str(deployment_root), "--apachectl", apache, "--apache-config", str(config), "--validator", str(fixtures.CLI), "--status-url", status_url]
        subprocess.run(helper+["validate", str(bundle)]+flags, check=True)
        subprocess.run(helper+["install", str(bundle)]+flags, check=True)
        assert status()["release_id"] == new_release["release_id"]
        subprocess.run(helper+["rollback"]+flags, check=True)
        assert status()["release_id"] == releases["release_id"]
        if args.benchmark:
            enabled_config = config.read_text()
            samples = []
            def cpu_memory():
                pids = [process.pid]
                try:
                    pids += [int(value) for value in Path(f"/proc/{process.pid}/task/{process.pid}/children").read_text().split()]
                except OSError:
                    pass
                cpu = rss = 0
                for pid in pids:
                    try:
                        values = Path(f"/proc/{pid}/stat").read_text().split()
                        cpu += (int(values[13])+int(values[14])) / os.sysconf("SC_CLK_TCK")
                        rss += int(values[23]) * os.sysconf("SC_PAGE_SIZE")
                    except OSError:
                        pass
                return cpu, rss
            for enabled in (False, True):
                # The baseline must not load the detector or allocate its state.
                # Restart both cases so previous acceptance traffic cannot affect
                # process memory or leave a draining generation in the sample.
                process.terminate(); process.wait(timeout=15)
                baseline = "\n".join(line for line in enabled_config.splitlines()
                                     if not line.strip().startswith(("PragmaChange", "LoadModule pragmachange_module"))) + "\n"
                config.write_text(enabled_config if enabled else baseline)
                process = subprocess.Popen(command+["-DFOREGROUND"], stdout=output, stderr=output, start_new_session=True)
                for _ in range(100):
                    try:
                        if bool(request("/allow")[1]["risk"]) == enabled: break
                    except OSError:
                        pass
                    assert process.poll() is None, "Apache benchmark instance exited"
                    time.sleep(.05)
                else:
                    raise AssertionError("Apache benchmark instance did not become ready")
                assert ("mod_pragmachange.so" in Path(f"/proc/{process.pid}/maps").read_text()) == enabled
                time.sleep(.2)
                before, _ = cpu_memory()
                start = time.perf_counter()
                def measured(_):
                    begin = time.perf_counter()
                    code, _ = request("/allow")
                    return (time.perf_counter()-begin)*1000, code
                with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                    measured_requests = list(pool.map(measured, range(400)))
                elapsed = time.perf_counter()-start
                after, rss = cpu_memory()
                latencies = sorted(value for value,code in measured_requests if code == 200)
                samples.append({"enabled":enabled,"module_loaded":enabled,"requests":400,"concurrency":8,"seconds":elapsed,"requests_per_second":400/elapsed,"p50_ms":latencies[len(latencies)//2],"p95_ms":latencies[int(len(latencies)*.95)],"p99_ms":latencies[int(len(latencies)*.99)],"cpu_seconds":after-before,"summed_rss_bytes":rss,"errors":sum(code != 200 for _,code in measured_requests)})
            (work/"benchmark.json").write_text(json.dumps({"environment":"isolated native acceptance instance; Python HTTP echo backend; one current-value feature; synthetic requests; not a production capacity estimate","samples":samples}, indent=2))
        # A full restart intentionally loses in-memory history.
        assert request("/window", client="restart")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        time.sleep(1.05)
        assert request("/window", client="restart")[1]["risk"]["x-pragmachange-score"] != ""
        process.terminate(); process.wait(timeout=15)
        process = subprocess.Popen(command+["-DFOREGROUND"], stdout=output, stderr=output, start_new_session=True)
        for _ in range(100):
            try:
                status(); break
            except OSError:
                time.sleep(.05)
        assert request("/window", client="restart")[1]["risk"]["x-pragmachange-reason"] == "warmup"
        for supplied in args.release_bundle:
            with tarfile.open(supplied) as archive:
                exported = json.load(archive.extractfile("release.json"))
            subprocess.run(helper+["install", str(Path(supplied).resolve())]+flags, check=True)
            assert status()["release_id"] == exported["release_id"]
            pipeline = exported["deployments"][0]["pipeline"]
            path = re.sub(r"\{[^}]+\}", "42", pipeline["path_template"])
            headers = {"Host":pipeline["api_host"]}
            query, payload = {}, {}
            for extractor in pipeline["extractors"]:
                source = extractor["source"]
                if source["kind"] == "header": headers[source["name"]] = "42"
                if source["kind"] == "query_parameter": query[source["name"]] = "42"
                if source["kind"] == "json_pointer":
                    parts = [part.replace("~1", "/").replace("~0", "~") for part in source["pointer"].split("/")[1:]]
                    current_payload = payload
                    for part in parts[:-1]: current_payload = current_payload.setdefault(part,{})
                    if parts: current_payload[parts[-1]] = 42
            if query: path += "?" + urlencode(query)
            assert pipeline["method"] == "POST", "the exported-bundle fixture expects a POST endpoint"
            longest = max((window["duration_ms"] for window in pipeline["windows"]), default=0)
            assert longest <= 60000, "use an acceptance fixture with windows at most one minute"
            request(path, json.dumps(payload).encode(), extra=headers)
            time.sleep(longest / 1000 + .05)
            code, body = request(path, json.dumps(payload).encode(), extra=headers)
            assert code in (200,403), (code,body)
            if code == 200:
                assert body["risk"]["x-pragmachange-score"] != "", body
                assert body["risk"]["x-pragmachange-pipeline"] == pipeline["id"]
            subprocess.run(helper+["rollback"]+flags, check=True)
            assert status()["release_id"] == releases["release_id"]
        report = {"mpm": args.mpm, "http2": args.http2, "apache": subprocess.check_output([apache, "-v"], text=True).splitlines()[0], "observations": observations, "shared_window_results": counts}
        (work / "result.json").write_text(json.dumps(report, indent=2))
        print("PASS:", json.dumps(report))
    finally:
        process.terminate()
        process.wait(timeout=15)
        upstream.shutdown()
        upstream.server_close()
        output.close()


if __name__ == "__main__":
    main()
