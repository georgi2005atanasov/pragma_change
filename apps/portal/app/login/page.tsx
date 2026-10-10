"use client";
import { Suspense, useState } from "react";
import { useSearchParams } from "next/navigation";
import { sectionHref, sectionIndex } from "../navigation";
import { Brand, WorkspaceLoading } from "../brand";
export default function Login() {
  return (
    <Suspense fallback={<WorkspaceLoading />}>
      <LoginForm />
    </Suspense>
  );
}
function LoginForm() {
  const searchParams = useSearchParams();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <main className="auth-page">
      <div className="auth-story">
        <Brand />
        <h1>
          Understand behavior.
          <br />
          <em>Protect every request.</em>
        </h1>
        <p>
          Design your signals, learn the baseline, and deploy decisions directly
          at the proxy.
        </p>
        <div className="flow">
          Request <span>→</span> Behavior <span>→</span> Decision
        </div>
      </div>
      <form
        className="auth-form"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError("");
          const data = new FormData(e.currentTarget);
          try {
            const r = await fetch("/api/login", {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({
                username: data.get("username"),
                password: data.get("password"),
              }),
            });
            const body = await r.json();
            if (!r.ok) throw Error(body.error || "Login failed");
            location.replace(
              sectionHref(sectionIndex(searchParams.get("section"))),
            );
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="auth-mobile-brand">
          <Brand />
        </div>
        <div className="eyebrow">WORKSPACE ACCESS</div>
        <h2>Welcome back</h2>
        <p>Sign in to your self-hosted workspace.</p>
        <label>
          Username
          <input
            name="username"
            autoComplete="username"
            required
            defaultValue="admin"
          />
        </label>
        <label>
          Password
          <input
            name="password"
            type="password"
            autoComplete="current-password"
            required
          />
        </label>
        {error && (
          <div role="alert" className="error">
            {error}
          </div>
        )}
        <button disabled={busy}>{busy ? "Signing in…" : "Sign in →"}</button>
        <small>
          Administrator credentials are configured during installation.
        </small>
      </form>
    </main>
  );
}
