import { tsdiUnplugin, type TsdiPluginOptions } from "./index.js";

/**
 * Vite plugin that runs tsdi codegen at build start and on file
 * changes in dev. Equivalent to `tsdi build` integrated into the
 * normal Vite pipeline.
 *
 * @example
 * ```ts
 * import tsdi from "tsdi-unplugin/vite";
 *
 * export default defineConfig({
 *   plugins: [tsdi({ tsconfig: "./tsconfig.json" })],
 * });
 * ```
 */
const vitePlugin = tsdiUnplugin.vite as (options?: TsdiPluginOptions) => unknown;
export default vitePlugin;
