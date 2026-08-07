import { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";

/** Esbuild plugin form of `@anvil-di/strip-decorators`. See the package README. */
const esbuildPlugin = stripDecoratorsUnplugin.esbuild as (
  options?: StripDecoratorsPluginOptions,
) => unknown;
export default esbuildPlugin;
