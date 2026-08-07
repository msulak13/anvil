import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { rolldown } from "rolldown";
import { stripDecoratorsUnplugin } from "./plugin.js";

/**
 * The end-to-end claim: a Rolldown bundle of decorated anvil source RUNS.
 *
 * A unit test on the transform cannot show this. oxc's decorator support is
 * legacy-only, so without the plugin the decorators reach the output
 * verbatim and no JS engine will parse them — which is exactly the failure
 * this package exists to prevent, and exactly what the negative case below
 * pins.
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
 */
async function bundle(withPlugin: boolean, outName: string): Promise<string> {
  const aliasStubs = {
    name: "alias-stubs",
    resolveId(source: string) {
      return source === "@anvil-di/bellows" ? path.join(dir, "src", "stubs.ts") : null;
    },
  };
  const plugins = withPlugin
    ? [aliasStubs, stripDecoratorsUnplugin.rolldown()]
    : [aliasStubs];
  const build = await rolldown({
    input: path.join(dir, "src", "index.ts"),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    plugins: plugins as any,
    logLevel: "silent",
  });
  const outDir = path.join(dir, outName);
  await build.write({ dir: outDir, format: "esm" });
  await build.close();
  return path.join(outDir, "index.js");
}

describe("stripDecoratorsUnplugin (rolldown, end to end)", () => {
  it("produces a bundle that imports and runs", async () => {
    const out = await bundle(true, "dist-stripped");
    const mod = (await import(pathToFileURL(out).href)) as {
      CallsController: new (g: { greet(n: string): string }) => { findOne(id: string): string };
      Greeter: new () => { greet(n: string): string };
    };
    const controller = new mod.CallsController(new mod.Greeter());
    expect(controller.findOne("world")).toBe("hello world");
  });

  it("emits no decorator syntax and no decorator helpers", async () => {
    const out = await bundle(true, "dist-clean");
    const { readFileSync } = await import("node:fs");
    const code = readFileSync(out, "utf8");
    expect(code).not.toMatch(/(?:^|[\s{;(])@[A-Za-z_$]/m);
    expect(code).not.toContain("__esDecorate");
    expect(code).not.toContain("__runInitializers");
    expect(code).not.toContain("__decorate");
    // The stubs are dead once nothing references them.
    expect(code).not.toContain("function Controller");
  });

  it("WITHOUT the plugin, the same bundle is not even parseable", async () => {
    const out = await bundle(false, "dist-raw");
    await expect(import(pathToFileURL(out).href)).rejects.toThrowError(SyntaxError);
  });
});
