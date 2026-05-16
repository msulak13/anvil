#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { resolveBinaryPath, unresolvableBinaryError } from "../dist/index.js";

const binary = resolveBinaryPath();
if (binary === null) {
  process.stderr.write(unresolvableBinaryError() + "\n");
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
  windowsHide: true,
});

if (result.error !== undefined) {
  process.stderr.write(
    `@anvil-di/bellows-cli: failed to spawn ${binary}: ${result.error.message}\n`,
  );
  process.exit(1);
}
process.exit(result.status ?? 1);
