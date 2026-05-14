/**
 * `@anvil-di/anvil-cli` — npm launcher for the native `anvil` Rust binary.
 *
 * The package itself ships **no binary**. At install time, npm
 * resolves exactly one of the per-platform `optionalDependencies`
 * (e.g. `@anvil-di/anvil-cli-linux-x64` or `@anvil-di/anvil-cli-win32-x64`)
 * based on the `os`/`cpu` fields each declares; only that one is fetched.
 * At runtime, `resolveBinaryPath()` dynamically requires the matching
 * platform package and reads the prebuilt binary path from it.
 *
 * This mirrors esbuild's distribution model — the user installs one
 * thing (`npm install @anvil-di/anvil-cli`) and gets the right native
 * binary for their platform without a `postinstall` build step.
 */
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);

/**
 * The platform-package suffix mapping. Matches the names declared
 * in this package's `optionalDependencies`.
 */
const PLATFORM_SUFFIXES: Record<string, Record<string, string>> = {
  darwin: { arm64: "darwin-arm64", x64: "darwin-x64" },
  linux: { arm64: "linux-arm64", x64: "linux-x64" },
  win32: { x64: "win32-x64" },
};

/** The base name of the binary inside each platform package. */
const BIN_NAME = process.platform === "win32" ? "anvil.exe" : "anvil";

/**
 * Resolve the platform-package name for the current Node process.
 * Returns `null` for unsupported (platform, arch) combinations.
 */
export function platformPackageName(): string | null {
  const arches = PLATFORM_SUFFIXES[process.platform];
  if (arches === undefined) return null;
  const suffix = arches[process.arch];
  if (suffix === undefined) return null;
  return `@anvil-di/anvil-cli-${suffix}`;
}

/**
 * Locate the native `anvil` binary on disk.
 *
 * Resolution order:
 *
 * 1. `ANVIL_CLI_BIN` env var — handy for tests, monorepo dev, and
 *    CI where the binary lives outside any npm package (e.g. a
 *    `cargo build` artifact).
 * 2. The matching per-platform npm package, located via
 *    `require.resolve()` and the `binPath` constant each one
 *    exports.
 *
 * Returns `null` if no binary can be found — callers then surface a
 * platform-specific error message to the user.
 */
export function resolveBinaryPath(): string | null {
  // 1. Explicit override.
  const override = process.env.ANVIL_CLI_BIN;
  if (override !== undefined && existsSync(override)) {
    return override;
  }

  // 2. Per-platform npm package.
  const pkg = platformPackageName();
  if (pkg === null) {
    return null;
  }
  let installed: { binPath?: string };
  try {
    installed = require(pkg) as { binPath?: string };
  } catch {
    // npm skipped this optional dep (because the platform doesn't
    // match) or the user hasn't installed deps yet. Fall through.
    return null;
  }
  if (typeof installed.binPath !== "string") return null;
  if (!existsSync(installed.binPath)) return null;
  return installed.binPath;
}

/**
 * Construct a human-readable error message for the case where no
 * binary could be resolved. Surfaces the active platform/arch so the
 * user can quickly tell whether they need a different prebuilt or
 * whether an `npm install --include=optional` is missing.
 */
export function unresolvableBinaryError(): string {
  const pkg = platformPackageName();
  const platformInfo = `${process.platform}/${process.arch}`;
  if (pkg === null) {
    return [
      `@anvil-di/anvil-cli: no prebuilt binary is available for ${platformInfo}.`,
      "",
      "Supported platforms in v0.2:",
      "  - darwin / arm64, darwin / x64",
      "  - linux  / arm64, linux  / x64",
      "  - win32  / x64",
      "",
      "If you need another platform, build the Rust crate from source:",
      "  git clone https://github.com/msulak13/anvil && cargo build --release -p anvil-cli",
      "and either set ANVIL_CLI_BIN=/path/to/anvil or place the binary on $PATH.",
    ].join("\n");
  }
  return [
    `@anvil-di/anvil-cli: couldn't locate the ${BIN_NAME} binary for ${platformInfo}.`,
    "",
    `Expected the npm package "${pkg}" to be installed alongside`,
    "@anvil-di/anvil-cli. If npm skipped optional dependencies, retry the install:",
    "  npm install --include=optional",
    "  pnpm install               # honors optional deps by default",
    "  yarn install               # honors optional deps by default",
    "",
    "Or set ANVIL_CLI_BIN to a binary you've built yourself.",
  ].join("\n");
}

/** Where this `@anvil-di/anvil-cli` package itself is installed (informational). */
export const PACKAGE_DIR = path.dirname(fileURLToPath(import.meta.url));

export { BIN_NAME };
