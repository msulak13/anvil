import { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";

/** Rspack plugin form of `@anvil-di/strip-decorators`. See the package README. */
const rspackPlugin = stripDecoratorsUnplugin.rspack as (
  options?: StripDecoratorsPluginOptions,
) => unknown;
export default rspackPlugin;
