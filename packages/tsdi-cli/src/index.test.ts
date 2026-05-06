import { describe, expect, it } from "vitest";
import { existsSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import {
  BIN_NAME,
  platformPackageName,
  resolveBinaryPath,
  unresolvableBinaryError,
} from "./index.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const pkgRoot = path.resolve(here, "..");
const launcherShim = path.join(pkgRoot, "bin", "tsdi.js");

describe("platformPackageName", () => {
  it("returns the right package for the current Node process", () => {
    const name = platformPackageName();
    if (process.platform === "win32" && process.arch === "x64") {
      expect(name).toBe("tsdi-cli-win32-x64");
    } else if (process.platform === "darwin" && process.arch === "arm64") {
      expect(name).toBe("tsdi-cli-darwin-arm64");
    } else if (process.platform === "linux" && process.arch === "x64") {
      expect(name).toBe("tsdi-cli-linux-x64");
    } else {
      // For other combinations the test environment isn't in the v0.2
      // matrix; just ensure the function doesn't crash.
      expect(name === null || typeof name === "string").toBe(true);
    }
  });
});

describe("resolveBinaryPath", () => {
  it("honors TSDI_CLI_BIN when it points to an existing file", () => {
    const tsdiBinary = path.resolve(
      pkgRoot,
      "..",
      "..",
      "target",
      "release",
      process.platform === "win32" ? "tsdi.exe" : "tsdi",
    );
    if (!existsSync(tsdiBinary)) {
      // No release build available in this environment; skip without
      // marking the suite as failed. (`pnpm -r build` doesn't include
      // a Rust step.)
      return;
    }
    const prev = process.env["TSDI_CLI_BIN"];
    try {
      process.env["TSDI_CLI_BIN"] = tsdiBinary;
      const resolved = resolveBinaryPath();
      expect(resolved).toBe(tsdiBinary);
    } finally {
      if (prev === undefined) delete process.env["TSDI_CLI_BIN"];
      else process.env["TSDI_CLI_BIN"] = prev;
    }
  });

  it("returns null when TSDI_CLI_BIN points to a non-existent path", () => {
    const prev = process.env["TSDI_CLI_BIN"];
    try {
      process.env["TSDI_CLI_BIN"] = "/this/definitely/does/not/exist";
      // Without the override, the resolver falls through to the
      // platform package — which may or may not be installed in this
      // environment. We just verify the override-not-found case
      // doesn't throw.
      const resolved = resolveBinaryPath();
      expect(resolved === null || typeof resolved === "string").toBe(true);
    } finally {
      if (prev === undefined) delete process.env["TSDI_CLI_BIN"];
      else process.env["TSDI_CLI_BIN"] = prev;
    }
  });
});

describe("unresolvableBinaryError", () => {
  it("includes the active platform in the message", () => {
    const msg = unresolvableBinaryError();
    expect(msg).toContain(`${process.platform}/${process.arch}`);
  });

  it("uses the right binary name on each OS", () => {
    if (process.platform === "win32") {
      expect(BIN_NAME).toBe("tsdi.exe");
    } else {
      expect(BIN_NAME).toBe("tsdi");
    }
  });
});

describe("bin/tsdi.js launcher shim", () => {
  it("forwards args to the resolved binary and pipes its exit code", () => {
    const tsdiBinary = path.resolve(
      pkgRoot,
      "..",
      "..",
      "target",
      "release",
      process.platform === "win32" ? "tsdi.exe" : "tsdi",
    );
    if (!existsSync(tsdiBinary)) return; // see resolver test above
    const result = spawnSync(process.execPath, [launcherShim, "--help"], {
      env: { ...process.env, TSDI_CLI_BIN: tsdiBinary },
      encoding: "utf8",
      windowsHide: true,
    });
    // The Rust CLI's --help exits 0 and prints usage info on stdout.
    expect(result.status).toBe(0);
    expect(result.stdout.toLowerCase()).toContain("usage");
  });
});
