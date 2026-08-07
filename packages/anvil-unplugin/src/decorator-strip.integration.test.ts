import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { rolldown } from "rolldown";
import { stripDecoratorsUnplugin } from "./decorator-strip.js";

/**
 * The end-to-end claim: a Rolldown bundle of decorated anvil source RUNS.
 *
 * A unit test on the transform cannot show this. oxc's decorator support is
 * legacy-only, so without the plugin the decorators reach the output
 * verbatim and no JS engine will parse them — which is exactly the failure
 * this transform exists to prevent, and exactly what the negative case
 * below pins.
 *
 * The bundle is executed by spawning a real `node`, not by `import()`ing it
 * from the test. That is both a more faithful check of "this ships and
 * runs" and the only portable one: vitest resolves a dynamic import through
 * Vite's module loader, and on Windows the runner's temp directory is an
 * 8.3 short path (`C:\Users\RUNNER~1\...`) whose `~` `pathToFileURL`
 * percent-encodes to `%7E` and the loader never decodes.
 */

let dir: string;

/** Decorator stubs shaped like anvil's: identity, no runtime effect. */
const STUBS = `export function Controller(_p: string) {
  return <T extends abstract new (...a: never[]) => unknown>(t: T, _c: ClassDecoratorContext<T>): T => t;
}
export function Get(_p: string) {
  return <This, A extends readonly unknown[], R>(
    t: (this: This, ...a: A) => R,
    _c: ClassMethodDecoratorContext<This, (this: This, ...a: A) => R>,
  ) => t;
}
export function Inject<T extends abstract new (...a: never[]) => unknown>(
  t: T,
  _c: ClassDecoratorContext<T>,
): T {
  return t;
}
`;

/**
 * Self-executing on purpose: the bundle's own top-level statement is what
 * proves the decorated classes survived and still behave, so `node
 * index.mjs` is the whole assertion.
 */
const SOURCE = `import { Controller, Get, Inject } from "@anvil-di/bellows";

@Inject
export class Greeter {
  greet(name: string): string {
    return "hello " + name;
  }
}

@Controller("/calls")
export class CallsController {
  constructor(private readonly greeter: Greeter) {}

  @Get("/:id")
  findOne(id: string): string {
    return this.greeter.greet(id);
  }
}

console.log(new CallsController(new Greeter()).findOne("world"));
`;

beforeAll(() => {
  dir = mkdtempSync(path.join(tmpdir(), "anvil-strip-"));
  mkdirSync(path.join(dir, "src"), { recursive: true });
  writeFileSync(path.join(dir, "src", "stubs.ts"), STUBS, "utf8");
  writeFileSync(path.join(dir, "src", "index.ts"), SOURCE, "utf8");
});

afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

/**
 * Bundle `src/index.ts`, resolving the bare `@anvil-di/bellows` specifier to
 * the local stub file so the fixture needs no installed dependency.
 *
 * Emitted as `.mjs` so Node reads it as ESM without a package.json marker.
 */
async function bundle(withPlugin: boolean, outName: string): Promise<string> {
  const aliasStubs = {
    name: "alias-stubs",
    resolveId(source: string) {
      return source === "@anvil-di/bellows" ? path.join(dir, "src", "stubs.ts") : null;
    },
  };
  const plugins = withPlugin ? [aliasStubs, stripDecoratorsUnplugin.rolldown()] : [aliasStubs];
  const build = await rolldown({
    input: path.join(dir, "src", "index.ts"),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    plugins: plugins as any,
    logLevel: "silent",
  });
  const outDir = path.join(dir, outName);
  await build.write({ dir: outDir, format: "esm", entryFileNames: "index.mjs" });
  await build.close();
  return path.join(outDir, "index.mjs");
}

/** Execute a bundle with the same Node that is running the tests. */
function runInNode(entry: string): { status: number | null; stdout: string; stderr: string } {
  const result = spawnSync(process.execPath, [entry], { encoding: "utf8" });
  return { status: result.status, stdout: result.stdout, stderr: result.stderr };
}

describe("stripDecoratorsUnplugin (rolldown, end to end)", () => {
  it("produces a bundle that node runs", async () => {
    const out = await bundle(true, "dist-stripped");
    const { status, stdout, stderr } = runInNode(out);
    // stderr rides along as the failure message rather than as its own
    // assertion, so an unrelated node warning cannot fail the test but a
    // real crash still shows its stack.
    expect(status, stderr).toBe(0);
    expect(stdout.trim()).toBe("hello world");
  });

  it("emits no decorator syntax and no decorator helpers", async () => {
    const out = await bundle(true, "dist-clean");
    const code = readFileSync(out, "utf8");
    expect(code).not.toMatch(/(?:^|[\s{;(])@[A-Za-z_$]/m);
    expect(code).not.toContain("__esDecorate");
    expect(code).not.toContain("__runInitializers");
    expect(code).not.toContain("__decorate");
    // The stubs are dead once nothing references them.
    expect(code).not.toContain("function Controller");
  });

  it("WITHOUT the plugin, node cannot even parse the same bundle", async () => {
    const out = await bundle(false, "dist-raw");
    const { status, stderr } = runInNode(out);
    expect(status).not.toBe(0);
    expect(stderr).toMatch(/SyntaxError/);
  });
});
