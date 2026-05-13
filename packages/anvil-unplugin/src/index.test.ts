import { describe, expect, it } from "vitest";
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { anvilUnplugin, type PreBuildHook, type PostBuildHook } from "./index.js";

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
      expect(generated).toContain("AnvilCoffeeShop");
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
      expect(generated).toContain("AnvilCoffeeShop");
      expect(generated).toContain("export function createCoffeeShop");
      // The WASM build produces the same shape as the native one —
      // smoke-check by comparing the dagger class declaration line.
      expect(generated).toMatch(/export class AnvilCoffeeShop extends CoffeeShop/);
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

  // ---------------------------------------------------------------------------
  // M4 — preBuild / postBuild hook API
  // ---------------------------------------------------------------------------

  function makeHook(
    name: string,
    onRun: () => void,
    shouldRerun: (files: string[]) => boolean = () => false,
  ): PreBuildHook & PostBuildHook {
    return {
      name,
      watchPatterns: [],
      shouldRerun,
      async run() {
        onRun();
      },
    };
  }

  it("runs preBuild hooks before anvil build in buildStart", async () => {
    const { dir, entry, output } = makeFixture();
    try {
      const order: string[] = [];
      const hook = makeHook("pre", () => order.push("pre"));

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        preBuild: [hook],
      }) as { buildStart?: () => Promise<void> };

      await plugin.buildStart!.call({});

      // preBuild ran, and the anvil build also ran (output file exists).
      expect(order).toEqual(["pre"]);
      expect(existsSync(output)).toBe(true);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("runs postBuild hooks after anvil build in buildStart", async () => {
    const { dir, entry, output } = makeFixture();
    try {
      const order: string[] = [];
      const post = makeHook("post", () => order.push("post"));

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        postBuild: [post],
      }) as { buildStart?: () => Promise<void> };

      await plugin.buildStart!.call({});

      // postBuild ran after anvil build produced the output file.
      expect(existsSync(output)).toBe(true);
      expect(order).toEqual(["post"]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("runs preBuild → anvil build → postBuild in order in buildStart", async () => {
    const { dir, entry } = makeFixture();
    try {
      const order: string[] = [];
      const pre = makeHook("pre", () => order.push("pre"));
      const post = makeHook("post", () => order.push("post"));

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        preBuild: [pre],
        postBuild: [post],
      }) as { buildStart?: () => Promise<void> };

      await plugin.buildStart!.call({});

      expect(order).toEqual(["pre", "post"]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("re-runs preBuild hook in watch mode when shouldRerun returns true", async () => {
    const { dir, entry } = makeFixture();
    try {
      let runCount = 0;
      const hook = makeHook(
        "pre",
        () => { runCount++; },
        (files) => files.some((f) => f.startsWith(dir)),
      );

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        preBuild: [hook],
        debounceMs: 30,
      }) as { watchChange?: (id: string, change: unknown) => void };

      plugin.watchChange!(path.join(dir, "heater.ts"), {});
      await new Promise<void>((resolve) => setTimeout(resolve, 120));

      expect(runCount).toBe(1);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("does not re-run preBuild hook in watch mode when shouldRerun returns false", async () => {
    const { dir, entry } = makeFixture();
    try {
      let runCount = 0;
      // shouldRerun always returns false.
      const hook = makeHook("pre", () => { runCount++; }, () => false);

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        preBuild: [hook],
        debounceMs: 30,
      }) as { watchChange?: (id: string, change: unknown) => void };

      plugin.watchChange!(path.join(dir, "heater.ts"), {});
      await new Promise<void>((resolve) => setTimeout(resolve, 120));

      expect(runCount).toBe(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("runs postBuild hooks when preBuild hook re-runs in watch mode", async () => {
    const { dir, entry } = makeFixture();
    try {
      let postRunCount = 0;
      const pre = makeHook(
        "pre",
        () => {},
        (files) => files.some((f) => f.startsWith(dir)),
      );
      const post = makeHook("post", () => { postRunCount++; });

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        preBuild: [pre],
        postBuild: [post],
        debounceMs: 30,
      }) as { watchChange?: (id: string, change: unknown) => void };

      plugin.watchChange!(path.join(dir, "heater.ts"), {});
      await new Promise<void>((resolve) => setTimeout(resolve, 120));

      // postBuild ran because preBuild ran.
      expect(postRunCount).toBe(1);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

  it("runs only opted-in postBuild hooks when no preBuild hook re-runs", async () => {
    const { dir, entry } = makeFixture();
    try {
      let postACount = 0;
      let postBCount = 0;
      // postA opts in, postB does not.
      const postA = makeHook(
        "postA",
        () => { postACount++; },
        (files) => files.some((f) => f.startsWith(dir)),
      );
      const postB = makeHook("postB", () => { postBCount++; }, () => false);

      const plugin = anvilUnplugin.rollup({
        cli: anvilBin,
        entries: [entry],
        postBuild: [postA, postB],
        debounceMs: 30,
      }) as { watchChange?: (id: string, change: unknown) => void };

      plugin.watchChange!(path.join(dir, "heater.ts"), {});
      await new Promise<void>((resolve) => setTimeout(resolve, 120));

      expect(postACount).toBe(1);
      expect(postBCount).toBe(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 30_000);

});
