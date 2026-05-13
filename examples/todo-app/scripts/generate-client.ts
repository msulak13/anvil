import { execSync, spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

const server = spawn("tsx", ["server/src/spec-server.ts"], { stdio: "inherit" });

server.on("error", (e) => {
  console.error("Failed to start spec server:", e);
  process.exit(1);
});

await sleep(1500);

try {
  execSync("openapi-ts", { stdio: "inherit" });
} finally {
  server.kill();
}
