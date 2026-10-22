import math
from uuid import uuid4
import numpy as np
import pytest
from fastapi.testclient import TestClient
from pydantic import ValidationError
from app import TrainingRequest, app, train_model


def request():
    return TrainingRequest(job_id=uuid4(), pipeline_version_id=uuid4(), schema_hash="a" * 64,
                           feature_names=["count", "mean"], vectors=np.random.default_rng(42).normal(size=(200, 2)).tolist(),
                           parameters={"n_estimators": 20, "max_samples": 64})


def test_reproducible_data_only_export():
    r = request()
    a, b = train_model(r), train_model(r)
    assert a == b
    assert a["model"]["id"] == str(r.job_id)
    assert all(0 <= s <= 1 for s in a["parity_scores"])
    assert a["evaluation"]["held_out_rows"] == 40
    assert len(a["model"]["trees"]) == 20


def test_rejects_dimensions_and_nonfinite():
    raw = request().model_dump()
    raw["vectors"][0] = [1]
    with pytest.raises(ValidationError):
        TrainingRequest(**raw)
    raw = request().model_dump()
    raw["vectors"][0][0] = math.inf
    with pytest.raises(ValidationError):
        TrainingRequest(**raw)


def test_service_auth(monkeypatch):
    monkeypatch.setenv("TRAINER_TOKEN", "service-test-token")
    with TestClient(app) as client:
        assert client.post("/v1/train", json=request().model_dump(mode="json")).status_code == 401
        assert client.post("/v1/train", headers={"Authorization": "Bearer service-test-token"}, json=request().model_dump(mode="json")).status_code == 200
