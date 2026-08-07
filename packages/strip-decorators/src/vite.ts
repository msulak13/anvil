import { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";

/**
 * Vite plugin that removes anvil's no-op decorators before Vite's own
 * TypeScript transform sees them.
 *
 * @example
 * ```ts
 * import stripDecorators from "@anvil-di/strip-decorators/vite";
 *
 * export default defineConfig({
 *   plugins: [stripDecorators()],
 * });
 * ```
 */
const vitePlugin = stripDecoratorsUnplugin.vite as (
  options?: StripDecoratorsPluginOptions,
) => unknown;
export default vitePlugin;
