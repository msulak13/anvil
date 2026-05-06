#!/usr/bin/env node
// Thin launcher. Resolves the per-platform binary via the same
// logic the programmatic `resolveBinaryPath()` uses, then `spawnSync`s
// it with the user's argv pass-through.

import { spawnSync } from "node:child_process";
import { resolveBinaryPath, unresolvableBinaryError } from "../dist/index.js";

const binary = resolveBinaryPath();
if (binary === null) {
  process.stderr.write(unresolvableBinaryError() + "\n");
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  // Forward all env vars (the Rust CLI consults TSDI_WATCH_ITERATIONS, etc.)
  env: process.env,
  windowsHide: true,
});

if (result.error !== undefined) {
  process.stderr.write(`tsdi-cli: failed to spawn ${binary}: ${result.error.message}\n`);
  process.exit(1);
}
process.exit(result.status ?? 1);
