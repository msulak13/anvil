/**
 * `tsdi-codegen-wasm` — runs the entire tsdi codegen pipeline in
 * pure WebAssembly. No native binary, no platform-specific package,
 * no `spawnSync` cost in the bundler hot path.
 *
 * Same parser, graph builder, validator, and emitter as the native
 * CLI — built from the identical Rust source via `wasm-pack`. The
 * only difference is the file source: this build reads from an
 * in-memory `Record<string, string>` instead of `std::fs`.
 *
 * @example
 * ```ts
 * import { compile } from "tsdi-codegen-wasm";
 *
 * const result = compile({
 *   entryPath: "/abs/src/app-component.ts",
 *   files: {
 *     "/abs/src/app-component.ts": "...source...",
 *     "/abs/src/heater.ts":        "...source...",
 *     "/abs/node_modules/tsdi/index.d.ts": "export const Inject: any; ...",
 *   },
 *   version: "0.0.1",
 * });
 *
 * for (const file of result.emittedFiles) {
 *   console.log(`emit ${file.path} (${file.contents.length} bytes)`);
 * }
 * for (const diag of result.diagnostics) {
 *   console.error(`${diag.code}: ${diag.summary}`);
 * }
 * ```
 */

// The wasm-pack output is CommonJS-ish — it uses synchronous
// `require("./tsdi_codegen_wasm_bg.wasm")` to load the blob. We
// re-export it under a typed surface so consumers don't see the
// wasm-bindgen `any` types directly.
import { compile as wasmCompile } from "../dist/tsdi_codegen_wasm.js";

/** Input bundle for {@link compile}. */
export interface CompileInput {
  /**
   * Absolute path of the `@Component` entry file. Must be a key
   * in `files`.
   */
  entryPath: string;
  /**
   * Project sources keyed by absolute path. Must include the entry
   * and every transitively-reachable source file (the host walks
   * the graph; tsdi only consumes what it's given).
   *
   * Bare-specifier imports like `import { Inject } from "tsdi"`
   * are resolved against entries whose paths contain
   * `node_modules/tsdi/...` — so include the runtime stub package
   * under such a path if your source uses tsdi decorators.
   */
  files: Record<string, string>;
  /**
   * Optional tsconfig `compilerOptions.paths` aliases. The host is
   * expected to pre-parse the user's tsconfig and forward the alias
   * table.
   */
  aliases?: PathAlias[];
  /**
   * Version string surfaced into the generated dagger's banner
   * comment, so users can correlate emitted output with the
   * tsdi-codegen-wasm package version that produced it.
   */
  version: string;
}

/** One tsconfig `paths` alias, in tsdi's wire format. */
export interface PathAlias {
  /** Alias pattern (e.g. `"@/*"`). */
  pattern: string;
  /** Target template (e.g. `"src/*"`). */
  target: string;
  /**
   * Absolute directory the relative `target` paths resolve against
   * (typically the tsconfig's own dir, or its `baseUrl`).
   */
  baseDir: string;
}

/** Output from {@link compile}. */
export interface CompileOutput {
  /**
   * Generated `.tsdi.ts` files, one per `@Component` that
   * validated successfully. Empty when **every** component had
   * diagnostics; partial when some components passed and others
   * didn't.
   */
  emittedFiles: EmittedFile[];
  /**
   * Structured validation diagnostics. Empty for a clean compile.
   * Hosts route these into the bundler's error pipeline (Vite
   * overlay, Rollup `this.warn`, Webpack stats output, etc.).
   */
  diagnostics: Diagnostic[];
}

/** A single emitted dagger file. */
export interface EmittedFile {
  /**
   * Absolute path (mirrors the source `@Component` file's path
   * with `.ts` swapped for `.tsdi.ts`).
   */
  path: string;
  /** The full TypeScript source — banner comment + dagger class. */
  contents: string;
}

/** A validation diagnostic. */
export interface Diagnostic {
  /**
   * Stable, machine-readable code. Examples: `"tsdi::missing_binding"`,
   * `"tsdi::cycle"`, `"tsdi::scope_mismatch"`.
   */
  code: string;
  /** One-line human-readable summary suitable for `Vite.overlay.error`. */
  summary: string;
  /** Primary source location (where the error anchors). */
  primary: SpanLabel;
  /**
   * Additional related locations — duplicate declarations, cycle
   * members, the requesting binding for a missing dep, etc.
   */
  related: SpanLabel[];
}

/** A source-anchored sub-message attached to a {@link Diagnostic}. */
export interface SpanLabel {
  /** Absolute path of the file the span is in. */
  path: string;
  /** Inclusive byte offset of the first character. */
  start: number;
  /** Exclusive byte offset just past the last character. */
  end: number;
  /** Human-readable note rendered next to the source span. */
  message: string;
}

/**
 * Run the codegen pipeline end-to-end against an in-memory file map.
 *
 * Synchronous — the WASM module is loaded eagerly the first time
 * this package is imported, and `compile` itself is a pure function
 * over the input bundle. Subsequent calls reuse the same WASM
 * instance.
 *
 * Throws when the **input** is malformed (bad paths, missing files,
 * malformed JSON shape). **Validation diagnostics** are not
 * exceptions — they flow through `output.diagnostics` so partial
 * progress is visible (e.g. component A validated, component B
 * didn't, here's why).
 */
export function compile(input: CompileInput): CompileOutput {
  return wasmCompile(input) as CompileOutput;
}
