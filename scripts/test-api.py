#!/usr/bin/env python3
"""Exercise a running management API/trainer against a disposable database."""
import json
import os
from pathlib import Path
import random
import time
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from uuid import uuid4

BASE = os.environ.get("API_URL", "http://127.0.0.1:8080") + "/api/v1"
token, csrf = "", ""


def call(path, body=None, expected=200, binary=False, authenticated=True):
    headers = {"Content-Type": "application/json"}
    if authenticated:
        headers.update(Authorization="Bearer " + token, **{"X-CSRF-Token": csrf})
    req = Request(BASE + "/" + path, headers=headers,
                  data=None if body is None else json.dumps(body).encode())
    try:
        response = urlopen(req, timeout=60)
    except HTTPError as error:
        response = error
    data = response.read()
    assert response.status == expected, (path, response.status, data.decode()[:500])
    return data if binary else json.loads(data)


def main():
    global token, csrf
    call("overview", expected=401, authenticated=False)
    auth = call("login", {"username": os.environ.get("ADMIN_USERNAME", "admin"), "password": os.environ["ADMIN_PASSWORD"]}, authenticated=False)
    token, csrf = auth["token"], auth["csrf_token"]
    saved = csrf
    csrf = "invalid"
    call("apis", {"name": "forbidden", "host": "forbidden.test"}, expected=403)
    csrf = saved
    suffix = uuid4().hex[:8]
    api_id = call("apis", {"name": "Integration " + suffix, "host": suffix + ".example.test"})["id"]
    endpoint_id = call("endpoints", {"api_id": api_id, "name": "Orders", "method": "POST", "path_template": "/orders"})["id"]
    call("endpoints", {"api_id": api_id, "name": "Ambiguous", "method": "POST", "path_template": "/{anything}"}, expected=422)
    config = json.loads(Path("examples/pipeline.json").read_text())
    config["endpoint_id"] = endpoint_id
    pipeline = call("pipelines", config)
    template = call(f"pipelines/{pipeline['id']}/template")
    assert template["manifest"]["schema_hash"] == pipeline["schema_hash"]
    events = []
    for i in range(30):
        events.append({"timestamp_ms": i * 100, "endpoint_id": endpoint_id, "method": "POST", "path": "/orders", "headers": {"X-Client-ID": "test"}, "body": {"amount": 10 + i % 5}, "body_size": 32})
    request_import = {"name": "Request replay " + suffix, "pipeline_version_id": pipeline["id"], "schema_hash": pipeline["schema_hash"], "format": "request_events", "content": "\n".join(json.dumps(e) for e in events)}
    preview = call("datasets/preview", request_import)
    assert preview["warmup_rows"] == 10 and preview["training_rows"] == 20 and preview["valid"]
    call("datasets", request_import)
    replay = call(f"pipelines/{pipeline['id']}/replay", {"events": events})
    assert replay["results"][10]["features"]["vector"][0] == 10
    rng = random.Random(42)
    rows = [[rng.randint(2, 12), rng.uniform(10, 100), rng.randint(20, 150)] for _ in range(250)]
    csv = "request_count,amount_mean,body_bytes\n" + "\n".join(",".join(map(str, row)) for row in rows)
    imported = {"name": "Baseline " + suffix, "pipeline_version_id": pipeline["id"], "schema_hash": pipeline["schema_hash"], "format": "feature_vectors", "content": csv}
    invalid = dict(imported, content="amount_mean,request_count,body_bytes\n1,2,3")
    call("datasets/preview", invalid, expected=422)
    invalid = dict(imported, content=csv + "\nNaN,1,2")
    assert not call("datasets/preview", invalid)["valid"]
    call("datasets/preview", dict(imported, schema_hash="bad"), expected=422)
    dataset_id = call("datasets", imported)["id"]
    job_id = call("training-jobs", {"dataset_id": dataset_id, "parameters": {"n_estimators": 20, "max_samples": 64, "seed": 42}})["id"]
    for _ in range(90):
        job = next(j for j in call("training-jobs") if j["id"] == job_id)
        if job["status"] in ("succeeded", "failed"):
            break
        time.sleep(1)
    assert job["status"] == "succeeded", job
    model = next(m for m in call("models") if m["training_job_id"] == job_id)
    detail = call(f"models/{model['id']}")
    assert detail["dataset"]["id"] == dataset_id
    assert detail["endpoint"]["id"] == endpoint_id
    assert detail["training_job"]["id"] == job_id
    assert detail["pipeline"]["id"] == pipeline["id"]
    call(f"models/{uuid4()}", expected=404)
    for thresholds, expected in [({"check": 0.99, "block": 1.0}, "allow"), ({"check": 0.0, "block": 1.0}, "check"), ({"check": 0.0, "block": 0.001}, "block")]:
        result = call(f"models/{model['id']}/score", {"vector": rows[0], "thresholds": thresholds})
        assert result["action"] == expected
    call(f"models/{model['id']}/score", {"vector": [1], "thresholds": {"check": .5, "block": .7}}, expected=422)
    call("releases", {"name": "Invalid", "models": [{"model_version_id": model["id"], "thresholds": {"check": .7, "block": .6}}]}, expected=422)
    selection = {"model_version_id": model["id"], "thresholds": {"check": .6, "block": .8}}
    release_id = call("releases", {"name": "Integration release " + suffix, "models": [selection]})["id"]
    call("releases", {"name": "Ambiguous", "models": [selection, selection]}, expected=422)
    output = Path("build/e2e")
    output.mkdir(parents=True, exist_ok=True)
    (output / "release.tar.gz").write_bytes(call(f"releases/{release_id}/download", binary=True))
    (output / "api-result.json").write_text(json.dumps({"api_id": api_id, "endpoint_id": endpoint_id, "pipeline_id": pipeline["id"], "dataset_id": dataset_id, "model_id": model["id"], "release_id": release_id}))
    call("logout", {})
    call("overview", expected=401)
    print("PASS: authentication/CSRF, routes, immutable pipelines, both imports, validation, replay, training + Python/Rust parity, all actions, release export, logout")


if __name__ == "__main__":
    main()
