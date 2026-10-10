"use client";
import { useWorkspace } from "../workspace";
import { Card, Badge } from "../ui";

export default function OverviewSection() {
  const { counts, setSection } = useWorkspace();
  return (
    <>
      <div className="stats">
        {[
          ["Configured endpoints", counts.endpoints],
          ["Feature pipelines", counts.pipelines],
          ["Trained models", counts.models],
          ["Release bundles", counts.releases],
        ].map(([name, n]) => (
          <div className="stat" key={name}>
            <span>{name}</span>
            <strong>{n || 0}</strong>
            <small>
              {name === "Release bundles"
                ? "Available for deployment"
                : "In this workspace"}
            </small>
          </div>
        ))}
      </div>
      <div className="hero-card">
        <div>
          <Badge>YOUR WORKFLOW</Badge>
          <h2>
            From requests to
            <br />
            confident decisions.
          </h2>
          <p>
            Define how to group traffic, learn its normal behavior, and export a
            model that runs directly in Nginx.
          </p>
          <button onClick={() => setSection(1)}>
            Configure an endpoint <span>→</span>
          </button>
        </div>
        <div className="pipeline-visual">
          <div>
            01 <strong>Partition</strong>
            <small>Group related requests</small>
          </div>
          <i>↓</i>
          <div>
            02 <strong>Observe</strong>
            <small>Build windows and features</small>
          </div>
          <i>↓</i>
          <div>
            03 <strong>Decide</strong>
            <small>
              <b className="allow-text">Allow</b> · Check · Block
            </small>
          </div>
        </div>
      </div>
      <div className="two-col">
        <Card title="Start with a baseline">
          <ol className="steps">
            <li>
              <b>Define your endpoint</b>
              <span>
                Choose fields, partitions, windows, and feature order.
              </span>
            </li>
            <li>
              <b>Import representative data</b>
              <span>Replay requests or upload matching feature vectors.</span>
            </li>
            <li>
              <b>Train, review, and export</b>
              <span>Test scores and set thresholds before deployment.</span>
            </li>
          </ol>
        </Card>
        <Card title="Understanding the score">
          <p>
            An Isolation Forest measures how unusual a feature vector is. A
            higher score means a stronger anomaly.
          </p>
          <div className="risk-scale">
            <span>Allow</span>
            <span>Check</span>
            <span>Block</span>
          </div>
          <p className="muted">
            It is not an attack probability. The backend receives trusted risk
            headers for allow and check decisions; block requests stop at Nginx.
          </p>
        </Card>
      </div>
    </>
  );
}
