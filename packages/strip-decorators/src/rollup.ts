import { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";

/** Rollup plugin form of `@anvil-di/strip-decorators`. See the package README. */
const rollupPlugin = stripDecoratorsUnplugin.rollup as (
  options?: StripDecoratorsPluginOptions,
) => unknown;
export default rollupPlugin;
