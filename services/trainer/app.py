"""Private, deterministic training endpoint. Never accepts pickle or executable models."""
import hmac
import os
import threading
from typing import Annotated
from uuid import UUID

import numpy as np
import sklearn
from fastapi import Depends, FastAPI, Header, HTTPException
from pydantic import BaseModel, ConfigDict, Field, model_validator
from sklearn.ensemble import IsolationForest

app = FastAPI(title="PragmaChange trainer", version="0.1.0")
training_slot = threading.Lock()


class Parameters(BaseModel):
    model_config = ConfigDict(extra="forbid")
    n_estimators: int = Field(default=100, ge=1, le=512)
    max_samples: int = Field(default=256, ge=2, le=65536)
    seed: int = Field(default=42, ge=0, le=2**32 - 1)


class TrainingRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    job_id: UUID
    pipeline_version_id: UUID
    schema_hash: str = Field(pattern=r"^[0-9a-f]{64}$")
    feature_names: list[str] = Field(min_length=1, max_length=128)
    vectors: list[list[float]] = Field(min_length=20, max_length=100000)
    parameters: Parameters = Field(default_factory=Parameters)

    @model_validator(mode="after")
    def valid_matrix(self):
        if len(set(self.feature_names)) != len(self.feature_names):
            raise ValueError("duplicate feature names")
        if any(len(row) != len(self.feature_names) for row in self.vectors):
            raise ValueError("feature dimensions do not match")
        with np.errstate(over="ignore"):
            matrix = np.asarray(self.vectors, dtype=np.float32)
        if not np.isfinite(matrix).all():
            raise ValueError("all features must be finite float32 values")
        return self


def authorize(authorization: Annotated[str | None, Header()] = None):
    expected = os.environ.get("TRAINER_TOKEN")
    if not expected:
        raise HTTPException(503, "trainer credential is not configured")
    if not hmac.compare_digest(authorization or "", "Bearer " + expected):
        raise HTTPException(401, "service authentication required")


@app.get("/health")
def health():
    return {"status": "ok"}


def train_model(request: TrainingRequest):
    matrix = np.asarray(request.vectors, dtype=np.float32)
    split = int(len(matrix) * 0.8)
    training, held_out = matrix[:split], matrix[split:]
    parameters = request.parameters
    forest = IsolationForest(
        n_estimators=parameters.n_estimators,
        max_samples=min(parameters.max_samples, len(training)),
        max_features=1.0,
        contamination="auto",
        bootstrap=False,
        random_state=parameters.seed,
        n_jobs=1,
    ).fit(training)
    trees = []
    for estimator in forest.estimators_:
        tree = estimator.tree_
        trees.append([
            {"left": int(tree.children_left[i]), "right": int(tree.children_right[i]),
             "feature": int(tree.feature[i]), "threshold": float(tree.threshold[i]),
             "samples": int(tree.n_node_samples[i])}
            for i in range(tree.node_count)
        ])
    scores = -forest.score_samples(held_out)
    counts, edges = np.histogram(scores, bins=20, range=(0, 1))
    # Include split boundary probes to catch float32 conversion and <= discrepancies.
    probes = [row.copy() for row in held_out[:64]]
    base = training[0].copy()
    for tree in trees[:8]:
        for node in tree:
            if node["left"] >= 0:
                value = np.float32(node["threshold"])
                for direction in (np.float32(-np.inf), np.float32(np.inf)):
                    probe = base.copy()
                    probe[node["feature"]] = np.nextafter(value, direction)
                    probes.append(probe)
                break
    probes = np.asarray(probes, dtype=np.float32)
    return {
        "model": {
            "format_version": 1, "id": str(request.job_id),
            "pipeline_version_id": str(request.pipeline_version_id),
            "schema_hash": request.schema_hash, "feature_names": request.feature_names,
            "max_samples": int(forest.max_samples_), "trees": trees,
        },
        "evaluation": {
            "training_rows": len(training), "held_out_rows": len(held_out),
            "split": "chronological_80_20", "sklearn_version": sklearn.__version__,
            "parameters": parameters.model_dump(),
            "suggested_check": float(np.quantile(scores, 0.95)),
            "suggested_block": float(np.quantile(scores, 0.99)),
            "minimum": float(scores.min()), "maximum": float(scores.max()),
            "histogram": {"counts": counts.tolist(), "edges": edges.tolist()},
            "notice": "Anomaly scores are not attack probabilities. Unlabeled evaluation cannot measure detection accuracy. Suggested thresholds need administrator review; equal thresholds must be adjusted.",
        },
        "parity_vectors": probes.astype(float).tolist(),
        "parity_scores": (-forest.score_samples(probes)).tolist(),
    }


@app.post("/v1/train", dependencies=[Depends(authorize)])
def train(request: TrainingRequest):
    if not training_slot.acquire(blocking=False):
        raise HTTPException(503, "trainer is busy; retry later")
    try:
        return train_model(request)
    finally:
        training_slot.release()
