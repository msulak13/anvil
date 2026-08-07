import { stripDecoratorsUnplugin, type StripDecoratorsPluginOptions } from "./plugin.js";

/** Webpack plugin form of `@anvil-di/strip-decorators`. See the package README. */
const webpackPlugin = stripDecoratorsUnplugin.webpack as (
  options?: StripDecoratorsPluginOptions,
) => unknown;
export default webpackPlugin;
