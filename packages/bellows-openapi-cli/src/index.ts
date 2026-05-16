/**
 * `@anvil-di/bellows-openapi-cli` — npm launcher for the native
 * `anvil-bellows-openapi` Rust binary.
 *
 * Resolution order:
 * 1. `ANVIL_BELLOWS_OPENAPI_CLI_BIN` env var — for monorepo dev and CI.
 * 2. The matching per-platform npm package.
 */
import { existsSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

const PLATFORM_SUFFIXES: Record<string, Record<string, string>> = {
  darwin: { arm64: "darwin-arm64", x64: "darwin-x64" },
  linux: { arm64: "linux-arm64", x64: "linux-x64" },
  win32: { x64: "win32-x64" },
};

export const BIN_NAME =
  process.platform === "win32"
    ? "anvil-bellows-openapi.exe"
    : "anvil-bellows-openapi";

export function platformPackageName(): string | null {
  const arches = PLATFORM_SUFFIXES[process.platform];
  if (arches === undefined) return null;
  const suffix = arches[process.arch];
  if (suffix === undefined) return null;
  return `@anvil-di/bellows-openapi-cli-${suffix}`;
}

export function resolveBinaryPath(): string | null {
  const override = process.env["ANVIL_BELLOWS_OPENAPI_CLI_BIN"];
  if (override !== undefined && existsSync(override)) {
    return override;
  }

  const pkg = platformPackageName();
  if (pkg === null) return null;

  let installed: { binPath?: string };
  try {
    installed = require(pkg) as { binPath?: string };
  } catch {
    return null;
  }
  if (typeof installed.binPath !== "string") return null;
  if (!existsSync(installed.binPath)) return null;
  return installed.binPath;
}

export function unresolvableBinaryError(): string {
  const pkg = platformPackageName();
  const platformInfo = `${process.platform}/${process.arch}`;
  if (pkg === null) {
    return [
      `@anvil-di/bellows-openapi-cli: no prebuilt binary for ${platformInfo}.`,
      "",
      "Supported platforms: darwin/arm64, darwin/x64, linux/arm64, linux/x64, win32/x64.",
      "",
      "Build from source: cargo build --release -p anvil-bellows-openapi",
      "then set ANVIL_BELLOWS_OPENAPI_CLI_BIN=/path/to/anvil-bellows-openapi.",
    ].join("\n");
  }
  return [
    `@anvil-di/bellows-openapi-cli: couldn't locate the ${BIN_NAME} binary for ${platformInfo}.`,
    "",
    `Expected "${pkg}" to be installed alongside @anvil-di/bellows-openapi-cli.`,
    "If npm skipped optional dependencies:",
    "  npm install --include=optional",
    "  pnpm install",
    "",
    "Or set ANVIL_BELLOWS_OPENAPI_CLI_BIN to a binary built from source.",
  ].join("\n");
}
