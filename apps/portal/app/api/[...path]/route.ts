import { NextRequest, NextResponse } from "next/server";
export const runtime = "nodejs";
const base = process.env.API_URL || "http://127.0.0.1:8080";
const origin = process.env.PUBLIC_ORIGIN || "http://localhost:3000";
async function proxy(
  request: NextRequest,
  context: { params: Promise<{ path: string[] }> },
) {
  const { path } = await context.params;
  if (path.some((p) => !/^[a-zA-Z0-9_.-]+$/.test(p)))
    return NextResponse.json({ error: "Invalid path" }, { status: 400 });
  const mutation = !["GET", "HEAD"].includes(request.method);
  if (mutation && request.headers.get("origin") !== origin)
    return NextResponse.json(
      { error: "Invalid request origin" },
      { status: 403 },
    );
  const headers = new Headers();
  headers.set("content-type", "application/json");
  const session = request.cookies.get("pragma_session")?.value;
  if (session) headers.set("authorization", `Bearer ${session}`);
  const csrf = request.headers.get("x-csrf-token");
  if (csrf) headers.set("x-csrf-token", csrf);
  if (Number(request.headers.get("content-length") || 0) > 16 * 1024 * 1024)
    return NextResponse.json(
      { error: "Upload limit is 16 MiB" },
      { status: 413 },
    );
  try {
    const body = mutation ? await request.text() : undefined;
    if (body && Buffer.byteLength(body) > 16 * 1024 * 1024)
      return NextResponse.json(
        { error: "Upload limit is 16 MiB" },
        { status: 413 },
      );
    const response = await fetch(
      `${base}/api/v1/${path.join("/")}${request.nextUrl.search}`,
      {
        method: request.method,
        headers,
        body,
        cache: "no-store",
        signal: AbortSignal.timeout(60000),
      },
    );
    if (path[0] === "login" && response.ok) {
      const auth = await response.json();
      const result = NextResponse.json({ ok: true });
      const cookie = {
        secure: process.env.COOKIE_SECURE !== "false",
        sameSite: "strict" as const,
        path: "/",
        maxAge: auth.expires_in,
      };
      result.cookies.set("pragma_session", auth.token, {
        ...cookie,
        httpOnly: true,
      });
      result.cookies.set("pragma_csrf", auth.csrf_token, {
        ...cookie,
        httpOnly: false,
      });
      return result;
    }
    const result = new NextResponse(response.body, {
      status: response.status,
      headers: {
        "content-type":
          response.headers.get("content-type") || "application/json",
        "cache-control": "no-store",
      },
    });
    const disposition = response.headers.get("content-disposition");
    if (disposition) result.headers.set("content-disposition", disposition);
    if (path[0] === "logout" || response.status === 401) {
      result.cookies.delete("pragma_session");
      result.cookies.delete("pragma_csrf");
    }
    return result;
  } catch {
    return NextResponse.json(
      {
        error:
          "Management API unavailable. Check service health and try again.",
      },
      { status: 502 },
    );
  }
}
export { proxy as GET, proxy as POST };
