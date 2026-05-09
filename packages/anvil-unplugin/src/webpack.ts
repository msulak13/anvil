import { anvilUnplugin, type AnvilPluginOptions } from "./index.js";

/** Webpack plugin form of `@msulak/anvil-unplugin`. See the package README. */
const webpackPlugin = anvilUnplugin.webpack as (options?: AnvilPluginOptions) => unknown;
export default webpackPlugin;
