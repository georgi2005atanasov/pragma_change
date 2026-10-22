"use client";
import { useWorkspace } from "../workspace";
import { Card, Empty, Badge } from "../ui";

export default function EndpointsSection() {
  const { submitResource, busy, rows, changeConfig, setSection } =
    useWorkspace();
  return (
    <>
      <div className="two-col">
        <Card title="Register an API">
          <form onSubmit={(e) => submitResource(e, "apis")}>
            <label>
              Name
              <input
                name="name"
                placeholder="Payments API"
                required
                maxLength={128}
              />
            </label>
            <label>
              API host
              <input name="host" placeholder="api.example.test" required />
              <small>DNS host only, without a scheme or port.</small>
            </label>
            <button disabled={busy}>Create API</button>
          </form>
        </Card>
        <Card title="Add an endpoint">
          <form onSubmit={(e) => submitResource(e, "endpoints")}>
            <label>
              API
              <select name="api_id" required>
                <option value="">Choose an API…</option>
                {rows("apis").map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name} · {a.host}
                  </option>
                ))}
              </select>
            </label>
            <div className="form-grid">
              <label>
                Name
                <input name="name" placeholder="Create payment" required />
              </label>
              <label>
                Method
                <select name="method">
                  {[
                    "GET",
                    "POST",
                    "PUT",
                    "PATCH",
                    "DELETE",
                    "HEAD",
                    "OPTIONS",
                  ].map((m) => (
                    <option key={m}>{m}</option>
                  ))}
                </select>
              </label>
            </div>
            <label>
              Path template
              <input
                name="path_template"
                placeholder="/payments/{payment_id}"
                required
              />
            </label>
            <button disabled={busy || !rows("apis").length}>
              Create endpoint
            </button>
          </form>
        </Card>
      </div>
      <Card title="API endpoints">
        {!rows("endpoints").length ? (
          <Empty>No endpoints yet. Register your API to get started.</Empty>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Method</th>
                  <th>Path</th>
                  <th>API</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows("endpoints").map((e) => (
                  <tr key={e.id}>
                    <td>{e.name}</td>
                    <td>
                      <Badge>{e.method}</Badge>
                    </td>
                    <td>
                      <code>{e.path_template}</code>
                    </td>
                    <td>{rows("apis").find((a) => a.id === e.api_id)?.name}</td>
                    <td>
                      <button
                        className="small secondary"
                        onClick={() => {
                          changeConfig({
                            endpoint_id: e.id,
                            additional_endpoints: [],
                            name: "",
                          });
                          setSection(2);
                        }}
                      >
                        Configure →
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </>
  );
}
