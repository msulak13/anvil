/**
 * `@anvil-di/bellows-cli` — npm launcher for the native `anvil-bellows` Rust binary.
 *
 * The package itself ships **no binary**. At install time, npm resolves exactly
 * one of the per-platform `optionalDependencies` based on the `os`/`cpu` fields
 * each declares; only that one is fetched. At runtime, `resolveBinaryPath()`
 * dynamically requires the matching platform package and reads the prebuilt
 * binary path from it.
 *
 * This mirrors esbuild's distribution model — the user installs one thing
 * (`npm install @anvil-di/bellows-cli`) and gets the right native binary
 * for their platform without a `postinstall` build step.
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
  process.platform === "win32" ? "anvil-bellows.exe" : "anvil-bellows";

export function platformPackageName(): string | null {
  const arches = PLATFORM_SUFFIXES[process.platform];
  if (arches === undefined) return null;
  const suffix = arches[process.arch];
  if (suffix === undefined) return null;
  return `@anvil-di/bellows-cli-${suffix}`;
}

/**
 * Locate the native `anvil-bellows` binary on disk.
 *
 * Resolution order:
 * 1. `ANVIL_BELLOWS_CLI_BIN` env var — for monorepo dev and CI.
 * 2. The matching per-platform npm package.
 *
 * Returns `null` if no binary can be found.
 */
export function resolveBinaryPath(): string | null {
  const override = process.env["ANVIL_BELLOWS_CLI_BIN"];
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
      `@anvil-di/bellows-cli: no prebuilt binary for ${platformInfo}.`,
      "",
      "Supported platforms: darwin/arm64, darwin/x64, linux/arm64, linux/x64, win32/x64.",
      "",
      "Build from source: cargo build --release -p anvil-bellows",
      "then set ANVIL_BELLOWS_CLI_BIN=/path/to/anvil-bellows.",
    ].join("\n");
  }
  return [
    `@anvil-di/bellows-cli: couldn't locate the ${BIN_NAME} binary for ${platformInfo}.`,
    "",
    `Expected "${pkg}" to be installed alongside @anvil-di/bellows-cli.`,
    "If npm skipped optional dependencies:",
    "  npm install --include=optional",
    "  pnpm install",
    "",
    "Or set ANVIL_BELLOWS_CLI_BIN to a binary built from source.",
  ].join("\n");
}
