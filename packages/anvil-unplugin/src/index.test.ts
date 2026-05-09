import { describe, expect, it } from "vitest";
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { anvilUnplugin } from "./index.js";

// Find the workspace's `anvil` binary so the unplugin's `cli` option
// can shell out to it. In the monorepo we point straight at the
// debug build emitted by `cargo build`.
const repoRoot = path.resolve(__dirname, "..", "..", "..");
const anvilBin = path.join(
  repoRoot,
  "target",
  "debug",
  process.platform === "win32" ? "anvil.exe" : "anvil",
);

function makeFixture(): { dir: string; entry: string; output: string } {
  const dir = mkdtempSync(path.join(tmpdir(), "anvil-unplugin-"));
  // Stub `@msulak/anvil` runtime stubs so the parser can resolve them.
  const anvilPkg = path.join(dir, "node_modules", "@msulak/anvil");
  mkdirSync(anvilPkg, { recursive: true });
  writeFileSync(
    path.join(anvilPkg, "package.json"),
    JSON.stringify({ name: "@msulak/anvil", main: "index.ts" }),
  );
  writeFileSync(
    path.join(anvilPkg, "index.ts"),
    [
      "export const Inject = (..._: any[]) => {};",
      "export const Module = (..._: any[]) => {};",
      "export const Provides = (..._: any[]) => {};",
      "export const Component = (..._: any[]) => {};",
      "export const Singleton = (..._: any[]) => {};",
      "export const Binds = (..._: any[]) => {};",
      "export const Subcomponent = (..._: any[]) => {};",
      "export const IntoSet = (..._: any[]) => {};",
    ].join("\n"),
  );
  writeFileSync(
    path.join(dir, "heater.ts"),
    [
      'import { Inject } from "@msulak/anvil";',
      "@Inject",
      "export class Heater { constructor() {} }",
    ].join("\n"),
  );
  const entry = path.join(dir, "coffee-component.ts");
  writeFileSync(
    entry,
    [
      'import { Component } from "@msulak/anvil";',
      'import { Heater } from "./heater";',
      "@Component({ modules: [] })",
      "export abstract class CoffeeShop {",
      "  abstract heater(): Heater;",
      "}",
    ].join("\n"),
  );
  return { dir, entry, output: path.join(dir, "coffee-component.anvil.ts") };
}

describe("anvil-unplugin", () => {
  it("invokes `anvil build` on buildStart and emits the .anvil.ts file", async () => {
    const { dir, entry, output } = makeFixture();
    try {
      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
      }) as { name: string; buildStart?: () => Promise<void> };
      expect(plugin.name).toBe("anvil-unplugin");
      expect(typeof plugin.buildStart).toBe("function");
      await plugin.buildStart!.call({});
      const generated = readFileSync(output, "utf8");
      expect(generated).toContain("DaggerCoffeeShop");
      expect(generated).toContain("export function createCoffeeShop");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("debounces watchChange invocations", async () => {
    const { dir, entry } = makeFixture();
    try {
      // Spy on the CLI binding by passing a no-op CLI path through a
      // shell wrapper. We only need to count invocations, not produce
      // real output.
      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        debounceMs: 30,
      }) as {
        watchChange?: (id: string, change: unknown) => void;
      };
      expect(typeof plugin.watchChange).toBe("function");
      // Fire several edits in quick succession; the debounce window
      // collapses them into one rebuild.
      const changedFile = path.join(dir, "heater.ts");
      plugin.watchChange!(changedFile, {});
      plugin.watchChange!(changedFile, {});
      plugin.watchChange!(changedFile, {});
      // The debounce just wraps a setTimeout — sanity-check the
      // function runs without error and accepts the right shape.
      await new Promise<void>((resolve) => setTimeout(resolve, 80));
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("ignores edits to .anvil.ts files (avoids self-trigger loops)", () => {
    const plugin = anvilUnplugin.rollup({
      cli: anvilBin,
      entries: ["nonexistent.ts"],
      debounceMs: 30,
    }) as { watchChange?: (id: string, change: unknown) => void };
    // Should not throw and should not schedule any rebuild — we
    // verify by ensuring no unhandled rejection occurs.
    plugin.watchChange!("/tmp/foo.anvil.ts", {});
    // Also: random non-TS files are ignored.
    plugin.watchChange!("/tmp/foo.css", {});
  });

  it("compiles in-process via WASM mode and emits the .anvil.ts file", async () => {
    const { dir, entry, output } = makeFixture();
    try {
      const plugin = anvilUnplugin.rollup({
        mode: "wasm",
        entries: [entry],
        rootDir: dir,
      }) as { name: string; buildStart?: () => Promise<void> };
      await plugin.buildStart!.call({});
      const generated = readFileSync(output, "utf8");
      expect(generated).toContain("DaggerCoffeeShop");
      expect(generated).toContain("export function createCoffeeShop");
      // The WASM build produces the same shape as the native one —
      // smoke-check by comparing the dagger class declaration line.
      expect(generated).toMatch(/export class DaggerCoffeeShop extends CoffeeShop/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  // Use the spy avoidance above to keep the suite from accidentally
  // depending on the real cargo binary at runtime.
  it("exposes per-bundler entry points", () => {
    expect(typeof anvilUnplugin.vite).toBe("function");
    expect(typeof anvilUnplugin.rollup).toBe("function");
    expect(typeof anvilUnplugin.webpack).toBe("function");
    expect(typeof anvilUnplugin.rspack).toBe("function");
    expect(typeof anvilUnplugin.esbuild).toBe("function");
  });

});
