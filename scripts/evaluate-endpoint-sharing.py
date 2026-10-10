#!/usr/bin/env python3
"""Reproduce a synthetic counterexample to unconditional endpoint model sharing.

Use services/trainer/.venv/bin/python. This is not an API accuracy benchmark.
Training, threshold calibration, and evaluation use disjoint samples.
"""
import json

import numpy as np
import sklearn
from sklearn.ensemble import IsolationForest


def evaluate(seed):
    rng = np.random.default_rng(seed)
    quiet = rng.normal(3, 0.5, (1500, 1)).astype(np.float32)
    busy = rng.normal(60, 3, (1500, 1)).astype(np.float32)
    cases = {
        "quiet_endpoint_only": (quiet[:1000], quiet[1000:1250]),
        "pooled_endpoints": (
            np.vstack([quiet[:1000], busy[:1000]]),
            np.vstack([quiet[1000:1250], busy[1000:1250]]),
        ),
    }
    result = {"seed": seed}
    for name, (training, calibration) in cases.items():
        model = IsolationForest(
            n_estimators=100, max_samples=256, max_features=1.0,
            contamination="auto", bootstrap=False, random_state=seed, n_jobs=1,
        ).fit(training)
        threshold = float(np.quantile(-model.score_samples(calibration), 0.99))
        # These same counts are normal for the busy endpoint and anomalous
        # by construction for the quiet endpoint. Endpoint identity is absent.
        probes = busy[1250:]
        scores = -model.score_samples(probes)
        result[name] = {
            "threshold": threshold,
            "score_at_60_requests_per_minute": float(-model.score_samples([[60.]])[0]),
            "quiet_endpoint_bursts_flagged": int((scores >= threshold).sum()),
            "quiet_endpoint_burst_count": len(probes),
            "normal_quiet_test_rows_flagged": int((-model.score_samples(quiet[1250:]) >= threshold).sum()),
            "normal_quiet_test_count": len(quiet[1250:]),
        }
    return result


if __name__ == "__main__":
    print(json.dumps({
        "purpose": "Synthetic counterexample, not a production accuracy estimate",
        "feature": "requests_per_minute",
        "normal_quiet_distribution": "Normal(mean=3, stddev=0.5)",
        "normal_busy_distribution": "Normal(mean=60, stddev=3)",
        "sklearn_version": sklearn.__version__,
        "runs": [evaluate(seed) for seed in [42, 137, 2026]],
        "interpretation": "A shared forest cannot distinguish the same vector's different endpoint meanings. Separate histories alone do not fix pooled training. This example does not prove sharing is always harmful.",
    }, indent=2))
