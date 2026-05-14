import { anvilUnplugin, type AnvilPluginOptions } from "./index.js";

/**
 * Vite plugin that runs anvil codegen at build start and on file
 * changes in dev. Equivalent to `anvil build` integrated into the
 * normal Vite pipeline.
 *
 * @example
 * ```ts
 * import anvil from "@anvil-di/anvil-unplugin/vite";
 *
 * export default defineConfig({
 *   plugins: [anvil({ tsconfig: "./tsconfig.json" })],
 * });
 * ```
 */
const vitePlugin = anvilUnplugin.vite as (options?: AnvilPluginOptions) => unknown;
export default vitePlugin;
