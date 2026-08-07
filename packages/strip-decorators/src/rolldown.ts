import { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";

/**
 * Rolldown plugin form of `@anvil-di/strip-decorators`.
 *
 * Rolldown is the case this package exists for: its decorator transform is
 * oxc's, which implements only the legacy convention, so a standard
 * decorator either passes through unparseable or is mis-applied.
 *
 * @example
 * ```ts
 * // rolldown.config.ts
 * import stripDecorators from "@anvil-di/strip-decorators/rolldown";
 *
 * export default {
 *   input: "src/index.ts",
 *   plugins: [stripDecorators()],
 * };
 * ```
 */
const rolldownPlugin = stripDecoratorsUnplugin.rolldown as (
  options?: StripDecoratorsPluginOptions,
) => unknown;
export default rolldownPlugin;
