#!/usr/bin/env python3
"""Generate deterministic example source events and compatible sample vectors."""
import csv
import json
from pathlib import Path
import random
import subprocess

root = Path(__file__).resolve().parent.parent
p = json.loads((root / "examples/pipeline.json").read_text())
rng = random.Random(42)
events = []
for i in range(500):
    body = {"amount": round(rng.uniform(10, 100), 2)}
    events.append({"timestamp_ms": i * 100, "endpoint_id": p["endpoint_id"], "method": "POST", "path": "/orders", "headers": {"X-Client-ID": f"client-{i % 3}"}, "body": body, "body_size": len(json.dumps(body).encode())})
(root / "examples/request-events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))
with (root / "examples/feature-vectors.csv").open("w") as f:
    writer = csv.writer(f)
    writer.writerow([x["name"] for x in p["features"]])
    for i in range(200):
        writer.writerow([rng.randint(2, 10), round(rng.uniform(10, 100), 2), rng.randint(15, 40)])
print("Generated examples/request-events.jsonl and examples/feature-vectors.csv. Use the saved pipeline's own ID/hash and field definitions when importing through the portal.")
