import { cpSync, existsSync } from "node:fs";
import { spawn } from "node:child_process";
const dist = process.env.NEXT_DIST_DIR || ".next";
if (!existsSync(`${dist}/standalone/server.js`)) {
  throw new Error("Run pnpm build before starting the production portal.");
}
cpSync(`${dist}/static`, `${dist}/standalone/${dist}/static`, {
  recursive: true,
});
const server = spawn(process.execPath, [`${dist}/standalone/server.js`], {
  stdio: "inherit",
  env: { ...process.env, HOSTNAME: process.env.BIND_HOST || "127.0.0.1" },
});
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.kill(signal));
}
server.on("exit", (code) => process.exit(code ?? 0));
