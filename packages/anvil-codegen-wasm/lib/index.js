/**
 * `@anvil-di/anvil-codegen-wasm` — runs the entire anvil codegen pipeline in
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
 * import { compile } from "@anvil-di/anvil-codegen-wasm";
 *
 * const result = compile({
 *   entryPath: "/abs/src/app-component.ts",
 *   files: {
 *     "/abs/src/app-component.ts": "...source...",
 *     "/abs/src/heater.ts":        "...source...",
 *     "/abs/node_modules/@anvil-di/anvil/index.d.ts": "export const Inject: any; ...",
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
// `require("./anvil_codegen_wasm_bg.wasm")` to load the blob. We
// re-export it under a typed surface so consumers don't see the
// wasm-bindgen `any` types directly.
import { compile as wasmCompile } from "../dist/anvil_codegen_wasm.js";
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
export function compile(input) {
    return wasmCompile(input);
}
//# sourceMappingURL=index.js.map