import { describe, expect, it } from "vitest";
import { mayContainNoopDecorators, stripNoopDecorators } from "./decorator-strip.js";

/**
 * The load-bearing property of this package is not "decorators disappear" —
 * it is "*only anvil's* decorators disappear". A blanket strip would be a
 * silent correctness bug in any project that also uses a decorator with real
 * runtime behaviour, so most of what follows pins the boundary rather than
 * the happy path.
 */

const strip = (code: string, id = "input.ts"): ReturnType<typeof stripNoopDecorators> =>
  stripNoopDecorators(code, id);

describe("stripNoopDecorators", () => {
  it("removes class and method decorators imported from bellows", () => {
    const result = strip(`import { Controller, Get } from "@anvil-di/bellows";
@Controller("/calls")
export class CallsController {
  @Get("/:id") async findOne(id: string): Promise<string> { return id; }
}`);
    expect(result?.removed).toBe(2);
    expect(result?.code).not.toContain("@Controller");
    expect(result?.code).not.toContain("@Get");
    // The class itself, and everything in it, must survive verbatim.
    expect(result?.code).toContain("export class CallsController");
    expect(result?.code).toContain("async findOne(id: string): Promise<string> { return id; }");
  });

  it("removes anvil's module/provider decorators", () => {
    const result = strip(`import { Module, Provides, Singleton } from "@anvil-di/anvil";
@Module
export class CoffeeModule {
  @Singleton @Provides static providePump(): Pump { return new Pump(); }
}`);
    expect(result?.removed).toBe(3);
    expect(result?.code).toContain("static providePump(): Pump");
  });

  it("resolves aliased imports", () => {
    const result = strip(`import { Inject as I } from "@anvil-di/anvil";
@I export class Pump {}`);
    expect(result?.removed).toBe(1);
    expect(result?.code).toContain("export class Pump {}");
  });

  it("resolves namespace imports through the object, not the property", () => {
    const result = strip(`import * as anvil from "@anvil-di/anvil";
@anvil.Inject export class Pump {}`);
    expect(result?.removed).toBe(1);
  });

  it("leaves a decorator from any other package alone", () => {
    const result = strip(`import { Controller } from "@anvil-di/bellows";
import { observable } from "mobx";
@Controller("/x") export class C { @observable count = 1; }`);
    expect(result?.removed).toBe(1);
    expect(result?.code).not.toContain("@Controller");
    // The whole point: a decorator with real behaviour survives.
    expect(result?.code).toContain("@observable count = 1");
  });

  it("leaves a same-named decorator that came from somewhere else", () => {
    // `Get` here is a local helper, not bellows'. Name-based matching would
    // delete it; binding-based matching must not.
    const result = strip(`import { Get } from "./my-own-decorators.js";
@Get("/x") export class C {}`);
    expect(result).toBeNull();
  });

  it("ignores a type-only import", () => {
    const result = strip(`import type { Validator } from "@anvil-di/bellows";
export type V = Validator<string>;`);
    expect(result).toBeNull();
  });

  it("returns null when there is nothing to strip", () => {
    expect(strip(`export const x = 1;`)).toBeNull();
    expect(strip(`import { Token } from "@anvil-di/anvil";\nexport const t = Token;`)).toBeNull();
  });

  it("throws with the file name when the source does not parse", () => {
    expect(() => strip(`@Controller( export class {`, "broken.ts")).toThrow(/broken\.ts/);
  });

  it("parses tsx by extension", () => {
    const result = strip(
      `import { Inject } from "@anvil-di/anvil";
@Inject export class W { render() { return <div className="x" />; } }`,
      "widget.tsx",
    );
    expect(result?.removed).toBe(1);
    expect(result?.code).toContain("<div className=\"x\" />");
  });

  it("accepts additional no-op modules for a project's own markers", () => {
    const code = `import { Public } from "../http/public-route.js";
import { Controller } from "@anvil-di/bellows";
@Controller("/auth") export class AuthController {
  @Public("login runs before a session exists") login() {}
}`;
    // Without the opt-in, the project's own marker is left in place...
    expect(stripNoopDecorators(code, "auth.ts")?.removed).toBe(1);
    // ...and with it, both go.
    const opted = stripNoopDecorators(code, "auth.ts", {
      additionalModules: [/(?:^|\/)http\/public-route(?:\.js)?$/],
    });
    expect(opted?.removed).toBe(2);
    expect(opted?.code).not.toContain("@Public");
  });

  it("matches an additional module given as an exact string", () => {
    const result = stripNoopDecorators(
      `import { Marker } from "#markers";\n@Marker export class C {}`,
      "a.ts",
      { additionalModules: ["#markers"] },
    );
    expect(result?.removed).toBe(1);
  });

  it("produces a source map covering the removals", () => {
    const result = strip(`import { Inject } from "@anvil-di/anvil";
@Inject
export class Pump {}`);
    expect(result?.map.mappings).toBeTruthy();
    expect(result?.map.sources).toContain("input.ts");
  });
});

describe("mayContainNoopDecorators", () => {
  it("rejects files with no decorator syntax at all", () => {
    expect(mayContainNoopDecorators(`import { x } from "@anvil-di/anvil";`)).toBe(false);
  });

  it("rejects decorator files that never mention a no-op module", () => {
    expect(mayContainNoopDecorators(`import { o } from "mobx";\n@o class C {}`)).toBe(false);
  });

  it("accepts a file importing anvil and using a decorator", () => {
    expect(mayContainNoopDecorators(`import { Inject } from "@anvil-di/anvil";\n@Inject class C {}`)).toBe(
      true,
    );
  });

  it("never gives a false negative for anything the stripper would strip", () => {
    const cases = [
      `import { Controller } from "@anvil-di/bellows";\n@Controller("/x") class C {}`,
      `import { Inject as I } from "@anvil-di/anvil";\n@I class C {}`,
      `import * as a from "@anvil-di/anvil";\n@a.Inject class C {}`,
      `class C { @Get("/x") f() {} }\nimport { Get } from "@anvil-di/bellows";`,
    ];
    for (const code of cases) {
      if (stripNoopDecorators(code, "c.ts") !== null) {
        expect(mayContainNoopDecorators(code)).toBe(true);
      }
    }
  });

  it("treats any decorator file as a candidate once a RegExp module is configured", () => {
    const code = `import { Marker } from "./m.js";\n@Marker class C {}`;
    expect(mayContainNoopDecorators(code)).toBe(false);
    expect(mayContainNoopDecorators(code, { additionalModules: [/m\.js$/] })).toBe(true);
  });
});
